use crate::terminal::fonts::{cell_metrics, intern_font_name, terminal_font};
use gitfleet_core::SessionId;
use gitfleet_terminal::{TermColor, TermScreenSnapshot};
use iced::alignment;
use iced::mouse;
use iced::widget::canvas::{self, Canvas, Geometry, Path, Text as CanvasText};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, event};
// ── ANSI 16 / 256 / TrueColor Palette (Windows Terminal Campbell / Dark & Light) ─

const ANSI_16_DARK: [Color; 16] = [
    Color::from_rgb(0.047, 0.047, 0.047), // 0 Black (#0c0c0c)
    Color::from_rgb(0.773, 0.059, 0.122), // 1 Red (#c50f1f)
    Color::from_rgb(0.075, 0.631, 0.055), // 2 Green (#13a10e)
    Color::from_rgb(0.757, 0.612, 0.000), // 3 Yellow (#c19c00)
    Color::from_rgb(0.000, 0.216, 0.855), // 4 Blue (#0037da)
    Color::from_rgb(0.533, 0.090, 0.596), // 5 Magenta (#881798)
    Color::from_rgb(0.227, 0.588, 0.867), // 6 Cyan (#3a96dd)
    Color::from_rgb(0.800, 0.800, 0.800), // 7 White (#cccccc)
    Color::from_rgb(0.463, 0.463, 0.463), // 8 Bright Black (#767676)
    Color::from_rgb(0.906, 0.282, 0.337), // 9 Bright Red (#e74856)
    Color::from_rgb(0.086, 0.776, 0.047), // 10 Bright Green (#16c60c)
    Color::from_rgb(0.976, 0.945, 0.647), // 11 Bright Yellow (#f9f1a5)
    Color::from_rgb(0.231, 0.471, 1.000), // 12 Bright Blue (#3b78ff)
    Color::from_rgb(0.706, 0.000, 0.620), // 13 Bright Magenta (#b4009e)
    Color::from_rgb(0.380, 0.839, 0.839), // 14 Bright Cyan (#61d6d6)
    Color::from_rgb(0.949, 0.949, 0.949), // 15 Bright White (#f2f2f2)
];

const ANSI_16_LIGHT: [Color; 16] = [
    Color::from_rgb(0.102, 0.122, 0.149), // 0
    Color::from_rgb(0.753, 0.176, 0.165), // 1
    Color::from_rgb(0.114, 0.478, 0.255), // 2
    Color::from_rgb(0.650, 0.450, 0.020), // 3
    Color::from_rgb(0.100, 0.380, 0.780), // 4
    Color::from_rgb(0.550, 0.180, 0.620), // 5
    Color::from_rgb(0.080, 0.500, 0.620), // 6
    Color::from_rgb(0.420, 0.460, 0.500), // 7
    Color::from_rgb(0.360, 0.400, 0.450), // 8
    Color::from_rgb(0.840, 0.220, 0.240), // 9
    Color::from_rgb(0.120, 0.580, 0.300), // 10
    Color::from_rgb(0.760, 0.530, 0.040), // 11
    Color::from_rgb(0.150, 0.460, 0.880), // 12
    Color::from_rgb(0.680, 0.200, 0.720), // 13
    Color::from_rgb(0.060, 0.590, 0.700), // 14
    Color::from_rgb(0.150, 0.180, 0.220), // 15
];

fn resolve_term_color(c: TermColor, default: Color, is_dark: bool, bold_bright: bool) -> Color {
    let base16 = if is_dark { &ANSI_16_DARK } else { &ANSI_16_LIGHT };
    match c {
        TermColor::Default => default,
        TermColor::Rgb(r, g, b) => Color::from_rgb8(r, g, b),
        TermColor::Indexed(idx) => {
            let i = if bold_bright && idx < 8 { idx + 8 } else { idx };
            if i < 16 {
                base16[i as usize]
            } else if i < 232 {
                let n = i - 16;
                let r = n / 36;
                let g = (n % 36) / 6;
                let b = n % 6;
                let to_u8 = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
                Color::from_rgb8(to_u8(r), to_u8(g), to_u8(b))
            } else {
                let gray = 8 + (i - 232) * 10;
                Color::from_rgb8(gray, gray, gray)
            }
        }
    }
}


#[derive(Debug, Clone)]
pub enum TerminalCanvasAction {
    Focus(SessionId),
    Resize(SessionId, u16, u16),
    Scroll(SessionId, isize),
}

#[derive(Debug, Default)]
pub struct TerminalCanvasState {
    last_cols: u16,
    last_rows: u16,
}

pub struct TerminalCanvasProgram<'a> {
    pub session_id: SessionId,
    pub snapshot: &'a TermScreenSnapshot,
    pub font_name: &'static str,
    pub font_size: f32,
    pub is_dark: bool,
    pub focused: bool,
    pub default_bg: Color,
    pub default_fg: Color,
    pub accent: Color,
}

impl<'a, M> canvas::Program<M> for TerminalCanvasProgram<'a>
where
    M: From<TerminalCanvasAction>,
{
    type State = TerminalCanvasState;

    fn update(
        &self,
        state: &mut Self::State,
        ev: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (event::Status, Option<M>) {
        let (cell_w, cell_h) = cell_metrics(self.font_name, self.font_size);
        let pad_x = 8.0_f32;
        let pad_y = 6.0_f32;
        let inner_w = (bounds.width - pad_x * 2.0).max(cell_w * 30.0);
        let inner_h = (bounds.height - pad_y * 2.0).max(cell_h * 6.0);
        let cols = (inner_w / cell_w).floor().clamp(30.0, 400.0) as u16;
        let rows = (inner_h / cell_h).floor().clamp(6.0, 120.0) as u16;

        match ev {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if cursor.is_over(bounds) {
                    return (
                        event::Status::Captured,
                        Some(TerminalCanvasAction::Focus(self.session_id.clone()).into()),
                    );
                }
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if cursor.is_over(bounds) {
                    let lines: isize = match delta {
                        mouse::ScrollDelta::Lines { y, .. } => (y * 3.0).round() as isize,
                        mouse::ScrollDelta::Pixels { y, .. } => (y / cell_h).round() as isize,
                    };
                    if lines != 0 {
                        return (
                            event::Status::Captured,
                            Some(
                                TerminalCanvasAction::Scroll(self.session_id.clone(), lines).into(),
                            ),
                        );
                    }
                }
            }
            _ => {}
        }

        if (cols != state.last_cols || rows != state.last_rows)
            && (cols != self.snapshot.cols || rows != self.snapshot.rows)
        {
            state.last_cols = cols;
            state.last_rows = rows;
            return (
                event::Status::Ignored,
                Some(TerminalCanvasAction::Resize(self.session_id.clone(), cols, rows).into()),
            );
        }
        state.last_cols = cols;
        state.last_rows = rows;

        (event::Status::Ignored, None)
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), self.default_bg);

        let (cell_w, cell_h) = cell_metrics(self.font_name, self.font_size);
        let pad_x = 8.0_f32;
        let pad_y = 6.0_f32;

        let font_regular = terminal_font(self.font_name, false, false);
        let font_bold = terminal_font(self.font_name, true, false);
        let font_italic = terminal_font(self.font_name, false, true);
        let font_bold_italic = terminal_font(self.font_name, true, true);

        let name_lower = self.font_name.to_lowercase();
        let has_nerd_font = name_lower.contains("nerd")
            || name_lower.ends_with(" nf")
            || name_lower.contains(" nf ")
            || name_lower.contains("nfm");

        for (row_idx, row) in self.snapshot.lines.iter().enumerate() {
            let y = pad_y + (row_idx as f32) * cell_h;
            if y > bounds.height {
                break;
            }

            // Pass 1: colored cell backgrounds
            for run in &row.runs {
                let raw_fg = resolve_term_color(run.fg, self.default_fg, self.is_dark, run.bold);
                let raw_bg = resolve_term_color(run.bg, self.default_bg, self.is_dark, false);
                let bg = if run.inverse { raw_fg } else { raw_bg };
                if bg != self.default_bg {
                    let x = pad_x + (run.col as f32) * cell_w;
                    let w = (run.cell_width as f32) * cell_w + 0.6;
                    frame.fill_rectangle(Point::new(x, y), Size::new(w, cell_h + 0.4), bg);
                }
            }

            // Pass 2: foreground glyphs & text runs
            for run in &row.runs {
                let raw_fg = resolve_term_color(run.fg, self.default_fg, self.is_dark, run.bold);
                let raw_bg = resolve_term_color(run.bg, self.default_bg, self.is_dark, false);
                let fg = if run.inverse { raw_bg } else { raw_fg };
                let x = pad_x + (run.col as f32) * cell_w;

                // Vector fallback for Powerline separators only if user selected a non-Nerd font
                if !has_nerd_font
                    && run.is_glyph
                    && draw_powerline_fallback(&mut frame, &run.text, x, y, cell_w, cell_h, fg)
                {
                    continue;
                }

                if !run.text.trim().is_empty() {
                    let content = sanitize_canvas_text(&run.text);
                    let font = match (run.bold, run.italic) {
                        (false, false) => font_regular,
                        (true, false) => font_bold,
                        (false, true) => font_italic,
                        (true, true) => font_bold_italic,
                    };
                    frame.fill_text(CanvasText {
                        content,
                        position: Point::new(x, y + cell_h * 0.5),
                        color: fg,
                        size: iced::Pixels(self.font_size),
                        line_height: iced::widget::text::LineHeight::Absolute(iced::Pixels(cell_h)),
                        font,
                        horizontal_alignment: alignment::Horizontal::Left,
                        vertical_alignment: alignment::Vertical::Center,
                        shaping: iced::widget::text::Shaping::Advanced,
                    });
                }

                if run.underline {
                    let u_w = (run.cell_width as f32) * cell_w;
                    frame.fill_rectangle(
                        Point::new(x, y + cell_h - 1.5),
                        Size::new(u_w, 1.0),
                        fg,
                    );
                }
            }
        }

        // Cursor bar / block
        if self.snapshot.cursor_visible {
            let cx = pad_x + (self.snapshot.cursor_col as f32) * cell_w;
            let cy = pad_y + (self.snapshot.cursor_row as f32) * cell_h;
            if cy + cell_h <= bounds.height + 2.0 {
                if self.focused {
                    frame.fill_rectangle(
                        Point::new(cx, cy + 1.0),
                        Size::new(2.0, cell_h - 2.0),
                        self.accent,
                    );
                } else {
                    let outline = Path::rectangle(
                        Point::new(cx, cy + 1.0),
                        Size::new(cell_w, cell_h - 2.0),
                    );
                    frame.stroke(
                        &outline,
                        canvas::Stroke::default()
                            .with_width(1.0)
                            .with_color(Color { a: 0.55, ..self.accent }),
                    );
                }
            }
        }

        // Scrollback indicator pill at top-right when scrolled up
        if self.snapshot.scrollback_offset > 0 {
            let label = format!("Scrollback +{}", self.snapshot.scrollback_offset);
            let pill_w = 104.0;
            let pill_h = 18.0;
            let px = (bounds.width - pill_w - 8.0).max(4.0);
            let py = 6.0;
            frame.fill_rectangle(
                Point::new(px, py),
                Size::new(pill_w, pill_h),
                Color::from_rgba(0.15, 0.45, 0.80, 0.85),
            );
            frame.fill_text(CanvasText {
                content: label,
                position: Point::new(px + pill_w * 0.5, py + pill_h * 0.5),
                color: Color::WHITE,
                size: iced::Pixels(10.0),
                font: font_regular,
                horizontal_alignment: alignment::Horizontal::Center,
                vertical_alignment: alignment::Vertical::Center,
                shaping: iced::widget::text::Shaping::Basic,
                ..CanvasText::default()
            });
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }
}

fn draw_powerline_fallback(
    frame: &mut canvas::Frame,
    glyph: &str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Color,
) -> bool {
    match glyph {
        "\u{e0b0}" => {
            // Right solid arrow
            let p = Path::new(|b| {
                b.move_to(Point::new(x, y));
                b.line_to(Point::new(x + w, y + h * 0.5));
                b.line_to(Point::new(x, y + h));
                b.close();
            });
            frame.fill(&p, color);
            true
        }
        "\u{e0b2}" => {
            // Left solid arrow
            let p = Path::new(|b| {
                b.move_to(Point::new(x + w, y));
                b.line_to(Point::new(x, y + h * 0.5));
                b.line_to(Point::new(x + w, y + h));
                b.close();
            });
            frame.fill(&p, color);
            true
        }
        "\u{e0b4}" => {
            // Right rounded pill cap
            let r = h * 0.5;
            let p = Path::new(|b| {
                b.move_to(Point::new(x, y));
                b.quadratic_curve_to(Point::new(x + w, y), Point::new(x + w, y + r));
                b.quadratic_curve_to(Point::new(x + w, y + h), Point::new(x, y + h));
                b.close();
            });
            frame.fill(&p, color);
            true
        }
        "\u{e0b6}" => {
            // Left rounded pill cap
            let r = h * 0.5;
            let p = Path::new(|b| {
                b.move_to(Point::new(x + w, y));
                b.quadratic_curve_to(Point::new(x, y), Point::new(x, y + r));
                b.quadratic_curve_to(Point::new(x, y + h), Point::new(x + w, y + h));
                b.close();
            });
            frame.fill(&p, color);
            true
        }
        _ => false,
    }
}

/// Prepares a terminal cell/run string for Iced 0.13's `Canvas::fill_text`.
///
/// Iced 0.13's `Canvas::fill_text` passes `bounds = Size::INFINITY` into `cosmic-text 0.12.1`.
/// When a run's first strong Unicode BiDi character is RTL — which includes Arabic/Hebrew text
/// AND legacy Nerd Font v2 Material Design icons (`U+FB50..=U+FDFF`, e.g. `U+FC32` in Oh My Posh)
/// mapped into the Arabic Presentation Forms-A block — `cosmic-text` sets
/// `start_x = line_width = f32::INFINITY`, causing `SubpixelBin::new` to panic with
/// `attempt to add with overflow`.
///
/// Embedded control/separator characters (`\n`, `\r`, `\t`, `U+0085`, `U+2028`, `U+2029`) can
/// also trigger `cosmic-text` multi-paragraph BiDi assertions or zero-advance tab division.
///
/// Replacing control/separator characters with spaces and prepending `U+200E` (LEFT-TO-RIGHT MARK,
/// a zero-width strong `L` default-ignorable codepoint) for non-ASCII runs forces LTR layout
/// (`start_x = 0.0`) with zero visual offset.
fn sanitize_canvas_text(raw: &str) -> String {
    let has_non_ascii = !raw.is_ascii();
    let has_control = raw.bytes().any(|b| b < 0x20 || b == 0x7f);
    if !has_non_ascii && !has_control {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len() + if has_non_ascii { 3 } else { 0 });
    if has_non_ascii {
        out.push('\u{200E}');
    }
    for ch in raw.chars() {
        if ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}') {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn terminal_pane_canvas<'a, M>(
    session_id: SessionId,
    snapshot: &'a TermScreenSnapshot,
    font_name: &str,
    font_size: f32,
    is_dark: bool,
    focused: bool,
    default_bg: Color,
    default_fg: Color,
    accent: Color,
) -> Element<'a, M>
where
    M: From<TerminalCanvasAction> + 'a,
{
    Canvas::new(TerminalCanvasProgram {
        session_id,
        snapshot,
        font_name: intern_font_name(font_name),
        font_size,
        is_dark,
        focused,
        default_bg,
        default_fg,
        accent,
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::fonts::detect_terminal_fonts;
    use iced::advanced::graphics::text::{self as gfx_text, cache as text_cache};

    #[test]
    fn sanitize_canvas_text_prevents_cosmic_text_rtl_infinity_overflow() {
        let catalog = detect_terminal_fonts();
        let font_name = intern_font_name(&catalog.default_font);
        let font_size = 13.0_f32;
        let (_cell_w, cell_h) = cell_metrics(font_name, font_size);
        let font = terminal_font(font_name, false, false);

        let samples = [
            "quach",
            "\u{fc32}",             // Nerd Font v2 mdi arrow in Arabic Presentation Forms-A (AL/RTL)
            "\u{fb50}",             // Arabic Presentation Forms-A start
            "\u{fd46}",             // Nerd Font v2 end
            "\u{fe70}",             // Arabic Presentation Forms-B
            "\u{e0b0}",             // Powerline arrow
            "\u{e0b4}",             // Powerline rounded cap
            "\u{e0b6}",             // Powerline left rounded cap
            "\u{f0054}",            // Nerd Font v3 Material Design icon
            "مرحبا",                // Arabic RTL text
            "שלום",                 // Hebrew RTL text
            "\u{202E}abc",          // Right-to-Left Override
            "a\nb\rc\td\u{0085}e\u{2028}f\u{2029}g", // Control & paragraph/line separators
            "╭─╰",                  // Box drawing
            "Tiếng Việt 🔥 中文",   // Unicode + emoji + CJK
        ];

        let mut font_system = gfx_text::font_system().write().unwrap();
        let mut cache = text_cache::Cache::new();

        for sample in samples {
            let safe = sanitize_canvas_text(sample);
            let (_key, entry) = cache.allocate(
                font_system.raw(),
                text_cache::Key {
                    content: &safe,
                    size: font_size,
                    line_height: cell_h,
                    font,
                    bounds: Size::INFINITY,
                    shaping: iced::widget::text::Shaping::Advanced,
                },
            );
            assert!(
                entry.min_bounds.width.is_finite() && entry.min_bounds.height.is_finite(),
                "non-finite min_bounds for sample {sample:?}: {:?}",
                entry.min_bounds
            );
            for lrun in entry.buffer.layout_runs() {
                for g in lrun.glyphs.iter() {
                    assert!(
                        g.x.is_finite() && g.y.is_finite(),
                        "non-finite glyph coords for sample {sample:?}: x={}, y={}",
                        g.x,
                        g.y
                    );
                    let _pg = g.physical((100.0, 20.0), 1.0);
                }
            }
        }
        assert_eq!(sanitize_canvas_text("quach"), "quach");
        assert!(sanitize_canvas_text("\u{fc32}").starts_with('\u{200E}'));
        assert!(!sanitize_canvas_text("a\nb\rc\td").contains(['\n', '\r', '\t']));
    }
}
