//! Low-level diff UI primitives: `DiffLineBox` row widget with drag-to-select,
//! cubic Bezier center connector canvas, edge overview minimap, hidden gap expander,
//! and syntax + inline-diff code span compositor.

use super::inline_diff::InlineSpan;
use super::layout::ConnectorSpan;
use super::selection::{col_to_x, diff_code_char_width, x_to_col};
use super::{DIFF_EXPAND_STEP, DIFF_HIDDEN_H, DIFF_LINE_H};
use crate::app::{DiffGapExpandMode, DiffSelectionSide, Message};
use crate::icons;
use crate::syntax::TokenFormat;
use crate::theme::{self, Palette, type_scale};
use gitfleet_git::DiffChangeKind;
use iced::advanced::layout as adv_layout;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, overlay, renderer};
use iced::widget::canvas::{self, Geometry, Path as CanvasPath, Stroke};
use iced::widget::{button, container, rich_text, row, span, text};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Point, Rectangle, Renderer, Shadow,
    Size, Theme, Vector, alignment, event, mouse,
};
use std::ops::Range;

pub(crate) fn change_colors(kind: DiffChangeKind, p: Palette) -> (Color, Color, Color) {
    // Returns (soft_bg, accent_border, text_fg)
    match kind {
        DiffChangeKind::Added => (p.success_bg, p.success_fg, p.text_primary),
        DiffChangeKind::Removed => (p.danger_bg, p.danger_fg, p.text_primary),
        DiffChangeKind::Modified => (p.warning_bg, p.warning_fg, p.text_primary),
    }
}

pub(crate) struct DiffRowSelConfig {
    pub(crate) side: DiffSelectionSide,
    pub(crate) row_idx: usize,
    pub(crate) code_start_x: f32,
    pub(crate) line_text: String,
    pub(crate) sel_range: Option<(usize, usize, bool)>,
    pub(crate) is_dragging_same_side: bool,
    pub(crate) sel_bg: Color,
    pub(crate) sel_overlay: Color,
}

/// Single fixed-height diff row widget that paints selective top/bottom borders, 2px SmartGit
/// insertion/deletion seam hairlines, and character-accurate drag-to-select highlights.
#[allow(clippy::too_many_arguments)]
pub(crate) fn diff_code_row_box<'a>(
    bg: Color,
    top_border: Option<Color>,
    bottom_border: Option<Color>,
    top_seam: Option<Color>,
    bottom_seam: Option<Color>,
    gutter_x: f32,
    gutter_border: Color,
    sel: DiffRowSelConfig,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    struct DiffLineBox<'a> {
        bg: Color,
        top_border: Option<Color>,
        bottom_border: Option<Color>,
        top_seam: Option<Color>,
        bottom_seam: Option<Color>,
        gutter_x: f32,
        gutter_border: Color,
        sel: DiffRowSelConfig,
        content: Element<'a, Message>,
    }

    impl<'a> Widget<Message, Theme, Renderer> for DiffLineBox<'a> {
        fn tag(&self) -> tree::Tag {
            tree::Tag::stateless()
        }

        fn state(&self) -> tree::State {
            tree::State::None
        }

        fn children(&self) -> Vec<Tree> {
            vec![Tree::new(&self.content)]
        }

        fn diff(&self, tree: &mut Tree) {
            tree.diff_children(std::slice::from_ref(&self.content));
        }

        fn size(&self) -> Size<Length> {
            Size::new(Length::Fill, Length::Fixed(DIFF_LINE_H))
        }

        fn layout(
            &self,
            tree: &mut Tree,
            renderer: &Renderer,
            limits: &adv_layout::Limits,
        ) -> adv_layout::Node {
            let max_w = if limits.max().width.is_finite() {
                limits.max().width
            } else {
                4096.0
            };
            let child_limits = adv_layout::Limits::new(
                Size::new(max_w, DIFF_LINE_H),
                Size::new(max_w, DIFF_LINE_H),
            );
            let child = self
                .content
                .as_widget()
                .layout(&mut tree.children[0], renderer, &child_limits);
            adv_layout::Node::with_children(Size::new(max_w, DIFF_LINE_H), vec![child])
        }

        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut Renderer,
            theme: &Theme,
            style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            use iced::advanced::Renderer as _;

            let b = layout.bounds();
            let flat_quad = |bounds: Rectangle| renderer::Quad {
                bounds,
                border: Border::default(),
                shadow: Shadow::default(),
            };

            if self.bg != Color::TRANSPARENT {
                renderer.fill_quad(flat_quad(b), Background::Color(self.bg));
            }
            if self.gutter_x > 0.0 {
                renderer.fill_quad(
                    flat_quad(Rectangle {
                        x: b.x + self.gutter_x,
                        y: b.y,
                        width: 1.0,
                        height: b.height,
                    }),
                    Background::Color(self.gutter_border),
                );
            }
            if let Some(c) = self.top_border {
                renderer.fill_quad(
                    flat_quad(Rectangle {
                        x: b.x,
                        y: b.y,
                        width: b.width,
                        height: 1.0,
                    }),
                    Background::Color(Color { a: 0.72, ..c }),
                );
            }
            if let Some(c) = self.bottom_border {
                renderer.fill_quad(
                    flat_quad(Rectangle {
                        x: b.x,
                        y: b.y + b.height - 1.0,
                        width: b.width,
                        height: 1.0,
                    }),
                    Background::Color(Color { a: 0.72, ..c }),
                );
            }
            if let Some(c) = self.top_seam {
                renderer.fill_quad(
                    flat_quad(Rectangle {
                        x: b.x,
                        y: b.y,
                        width: b.width,
                        height: 2.0,
                    }),
                    Background::Color(c),
                );
            }
            if let Some(c) = self.bottom_seam {
                renderer.fill_quad(
                    flat_quad(Rectangle {
                        x: b.x,
                        y: b.y + b.height - 2.0,
                        width: b.width,
                        height: 2.0,
                    }),
                    Background::Color(c),
                );
            }

            let sel_rect = self.sel.sel_range.and_then(|(s_col, e_col, extends_eol)| {
                let char_w = diff_code_char_width();
                let x0 = b.x + self.sel.code_start_x + col_to_x(&self.sel.line_text, s_col, char_w);
                let x1 = b.x
                    + self.sel.code_start_x
                    + col_to_x(&self.sel.line_text, e_col, char_w)
                    + if extends_eol { char_w } else { 0.0 };
                let w = (x1 - x0).max(if extends_eol { char_w } else { 0.0 });
                (w > 0.5).then_some(Rectangle {
                    x: x0,
                    y: b.y,
                    width: w,
                    height: b.height,
                })
            });

            if let Some(rect) = sel_rect {
                renderer.fill_quad(flat_quad(rect), Background::Color(self.sel.sel_bg));
            }

            if let Some(child_layout) = layout.children().next() {
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    child_layout,
                    cursor,
                    viewport,
                );
            }

            if let Some(rect) = sel_rect {
                renderer.fill_quad(flat_quad(rect), Background::Color(self.sel.sel_overlay));
            }
        }

        fn on_event(
            &mut self,
            tree: &mut Tree,
            event: iced::Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &Renderer,
            clipboard: &mut dyn Clipboard,
            shell: &mut Shell<'_, Message>,
            viewport: &Rectangle,
        ) -> event::Status {
            let b = layout.bounds();
            match &event {
                iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                    if let Some(pos) = cursor.position_in(b)
                        && cursor
                            .position()
                            .is_some_and(|abs| viewport.contains(abs))
                    {
                        let char_w = diff_code_char_width();
                        let col = x_to_col(&self.sel.line_text, pos.x - self.sel.code_start_x, char_w);
                        shell.publish(Message::DiffSelStart {
                            side: self.sel.side,
                            row: self.sel.row_idx,
                            col,
                        });
                        return event::Status::Captured;
                    }
                }
                iced::Event::Mouse(mouse::Event::CursorMoved { .. })
                    if self.sel.is_dragging_same_side =>
                {
                    if let Some(abs_pos) = cursor.position()
                        && abs_pos.y >= b.y
                        && abs_pos.y < b.y + b.height
                        && abs_pos.y >= viewport.y
                        && abs_pos.y <= viewport.y + viewport.height
                    {
                        let char_w = diff_code_char_width();
                        let col = x_to_col(
                            &self.sel.line_text,
                            abs_pos.x - b.x - self.sel.code_start_x,
                            char_w,
                        );
                        shell.publish(Message::DiffSelDrag {
                            side: self.sel.side,
                            row: self.sel.row_idx,
                            col,
                        });
                    }
                }
                _ => {}
            }

            let Some(child_layout) = layout.children().next() else {
                return event::Status::Ignored;
            };
            self.content.as_widget_mut().on_event(
                &mut tree.children[0],
                event,
                child_layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            )
        }

        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &Renderer,
        ) -> mouse::Interaction {
            let b = layout.bounds();
            if let Some(pos) = cursor.position_in(b)
                && pos.x >= self.sel.code_start_x - 4.0
            {
                return mouse::Interaction::Text;
            }
            let Some(child_layout) = layout.children().next() else {
                return mouse::Interaction::None;
            };
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                child_layout,
                cursor,
                viewport,
                renderer,
            )
        }

        fn operate(
            &self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            operation: &mut dyn iced::advanced::widget::Operation,
        ) {
            if let Some(child_layout) = layout.children().next() {
                self.content.as_widget().operate(
                    &mut tree.children[0],
                    child_layout,
                    renderer,
                    operation,
                );
            }
        }

        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            translation: Vector,
        ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
            let child_layout = layout.children().next()?;
            self.content.as_widget_mut().overlay(
                &mut tree.children[0],
                child_layout,
                renderer,
                translation,
            )
        }
    }

    Element::new(DiffLineBox {
        bg,
        top_border,
        bottom_border,
        top_seam,
        bottom_seam,
        gutter_x,
        gutter_border,
        sel,
        content: content.into(),
    })
}

/// Center Bezier connector strip connecting Left (Old) and Right (New) diff blocks like SmartGit.
pub(crate) struct DiffConnectorCanvas {
    pub(crate) spans: Vec<ConnectorSpan>,
    pub(crate) left_scroll_y: f32,
    pub(crate) right_scroll_y: f32,
    pub(crate) p: Palette,
}

impl canvas::Program<Message> for DiffConnectorCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;

        frame.fill_rectangle(Point::ORIGIN, bounds.size(), self.p.surface_titlebar);
        frame.fill_rectangle(
            Point::new(0.0, 0.0),
            Size::new(1.0, h),
            self.p.border_subtle,
        );
        frame.fill_rectangle(
            Point::new((w - 1.0).max(0.0), 0.0),
            Size::new(1.0, h),
            self.p.border_subtle,
        );

        let mid_x = w * 0.5;
        for span in &self.spans {
            let mut ly0 = span.left_top - self.left_scroll_y;
            let mut ly1 = span.left_bot - self.left_scroll_y;
            let mut ry0 = span.right_top - self.right_scroll_y;
            let mut ry1 = span.right_bot - self.right_scroll_y;

            if ly1.max(ry1) < -6.0 || ly0.min(ry0) > h + 6.0 {
                continue;
            }
            if (ly1 - ly0).abs() < 2.0 {
                ly0 -= 1.0;
                ly1 += 1.0;
            }
            if (ry1 - ry0).abs() < 2.0 {
                ry0 -= 1.0;
                ry1 += 1.0;
            }

            let (_, accent, _) = change_colors(span.kind, self.p);
            let fill = Color { a: 0.24, ..accent };
            let stroke_color = Color { a: 0.78, ..accent };

            let ribbon = CanvasPath::new(|b| {
                b.move_to(Point::new(0.0, ly0));
                b.bezier_curve_to(
                    Point::new(mid_x, ly0),
                    Point::new(mid_x, ry0),
                    Point::new(w, ry0),
                );
                b.line_to(Point::new(w, ry1));
                b.bezier_curve_to(
                    Point::new(mid_x, ry1),
                    Point::new(mid_x, ly1),
                    Point::new(0.0, ly1),
                );
                b.close();
            });
            frame.fill(&ribbon, fill);

            let top_edge = CanvasPath::new(|b| {
                b.move_to(Point::new(0.0, ly0));
                b.bezier_curve_to(
                    Point::new(mid_x, ly0),
                    Point::new(mid_x, ry0),
                    Point::new(w, ry0),
                );
            });
            let bot_edge = CanvasPath::new(|b| {
                b.move_to(Point::new(0.0, ly1));
                b.bezier_curve_to(
                    Point::new(mid_x, ly1),
                    Point::new(mid_x, ry1),
                    Point::new(w, ry1),
                );
            });
            let stroke = Stroke::default().with_color(stroke_color).with_width(1.2);
            frame.stroke(&top_edge, stroke);
            frame.stroke(&bot_edge, stroke);
        }

        vec![frame.into_geometry()]
    }
}

/// Thin SmartGit edge minimap strip highlighting where added/removed/modified blocks live.
pub(crate) struct DiffOverviewBar {
    pub(crate) spans: Vec<ConnectorSpan>,
    pub(crate) is_left: bool,
    pub(crate) total_h: f32,
    pub(crate) p: Palette,
}

impl canvas::Program<Message> for DiffOverviewBar {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), self.p.surface_titlebar);
        if self.total_h <= 1.0 || bounds.height <= 1.0 {
            return vec![frame.into_geometry()];
        }
        let scale = bounds.height / self.total_h;
        for span in &self.spans {
            let (top, bot) = if self.is_left {
                (span.left_top, span.left_bot)
            } else {
                (span.right_top, span.right_bot)
            };
            let (_, accent, _) = change_colors(span.kind, self.p);
            let y = (top * scale).clamp(0.0, (bounds.height - 3.0).max(0.0));
            let h = ((bot - top) * scale).max(3.0).min(bounds.height - y);
            frame.fill_rectangle(
                Point::new(1.0, y),
                Size::new((bounds.width - 2.0).max(2.0), h),
                accent,
            );
        }
        vec![frame.into_geometry()]
    }
}

pub(crate) fn hidden_gap_bar<'a>(
    gap_id: usize,
    hidden_count: usize,
    can_expand_top: bool,
    can_expand_bottom: bool,
    p: Palette,
) -> Element<'a, Message> {
    let mut bar = row![
        icons::chevron_up(11.0, p.text_muted),
        text(format!(
            "{hidden_count} {} hidden",
            if hidden_count == 1 { "line" } else { "lines" }
        ))
        .size(type_scale::CAPTION)
        .color(p.text_muted)
        .wrapping(text::Wrapping::None),
    ]
    .spacing(5)
    .align_y(Alignment::Center);

    if hidden_count > DIFF_EXPAND_STEP && can_expand_top {
        bar = bar.push(
            button(
                row![
                    icons::chevron_down(10.0, p.accent_fg),
                    text(format!("+{DIFF_EXPAND_STEP}"))
                        .size(type_scale::BADGE)
                        .color(p.accent_fg),
                ]
                .spacing(2)
                .align_y(Alignment::Center),
            )
            .padding([1, 6])
            .style(theme::subtle_btn(p))
            .on_press(Message::ExpandDiffGap {
                gap_id,
                mode: DiffGapExpandMode::Top(DIFF_EXPAND_STEP),
            }),
        );
    }

    bar = bar.push(
        button(
            row![
                icons::chevron_up(10.0, p.text_secondary),
                text("show").size(type_scale::BADGE).color(p.text_primary),
                icons::chevron_down(10.0, p.text_secondary),
            ]
            .spacing(3)
            .align_y(Alignment::Center),
        )
        .padding([1, 7])
        .style(theme::subtle_btn(p))
        .on_press(Message::ExpandDiffGap {
            gap_id,
            mode: DiffGapExpandMode::All,
        }),
    );

    if hidden_count > DIFF_EXPAND_STEP && can_expand_bottom {
        bar = bar.push(
            button(
                row![
                    icons::chevron_up(10.0, p.accent_fg),
                    text(format!("+{DIFF_EXPAND_STEP}"))
                        .size(type_scale::BADGE)
                        .color(p.accent_fg),
                ]
                .spacing(2)
                .align_y(Alignment::Center),
            )
            .padding([1, 6])
            .style(theme::subtle_btn(p))
            .on_press(Message::ExpandDiffGap {
                gap_id,
                mode: DiffGapExpandMode::Bottom(DIFF_EXPAND_STEP),
            }),
        );
    }

    container(bar)
        .width(Length::Fill)
        .height(Length::Fixed(DIFF_HIDDEN_H))
        .padding([0, 10])
        .align_y(alignment::Vertical::Center)
        .style(move |_| container::Style {
            background: Some(Background::Color(p.surface_card)),
            text_color: Some(p.text_muted),
            border: Border {
                color: p.border_subtle,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

#[inline]
fn token_font(tok: Option<TokenFormat>) -> iced::Font {
    match tok {
        Some(t) if t.bold || t.italic => iced::Font {
            weight: if t.bold {
                iced::font::Weight::Bold
            } else {
                theme::CODE_FONT.weight
            },
            style: if t.italic {
                iced::font::Style::Italic
            } else {
                theme::CODE_FONT.style
            },
            ..theme::CODE_FONT
        },
        _ => theme::CODE_FONT,
    }
}

/// Renders a single line of diff code combining cached syntax highlight spans with optional
/// intra-line diff token highlight badges. Uses a zero-sort O(K) linear pass on the common
/// path (context / pure add / pure remove lines).
pub(crate) fn render_highlighted_code<'a>(
    code: &str,
    spans: &[InlineSpan],
    syntax_spans: &[(Range<usize>, TokenFormat)],
    fg: Color,
    accent: Option<Color>,
) -> Element<'a, Message> {
    let has_inline = !spans.is_empty() && spans.iter().any(|s| s.highlighted);
    if !has_inline && syntax_spans.is_empty() {
        return text(code.to_string())
            .size(type_scale::CODE)
            .font(theme::CODE_FONT)
            .color(fg)
            .wrapping(text::Wrapping::None)
            .into();
    }

    // Fast O(K) linear path when there is no intra-line diff highlight overlay
    if !has_inline {
        let mut rich_spans: Vec<text::Span<'a, Message, iced::Font>> =
            Vec::with_capacity(syntax_spans.len() * 2 + 1);
        let mut pos = 0usize;
        for (r, tok) in syntax_spans {
            if r.start < pos || r.end > code.len() || r.start >= r.end {
                continue;
            }
            if r.start > pos && code.is_char_boundary(pos) && code.is_char_boundary(r.start) {
                rich_spans.push(
                    span(code[pos..r.start].to_string())
                        .font(theme::CODE_FONT)
                        .size(type_scale::CODE)
                        .color(fg),
                );
            }
            if code.is_char_boundary(r.start) && code.is_char_boundary(r.end) {
                rich_spans.push(
                    span(code[r.start..r.end].to_string())
                        .font(token_font(Some(*tok)))
                        .size(type_scale::CODE)
                        .color(tok.color),
                );
                pos = r.end;
            }
        }
        if pos < code.len() && code.is_char_boundary(pos) {
            rich_spans.push(
                span(code[pos..].to_string())
                    .font(theme::CODE_FONT)
                    .size(type_scale::CODE)
                    .color(fg),
            );
        }
        return rich_text(rich_spans)
            .size(type_scale::CODE)
            .font(theme::CODE_FONT)
            .wrapping(text::Wrapping::None)
            .into();
    }

    let hl_accent = accent.unwrap_or(fg);
    let hl_bg = Color {
        a: 0.34,
        ..hl_accent
    };
    let hl_border = Border {
        color: Color {
            a: 0.50,
            ..hl_accent
        },
        width: 1.0,
        radius: 2.0.into(),
    };

    // Build byte-offset inline highlight ranges and sweep both sorted interval lists
    let mut inline_ranges: Vec<(Range<usize>, bool)> = Vec::with_capacity(spans.len());
    let mut cuts: Vec<usize> = Vec::with_capacity(spans.len() * 2 + syntax_spans.len() * 2 + 2);
    cuts.push(0);
    cuts.push(code.len());

    let mut cursor = 0usize;
    for seg in spans {
        let end = (cursor + seg.text.len()).min(code.len());
        if end > cursor {
            inline_ranges.push((cursor..end, seg.highlighted));
            cuts.push(cursor);
            cuts.push(end);
        }
        cursor = end;
    }
    for (r, _) in syntax_spans {
        if r.start < code.len() && r.end <= code.len() && r.start < r.end {
            cuts.push(r.start);
            cuts.push(r.end);
        }
    }
    cuts.sort_unstable();
    cuts.dedup();

    let mut rich_spans: Vec<text::Span<'a, Message, iced::Font>> =
        Vec::with_capacity(cuts.len().saturating_sub(1));
    let mut syn_idx = 0usize;
    let mut inl_idx = 0usize;

    for win in cuts.windows(2) {
        let (a, b) = (win[0], win[1]);
        if a >= b || !code.is_char_boundary(a) || !code.is_char_boundary(b) {
            continue;
        }
        while syn_idx < syntax_spans.len() && syntax_spans[syn_idx].0.end <= a {
            syn_idx += 1;
        }
        let tok = syntax_spans
            .get(syn_idx)
            .filter(|(r, _)| r.start <= a && b <= r.end)
            .map(|(_, f)| *f);

        while inl_idx < inline_ranges.len() && inline_ranges[inl_idx].0.end <= a {
            inl_idx += 1;
        }
        let in_diff_hl = inline_ranges
            .get(inl_idx)
            .is_some_and(|(r, hl)| *hl && r.start <= a && b <= r.end);

        let seg_color = tok.map(|t| t.color).unwrap_or(fg);
        let s = span(code[a..b].to_string())
            .font(token_font(tok))
            .size(type_scale::CODE)
            .color(seg_color);
        if in_diff_hl {
            rich_spans.push(s.background(hl_bg).border(hl_border).padding([1, 0]));
        } else {
            rich_spans.push(s);
        }
    }

    rich_text(rich_spans)
        .size(type_scale::CODE)
        .font(theme::CODE_FONT)
        .wrapping(text::Wrapping::None)
        .into()
}
