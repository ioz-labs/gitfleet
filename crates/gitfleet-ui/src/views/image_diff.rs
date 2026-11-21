use crate::app::{GitFleetApp, Message};
use crate::icons;
use crate::theme::{self, Palette, ThemeMode, type_scale};
use gitfleet_git::FileVersionPair;
use iced::advanced::image::{self as adv_image, Renderer as AdvImageRenderer};
use iced::advanced::layout as adv_layout;
use iced::advanced::text as adv_text;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Renderer as AdvRenderer, Shell, Widget, renderer};
use iced::widget::{
    Space, button, column, container, horizontal_rule, row, scrollable, slider, text, tooltip,
};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Point, Rectangle, Renderer, Shadow,
    Size, Theme, Vector, alignment, event, mouse,
};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) enum ImageDiffMsg {
    SetMode(ImageDiffMode),
    SetZoom(ImageDiffZoom),
    ToggleSwap,
    SwipeChanged(f32),
    OnionChanged(f32),
    BlinkFlip,
    ToggleBlinkPause,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ImageDiffViewState {
    pub(crate) mode: ImageDiffMode,
    pub(crate) zoom: ImageDiffZoom,
    pub(crate) swapped: bool,
    pub(crate) swipe: f32,
    pub(crate) onion: f32,
    pub(crate) blink_after: bool,
    pub(crate) blink_paused: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ImageDiffMode {
    TwoUp,
    Blink,
    #[default]
    Swipe,
    Onion,
    PixelDiff,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) enum ImageDiffZoom {
    #[default]
    Fit,
    Scale(f32),
}

impl ImageDiffZoom {
    pub(crate) fn step_in(self) -> Self {
        let cur = match self {
            Self::Fit => 1.0,
            Self::Scale(s) => s,
        };
        let steps = [0.25_f32, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];
        let next = steps
            .iter()
            .copied()
            .find(|&v| v > cur + 0.01)
            .unwrap_or(4.0);
        Self::Scale(next)
    }

    pub(crate) fn step_out(self) -> Self {
        let cur = match self {
            Self::Fit => 1.0,
            Self::Scale(s) => s,
        };
        let steps = [0.25_f32, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];
        let next = steps
            .iter()
            .rev()
            .copied()
            .find(|&v| v < cur - 0.01)
            .unwrap_or(0.25);
        Self::Scale(next)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DecodedImageSide {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) byte_len: usize,
    pub(crate) rgba: Arc<Vec<u8>>,
    pub(crate) handle: adv_image::Handle,
}

#[derive(Debug, Clone)]
pub(crate) struct ImageDiffData {
    pub(crate) file_path: String,
    pub(crate) is_svg: bool,
    pub(crate) before_label: String,
    pub(crate) after_label: String,
    pub(crate) before: Option<DecodedImageSide>,
    pub(crate) after: Option<DecodedImageSide>,
    pub(crate) diff_handle: Option<adv_image::Handle>,
    pub(crate) diff_width: u32,
    pub(crate) diff_height: u32,
    pub(crate) diff_changed_pixels: usize,
    pub(crate) diff_total_pixels: usize,
    pub(crate) load_error: Option<String>,
}

pub(crate) fn is_svg_path(path: &str) -> bool {
    path.rsplit('.')
        .next()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
}

pub(crate) fn is_raster_image_path(path: &str) -> bool {
    let Some(ext) = path.rsplit('.').next() else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "tif" | "tiff"
    )
}

pub(crate) fn is_image_diff_path(path: &str) -> bool {
    is_raster_image_path(path) || is_svg_path(path)
}

fn format_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn decode_raster_bytes(bytes: &[u8]) -> Result<DecodedImageSide, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("Image decode failed: {e}"))?;
    let rgba_img = img.to_rgba8();
    let (width, height) = rgba_img.dimensions();
    if width == 0 || height == 0 {
        return Err("Image has zero dimensions".into());
    }
    let raw = rgba_img.into_raw();
    let handle = adv_image::Handle::from_rgba(width, height, raw.clone());
    Ok(DecodedImageSide {
        width,
        height,
        byte_len: bytes.len(),
        rgba: Arc::new(raw),
        handle,
    })
}

fn decode_svg_bytes(bytes: &[u8]) -> Result<DecodedImageSide, String> {
    let opt = resvg::usvg::Options::default();
    let tree =
        resvg::usvg::Tree::from_data(bytes, &opt).map_err(|e| format!("SVG parse failed: {e}"))?;
    let sz = tree.size();
    let nat_w = sz.width().max(1.0);
    let nat_h = sz.height().max(1.0);
    let max_dim = nat_w.max(nat_h);
    // Upscale tiny icon SVGs cleanly for crisp inspection; cap huge SVGs at 1600px
    let scale = if max_dim < 256.0 {
        (512.0 / max_dim).min(8.0)
    } else if max_dim > 1600.0 {
        1600.0 / max_dim
    } else {
        1.0
    };
    let out_w = (nat_w * scale).round().clamp(1.0, 1600.0) as u32;
    let out_h = (nat_h * scale).round().clamp(1.0, 1600.0) as u32;

    let mut pixmap = tiny_skia::Pixmap::new(out_w, out_h)
        .ok_or_else(|| "Failed to allocate SVG raster buffer".to_string())?;
    let transform = tiny_skia::Transform::from_scale(
        out_w as f32 / nat_w,
        out_h as f32 / nat_h,
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // Convert premultiplied RGBA from tiny-skia into straight RGBA for Iced
    let premul = pixmap.data();
    let mut straight = vec![0u8; premul.len()];
    for (dst, src) in straight.chunks_exact_mut(4).zip(premul.chunks_exact(4)) {
        let a = src[3];
        if a == 0 {
            dst.copy_from_slice(&[0, 0, 0, 0]);
        } else if a == 255 {
            dst.copy_from_slice(src);
        } else {
            let af = a as u32;
            dst[0] = ((src[0] as u32 * 255) / af).min(255) as u8;
            dst[1] = ((src[1] as u32 * 255) / af).min(255) as u8;
            dst[2] = ((src[2] as u32 * 255) / af).min(255) as u8;
            dst[3] = a;
        }
    }

    let handle = adv_image::Handle::from_rgba(out_w, out_h, straight.clone());
    Ok(DecodedImageSide {
        width: nat_w.round().max(1.0) as u32,
        height: nat_h.round().max(1.0) as u32,
        byte_len: bytes.len(),
        rgba: Arc::new(straight),
        handle,
    })
}

/// Computes a vivid pixel-difference RGBA image comparing `before` and `after` (centered when
/// dimensions differ). Identical pixels are dimmed grayscale for context; changed pixels are
/// highlighted in magenta/green/red.
pub(crate) fn compute_pixel_diff(
    before: Option<&DecodedImageSide>,
    after: Option<&DecodedImageSide>,
) -> Option<(adv_image::Handle, u32, u32, usize, usize)> {
    let bw = before.map(|s| s.rgba.len() / 4).unwrap_or(0);
    let aw = after.map(|s| s.rgba.len() / 4).unwrap_or(0);
    if bw == 0 && aw == 0 {
        return None;
    }

    // Use actual pixel buffer dimensions (matters when SVG was upscaled to a crisp raster size)
    let side_dims = |s: &DecodedImageSide| -> (u32, u32) {
        match &s.handle {
            adv_image::Handle::Rgba { width, height, .. } => (*width, *height),
            _ => (s.width, s.height),
        }
    };
    let (b_w, b_h) = before.map(side_dims).unwrap_or((0, 0));
    let (a_w, a_h) = after.map(side_dims).unwrap_or((0, 0));
    let out_w = b_w.max(a_w).max(1);
    let out_h = b_h.max(a_h).max(1);

    let b_ox = (out_w - b_w) / 2;
    let b_oy = (out_h - b_h) / 2;
    let a_ox = (out_w - a_w) / 2;
    let a_oy = (out_h - a_h) / 2;

    let sample = |side: Option<&DecodedImageSide>,
                  sw: u32,
                  sh: u32,
                  ox: u32,
                  oy: u32,
                  x: u32,
                  y: u32|
     -> [u8; 4] {
        let Some(s) = side else {
            return [0, 0, 0, 0];
        };
        if x < ox || y < oy || x >= ox + sw || y >= oy + sh {
            return [0, 0, 0, 0];
        }
        let idx = (((y - oy) * sw + (x - ox)) as usize) * 4;
        if idx + 4 <= s.rgba.len() {
            [
                s.rgba[idx],
                s.rgba[idx + 1],
                s.rgba[idx + 2],
                s.rgba[idx + 3],
            ]
        } else {
            [0, 0, 0, 0]
        }
    };

    let total_pixels = (out_w as usize).saturating_mul(out_h as usize);
    let mut out = vec![0u8; total_pixels * 4];
    let mut changed = 0usize;

    for y in 0..out_h {
        for x in 0..out_w {
            let p1 = sample(before, b_w, b_h, b_ox, b_oy, x, y);
            let p2 = sample(after, a_w, a_h, a_ox, a_oy, x, y);
            let dr = p1[0].abs_diff(p2[0]);
            let dg = p1[1].abs_diff(p2[1]);
            let db = p1[2].abs_diff(p2[2]);
            let da = p1[3].abs_diff(p2[3]);
            let max_d = dr.max(dg).max(db).max(da);
            let idx = ((y * out_w + x) as usize) * 4;

            if max_d <= 2 {
                // Dimmed grayscale context so image landmarks stay readable
                let luma = ((p2[0] as u32 * 77 + p2[1] as u32 * 150 + p2[2] as u32 * 29) >> 8) as u8;
                let dim = (luma as f32 * 0.34) as u8;
                let a = p1[3].max(p2[3]);
                out[idx] = dim;
                out[idx + 1] = dim;
                out[idx + 2] = dim;
                out[idx + 3] = a;
            } else {
                changed += 1;
                if p1[3] <= 4 && p2[3] > 4 {
                    // Purely added pixel (emerald)
                    out[idx] = 56;
                    out[idx + 1] = 228;
                    out[idx + 2] = 142;
                    out[idx + 3] = 255;
                } else if p1[3] > 4 && p2[3] <= 4 {
                    // Purely removed pixel (coral red)
                    out[idx] = 255;
                    out[idx + 1] = 86;
                    out[idx + 2] = 98;
                    out[idx + 3] = 255;
                } else {
                    // Modified pixel (vivid magenta scaled slightly by delta)
                    let strength = 0.55 + 0.45 * (max_d as f32 / 255.0);
                    out[idx] = (255.0 * strength).min(255.0) as u8;
                    out[idx + 1] = (48.0 * (1.0 - strength * 0.35)) as u8;
                    out[idx + 2] = (156.0 * strength).min(255.0) as u8;
                    out[idx + 3] = 255;
                }
            }
        }
    }

    let handle = adv_image::Handle::from_rgba(out_w, out_h, out);
    Some((handle, out_w, out_h, changed, total_pixels))
}

pub(crate) fn decode_image_diff_pair(pair: FileVersionPair) -> ImageDiffData {
    let is_svg = is_svg_path(&pair.file_path);
    let mut errors: Vec<String> = Vec::new();

    let decode_opt = |bytes: Option<Vec<u8>>, errs: &mut Vec<String>| -> Option<DecodedImageSide> {
        let data = bytes?;
        if data.is_empty() {
            return None;
        }
        let res = if is_svg {
            decode_svg_bytes(&data)
        } else {
            decode_raster_bytes(&data)
        };
        match res {
            Ok(side) => Some(side),
            Err(e) => {
                errs.push(e);
                None
            }
        }
    };

    let before = decode_opt(pair.before_bytes, &mut errors);
    let after = decode_opt(pair.after_bytes, &mut errors);
    let diff = compute_pixel_diff(before.as_ref(), after.as_ref());
    let (diff_handle, diff_width, diff_height, diff_changed_pixels, diff_total_pixels) =
        match diff {
            Some((h, w, hh, c, t)) => (Some(h), w, hh, c, t),
            None => (None, 0, 0, 0, 0),
        };

    ImageDiffData {
        file_path: pair.file_path,
        is_svg,
        before_label: pair.before_label,
        after_label: pair.after_label,
        before,
        after,
        diff_handle,
        diff_width,
        diff_height,
        diff_changed_pixels,
        diff_total_pixels,
        load_error: if errors.is_empty() {
            None
        } else {
            Some(errors.join(" · "))
        },
    }
}

#[derive(Clone)]
enum CompareRenderKind {
    Single {
        side: Option<DecodedImageSide>,
        empty_label: String,
    },
    Swipe {
        before: Option<DecodedImageSide>,
        after: Option<DecodedImageSide>,
        pos: f32,
    },
    Onion {
        before: Option<DecodedImageSide>,
        after: Option<DecodedImageSide>,
        opacity: f32,
    },
    Blink {
        before: Option<DecodedImageSide>,
        after: Option<DecodedImageSide>,
        show_after: bool,
    },
    PixelDiff {
        handle: Option<adv_image::Handle>,
        width: u32,
        height: u32,
    },
}

fn draw_checkerboard(renderer: &mut Renderer, rect: Rectangle, dark_mode: bool) {
    let (c1, c2) = if dark_mode {
        (
            Color::from_rgb(0.115, 0.138, 0.160),
            Color::from_rgb(0.155, 0.184, 0.212),
        )
    } else {
        (
            Color::from_rgb(0.92, 0.935, 0.95),
            Color::from_rgb(0.84, 0.865, 0.89),
        )
    };
    renderer.fill_quad(
        renderer::Quad {
            bounds: rect,
            border: Border::default(),
            shadow: Shadow::default(),
        },
        Background::Color(c1),
    );
    let tile = 16.0_f32;
    let cols = (rect.width / tile).ceil().min(120.0) as u32;
    let rows = (rect.height / tile).ceil().min(120.0) as u32;
    for r in 0..rows {
        let y = rect.y + r as f32 * tile;
        let h = (rect.y + rect.height - y).min(tile);
        if h <= 0.0 {
            break;
        }
        for c in 0..cols {
            if (r + c) % 2 == 1 {
                let x = rect.x + c as f32 * tile;
                let w = (rect.x + rect.width - x).min(tile);
                if w <= 0.0 {
                    break;
                }
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x,
                            y,
                            width: w,
                            height: h,
                        },
                        border: Border::default(),
                        shadow: Shadow::default(),
                    },
                    Background::Color(c2),
                );
            }
        }
    }
}

fn image_pixel_dims(side: &DecodedImageSide) -> (f32, f32) {
    match &side.handle {
        adv_image::Handle::Rgba { width, height, .. } => (*width as f32, *height as f32),
        _ => (side.width as f32, side.height as f32),
    }
}

fn draw_centered_side(
    renderer: &mut Renderer,
    side: &DecodedImageSide,
    stage: Rectangle,
    base_w: f32,
    base_h: f32,
    opacity: f32,
) {
    let (sw, sh) = image_pixel_dims(side);
    let scale_x = stage.width / base_w.max(1.0);
    let scale_y = stage.height / base_h.max(1.0);
    let dw = (sw * scale_x).max(1.0);
    let dh = (sh * scale_y).max(1.0);
    let rect = Rectangle {
        x: stage.x + (stage.width - dw) * 0.5,
        y: stage.y + (stage.height - dh) * 0.5,
        width: dw,
        height: dh,
    };
    let img = adv_image::Image::new(side.handle.clone())
        .opacity(opacity.clamp(0.0, 1.0))
        .snap(true);
    AdvImageRenderer::draw_image(renderer, img, rect);
}

#[derive(Default)]
struct ImageCanvasState {
    dragging_swipe: bool,
}

struct ImageCompareCanvas {
    kind: CompareRenderKind,
    zoom: ImageDiffZoom,
    p: Palette,
    dark_mode: bool,
    left_corner_badge: Option<&'static str>,
    right_corner_badge: Option<&'static str>,
}

impl ImageCompareCanvas {
    fn natural_base_size(&self) -> (f32, f32) {
        match &self.kind {
            CompareRenderKind::Single { side, .. } => side
                .as_ref()
                .map(image_pixel_dims)
                .unwrap_or((320.0, 200.0)),
            CompareRenderKind::Swipe { before, after, .. }
            | CompareRenderKind::Onion { before, after, .. }
            | CompareRenderKind::Blink { before, after, .. } => {
                let (bw, bh) = before.as_ref().map(image_pixel_dims).unwrap_or((0.0, 0.0));
                let (aw, ah) = after.as_ref().map(image_pixel_dims).unwrap_or((0.0, 0.0));
                (bw.max(aw).max(1.0), bh.max(ah).max(1.0))
            }
            CompareRenderKind::PixelDiff { width, height, .. } => {
                ((*width).max(1) as f32, (*height).max(1) as f32)
            }
        }
    }

    fn compute_stage_rect(&self, bounds: Rectangle) -> Rectangle {
        let (base_w, base_h) = self.natural_base_size();
        let pad_x = 32.0;
        let pad_y = 32.0;
        let avail_w = (bounds.width - pad_x).max(40.0);
        let avail_h = (bounds.height - pad_y).max(40.0);
        let scale = match self.zoom {
            ImageDiffZoom::Fit => {
                let fit = (avail_w / base_w).min(avail_h / base_h);
                let max_up = if base_w.max(base_h) < 180.0 {
                    (220.0 / base_w.max(base_h)).min(4.0)
                } else {
                    1.0
                };
                fit.min(max_up).max(0.05)
            }
            ImageDiffZoom::Scale(s) => s.clamp(0.1, 8.0),
        };
        let w = (base_w * scale).max(1.0);
        let h = (base_h * scale).max(1.0);
        Rectangle {
            x: bounds.x + (bounds.width - w) * 0.5,
            y: bounds.y + (bounds.height - h) * 0.5,
            width: w,
            height: h,
        }
    }
}

fn draw_corner_label(
    renderer: &mut Renderer,
    label: &str,
    origin: Point,
    align_right: bool,
    p: Palette,
) {
    let w = label.len() as f32 * 7.6 + 16.0;
    let h = 22.0;
    let bx = if align_right { origin.x - w } else { origin.x };
    let rect = Rectangle {
        x: bx,
        y: origin.y,
        width: w,
        height: h,
    };
    let bg = Color {
        a: 0.84,
        ..p.surface_titlebar
    };
    renderer.fill_quad(
        renderer::Quad {
            bounds: rect,
            border: Border {
                color: p.border_default,
                width: 1.0,
                radius: 4.0.into(),
            },
            shadow: Shadow::default(),
        },
        Background::Color(bg),
    );
    adv_text::Renderer::fill_text(
        renderer,
        adv_text::Text {
            content: label.to_string(),
            bounds: Size::new(w, h),
            size: iced::Pixels(type_scale::BADGE),
            line_height: text::LineHeight::default(),
            font: theme::CODE_FONT,
            horizontal_alignment: alignment::Horizontal::Center,
            vertical_alignment: alignment::Vertical::Center,
            shaping: text::Shaping::Basic,
            wrapping: text::Wrapping::None,
        },
        Point::new(bx + w * 0.5, origin.y + h * 0.5),
        p.text_secondary,
        rect,
    );
}

impl Widget<ImageDiffMsg, Theme, Renderer> for ImageCompareCanvas {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<ImageCanvasState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(ImageCanvasState::default())
    }

    fn size(&self) -> Size<Length> {
        match self.zoom {
            ImageDiffZoom::Fit => Size::new(Length::Fill, Length::Fill),
            ImageDiffZoom::Scale(s) => {
                let (bw, bh) = self.natural_base_size();
                Size::new(
                    Length::Fixed((bw * s + 48.0).max(320.0)),
                    Length::Fixed((bh * s + 48.0).max(240.0)),
                )
            }
        }
    }

    fn layout(
        &self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &adv_layout::Limits,
    ) -> adv_layout::Node {
        let sz = self.size();
        adv_layout::Node::new(limits.resolve(sz.width, sz.height, Size::new(320.0, 240.0)))
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let Some(clipped_vp) = bounds.intersection(viewport) else {
            return;
        };

        // Deep canvas surface matching screenshot #1's crisp inspection stage
        let stage_bg = if self.dark_mode {
            Color::from_rgb(0.043, 0.055, 0.071) // #0b0e12
        } else {
            Color::from_rgb(0.935, 0.945, 0.958)
        };
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: Border {
                    color: self.p.border_subtle,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                shadow: Shadow::default(),
            },
            Background::Color(stage_bg),
        );

        let stage = self.compute_stage_rect(bounds);
        let (base_w, base_h) = self.natural_base_size();
        let stage_clip = stage.intersection(&clipped_vp);

        renderer.with_layer(clipped_vp, |renderer| {
            let has_any_image = match &self.kind {
                CompareRenderKind::Single { side, .. } => side.is_some(),
                CompareRenderKind::Swipe { before, after, .. }
                | CompareRenderKind::Onion { before, after, .. }
                | CompareRenderKind::Blink { before, after, .. } => {
                    before.is_some() || after.is_some()
                }
                CompareRenderKind::PixelDiff { handle, .. } => handle.is_some(),
            };

            // Layer 0 (base): checkerboard + outer border drawn directly in `clipped_vp` so
            // subsequent `with_layer(...)` image passes always render strictly ABOVE it in wgpu.
            if has_any_image {
                draw_checkerboard(renderer, stage, self.dark_mode);
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: stage.x - 1.0,
                            y: stage.y - 1.0,
                            width: stage.width + 2.0,
                            height: stage.height + 2.0,
                        },
                        border: Border {
                            color: self.p.border_default,
                            width: 1.0,
                            radius: 0.0.into(),
                        },
                        shadow: Shadow::default(),
                    },
                    Background::Color(Color::TRANSPARENT),
                );
            }

            // Layer +1..+2: image content per comparison mode
            match &self.kind {
                CompareRenderKind::Single { side, empty_label } => {
                    if let Some(s) = side {
                        if let Some(clip) = stage_clip {
                            renderer.with_layer(clip, |renderer| {
                                draw_centered_side(renderer, s, stage, base_w, base_h, 1.0);
                            });
                        }
                    } else {
                        adv_text::Renderer::fill_text(
                            renderer,
                            adv_text::Text {
                                content: empty_label.clone(),
                                bounds: bounds.size(),
                                size: iced::Pixels(type_scale::BODY),
                                line_height: text::LineHeight::default(),
                                font: iced::Font::DEFAULT,
                                horizontal_alignment: alignment::Horizontal::Center,
                                vertical_alignment: alignment::Vertical::Center,
                                shaping: text::Shaping::Basic,
                                wrapping: text::Wrapping::None,
                            },
                            Point::new(
                                bounds.x + bounds.width * 0.5,
                                bounds.y + bounds.height * 0.5,
                            ),
                            self.p.text_muted,
                            clipped_vp,
                        );
                    }
                }
                CompareRenderKind::Swipe { before, after, pos } => {
                    let split_x = (stage.x + stage.width * pos.clamp(0.0, 1.0))
                        .clamp(bounds.x + 2.0, bounds.x + bounds.width - 2.0);

                    if let Some(b) = before {
                        let left_clip = Rectangle {
                            x: bounds.x,
                            y: bounds.y,
                            width: (split_x - bounds.x).max(0.0),
                            height: bounds.height,
                        };
                        if let Some(inter) = left_clip.intersection(&clipped_vp)
                            && inter.width > 0.5
                        {
                            renderer.with_layer(inter, |renderer| {
                                draw_centered_side(renderer, b, stage, base_w, base_h, 1.0);
                            });
                        }
                    }

                    if let Some(a) = after {
                        let right_clip = Rectangle {
                            x: split_x,
                            y: bounds.y,
                            width: (bounds.x + bounds.width - split_x).max(0.0),
                            height: bounds.height,
                        };
                        if let Some(inter) = right_clip.intersection(&clipped_vp)
                            && inter.width > 0.5
                        {
                            renderer.with_layer(inter, |renderer| {
                                draw_centered_side(renderer, a, stage, base_w, base_h, 1.0);
                            });
                        }
                    }

                    // Draw swipe divider + circular drag handle in an overlay layer ABOVE both images
                    renderer.with_layer(clipped_vp, |renderer| {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle {
                                    x: split_x - 2.0,
                                    y: bounds.y,
                                    width: 4.0,
                                    height: bounds.height,
                                },
                                border: Border::default(),
                                shadow: Shadow::default(),
                            },
                            Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.45)),
                        );
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle {
                                    x: split_x - 1.0,
                                    y: bounds.y,
                                    width: 2.0,
                                    height: bounds.height,
                                },
                                border: Border::default(),
                                shadow: Shadow::default(),
                            },
                            Background::Color(Color::WHITE),
                        );

                        let handle_d = 30.0;
                        let cy = clipped_vp.y + clipped_vp.height * 0.5;
                        let handle_rect = Rectangle {
                            x: split_x - handle_d * 0.5,
                            y: cy - handle_d * 0.5,
                            width: handle_d,
                            height: handle_d,
                        };
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: handle_rect,
                                border: Border {
                                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
                                    width: 1.0,
                                    radius: (handle_d * 0.5).into(),
                                },
                                shadow: Shadow {
                                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.42),
                                    offset: Vector::new(0.0, 2.0),
                                    blur_radius: 8.0,
                                },
                            },
                            Background::Color(Color::WHITE),
                        );
                        adv_text::Renderer::fill_text(
                            renderer,
                            adv_text::Text {
                                content: "↔".to_string(),
                                bounds: handle_rect.size(),
                                size: iced::Pixels(15.0),
                                line_height: text::LineHeight::default(),
                                font: iced::Font::DEFAULT,
                                horizontal_alignment: alignment::Horizontal::Center,
                                vertical_alignment: alignment::Vertical::Center,
                                shaping: text::Shaping::Advanced,
                                wrapping: text::Wrapping::None,
                            },
                            Point::new(split_x, cy),
                            Color::from_rgb(0.12, 0.14, 0.18),
                            handle_rect,
                        );
                    });
                }
                CompareRenderKind::Onion {
                    before,
                    after,
                    opacity,
                } => {
                    if let Some(clip) = stage_clip {
                        if let Some(b) = before {
                            renderer.with_layer(clip, |renderer| {
                                draw_centered_side(renderer, b, stage, base_w, base_h, 1.0);
                            });
                        }
                        if let Some(a) = after {
                            renderer.with_layer(clip, |renderer| {
                                draw_centered_side(renderer, a, stage, base_w, base_h, *opacity);
                            });
                        }
                    }
                }
                CompareRenderKind::Blink {
                    before,
                    after,
                    show_after,
                } => {
                    let chosen = if *show_after { after } else { before };
                    if let Some(side) = chosen {
                        if let Some(clip) = stage_clip {
                            renderer.with_layer(clip, |renderer| {
                                draw_centered_side(renderer, side, stage, base_w, base_h, 1.0);
                            });
                        }
                    } else {
                        let missing = if *show_after {
                            "(After: deleted)"
                        } else {
                            "(Before: new file)"
                        };
                        adv_text::Renderer::fill_text(
                            renderer,
                            adv_text::Text {
                                content: missing.to_string(),
                                bounds: bounds.size(),
                                size: iced::Pixels(type_scale::BODY),
                                line_height: text::LineHeight::default(),
                                font: iced::Font::DEFAULT,
                                horizontal_alignment: alignment::Horizontal::Center,
                                vertical_alignment: alignment::Vertical::Center,
                                shaping: text::Shaping::Basic,
                                wrapping: text::Wrapping::None,
                            },
                            Point::new(
                                bounds.x + bounds.width * 0.5,
                                bounds.y + bounds.height * 0.5,
                            ),
                            self.p.text_muted,
                            clipped_vp,
                        );
                    }
                }
                CompareRenderKind::PixelDiff { handle, .. } => {
                    if let Some(h) = handle
                        && let Some(clip) = stage_clip
                    {
                        renderer.with_layer(clip, |renderer| {
                            let img = adv_image::Image::new(h.clone()).snap(true);
                            AdvImageRenderer::draw_image(renderer, img, stage);
                        });
                    }
                }
            }

            // Top overlay layer: corner labels (`BEFORE` / `AFTER`) always stay above images
            if self.left_corner_badge.is_some() || self.right_corner_badge.is_some() {
                renderer.with_layer(clipped_vp, |renderer| {
                    if let Some(lbl) = self.left_corner_badge {
                        draw_corner_label(
                            renderer,
                            lbl,
                            Point::new(clipped_vp.x + 10.0, clipped_vp.y + 8.0),
                            false,
                            self.p,
                        );
                    }
                    if let Some(lbl) = self.right_corner_badge {
                        draw_corner_label(
                            renderer,
                            lbl,
                            Point::new(clipped_vp.x + clipped_vp.width - 10.0, clipped_vp.y + 8.0),
                            true,
                            self.p,
                        );
                    }
                });
            }
        });
    }

    fn on_event(
        &mut self,
        tree: &mut Tree,
        event: iced::Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, ImageDiffMsg>,
        _viewport: &Rectangle,
    ) -> event::Status {
        let state = tree.state.downcast_mut::<ImageCanvasState>();
        let bounds = layout.bounds();

        match &self.kind {
            CompareRenderKind::Swipe { .. } => {
                let stage = self.compute_stage_rect(bounds);
                let compute_ratio = |px: f32| -> f32 {
                    ((px - stage.x) / stage.width.max(1.0)).clamp(0.0, 1.0)
                };
                match event {
                    iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                        if let Some(pos) = cursor.position_over(bounds) {
                            state.dragging_swipe = true;
                            shell.publish(ImageDiffMsg::SwipeChanged(compute_ratio(pos.x)));
                            return event::Status::Captured;
                        }
                    }
                    iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                        if state.dragging_swipe
                            && let Some(pos) = cursor.position()
                        {
                            shell.publish(ImageDiffMsg::SwipeChanged(compute_ratio(pos.x)));
                            return event::Status::Captured;
                        }
                    }
                    iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                        if state.dragging_swipe =>
                    {
                        state.dragging_swipe = false;
                        return event::Status::Captured;
                    }
                    _ => {}
                }
            }
            CompareRenderKind::Blink { .. } => {
                if let iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
                    && cursor.is_over(bounds)
                {
                    shell.publish(ImageDiffMsg::BlinkFlip);
                    return event::Status::Captured;
                }
            }
            _ => {}
        }

        event::Status::Ignored
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<ImageCanvasState>();
        match &self.kind {
            CompareRenderKind::Swipe { .. } => {
                if state.dragging_swipe || cursor.is_over(layout.bounds()) {
                    mouse::Interaction::ResizingHorizontally
                } else {
                    mouse::Interaction::None
                }
            }
            CompareRenderKind::Blink { .. } => {
                if cursor.is_over(layout.bounds()) {
                    mouse::Interaction::Pointer
                } else {
                    mouse::Interaction::None
                }
            }
            _ => mouse::Interaction::None,
        }
    }
}

fn meta_side_strip<'a>(
    p: Palette,
    badge_label: &'static str,
    file_name: &str,
    ref_label: &str,
    side: Option<&DecodedImageSide>,
    missing_label: &'static str,
) -> Element<'a, ImageDiffMsg> {
    let dim_text = match side {
        Some(s) => format!(
            "{}×{} px · {}",
            s.width,
            s.height,
            format_bytes(s.byte_len)
        ),
        None => missing_label.to_string(),
    };
    row![
        container(
            text(badge_label)
                .size(type_scale::BADGE)
                .font(theme::CODE_FONT)
                .color(p.text_secondary)
                .wrapping(text::Wrapping::None)
        )
        .padding([1, 5])
        .style(theme::section_panel_box(p)),
        text(file_name.to_string())
            .size(type_scale::CAPTION)
            .color(p.text_primary)
            .wrapping(text::Wrapping::None),
        text(format!("— {ref_label}"))
            .size(type_scale::CAPTION)
            .color(p.text_muted)
            .wrapping(text::Wrapping::None),
        Space::with_width(Length::Fill),
        text(dim_text)
            .size(type_scale::CAPTION)
            .font(theme::CODE_FONT)
            .color(if side.is_some() {
                p.text_secondary
            } else {
                p.warning_fg
            })
            .wrapping(text::Wrapping::None),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .width(Length::FillPortion(1))
    .into()
}

pub(crate) fn render_image_diff<'a>(
    data: Option<&'a ImageDiffData>,
    st: ImageDiffViewState,
    p: Palette,
    theme_mode: ThemeMode,
) -> Element<'a, ImageDiffMsg> {
    let Some(data) = data else {
        return theme::empty_state(
            p,
            icons::image_frame(theme::icon_size::LG, p.text_muted),
            "Loading image diff…",
            "Decoding before & after image revisions.",
            None,
        );
    };

    let dark_mode = theme_mode == ThemeMode::Dark;
    let file_name = data
        .file_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&data.file_path);

    let (left_side, right_side, left_ref, right_ref) = if st.swapped {
        (
            data.after.as_ref(),
            data.before.as_ref(),
            data.after_label.as_str(),
            data.before_label.as_str(),
        )
    } else {
        (
            data.before.as_ref(),
            data.after.as_ref(),
            data.before_label.as_str(),
            data.after_label.as_str(),
        )
    };

    // 1. Mode & Zoom Toolbar (matching RepoFlock screenshot #1)
    let mode_btn = |label: &'static str, mode: ImageDiffMode| -> Element<'a, ImageDiffMsg> {
        button(
            text(label)
                .size(type_scale::CAPTION)
                .wrapping(text::Wrapping::None),
        )
        .padding([3, 10])
        .style(theme::segmented_btn(p, st.mode == mode))
        .on_press(ImageDiffMsg::SetMode(mode))
        .into()
    };

    let mode_group = theme::segmented_group(
        p,
        row![
            mode_btn("2-up", ImageDiffMode::TwoUp),
            mode_btn("Blink", ImageDiffMode::Blink),
            mode_btn("Swipe", ImageDiffMode::Swipe),
            mode_btn("Onion", ImageDiffMode::Onion),
            mode_btn("Diff", ImageDiffMode::PixelDiff),
        ]
        .spacing(2)
        .align_y(Alignment::Center),
    );

    let swap_btn = tooltip(
        button(
            row![
                icons::swap_arrows(
                    13.0,
                    if st.swapped {
                        p.accent_fg
                    } else {
                        p.text_secondary
                    }
                ),
                text("Swap")
                    .size(type_scale::CAPTION)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .padding([4, 9])
        .style(theme::toolbar_btn(p, st.swapped))
        .on_press(ImageDiffMsg::ToggleSwap),
        container(
            text("Swap Before ↔ After sides")
                .size(type_scale::CAPTION)
                .color(p.text_primary),
        )
        .padding([5, 9])
        .style(theme::tooltip_box(p)),
        tooltip::Position::Bottom,
    )
    .gap(5);

    let mut toolbar = row![swap_btn, mode_group]
        .spacing(8)
        .align_y(Alignment::Center);

    // Mode-specific inline controls
    match st.mode {
        ImageDiffMode::Onion => {
            let pct = (st.onion * 100.0).round() as u32;
            toolbar = toolbar
                .push(
                    text(format!("Opacity {pct}%"))
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(p.text_secondary)
                        .wrapping(text::Wrapping::None),
                )
                .push(
                    container(
                        slider(0.0..=1.0, st.onion, ImageDiffMsg::OnionChanged)
                            .step(0.01)
                            .width(Length::Fixed(120.0)),
                    )
                    .align_y(alignment::Vertical::Center),
                );
        }
        ImageDiffMode::Blink => {
            let paused = st.blink_paused;
            let showing_after = st.blink_after;
            toolbar = toolbar
                .push(
                    button(
                        row![
                            if paused {
                                icons::play(11.0, p.text_primary)
                            } else {
                                icons::pause(11.0, p.accent_fg)
                            },
                            text(if paused { "Resume" } else { "Pause" })
                                .size(type_scale::CAPTION)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                    )
                    .padding([3, 8])
                    .style(theme::subtle_btn(p))
                    .on_press(ImageDiffMsg::ToggleBlinkPause),
                )
                .push(
                    button(
                        text(if showing_after {
                            "Showing: AFTER"
                        } else {
                            "Showing: BEFORE"
                        })
                        .size(type_scale::CAPTION)
                        .color(if showing_after {
                            p.success_fg
                        } else {
                            p.warning_fg
                        })
                        .wrapping(text::Wrapping::None),
                    )
                    .padding([3, 8])
                    .style(theme::subtle_btn(p))
                    .on_press(ImageDiffMsg::BlinkFlip),
                );
        }
        ImageDiffMode::PixelDiff => {
            let stat_label = if data.diff_total_pixels == 0 {
                "No pixels".to_string()
            } else if data.diff_changed_pixels == 0 {
                "Identical (0 px changed)".to_string()
            } else {
                let pct =
                    (data.diff_changed_pixels as f64 / data.diff_total_pixels as f64) * 100.0;
                format!("{} px changed ({pct:.2}%)", data.diff_changed_pixels)
            };
            let fg = if data.diff_changed_pixels == 0 {
                p.success_fg
            } else {
                p.danger_fg
            };
            let bg = if data.diff_changed_pixels == 0 {
                p.success_bg
            } else {
                p.danger_bg
            };
            toolbar = toolbar.push(theme::badge(stat_label, bg, fg, p.border_subtle, true));
        }
        ImageDiffMode::Swipe => {
            let pct = (st.swipe * 100.0).round() as u32;
            toolbar = toolbar.push(
                text(format!("Split {pct}%"))
                    .size(type_scale::CAPTION)
                    .font(theme::CODE_FONT)
                    .color(p.text_muted)
                    .wrapping(text::Wrapping::None),
            );
        }
        ImageDiffMode::TwoUp => {}
    }

    // Dimensions changed alert badge if before & after sizes differ
    if let (Some(b), Some(a)) = (left_side, right_side)
        && (b.width != a.width || b.height != a.height)
    {
        toolbar = toolbar.push(theme::badge(
            format!("Resized {}×{} → {}×{}", b.width, b.height, a.width, a.height),
            p.warning_bg,
            p.warning_fg,
            p.border_subtle,
            true,
        ));
    }

    let zoom_pct_label = match st.zoom {
        ImageDiffZoom::Fit => "Fit".to_string(),
        ImageDiffZoom::Scale(s) => format!("{}%", (s * 100.0).round() as u32),
    };
    let is_fit = matches!(st.zoom, ImageDiffZoom::Fit);
    let is_100 = matches!(st.zoom, ImageDiffZoom::Scale(s) if (s - 1.0).abs() < 0.01);

    let zoom_group = theme::segmented_group(
        p,
        row![
            button(text("−").size(type_scale::CAPTION))
                .padding([2, 7])
                .style(theme::segmented_btn(p, false))
                .on_press(ImageDiffMsg::SetZoom(st.zoom.step_out())),
            container(
                text(zoom_pct_label)
                    .size(type_scale::CAPTION)
                    .font(theme::CODE_FONT)
                    .color(p.text_secondary)
                    .wrapping(text::Wrapping::None),
            )
            .padding([2, 6]),
            button(text("+").size(type_scale::CAPTION))
                .padding([2, 7])
                .style(theme::segmented_btn(p, false))
                .on_press(ImageDiffMsg::SetZoom(st.zoom.step_in())),
            button(text("Fit").size(type_scale::CAPTION))
                .padding([2, 8])
                .style(theme::segmented_btn(p, is_fit))
                .on_press(ImageDiffMsg::SetZoom(ImageDiffZoom::Fit)),
            button(text("100%").size(type_scale::CAPTION))
                .padding([2, 8])
                .style(theme::segmented_btn(p, is_100))
                .on_press(ImageDiffMsg::SetZoom(ImageDiffZoom::Scale(1.0))),
        ]
        .spacing(2)
        .align_y(Alignment::Center),
    );

    toolbar = toolbar.push(Space::with_width(Length::Fill)).push(zoom_group);

    // 2. Dual metadata bar (Before | After)
    let badge_tag = if data.is_svg { "SVG" } else { "IMAGE" };
    let meta_bar = container(
        row![
            meta_side_strip(p, badge_tag, file_name, left_ref, left_side, "(no before image)"),
            container(Space::new(1, 16)).style(move |_| container::Style {
                background: Some(Background::Color(p.border_default)),
                ..Default::default()
            }),
            meta_side_strip(p, badge_tag, file_name, right_ref, right_side, "(deleted)"),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding([5, 10])
    .width(Length::Fill)
    .style(theme::section_header_box(p));

    // 3. Comparison Stage
    let wrap_zoom = |canvas: ImageCompareCanvas| -> Element<'a, ImageDiffMsg> {
        let el: Element<'a, ImageDiffMsg> = Element::new(canvas);
        match st.zoom {
            ImageDiffZoom::Fit => el,
            ImageDiffZoom::Scale(_) => scrollable(el)
                .width(Length::Fill)
                .height(Length::Fill)
                .direction(theme::slim_both_direction())
                .style(theme::slim_scrollable(p))
                .into(),
        }
    };

    let stage_el: Element<'a, ImageDiffMsg> = if let Some(err) = &data.load_error
        && left_side.is_none()
        && right_side.is_none()
    {
        theme::empty_state(
            p,
            icons::alert_triangle(theme::icon_size::LG, p.danger_fg),
            "Unable to decode image",
            err.clone(),
            None,
        )
    } else {
        match st.mode {
            ImageDiffMode::TwoUp => {
                let left_canvas = wrap_zoom(ImageCompareCanvas {
                    kind: CompareRenderKind::Single {
                        side: left_side.cloned(),
                        empty_label: "No Before image (new file)".into(),
                    },
                    zoom: st.zoom,
                    p,
                    dark_mode,
                    left_corner_badge: Some("BEFORE"),
                    right_corner_badge: None,
                });
                let right_canvas = wrap_zoom(ImageCompareCanvas {
                    kind: CompareRenderKind::Single {
                        side: right_side.cloned(),
                        empty_label: "No After image (deleted)".into(),
                    },
                    zoom: st.zoom,
                    p,
                    dark_mode,
                    left_corner_badge: Some("AFTER"),
                    right_corner_badge: None,
                });
                row![
                    container(left_canvas)
                        .width(Length::FillPortion(1))
                        .height(Length::Fill),
                    container(right_canvas)
                        .width(Length::FillPortion(1))
                        .height(Length::Fill),
                ]
                .spacing(6)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            }
            ImageDiffMode::Swipe => wrap_zoom(ImageCompareCanvas {
                kind: CompareRenderKind::Swipe {
                    before: left_side.cloned(),
                    after: right_side.cloned(),
                    pos: st.swipe,
                },
                zoom: st.zoom,
                p,
                dark_mode,
                left_corner_badge: Some("BEFORE"),
                right_corner_badge: Some("AFTER"),
            }),
            ImageDiffMode::Onion => wrap_zoom(ImageCompareCanvas {
                kind: CompareRenderKind::Onion {
                    before: left_side.cloned(),
                    after: right_side.cloned(),
                    opacity: st.onion,
                },
                zoom: st.zoom,
                p,
                dark_mode,
                left_corner_badge: Some("BEFORE"),
                right_corner_badge: Some("ONION"),
            }),
            ImageDiffMode::Blink => wrap_zoom(ImageCompareCanvas {
                kind: CompareRenderKind::Blink {
                    before: left_side.cloned(),
                    after: right_side.cloned(),
                    show_after: st.blink_after,
                },
                zoom: st.zoom,
                p,
                dark_mode,
                left_corner_badge: Some(if st.blink_after { "AFTER" } else { "BEFORE" }),
                right_corner_badge: Some("CLICK TO FLIP"),
            }),
            ImageDiffMode::PixelDiff => wrap_zoom(ImageCompareCanvas {
                kind: CompareRenderKind::PixelDiff {
                    handle: data.diff_handle.clone(),
                    width: data.diff_width,
                    height: data.diff_height,
                },
                zoom: st.zoom,
                p,
                dark_mode,
                left_corner_badge: Some("PIXEL DIFF"),
                right_corner_badge: None,
            }),
        }
    };

    column![
        toolbar,
        meta_bar,
        horizontal_rule(1).style(theme::subtle_rule(p)),
        container(stage_el).width(Length::Fill).height(Length::Fill),
    ]
    .spacing(5)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

impl GitFleetApp {
    pub(crate) fn view_image_diff(&self, p: Palette) -> Element<'_, Message> {
        let st = ImageDiffViewState {
            mode: self.image_diff_mode,
            zoom: self.image_diff_zoom,
            swapped: self.image_diff_swapped,
            swipe: self.image_diff_swipe,
            onion: self.image_diff_onion,
            blink_after: self.image_diff_blink_after,
            blink_paused: self.image_diff_blink_paused,
        };
        render_image_diff(self.diff_image.as_ref(), st, p, self.theme_mode).map(|m| match m {
            ImageDiffMsg::SetMode(mode) => Message::SetImageDiffMode(mode),
            ImageDiffMsg::SetZoom(zoom) => Message::SetImageDiffZoom(zoom),
            ImageDiffMsg::ToggleSwap => Message::ToggleImageDiffSwap,
            ImageDiffMsg::SwipeChanged(pos) => Message::ImageDiffSwipeChanged(pos),
            ImageDiffMsg::OnionChanged(a) => Message::ImageDiffOnionChanged(a),
            ImageDiffMsg::BlinkFlip => Message::ImageDiffBlinkFlip,
            ImageDiffMsg::ToggleBlinkPause => Message::ToggleImageDiffBlinkPause,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_solid_png(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        let mut img = image::RgbaImage::new(w, h);
        for p in img.pixels_mut() {
            *p = image::Rgba(rgba);
        }
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut buf, image::ImageOutputFormat::Png)
            .unwrap();
        buf.into_inner()
    }

    #[test]
    fn recognizes_raster_and_svg_extensions() {
        assert!(is_image_diff_path("assets/hero.png"));
        assert!(is_image_diff_path("photos/IMG.JPEG"));
        assert!(is_image_diff_path("icons/logo.svg"));
        assert!(is_svg_path("icons/logo.svg"));
        assert!(!is_svg_path("assets/hero.png"));
        assert!(!is_image_diff_path("data/inventory.csv"));
        assert!(!is_image_diff_path("src/main.rs"));
    }

    #[test]
    fn decodes_png_pair_and_counts_changed_pixels_accurately() {
        let before_png = make_solid_png(4, 4, [20, 40, 60, 255]);
        let mut after_img = image::RgbaImage::from_pixel(4, 4, image::Rgba([20, 40, 60, 255]));
        after_img.put_pixel(1, 1, image::Rgba([250, 20, 20, 255]));
        after_img.put_pixel(2, 2, image::Rgba([20, 250, 20, 255]));
        let mut after_buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(after_img)
            .write_to(&mut after_buf, image::ImageOutputFormat::Png)
            .unwrap();

        let data = decode_image_diff_pair(FileVersionPair {
            file_path: "hero.png".into(),
            before_label: "HEAD".into(),
            after_label: "Working Tree".into(),
            before_bytes: Some(before_png),
            after_bytes: Some(after_buf.into_inner()),
        });

        assert!(data.load_error.is_none());
        assert_eq!(data.before.as_ref().unwrap().width, 4);
        assert_eq!(data.after.as_ref().unwrap().height, 4);
        assert_eq!(data.diff_total_pixels, 16);
        assert_eq!(data.diff_changed_pixels, 2);
        assert!(data.diff_handle.is_some());
    }

    #[test]
    fn rasterizes_and_diffs_svg_documents() {
        let svg_a = br##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" fill="#2244cc"/></svg>"##;
        let svg_b = br##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" fill="#cc2244"/></svg>"##;

        let data = decode_image_diff_pair(FileVersionPair {
            file_path: "logo.svg".into(),
            before_label: "HEAD".into(),
            after_label: "Working Tree".into(),
            before_bytes: Some(svg_a.to_vec()),
            after_bytes: Some(svg_b.to_vec()),
        });

        assert!(data.is_svg);
        assert!(data.load_error.is_none());
        assert_eq!(data.before.as_ref().unwrap().width, 32);
        assert!(data.diff_changed_pixels > 0);
    }
}
