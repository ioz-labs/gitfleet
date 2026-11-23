//! Low-level diff UI primitives: `DiffLineBox` row widget with drag-to-select,
//! unified `SplitPairedCodeRow` + `split_diff_scroll` + `sticky_x_row` for single-scrollbar
//! Side-by-Side Diff, edge overview minimap, hidden gap expander, and syntax + inline-diff compositor.

use super::inline_diff::InlineSpan;
use super::layout::ConnectorSpan;
use super::selection::{col_to_x, diff_code_char_width, x_to_col};
use super::{CONNECTOR_W, DIFF_EXPAND_STEP, DIFF_HIDDEN_H, DIFF_LINE_H, LINE_NUM_W};
use crate::app::{DiffGapExpandMode, DiffSelectionSide, Message};
use crate::icons;
use crate::syntax::TokenFormat;
use crate::theme::{self, Palette, type_scale};
use gitfleet_git::DiffChangeKind;
use iced::advanced::layout as adv_layout;
use iced::advanced::text as adv_text;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, overlay, renderer};
use iced::widget::canvas::{self, Canvas, Geometry, Path as CanvasPath, Stroke};
use iced::widget::{button, container, rich_text, row, scrollable, span, text};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Pixels, Point, Rectangle, Renderer,
    Shadow, Size, Theme, Vector, alignment, event, mouse,
};
use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

pub(crate) fn change_colors(kind: DiffChangeKind, p: Palette) -> (Color, Color, Color) {
    // Returns (soft_bg, accent_border, text_fg) using SmartGit-aligned semantic tokens
    match kind {
        DiffChangeKind::Added => (p.diff_add_line_bg, p.git_added, p.text_primary),
        DiffChangeKind::Removed => (p.diff_del_line_bg, p.git_deleted, p.text_primary),
        DiffChangeKind::Modified => (
            Color {
                a: if p.is_dark { 0.13 } else { 0.11 },
                ..p.git_modified
            },
            p.git_modified,
            p.text_primary,
        ),
    }
}

/// Computes `(left_w, conn_x_rel, right_x_rel, right_w)` for Side-by-Side viewport width `vw`,
/// carving out `CONNECTOR_W` in the center for SmartGit cubic-Bezier diff connectors.
#[inline]
fn split_pane_columns(vw: f32) -> (f32, f32, f32, f32) {
    let vw = vw.max(240.0);
    let usable = (vw - CONNECTOR_W).max(160.0);
    let left_w = (usable * 0.5).floor();
    let conn_x = left_w;
    let right_x = left_w + CONNECTOR_W;
    let right_w = (vw - right_x).max(80.0);
    (left_w, conn_x, right_x, right_w)
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

#[inline]
fn shift_cursor_x(cursor: mouse::Cursor, dx: f32) -> mouse::Cursor {
    match cursor {
        mouse::Cursor::Available(p) => mouse::Cursor::Available(Point::new(p.x + dx, p.y)),
        mouse::Cursor::Unavailable => mouse::Cursor::Unavailable,
    }
}

/// Center cubic-Bezier S-curve connector overlay linking Left (Old) and Right (New) change blocks
/// across the 28px center gutter without obscuring full-width `"N lines hidden"` context bars.
struct DiffConnectorCanvas {
    spans: Vec<ConnectorSpan>,
    scroll_y: f32,
    active_idx: usize,
    p: Palette,
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
        if w <= 1.0 || h <= 1.0 {
            return vec![frame.into_geometry()];
        }

        let mid_x = w * 0.5;
        for (idx, span) in self.spans.iter().enumerate() {
            let ly0 = span.left_top - self.scroll_y;
            let mut ly1 = span.left_bot - self.scroll_y;
            let ry0 = span.right_top - self.scroll_y;
            let mut ry1 = span.right_bot - self.scroll_y;

            if ly1.max(ry1) < -6.0 || ly0.min(ry0) > h + 6.0 {
                continue;
            }
            if (ly1 - ly0).abs() < 2.0 {
                ly1 = ly0 + 2.0;
            }
            if (ry1 - ry0).abs() < 2.0 {
                ry1 = ry0 + 2.0;
            }

            let (_, accent, _) = change_colors(span.kind, self.p);
            let is_active = idx == self.active_idx;
            let fill = Color {
                a: if is_active { 0.34 } else { 0.22 },
                ..accent
            };
            let stroke_color = Color {
                a: if is_active { 0.96 } else { 0.80 },
                ..accent
            };
            let stroke_w = if is_active { 1.5 } else { 1.2 };

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
                b.move_to(Point::new(0.0, ly0 + 0.5));
                b.bezier_curve_to(
                    Point::new(mid_x, ly0 + 0.5),
                    Point::new(mid_x, ry0 + 0.5),
                    Point::new(w, ry0 + 0.5),
                );
            });
            let bot_edge = CanvasPath::new(|b| {
                b.move_to(Point::new(0.0, ly1 - 0.5));
                b.bezier_curve_to(
                    Point::new(mid_x, ly1 - 0.5),
                    Point::new(mid_x, ry1 - 0.5),
                    Point::new(w, ry1 - 0.5),
                );
            });
            let stroke = Stroke::default().with_color(stroke_color).with_width(stroke_w);
            frame.stroke(&top_edge, stroke);
            frame.stroke(&bot_edge, stroke);
        }

        vec![frame.into_geometry()]
    }
}

/// Unified Side-by-Side scrollable host with **one** vertical scrollbar and **one** horizontal
/// scrollbar that pans both Left and Right code columns simultaneously while keeping sticky
/// line-number gutters, the 28px SmartGit Bezier connector channel, and full-width `"N lines hidden"`
/// context bars pinned horizontally inside the viewport.
#[allow(clippy::too_many_arguments)]
pub(crate) fn split_diff_scroll<'a>(
    content: impl Into<Element<'a, Message>>,
    min_code_w: f32,
    viewport_w: Rc<Cell<f32>>,
    spans: Vec<ConnectorSpan>,
    scroll_y: f32,
    active_idx: usize,
    id: scrollable::Id,
    on_scroll: impl Fn(scrollable::Viewport) -> Message + 'a,
    p: Palette,
) -> Element<'a, Message> {
    struct SplitDiffCanvas<'a> {
        content: Element<'a, Message>,
        min_code_w: f32,
        viewport_w: Rc<Cell<f32>>,
        p: Palette,
    }

    impl<'a> Widget<Message, Theme, Renderer> for SplitDiffCanvas<'a> {
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
            Size::new(Length::Shrink, Length::Shrink)
        }

        fn layout(
            &self,
            tree: &mut Tree,
            renderer: &Renderer,
            _limits: &adv_layout::Limits,
        ) -> adv_layout::Node {
            let vw = self.viewport_w.get().max(240.0);
            let (left_w, _, _, right_w) = split_pane_columns(vw);
            let code_vp_w = (left_w - LINE_NUM_W)
                .min(right_w - LINE_NUM_W - 8.0)
                .max(40.0);
            let max_scroll_x = (self.min_code_w - code_vp_w).max(0.0);
            let overflows_x = max_scroll_x > 0.5;
            let total_w = if overflows_x { vw + max_scroll_x } else { vw };
            let bottom_pad = if overflows_x { 10.0 } else { 0.0 };

            let child_limits = adv_layout::Limits::new(
                Size::new(total_w, 0.0),
                Size::new(total_w, f32::INFINITY),
            );
            let child_node =
                self.content
                    .as_widget()
                    .layout(&mut tree.children[0], renderer, &child_limits);
            let child_size = child_node.size();
            adv_layout::Node::with_children(
                Size::new(total_w, child_size.height + bottom_pad),
                vec![child_node],
            )
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

            let Some(child_layout) = layout.children().next() else {
                return;
            };
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                child_layout,
                cursor,
                viewport,
            );

            // Extend subtle gutter hairlines and 28px center connector channel into any trailing space
            // below short diffs so the split workbench looks cohesive to the bottom of the pane.
            let b = layout.bounds();
            let child_b = child_layout.bounds();
            let empty_top = (child_b.y + child_b.height).max(viewport.y);
            let empty_bot = viewport.y + viewport.height;
            if empty_bot > empty_top + 0.5 {
                let vw = self.viewport_w.get().max(240.0);
                let (_, conn_x, right_x, _) = split_pane_columns(vw);
                let vp_x = b.x.max(viewport.x);
                let h = empty_bot - empty_top;
                let flat_quad = |bounds: Rectangle| renderer::Quad {
                    bounds,
                    border: Border::default(),
                    shadow: Shadow::default(),
                };
                renderer.fill_quad(
                    flat_quad(Rectangle {
                        x: vp_x + LINE_NUM_W - 1.0,
                        y: empty_top,
                        width: 1.0,
                        height: h,
                    }),
                    Background::Color(self.p.border_subtle),
                );
                renderer.fill_quad(
                    flat_quad(Rectangle {
                        x: vp_x + conn_x,
                        y: empty_top,
                        width: CONNECTOR_W,
                        height: h,
                    }),
                    Background::Color(self.p.surface_titlebar),
                );
                renderer.fill_quad(
                    flat_quad(Rectangle {
                        x: vp_x + conn_x,
                        y: empty_top,
                        width: 1.0,
                        height: h,
                    }),
                    Background::Color(self.p.border_subtle),
                );
                renderer.fill_quad(
                    flat_quad(Rectangle {
                        x: vp_x + right_x - 1.0,
                        y: empty_top,
                        width: 1.0,
                        height: h,
                    }),
                    Background::Color(self.p.border_subtle),
                );
                renderer.fill_quad(
                    flat_quad(Rectangle {
                        x: vp_x + right_x + LINE_NUM_W - 1.0,
                        y: empty_top,
                        width: 1.0,
                        height: h,
                    }),
                    Background::Color(self.p.border_subtle),
                );
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

    struct SplitDiffScroll<'a> {
        viewport_w: Rc<Cell<f32>>,
        children: [Element<'a, Message>; 2],
    }

    impl<'a> Widget<Message, Theme, Renderer> for SplitDiffScroll<'a> {
        fn tag(&self) -> tree::Tag {
            tree::Tag::stateless()
        }

        fn state(&self) -> tree::State {
            tree::State::None
        }

        fn children(&self) -> Vec<Tree> {
            self.children.iter().map(Tree::new).collect()
        }

        fn diff(&self, tree: &mut Tree) {
            tree.diff_children(&self.children);
        }

        fn size(&self) -> Size<Length> {
            Size::new(Length::Fill, Length::Fill)
        }

        fn layout(
            &self,
            tree: &mut Tree,
            renderer: &Renderer,
            limits: &adv_layout::Limits,
        ) -> adv_layout::Node {
            let vw = limits.max().width;
            if vw.is_finite() && vw > 1.0 {
                self.viewport_w.set(vw);
            }
            let scroll_node =
                self.children[0]
                    .as_widget()
                    .layout(&mut tree.children[0], renderer, limits);
            let size = scroll_node.size();
            let (_, conn_x, _, _) = split_pane_columns(size.width);
            let conn_limits = adv_layout::Limits::new(
                Size::new(CONNECTOR_W, size.height),
                Size::new(CONNECTOR_W, size.height),
            );
            let conn_node = self.children[1]
                .as_widget()
                .layout(&mut tree.children[1], renderer, &conn_limits)
                .move_to(Point::new(conn_x, 0.0));
            adv_layout::Node::with_children(size, vec![scroll_node, conn_node])
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

            let mut iter = layout.children();
            if let Some(scroll_layout) = iter.next() {
                self.children[0].as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    scroll_layout,
                    cursor,
                    viewport,
                );
            }
            if let Some(conn_layout) = iter.next() {
                let cb = conn_layout.bounds();
                let raw_clip = Rectangle {
                    x: cb.x,
                    y: cb.y,
                    width: cb.width,
                    height: (cb.height - 8.0).max(0.0),
                };
                if let Some(clip) = raw_clip.intersection(viewport)
                    && clip.height > 1.0
                {
                    renderer.with_layer(clip, |renderer| {
                        self.children[1].as_widget().draw(
                            &tree.children[1],
                            renderer,
                            theme,
                            style,
                            conn_layout,
                            mouse::Cursor::Unavailable,
                            &clip,
                        );
                    });
                }
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
            let Some(scroll_layout) = layout.children().next() else {
                return event::Status::Ignored;
            };
            self.children[0].as_widget_mut().on_event(
                &mut tree.children[0],
                event,
                scroll_layout,
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
            let Some(scroll_layout) = layout.children().next() else {
                return mouse::Interaction::None;
            };
            self.children[0].as_widget().mouse_interaction(
                &tree.children[0],
                scroll_layout,
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
            if let Some(scroll_layout) = layout.children().next() {
                self.children[0].as_widget().operate(
                    &mut tree.children[0],
                    scroll_layout,
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
            let scroll_layout = layout.children().next()?;
            self.children[0].as_widget_mut().overlay(
                &mut tree.children[0],
                scroll_layout,
                renderer,
                translation,
            )
        }
    }

    let canvas: Element<'a, Message> = Element::new(SplitDiffCanvas {
        content: content.into(),
        min_code_w,
        viewport_w: Rc::clone(&viewport_w),
        p,
    });
    let scroll: Element<'a, Message> = scrollable(canvas)
        .id(id)
        .on_scroll(on_scroll)
        .width(Length::Fill)
        .height(Length::Fill)
        .direction(theme::slim_both_direction())
        .style(theme::slim_scrollable(p))
        .into();
    let connector: Element<'a, Message> = Canvas::new(DiffConnectorCanvas {
        spans,
        scroll_y,
        active_idx,
        p,
    })
    .width(Length::Fixed(CONNECTOR_W))
    .height(Length::Fill)
    .into();

    Element::new(SplitDiffScroll {
        viewport_w,
        children: [scroll, connector],
    })
}

/// Horizontally-pinned full-width row wrapper for shared Side-by-Side rows (`Hidden` context
/// expanders and file `Separator` headers) so they span 100% of the visible viewport width
/// without scrolling off-screen when panning wide code horizontally.
pub(crate) fn sticky_x_row<'a>(
    height: f32,
    viewport_w: Rc<Cell<f32>>,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    struct StickyXRow<'a> {
        height: f32,
        viewport_w: Rc<Cell<f32>>,
        content: Element<'a, Message>,
    }

    impl<'a> Widget<Message, Theme, Renderer> for StickyXRow<'a> {
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
            Size::new(Length::Fill, Length::Fixed(self.height))
        }

        fn layout(
            &self,
            tree: &mut Tree,
            renderer: &Renderer,
            limits: &adv_layout::Limits,
        ) -> adv_layout::Node {
            let vw = self.viewport_w.get().max(240.0);
            let row_w = if limits.max().width.is_finite() {
                limits.max().width
            } else {
                vw
            };
            let child_limits = adv_layout::Limits::new(
                Size::new(vw, self.height),
                Size::new(vw, self.height),
            );
            let child = self
                .content
                .as_widget()
                .layout(&mut tree.children[0], renderer, &child_limits);
            adv_layout::Node::with_children(Size::new(row_w, self.height), vec![child])
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
            if b.y + b.height < viewport.y || b.y > viewport.y + viewport.height {
                return;
            }
            let Some(child_layout) = layout.children().next() else {
                return;
            };

            let vw = self.viewport_w.get().max(240.0);
            let scroll_x = (viewport.x - b.x).max(0.0);
            let row_vp = Rectangle {
                x: b.x + scroll_x,
                y: b.y,
                width: vw,
                height: b.height,
            };
            let Some(clip) = row_vp.intersection(viewport) else {
                return;
            };
            let local_vp = Rectangle {
                x: clip.x - scroll_x,
                y: clip.y,
                width: clip.width,
                height: clip.height,
            };
            let local_cursor = shift_cursor_x(cursor, -scroll_x);

            renderer.with_layer(clip, |renderer| {
                renderer.with_translation(Vector::new(scroll_x, 0.0), |renderer| {
                    self.content.as_widget().draw(
                        &tree.children[0],
                        renderer,
                        theme,
                        style,
                        child_layout,
                        local_cursor,
                        &local_vp,
                    );
                });
            });
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
            let Some(child_layout) = layout.children().next() else {
                return event::Status::Ignored;
            };
            let b = layout.bounds();
            let scroll_x = (viewport.x - b.x).max(0.0);
            let local_cursor = shift_cursor_x(cursor, -scroll_x);
            let local_vp = Rectangle {
                x: viewport.x - scroll_x,
                y: viewport.y,
                width: viewport.width,
                height: viewport.height,
            };
            self.content.as_widget_mut().on_event(
                &mut tree.children[0],
                event,
                child_layout,
                local_cursor,
                renderer,
                clipboard,
                shell,
                &local_vp,
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
            let Some(child_layout) = layout.children().next() else {
                return mouse::Interaction::None;
            };
            let b = layout.bounds();
            let scroll_x = (viewport.x - b.x).max(0.0);
            let local_cursor = shift_cursor_x(cursor, -scroll_x);
            let local_vp = Rectangle {
                x: viewport.x - scroll_x,
                y: viewport.y,
                width: viewport.width,
                height: viewport.height,
            };
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                child_layout,
                local_cursor,
                &local_vp,
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
            let b = layout.bounds();
            let _ = b;
            self.content.as_widget_mut().overlay(
                &mut tree.children[0],
                child_layout,
                renderer,
                translation,
            )
        }
    }

    Element::new(StickyXRow {
        height,
        viewport_w,
        content: content.into(),
    })
}

pub(crate) struct SplitSideVisual<'a> {
    pub(crate) line_no: Option<usize>,
    pub(crate) line_text: String,
    pub(crate) bg: Color,
    pub(crate) top_border: Option<Color>,
    pub(crate) bottom_border: Option<Color>,
    pub(crate) sel_range: Option<(usize, usize, bool)>,
    pub(crate) code: Element<'a, Message>,
}

/// Single fixed-height (`DIFF_LINE_H`) Side-by-Side paired row widget.
/// Renders Left and Right side-by-side around the 28px SmartGit Bezier connector channel,
/// with sticky line-number gutters, hardware-scissored horizontal scrolling for both code cells,
/// SmartGit 2px insertion/deletion seam hairlines, and independent drag-to-select on Left and Right.
#[allow(clippy::too_many_arguments)]
pub(crate) fn split_paired_code_row<'a>(
    row_idx: usize,
    left: Option<SplitSideVisual<'a>>,
    right: Option<SplitSideVisual<'a>>,
    block_kind: Option<DiffChangeKind>,
    block_is_first: bool,
    dragging_side: Option<DiffSelectionSide>,
    viewport_w: Rc<Cell<f32>>,
    p: Palette,
) -> Element<'a, Message> {
    struct SideMeta {
        line_no: Option<usize>,
        line_text: String,
        bg: Color,
        top_border: Option<Color>,
        bottom_border: Option<Color>,
        sel_range: Option<(usize, usize, bool)>,
    }

    struct SplitPairedCodeRow<'a> {
        row_idx: usize,
        left: Option<SideMeta>,
        right: Option<SideMeta>,
        left_child_idx: Option<usize>,
        right_child_idx: Option<usize>,
        children: Vec<Element<'a, Message>>,
        block_kind: Option<DiffChangeKind>,
        block_is_first: bool,
        dragging_side: Option<DiffSelectionSide>,
        viewport_w: Rc<Cell<f32>>,
        p: Palette,
    }

    impl<'a> Widget<Message, Theme, Renderer> for SplitPairedCodeRow<'a> {
        fn tag(&self) -> tree::Tag {
            tree::Tag::stateless()
        }

        fn state(&self) -> tree::State {
            tree::State::None
        }

        fn children(&self) -> Vec<Tree> {
            self.children.iter().map(Tree::new).collect()
        }

        fn diff(&self, tree: &mut Tree) {
            tree.diff_children(&self.children);
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
            let vw = self.viewport_w.get().max(240.0);
            let (_, _, right_x, _) = split_pane_columns(vw);
            let row_w = if limits.max().width.is_finite() {
                limits.max().width
            } else {
                vw
            };
            let code_w = row_w.max(4096.0);
            let code_limits = adv_layout::Limits::new(
                Size::new(code_w, DIFF_LINE_H),
                Size::new(code_w, DIFF_LINE_H),
            );

            let mut nodes = Vec::with_capacity(self.children.len());
            for (idx, child) in self.children.iter().enumerate() {
                let x = if Some(idx) == self.left_child_idx {
                    LINE_NUM_W + 8.0
                } else {
                    right_x + LINE_NUM_W + 8.0
                };
                let node = child
                    .as_widget()
                    .layout(&mut tree.children[idx], renderer, &code_limits)
                    .move_to(Point::new(x, 0.0));
                nodes.push(node);
            }

            adv_layout::Node::with_children(Size::new(row_w, DIFF_LINE_H), nodes)
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
            let row_vis_top = b.y.max(viewport.y);
            let row_vis_bot = (b.y + b.height).min(viewport.y + viewport.height);
            let row_vis_h = (row_vis_bot - row_vis_top).max(0.0);
            if row_vis_h <= 0.0 {
                return;
            }

            let vw = self.viewport_w.get().max(240.0);
            let (left_w, conn_x, right_x, right_w) = split_pane_columns(vw);
            let vp_x = b.x.max(viewport.x);
            let left_pane_x = vp_x;
            let right_pane_x = vp_x + right_x;

            let flat_quad = |bounds: Rectangle| renderer::Quad {
                bounds,
                border: Border::default(),
                shadow: Shadow::default(),
            };
            let filler_bg = Color {
                a: if self.p.is_dark { 0.45 } else { 0.55 },
                ..self.p.surface_sidebar
            };
            let sel_bg = Color {
                a: 0.42,
                ..self.p.accent_primary
            };
            let sel_overlay = Color {
                a: 0.16,
                ..self.p.accent_hover
            };
            let char_w = diff_code_char_width();

            let child_layouts: Vec<Layout<'_>> = layout.children().collect();

            // Helper closure for painting one side's background, borders/seam, sticky gutter, and clipped code
            let mut paint_side = |pane_x: f32,
                                  pane_w: f32,
                                  code_origin_x: f32,
                                  is_left: bool,
                                  meta: Option<&SideMeta>,
                                  child_idx: Option<usize>| {
                let (bg, top_b, bot_b, seam_c) = match meta {
                    Some(m) => (
                        m.bg,
                        m.top_border.map(|c| Color { a: 0.78, ..c }),
                        m.bottom_border.map(|c| Color { a: 0.78, ..c }),
                        None,
                    ),
                    None => {
                        let seam = if self.block_is_first {
                            match (is_left, self.block_kind) {
                                (true, Some(DiffChangeKind::Added)) => Some(self.p.git_added),
                                (false, Some(DiffChangeKind::Removed)) => Some(self.p.git_deleted),
                                _ => None,
                            }
                        } else {
                            None
                        };
                        (filler_bg, None, None, seam)
                    }
                };

                if bg != Color::TRANSPARENT {
                    renderer.fill_quad(
                        flat_quad(Rectangle {
                            x: pane_x,
                            y: b.y,
                            width: pane_w,
                            height: b.height,
                        }),
                        Background::Color(bg),
                    );
                }
                if let Some(c) = top_b {
                    renderer.fill_quad(
                        flat_quad(Rectangle {
                            x: pane_x,
                            y: b.y,
                            width: pane_w,
                            height: 1.0,
                        }),
                        Background::Color(c),
                    );
                }
                if let Some(c) = bot_b {
                    renderer.fill_quad(
                        flat_quad(Rectangle {
                            x: pane_x,
                            y: b.y + b.height - 1.0,
                            width: pane_w,
                            height: 1.0,
                        }),
                        Background::Color(c),
                    );
                }

                // Gutter right divider
                renderer.fill_quad(
                    flat_quad(Rectangle {
                        x: pane_x + LINE_NUM_W - 1.0,
                        y: b.y,
                        width: 1.0,
                        height: b.height,
                    }),
                    Background::Color(self.p.border_subtle),
                );

                // SmartGit 2px insertion/deletion seam hairline on opposite side of pure Added/Removed blocks
                if let Some(c) = seam_c {
                    renderer.fill_quad(
                        flat_quad(Rectangle {
                            x: pane_x,
                            y: b.y,
                            width: pane_w,
                            height: 2.0,
                        }),
                        Background::Color(Color { a: 0.88, ..c }),
                    );
                }

                // Sticky line number
                if let Some(ln) = meta.and_then(|m| m.line_no) {
                    let gutter_clip = Rectangle {
                        x: pane_x,
                        y: row_vis_top,
                        width: LINE_NUM_W,
                        height: row_vis_h,
                    };
                    if let Some(g_clip) = gutter_clip.intersection(viewport) {
                        adv_text::Renderer::fill_text(
                            renderer,
                            adv_text::Text {
                                content: ln.to_string(),
                                bounds: Size::new((LINE_NUM_W - 12.0).max(12.0), DIFF_LINE_H),
                                size: Pixels(type_scale::CAPTION),
                                line_height: text::LineHeight::default(),
                                font: theme::CODE_FONT,
                                horizontal_alignment: alignment::Horizontal::Right,
                                vertical_alignment: alignment::Vertical::Center,
                                shaping: text::Shaping::Basic,
                                wrapping: text::Wrapping::None,
                            },
                            Point::new(pane_x + LINE_NUM_W - 6.0, b.y + DIFF_LINE_H * 0.5),
                            self.p.text_muted,
                            g_clip,
                        );
                    }
                }

                // Horizontally-scrolled code + character-accurate selection inside scissored clip
                if let (Some(m), Some(idx)) = (meta, child_idx)
                    && let Some(&c_layout) = child_layouts.get(idx)
                {
                    let raw_code_clip = Rectangle {
                        x: pane_x + LINE_NUM_W,
                        y: row_vis_top,
                        width: (pane_w - LINE_NUM_W).max(0.0),
                        height: row_vis_h,
                    };
                    if let Some(code_clip) = raw_code_clip.intersection(viewport)
                        && code_clip.width > 0.5
                    {
                        let sel_rect = m.sel_range.and_then(|(s_col, e_col, extends_eol)| {
                            let x0 = code_origin_x + col_to_x(&m.line_text, s_col, char_w);
                            let x1 = code_origin_x
                                + col_to_x(&m.line_text, e_col, char_w)
                                + if extends_eol { char_w } else { 0.0 };
                            let w = (x1 - x0).max(if extends_eol { char_w } else { 0.0 });
                            (w > 0.5).then_some(Rectangle {
                                x: x0,
                                y: b.y,
                                width: w,
                                height: b.height,
                            })
                        });

                        let side_cursor = match cursor.position() {
                            Some(pos) if code_clip.contains(pos) => cursor,
                            _ => mouse::Cursor::Unavailable,
                        };

                        renderer.with_layer(code_clip, |renderer| {
                            if let Some(rect) = sel_rect {
                                renderer.fill_quad(flat_quad(rect), Background::Color(sel_bg));
                            }
                            self.children[idx].as_widget().draw(
                                &tree.children[idx],
                                renderer,
                                theme,
                                style,
                                c_layout,
                                side_cursor,
                                &code_clip,
                            );
                            if let Some(rect) = sel_rect {
                                renderer.fill_quad(flat_quad(rect), Background::Color(sel_overlay));
                            }
                        });
                    }
                }
            };

            paint_side(
                left_pane_x,
                left_w,
                b.x + LINE_NUM_W + 8.0,
                true,
                self.left.as_ref(),
                self.left_child_idx,
            );
            paint_side(
                right_pane_x,
                right_w,
                b.x + right_x + LINE_NUM_W + 8.0,
                false,
                self.right.as_ref(),
                self.right_child_idx,
            );

            // Center 28px SmartGit connector channel background + 1px side borders
            renderer.fill_quad(
                flat_quad(Rectangle {
                    x: vp_x + conn_x,
                    y: b.y,
                    width: CONNECTOR_W,
                    height: b.height,
                }),
                Background::Color(self.p.surface_titlebar),
            );
            renderer.fill_quad(
                flat_quad(Rectangle {
                    x: vp_x + conn_x,
                    y: b.y,
                    width: 1.0,
                    height: b.height,
                }),
                Background::Color(self.p.border_subtle),
            );
            renderer.fill_quad(
                flat_quad(Rectangle {
                    x: right_pane_x - 1.0,
                    y: b.y,
                    width: 1.0,
                    height: b.height,
                }),
                Background::Color(self.p.border_subtle),
            );
        }

        fn on_event(
            &mut self,
            _tree: &mut Tree,
            event: iced::Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            _renderer: &Renderer,
            _clipboard: &mut dyn Clipboard,
            shell: &mut Shell<'_, Message>,
            viewport: &Rectangle,
        ) -> event::Status {
            let b = layout.bounds();
            let vw = self.viewport_w.get().max(240.0);
            let (left_w, _, right_x, _) = split_pane_columns(vw);
            let vp_x = b.x.max(viewport.x);
            let left_end_x = vp_x + left_w;
            let right_pane_x = vp_x + right_x;
            let char_w = diff_code_char_width();

            match &event {
                iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                    if let Some(abs_pos) = cursor.position()
                        && viewport.contains(abs_pos)
                        && abs_pos.y >= b.y
                        && abs_pos.y < b.y + b.height
                    {
                        if abs_pos.x < left_end_x {
                            if let Some(meta) = &self.left {
                                let col = x_to_col(
                                    &meta.line_text,
                                    abs_pos.x - (b.x + LINE_NUM_W + 8.0),
                                    char_w,
                                );
                                shell.publish(Message::DiffSelStart {
                                    side: DiffSelectionSide::Left,
                                    row: self.row_idx,
                                    col,
                                });
                                return event::Status::Captured;
                            }
                        } else if abs_pos.x >= right_pane_x
                            && let Some(meta) = &self.right
                        {
                            let col = x_to_col(
                                &meta.line_text,
                                abs_pos.x - (b.x + right_x + LINE_NUM_W + 8.0),
                                char_w,
                            );
                            shell.publish(Message::DiffSelStart {
                                side: DiffSelectionSide::Right,
                                row: self.row_idx,
                                col,
                            });
                            return event::Status::Captured;
                        }
                    }
                }
                iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                    if let (Some(side), Some(abs_pos)) = (self.dragging_side, cursor.position())
                        && abs_pos.y >= b.y
                        && abs_pos.y < b.y + b.height
                        && abs_pos.y >= viewport.y
                        && abs_pos.y <= viewport.y + viewport.height
                    {
                        match side {
                            DiffSelectionSide::Left => {
                                let col = self
                                    .left
                                    .as_ref()
                                    .map(|m| {
                                        x_to_col(
                                            &m.line_text,
                                            abs_pos.x - (b.x + LINE_NUM_W + 8.0),
                                            char_w,
                                        )
                                    })
                                    .unwrap_or(0);
                                shell.publish(Message::DiffSelDrag {
                                    side,
                                    row: self.row_idx,
                                    col,
                                });
                            }
                            DiffSelectionSide::Right => {
                                let col = self
                                    .right
                                    .as_ref()
                                    .map(|m| {
                                        x_to_col(
                                            &m.line_text,
                                            abs_pos.x - (b.x + right_x + LINE_NUM_W + 8.0),
                                            char_w,
                                        )
                                    })
                                    .unwrap_or(0);
                                shell.publish(Message::DiffSelDrag {
                                    side,
                                    row: self.row_idx,
                                    col,
                                });
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }

            event::Status::Ignored
        }

        fn mouse_interaction(
            &self,
            _tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            _renderer: &Renderer,
        ) -> mouse::Interaction {
            let b = layout.bounds();
            let Some(abs_pos) = cursor.position() else {
                return mouse::Interaction::None;
            };
            if !viewport.contains(abs_pos) || abs_pos.y < b.y || abs_pos.y >= b.y + b.height {
                return mouse::Interaction::None;
            }
            let vw = self.viewport_w.get().max(240.0);
            let (left_w, _, right_x, _) = split_pane_columns(vw);
            let vp_x = b.x.max(viewport.x);
            let right_pane_x = vp_x + right_x;

            if abs_pos.x >= vp_x + LINE_NUM_W - 4.0
                && abs_pos.x < vp_x + left_w
                && self.left.is_some()
            {
                return mouse::Interaction::Text;
            }
            if abs_pos.x >= right_pane_x + LINE_NUM_W - 4.0 && self.right.is_some() {
                return mouse::Interaction::Text;
            }
            mouse::Interaction::None
        }

        fn operate(
            &self,
            tree: &mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            operation: &mut dyn iced::advanced::widget::Operation,
        ) {
            for ((child, c_tree), c_layout) in self
                .children
                .iter()
                .zip(tree.children.iter_mut())
                .zip(layout.children())
            {
                child.as_widget().operate(c_tree, c_layout, renderer, operation);
            }
        }

        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'_>,
            renderer: &Renderer,
            translation: Vector,
        ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
            for ((child, c_tree), c_layout) in self
                .children
                .iter_mut()
                .zip(tree.children.iter_mut())
                .zip(layout.children())
            {
                if let Some(ov) = child.as_widget_mut().overlay(c_tree, c_layout, renderer, translation) {
                    return Some(ov);
                }
            }
            None
        }
    }

    let mut children = Vec::with_capacity(2);
    let mut left_child_idx = None;
    let mut right_child_idx = None;

    let left_meta = left.map(|vis| {
        let idx = children.len();
        children.push(vis.code);
        left_child_idx = Some(idx);
        SideMeta {
            line_no: vis.line_no,
            line_text: vis.line_text,
            bg: vis.bg,
            top_border: vis.top_border,
            bottom_border: vis.bottom_border,
            sel_range: vis.sel_range,
        }
    });

    let right_meta = right.map(|vis| {
        let idx = children.len();
        children.push(vis.code);
        right_child_idx = Some(idx);
        SideMeta {
            line_no: vis.line_no,
            line_text: vis.line_text,
            bg: vis.bg,
            top_border: vis.top_border,
            bottom_border: vis.bottom_border,
            sel_range: vis.sel_range,
        }
    });

    Element::new(SplitPairedCodeRow {
        row_idx,
        left: left_meta,
        right: right_meta,
        left_child_idx,
        right_child_idx,
        children,
        block_kind,
        block_is_first,
        dragging_side,
        viewport_w,
        p,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverviewSide {
    Left,
    Right,
    Unified,
}

/// Interactive SmartGit edge diff-position overview bar showing Added/Removed/Modified markers,
/// a live viewport position indicator box, and click/drag-to-jump navigation.
pub(crate) struct DiffOverviewBar {
    pub(crate) spans: Vec<ConnectorSpan>,
    pub(crate) side: OverviewSide,
    pub(crate) total_h: f32,
    pub(crate) scroll_y: f32,
    pub(crate) viewport_h: f32,
    pub(crate) active_idx: usize,
    pub(crate) p: Palette,
}

impl canvas::Program<Message> for DiffOverviewBar {
    type State = bool;

    fn update(
        &self,
        is_dragging: &mut Self::State,
        ev: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (event::Status, Option<Message>) {
        if self.total_h <= 1.0 || bounds.height <= 1.0 {
            *is_dragging = false;
            return (event::Status::Ignored, None);
        }
        let unified = self.side == OverviewSide::Unified;
        match ev {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    *is_dragging = true;
                    let ratio = (pos.y / bounds.height).clamp(0.0, 1.0);
                    return (
                        event::Status::Captured,
                        Some(Message::DiffOverviewJump { unified, ratio }),
                    );
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) if *is_dragging => {
                if let Some(abs_pos) = cursor.position() {
                    let rel_y = abs_pos.y - bounds.y;
                    let ratio = (rel_y / bounds.height).clamp(0.0, 1.0);
                    return (
                        event::Status::Captured,
                        Some(Message::DiffOverviewJump { unified, ratio }),
                    );
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if *is_dragging =>
            {
                *is_dragging = false;
                return (event::Status::Captured, None);
            }
            _ => {}
        }
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
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), self.p.surface_titlebar);
        let border_x = match self.side {
            OverviewSide::Left => (bounds.width - 1.0).max(0.0),
            OverviewSide::Right | OverviewSide::Unified => 0.0,
        };
        frame.fill_rectangle(
            Point::new(border_x, 0.0),
            Size::new(1.0, bounds.height),
            self.p.border_subtle,
        );

        if self.total_h <= 1.0 || bounds.height <= 1.0 {
            return vec![frame.into_geometry()];
        }

        let eff_total = self.total_h.max(self.viewport_h).max(1.0);
        let scale = bounds.height / eff_total;

        for (idx, span) in self.spans.iter().enumerate() {
            let (top, bot) = match self.side {
                OverviewSide::Left => (span.left_top, span.left_bot),
                OverviewSide::Right => (span.right_top, span.right_bot),
                OverviewSide::Unified => (span.unified_top, span.unified_bot),
            };
            let (_, accent, _) = change_colors(span.kind, self.p);
            let is_seam_only = (bot - top).abs() < 1.0;
            let min_h = if is_seam_only { 2.0 } else { 3.0 };
            let y = (top * scale).clamp(0.0, (bounds.height - min_h).max(0.0));
            let h = ((bot - top) * scale).max(min_h).min(bounds.height - y);
            let tick_color = Color {
                a: if idx == self.active_idx {
                    1.0
                } else if is_seam_only {
                    0.70
                } else {
                    0.92
                },
                ..accent
            };
            frame.fill_rectangle(
                Point::new(1.5, y),
                Size::new((bounds.width - 3.0).max(2.0), h),
                tick_color,
            );
        }

        // Live viewport position window indicator
        let vp_h = (self.viewport_h * scale).clamp(10.0, bounds.height);
        let max_scroll = (self.total_h - self.viewport_h).max(0.0);
        let vp_y = if max_scroll > 0.5 {
            (self.scroll_y / max_scroll).clamp(0.0, 1.0) * (bounds.height - vp_h)
        } else {
            0.0
        };
        frame.fill_rectangle(
            Point::new(1.0, vp_y),
            Size::new((bounds.width - 2.0).max(2.0), vp_h),
            Color {
                a: if self.p.is_dark { 0.14 } else { 0.12 },
                ..self.p.text_primary
            },
        );
        let vp_box = CanvasPath::rectangle(
            Point::new(0.5, vp_y + 0.5),
            Size::new((bounds.width - 1.0).max(2.0), (vp_h - 1.0).max(3.0)),
        );
        frame.stroke(
            &vp_box,
            Stroke::default()
                .with_color(Color {
                    a: 0.52,
                    ..self.p.text_secondary
                })
                .with_width(1.0),
        );

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        is_dragging: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if *is_dragging {
            return mouse::Interaction::Grabbing;
        }
        if self.total_h > 1.0 && cursor.is_over(bounds) {
            return mouse::Interaction::Pointer;
        }
        mouse::Interaction::default()
    }
}

pub(crate) fn hidden_gap_bar<'a>(
    gap_id: usize,
    hidden_count: usize,
    can_expand_top: bool,
    can_expand_bottom: bool,
    centered: bool,
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

    let mut box_c = container(bar)
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
        });
    if centered {
        box_c = box_c.align_x(alignment::Horizontal::Center);
    }
    box_c.into()
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

    let hl_bg = accent.unwrap_or(Color {
        a: 0.30,
        ..fg
    });
    let hl_border = Border {
        color: Color {
            a: (hl_bg.a + 0.18).min(0.65),
            ..hl_bg
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
