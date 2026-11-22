//! Reusable numbered code editor & read-only code preview widget with a sticky
//! pixel-aligned line-number gutter, active-line highlight, wrap-aware visual row
//! measurement, and synchronized scroll/keyboard navigation.

use crate::theme::{self, Palette};
use iced::advanced::layout as adv_layout;
use iced::advanced::text::{self as adv_text, Paragraph as _};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Clipboard, Layout, Renderer as _, Shell, Widget, overlay, renderer};
use iced::widget::{container, scrollable, text, text_editor};
use iced::{
    Background, Border, Color, Element, Length, Padding, Pixels, Point, Rectangle, Renderer, Shadow,
    Size, Task, Theme, Vector, alignment, event, mouse,
};
use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

/// Line-height multiplier used by GitFleet Studio's code editor.
pub(crate) const STUDIO_LINE_HEIGHT: f32 = 1.48;
/// Line-height multiplier used by Host Live File Editor and Code Preview.
pub(crate) const HOST_LINE_HEIGHT: f32 = 1.45;

/// Editor inner padding for GitFleet Studio.
/// Note: `top == left` deliberately avoids an Iced 0.13 `TextEditor` click-offset bug
/// where `padding.top` and `padding.left` are swapped during hit-testing.
pub(crate) const STUDIO_EDITOR_PADDING: Padding = Padding {
    top: 12.0,
    right: 16.0,
    bottom: 12.0,
    left: 12.0,
};

/// Editor inner padding for Host Diff View (Edit & Code Preview).
/// Keeps `top == left` for exact click hit-testing in Iced 0.13.
pub(crate) const HOST_EDITOR_PADDING: Padding = Padding {
    top: 8.0,
    right: 12.0,
    bottom: 8.0,
    left: 8.0,
};

const GUTTER_PAD_LEFT: f32 = 10.0;
const GUTTER_PAD_RIGHT: f32 = 12.0;
const GUTTER_MIN_WIDTH: f32 = 44.0;
const GUTTER_MIN_DIGITS: usize = 3;
/// Tab stop width matching `cosmic_text::Buffer`'s default `tab_width = 8`.
const TAB_WIDTH: usize = 8;

/// Visual & behavioral configuration for a numbered code editor instance.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NumberedEditorConfig {
    pub(crate) font_size: f32,
    pub(crate) line_height: f32,
    pub(crate) padding: Padding,
    pub(crate) word_wrap: bool,
    pub(crate) boxed: bool,
    pub(crate) highlight_active_line: bool,
}

impl NumberedEditorConfig {
    pub(crate) fn studio(font_size: f32, word_wrap: bool) -> Self {
        Self {
            font_size,
            line_height: STUDIO_LINE_HEIGHT,
            padding: STUDIO_EDITOR_PADDING,
            word_wrap,
            boxed: false,
            highlight_active_line: true,
        }
    }

    pub(crate) fn host_edit() -> Self {
        Self {
            font_size: theme::type_scale::CODE,
            line_height: HOST_LINE_HEIGHT,
            padding: HOST_EDITOR_PADDING,
            word_wrap: false,
            boxed: true,
            highlight_active_line: true,
        }
    }

    pub(crate) fn preview(font_size: f32, word_wrap: bool) -> Self {
        Self {
            font_size,
            line_height: HOST_LINE_HEIGHT,
            padding: HOST_EDITOR_PADDING,
            word_wrap,
            boxed: true,
            highlight_active_line: false,
        }
    }

    pub(crate) fn line_height_px(self) -> f32 {
        line_height_px(self.font_size, self.line_height)
    }
}

/// Persistent scroll tracking state for a numbered code editor or code preview.
#[derive(Debug, Clone)]
pub(crate) struct CodeScrollState {
    pub(crate) id: scrollable::Id,
    pub(crate) scroll_x: f32,
    pub(crate) scroll_y: f32,
    pub(crate) viewport_w: f32,
    pub(crate) viewport_h: f32,
}

impl Default for CodeScrollState {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeScrollState {
    pub(crate) fn new() -> Self {
        Self {
            id: scrollable::Id::unique(),
            scroll_x: 0.0,
            scroll_y: 0.0,
            viewport_w: 900.0,
            viewport_h: 600.0,
        }
    }

    pub(crate) fn reset_offsets(&mut self) {
        self.scroll_x = 0.0;
        self.scroll_y = 0.0;
    }

    pub(crate) fn reset<M>(&mut self) -> Task<M> {
        self.reset_offsets();
        scrollable::scroll_to(
            self.id.clone(),
            scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
        )
    }

    pub(crate) fn on_scroll(&mut self, vp: scrollable::Viewport) {
        let off = vp.absolute_offset();
        let b = vp.bounds();
        self.scroll_x = off.x.max(0.0);
        self.scroll_y = off.y.max(0.0);
        if b.width > 1.0 {
            self.viewport_w = b.width;
        }
        if b.height > 1.0 {
            self.viewport_h = b.height;
        }
    }

    pub(crate) fn scroll_to_visual_row_centered<M>(
        &mut self,
        visual_row: usize,
        config: NumberedEditorConfig,
    ) -> Task<M> {
        let target_y = compute_centered_scroll_y(
            self.viewport_h,
            visual_row,
            config.line_height_px(),
            config.padding.top,
        );
        self.scroll_y = target_y;
        scrollable::scroll_to(
            self.id.clone(),
            scrollable::AbsoluteOffset {
                x: self.scroll_x,
                y: target_y,
            },
        )
    }

    pub(crate) fn ensure_visual_row_visible<M>(
        &mut self,
        visual_row: usize,
        config: NumberedEditorConfig,
    ) -> Task<M> {
        let Some(target_y) = compute_ensure_cursor_scroll_y(
            self.scroll_y,
            self.viewport_h,
            visual_row,
            config.line_height_px(),
            config.padding.top,
        ) else {
            return Task::none();
        };
        self.scroll_y = target_y;
        scrollable::scroll_to(
            self.id.clone(),
            scrollable::AbsoluteOffset {
                x: self.scroll_x,
                y: target_y,
            },
        )
    }
}

// ── Pure Geometry & Scroll Math ──────────────────────────────────────────────

/// Exact line height in logical pixels matching `cosmic_text::Metrics` inside `TextEditor`.
pub(crate) fn line_height_px(font_size: f32, relative_line_height: f32) -> f32 {
    text::LineHeight::Relative(relative_line_height)
        .to_absolute(Pixels(font_size))
        .0
}

/// Dynamic gutter width based on total line count and editor font size.
/// Keeps a stable minimum of 3 digits so small edits never shift the code column,
/// and smoothly widens at 1,000 / 10,000 / 100,000 lines.
pub(crate) fn gutter_width(line_count: usize, font_size: f32) -> f32 {
    let digits = line_count.max(1).ilog10() as usize + 1;
    let display_digits = digits.max(GUTTER_MIN_DIGITS);
    let char_w = font_size * 0.62;
    (GUTTER_PAD_LEFT + display_digits as f32 * char_w + GUTTER_PAD_RIGHT)
        .ceil()
        .max(GUTTER_MIN_WIDTH)
}

/// Estimates visual columns of a single line (accounting for tab stops).
pub(crate) fn line_visual_cols(line: &str) -> usize {
    if !line.contains('\t') && line.is_ascii() {
        return line.len();
    }
    let mut col = 0usize;
    for ch in line.chars() {
        if ch == '\t' {
            col += TAB_WIDTH - (col % TAB_WIDTH);
        } else {
            col += 1;
        }
    }
    col
}

/// Returns the range `[first..last]` of logical lines intersecting the visible viewport
/// when word wrapping is disabled (every logical line occupies exactly 1 visual row).
pub(crate) fn visible_unwrapped_line_range(
    line_count: usize,
    scroll_top: f32,
    viewport_h: f32,
    top_pad: f32,
    line_h: f32,
) -> Range<usize> {
    if line_count == 0 || line_h <= 0.0 {
        return 0..0;
    }
    let rel_top = (scroll_top - top_pad).max(0.0);
    let rel_bottom = (scroll_top + viewport_h - top_pad).max(0.0);
    let first = ((rel_top / line_h).floor() as usize)
        .saturating_sub(1)
        .min(line_count);
    let last = (((rel_bottom / line_h).ceil() as usize) + 1)
        .min(line_count)
        .max(first);
    first..last
}

/// Returns the range `[first..last]` of logical lines intersecting the visible viewport
/// when word wrapping is enabled, using `prefix_rows` (`len == line_count + 1`).
pub(crate) fn visible_wrapped_line_range(
    prefix_rows: &[u32],
    scroll_top: f32,
    viewport_h: f32,
    top_pad: f32,
    line_h: f32,
) -> Range<usize> {
    if prefix_rows.len() <= 1 || line_h <= 0.0 {
        return 0..0;
    }
    let line_count = prefix_rows.len() - 1;
    let first_row = (((scroll_top - top_pad) / line_h).floor().max(0.0) as u32).saturating_sub(1);
    let last_row = (((scroll_top + viewport_h - top_pad) / line_h)
        .ceil()
        .max(0.0) as u32)
        .saturating_add(1);

    let first = prefix_rows
        .partition_point(|&start| start <= first_row)
        .saturating_sub(1)
        .min(line_count);
    let last = prefix_rows
        .partition_point(|&start| start <= last_row)
        .min(line_count)
        .max(first);
    first..last
}

/// Computes a new vertical scroll offset if `visual_row` lies outside the comfortable
/// viewport band; returns `None` when the cursor line is already visible.
pub(crate) fn compute_ensure_cursor_scroll_y(
    current_scroll_y: f32,
    viewport_h: f32,
    visual_row: usize,
    line_h: f32,
    top_pad: f32,
) -> Option<f32> {
    if viewport_h <= line_h || line_h <= 0.0 {
        return None;
    }
    let line_top = top_pad + visual_row as f32 * line_h;
    let line_bottom = line_top + line_h;
    let margin = (line_h * 1.5).min(viewport_h * 0.25);

    if line_top < current_scroll_y + margin {
        let target = (line_top - margin).max(0.0);
        if (target - current_scroll_y).abs() >= 0.5 {
            return Some(target);
        }
    } else if line_bottom > current_scroll_y + viewport_h - margin {
        let target = (line_bottom + margin - viewport_h).max(0.0);
        if (target - current_scroll_y).abs() >= 0.5 {
            return Some(target);
        }
    }
    None
}

/// Computes a vertical scroll offset placing `visual_row` around 35% from the top of the viewport.
pub(crate) fn compute_centered_scroll_y(
    viewport_h: f32,
    visual_row: usize,
    line_h: f32,
    top_pad: f32,
) -> f32 {
    let line_top = top_pad + visual_row as f32 * line_h;
    (line_top - viewport_h * 0.35).max(0.0)
}

/// Estimates the starting visual row of `target_line` for keyboard/search auto-scrolling.
/// When `word_wrap` is false this is O(1) (`target_line`); when `word_wrap` is true, it
/// sums estimated wrapped rows using the current inner viewport width.
pub(crate) fn estimate_visual_row_for_line(
    content: &text_editor::Content,
    target_line: usize,
    viewport_w: f32,
    config: NumberedEditorConfig,
) -> usize {
    if !config.word_wrap || target_line == 0 {
        return target_line;
    }
    let g_w = gutter_width(content.line_count(), config.font_size);
    let inner_w = (viewport_w - g_w - config.padding.horizontal()).max(40.0);
    let chars_per_row = (inner_w / (config.font_size * 0.60)).floor().max(1.0) as usize;
    let mut row = 0usize;
    for (idx, line) in content.lines().enumerate() {
        if idx >= target_line {
            break;
        }
        let cols = line_visual_cols(&line);
        let rows = if cols == 0 {
            1
        } else {
            cols.div_ceil(chars_per_row)
        };
        row += rows;
    }
    row
}

// ── Action & Navigation Helpers ──────────────────────────────────────────────

/// Performs an editor action on a scrollable shrink-height `text_editor::Content`:
/// - Translates `PageUp` / `PageDown` into viewport-sized line motions (since shrink-height
///   editors have an infinite internal `cosmic_text` height).
/// - Automatically scrolls the outer `scrollable` when keyboard navigation or edits move the
///   cursor out of the visible viewport.
pub(crate) fn perform_numbered_editor_action<M>(
    content: &mut text_editor::Content,
    action: text_editor::Action,
    scroll: &mut CodeScrollState,
    config: NumberedEditorConfig,
) -> Task<M> {
    use text_editor::{Action, Motion};

    let should_follow_cursor = !matches!(action, Action::Scroll { .. } | Action::Click(_));
    let page_lines = ((scroll.viewport_h / config.line_height_px()).floor() as usize)
        .saturating_sub(2)
        .max(1);

    match action {
        Action::Move(Motion::PageUp) => {
            for _ in 0..page_lines {
                let prev = content.cursor_position();
                content.perform(Action::Move(Motion::Up));
                if content.cursor_position() == prev {
                    break;
                }
            }
        }
        Action::Move(Motion::PageDown) => {
            for _ in 0..page_lines {
                let prev = content.cursor_position();
                content.perform(Action::Move(Motion::Down));
                if content.cursor_position() == prev {
                    break;
                }
            }
        }
        Action::Select(Motion::PageUp) => {
            for _ in 0..page_lines {
                let prev = content.cursor_position();
                content.perform(Action::Select(Motion::Up));
                if content.cursor_position() == prev {
                    break;
                }
            }
        }
        Action::Select(Motion::PageDown) => {
            for _ in 0..page_lines {
                let prev = content.cursor_position();
                content.perform(Action::Select(Motion::Down));
                if content.cursor_position() == prev {
                    break;
                }
            }
        }
        other => content.perform(other),
    }

    if !should_follow_cursor {
        return Task::none();
    }

    let (cursor_line, _) = content.cursor_position();
    let visual_row = estimate_visual_row_for_line(content, cursor_line, scroll.viewport_w, config);
    scroll.ensure_visual_row_visible(visual_row, config)
}

/// Scrolls the numbered editor so `line_index` sits near the upper-middle of the viewport.
pub(crate) fn scroll_numbered_editor_to_line<M>(
    content: &text_editor::Content,
    scroll: &mut CodeScrollState,
    line_index: usize,
    config: NumberedEditorConfig,
) -> Task<M> {
    let visual_row = estimate_visual_row_for_line(content, line_index, scroll.viewport_w, config);
    scroll.scroll_to_visual_row_centered(visual_row, config)
}

// ── Numbered Code Editor Widget ──────────────────────────────────────────────

/// Wraps a configured `TextEditor` in a synchronized scrollable container with a sticky
/// line-number gutter, active-line highlight, and optional boxed frame.
pub(crate) fn numbered_code_editor<'a, H, M>(
    content: &'a text_editor::Content,
    editor: text_editor::TextEditor<'a, H, M>,
    scroll: &CodeScrollState,
    config: NumberedEditorConfig,
    p: Palette,
    on_scroll: impl Fn(scrollable::Viewport) -> M + 'a,
) -> Element<'a, M>
where
    H: adv_text::Highlighter + 'a,
    M: 'a,
{
    let wrapping = if config.word_wrap {
        text::Wrapping::WordOrGlyph
    } else {
        text::Wrapping::None
    };

    let configured_editor: Element<'a, M> = editor
        .font(theme::CODE_FONT)
        .size(config.font_size)
        .line_height(text::LineHeight::Relative(config.line_height))
        .padding(config.padding)
        .wrapping(wrapping)
        .height(Length::Shrink)
        .style(theme::transparent_code_editor(p, !config.boxed))
        .into();

    let viewport_cell = Rc::new(Cell::new(Size::new(
        scroll.viewport_w.max(200.0),
        scroll.viewport_h.max(120.0),
    )));

    let canvas = Element::new(NumberedEditorCanvas {
        content,
        editor: configured_editor,
        viewport_cell: Rc::clone(&viewport_cell),
        config,
        p,
    });

    let direction = if config.word_wrap {
        theme::slim_overlay_vertical_direction()
    } else {
        theme::slim_both_direction()
    };

    let scroller: Element<'a, M> = scrollable(canvas)
        .id(scroll.id.clone())
        .width(Length::Fill)
        .height(Length::Fill)
        .direction(direction)
        .on_scroll(on_scroll)
        .style(theme::slim_scrollable(p))
        .into();

    let framed: Element<'a, M> = if config.boxed {
        container(scroller)
            .padding(1)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(p.surface_input)),
                border: Border {
                    color: p.border_subtle,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
            .into()
    } else {
        container(scroller)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Color(p.surface_input)),
                ..Default::default()
            })
            .into()
    };

    Element::new(ViewportMeasure {
        viewport_cell,
        inset: if config.boxed { 2.0 } else { 0.0 },
        content: framed,
    })
}

/// Captures the exact finite viewport dimensions before `scrollable` expands child limits
/// to infinity, allowing `NumberedEditorCanvas` to size both unwrapped width and minimum
/// vertical fill accurately.
struct ViewportMeasure<'a, M> {
    viewport_cell: Rc<Cell<Size>>,
    inset: f32,
    content: Element<'a, M>,
}

impl<'a, M> Widget<M, Theme, Renderer> for ViewportMeasure<'a, M> {
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
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &adv_layout::Limits,
    ) -> adv_layout::Node {
        let max = limits.max();
        let prev = self.viewport_cell.get();
        let vw = if max.width.is_finite() {
            (max.width - self.inset).max(80.0)
        } else {
            prev.width
        };
        let vh = if max.height.is_finite() {
            (max.height - self.inset).max(60.0)
        } else {
            prev.height
        };
        self.viewport_cell.set(Size::new(vw, vh));

        let child_node = self
            .content
            .as_widget()
            .layout(&mut tree.children[0], renderer, limits);
        adv_layout::Node::with_children(child_node.size(), vec![child_node])
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
        if let Some(child) = layout.children().next() {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                child,
                cursor,
                viewport,
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
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) -> event::Status {
        let Some(child) = layout.children().next() else {
            return event::Status::Ignored;
        };
        self.content.as_widget_mut().on_event(
            &mut tree.children[0],
            event,
            child,
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
        let Some(child) = layout.children().next() else {
            return mouse::Interaction::None;
        };
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            child,
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
        if let Some(child) = layout.children().next() {
            self.content.as_widget().operate(
                &mut tree.children[0],
                child,
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
    ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        let child = layout.children().next()?;
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            child,
            renderer,
            translation,
        )
    }
}

#[derive(Default)]
struct GutterLayoutState {
    /// Cache key for wrapped visual-row prefix sums:
    /// `(inner_w_tenths, font_size_tenths, line_h_tenths, line_count, content_hash)`
    wrap_key: Option<(u32, u32, u32, usize, u64)>,
    /// Prefix sums of visual rows (`len == line_count + 1`).
    /// Logical line `i` starts at visual row `prefix_rows[i]`.
    prefix_rows: Vec<u32>,
    /// True while a mouse drag started in the editor or gutter.
    dragging: bool,
}

impl GutterLayoutState {
    fn ensure_wrap_prefix(
        &mut self,
        content: &text_editor::Content,
        inner_w: f32,
        config: NumberedEditorConfig,
    ) {
        let line_count = content.line_count();
        let line_h = config.line_height_px();
        let hash = fingerprint_content(content);
        let key = (
            (inner_w * 10.0).round() as u32,
            (config.font_size * 10.0).round() as u32,
            (config.line_height * 100.0).round() as u32,
            line_count,
            hash,
        );
        if self.wrap_key == Some(key) && self.prefix_rows.len() == line_count + 1 {
            return;
        }

        self.prefix_rows.clear();
        self.prefix_rows.reserve(line_count + 1);
        self.prefix_rows.push(0);

        let fast_char_upper_bound = config.font_size * 0.65;
        let mut acc: u32 = 0;

        for line in content.lines() {
            let rows: u32 = if line.is_empty()
                || (line.is_ascii()
                    && !line.contains('\t')
                    && (line.len() as f32 * fast_char_upper_bound) < inner_w)
            {
                1
            } else {
                let para =
                    <Renderer as adv_text::Renderer>::Paragraph::with_text(adv_text::Text {
                        content: &line,
                        bounds: Size::new(inner_w, f32::INFINITY),
                        size: Pixels(config.font_size),
                        line_height: text::LineHeight::Relative(config.line_height),
                        font: theme::CODE_FONT,
                        horizontal_alignment: alignment::Horizontal::Left,
                        vertical_alignment: alignment::Vertical::Top,
                        shaping: text::Shaping::Advanced,
                        wrapping: text::Wrapping::WordOrGlyph,
                    });
                ((para.min_bounds().height / line_h).round() as u32).max(1)
            };
            acc = acc.saturating_add(rows);
            self.prefix_rows.push(acc);
        }

        self.wrap_key = Some(key);
    }
}

fn fingerprint_content(content: &text_editor::Content) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for line in content.lines() {
        for &b in line.as_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100000001b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn max_line_visual_cols(content: &text_editor::Content) -> usize {
    let mut best_cols = 0usize;
    for line in content.lines() {
        // Fast lower-bound skip: an ASCII line without tabs has `cols == len`.
        if line.len() * TAB_WIDTH <= best_cols {
            continue;
        }
        let cols = line_visual_cols(&line);
        if cols > best_cols {
            best_cols = cols;
        }
    }
    best_cols
}

struct NumberedEditorCanvas<'a, M> {
    content: &'a text_editor::Content,
    editor: Element<'a, M>,
    viewport_cell: Rc<Cell<Size>>,
    config: NumberedEditorConfig,
    p: Palette,
}

impl<'a, M> Widget<M, Theme, Renderer> for NumberedEditorCanvas<'a, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<GutterLayoutState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(GutterLayoutState::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.editor)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.editor));
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn layout(
        &self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &adv_layout::Limits,
    ) -> adv_layout::Node {
        let vp = self.viewport_cell.get();
        let line_count = self.content.line_count().max(1);
        let g_w = gutter_width(line_count, self.config.font_size);

        let (editor_w, overflows_x) = if self.config.word_wrap {
            let avail = if limits.max().width.is_finite() {
                limits.max().width
            } else {
                vp.width
            };
            ((avail - g_w).max(60.0), false)
        } else {
            let base_editor_w = (vp.width - g_w).max(80.0);
            let max_cols = max_line_visual_cols(self.content);
            let needed_w = (max_cols as f32 * (self.config.font_size * 0.62)
                + self.config.padding.horizontal()
                + 48.0)
                .ceil();
            let chosen = needed_w.max(base_editor_w);
            (chosen, g_w + chosen > vp.width + 0.5)
        };

        let min_inner_h = (vp.height - self.config.padding.vertical()).max(0.0);
        let child_limits = adv_layout::Limits::new(
            Size::new(editor_w, min_inner_h),
            Size::new(editor_w, f32::MAX),
        );

        let child_node = self
            .editor
            .as_widget()
            .layout(&mut tree.children[0], renderer, &child_limits)
            .move_to(Point::new(g_w, 0.0));

        if self.config.word_wrap {
            let inner_w = (editor_w - self.config.padding.horizontal()).max(1.0);
            let state = tree.state.downcast_mut::<GutterLayoutState>();
            state.ensure_wrap_prefix(self.content, inner_w, self.config);
        }

        let child_h = child_node.size().height;
        // Leave room for the overlay horizontal scrollbar when both scrollbars are active.
        let extra_bottom = if overflows_x && child_h > vp.height + 0.5 {
            10.0
        } else {
            0.0
        };
        let total_w = g_w + editor_w;
        let total_h = child_h.max(vp.height) + extra_bottom;

        adv_layout::Node::with_children(Size::new(total_w, total_h), vec![child_node])
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
        let bounds = layout.bounds();
        let Some(child_layout) = layout.children().next() else {
            return;
        };
        let child_bounds = child_layout.bounds();
        let line_count = self.content.line_count().max(1);
        let g_w = gutter_width(line_count, self.config.font_size);
        let line_h = self.config.line_height_px();
        let (cursor_line, _) = self.content.cursor_position();

        let vis_top = viewport.y.max(bounds.y);
        let vis_bottom = (viewport.y + viewport.height).min(bounds.y + bounds.height);
        let vis_h = (vis_bottom - vis_top).max(0.0);
        if vis_h <= 0.0 {
            return;
        }

        let state = tree.state.downcast_ref::<GutterLayoutState>();
        let flat_quad = |rect: Rectangle| renderer::Quad {
            bounds: rect,
            border: Border::default(),
            shadow: Shadow::default(),
        };

        let sticky_gutter_x = bounds.x.max(viewport.x);
        let gutter_right_x = sticky_gutter_x + g_w;
        let vis_right = (viewport.x + viewport.width).min(bounds.x + bounds.width);
        let code_clip_w = (vis_right - gutter_right_x).max(0.0);

        let active_visual_span =
            if self.config.word_wrap && state.prefix_rows.len() > cursor_line + 1 {
                let start = state.prefix_rows[cursor_line] as usize;
                let end = state.prefix_rows[cursor_line + 1] as usize;
                (start, end.saturating_sub(start).max(1))
            } else {
                (cursor_line, 1)
            };

        // 1–3. Paint editor background, active-line band, and syntax-highlighted TextEditor
        // inside a hardware-scissored layer strictly to the right of the sticky gutter so
        // horizontally-scrolled code glyphs never bleed across the line numbers.
        if code_clip_w > 0.0 {
            let code_clip = Rectangle {
                x: gutter_right_x,
                y: vis_top,
                width: code_clip_w,
                height: vis_h,
            };
            let editor_cursor = match cursor.position() {
                Some(pos) if pos.x < gutter_right_x => mouse::Cursor::Unavailable,
                _ => cursor,
            };
            renderer.with_layer(code_clip, |renderer| {
                renderer.fill_quad(flat_quad(code_clip), Background::Color(self.p.surface_input));

                if self.config.highlight_active_line && cursor_line < line_count {
                    let band_y =
                        bounds.y + self.config.padding.top + active_visual_span.0 as f32 * line_h;
                    let band_h = active_visual_span.1 as f32 * line_h;
                    let band_rect = Rectangle {
                        x: child_bounds.x,
                        y: band_y,
                        width: child_bounds.width,
                        height: band_h,
                    };
                    if let Some(clipped) = band_rect.intersection(&code_clip) {
                        let band_color = Color {
                            a: 0.28,
                            ..self.p.fill_selected
                        };
                        renderer.fill_quad(flat_quad(clipped), Background::Color(band_color));
                    }
                }

                self.editor.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    child_layout,
                    editor_cursor,
                    &code_clip,
                );
            });
        }

        // 4–5. Paint Sticky Left Line-Number Gutter in its own hardware-clipped layer on top.
        let gutter_rect = Rectangle {
            x: sticky_gutter_x,
            y: vis_top,
            width: g_w,
            height: vis_h,
        };
        let local_scroll_top = (viewport.y - bounds.y).max(0.0);
        let vis_lines = if self.config.word_wrap && state.prefix_rows.len() == line_count + 1 {
            visible_wrapped_line_range(
                &state.prefix_rows,
                local_scroll_top,
                viewport.height,
                self.config.padding.top,
                line_h,
            )
        } else {
            visible_unwrapped_line_range(
                line_count,
                local_scroll_top,
                viewport.height,
                self.config.padding.top,
                line_h,
            )
        };

        let text_right_x = gutter_right_x - GUTTER_PAD_RIGHT;
        let inner_text_w = (g_w - GUTTER_PAD_LEFT - GUTTER_PAD_RIGHT).max(16.0);
        let muted_num = Color {
            a: 0.78,
            ..self.p.text_muted
        };

        renderer.with_layer(gutter_rect, |renderer| {
            renderer.fill_quad(
                flat_quad(gutter_rect),
                Background::Color(self.p.surface_sidebar),
            );

            // Subtle 1px right divider separating gutter and code canvas.
            renderer.fill_quad(
                flat_quad(Rectangle {
                    x: gutter_right_x - 1.0,
                    y: vis_top,
                    width: 1.0,
                    height: vis_h,
                }),
                Background::Color(self.p.border_subtle),
            );

            // Active line gutter fill + 2px left accent bar.
            if cursor_line < line_count {
                let active_y =
                    bounds.y + self.config.padding.top + active_visual_span.0 as f32 * line_h;
                let active_h = active_visual_span.1 as f32 * line_h;
                let cell_rect = Rectangle {
                    x: sticky_gutter_x,
                    y: active_y,
                    width: (g_w - 1.0).max(0.0),
                    height: active_h,
                };
                if let Some(clipped) = cell_rect.intersection(&gutter_rect) {
                    let cell_bg = Color {
                        a: if self.config.highlight_active_line {
                            0.42
                        } else {
                            0.22
                        },
                        ..self.p.fill_selected
                    };
                    renderer.fill_quad(flat_quad(clipped), Background::Color(cell_bg));
                    if self.config.highlight_active_line {
                        let bar_rect = Rectangle {
                            x: sticky_gutter_x,
                            y: clipped.y,
                            width: 2.0,
                            height: clipped.height,
                        };
                        renderer.fill_quad(
                            flat_quad(bar_rect),
                            Background::Color(self.p.accent_primary),
                        );
                    }
                }
            }

            // Render ONLY visible line numbers (O(visible_lines)).
            for line_idx in vis_lines {
                let visual_row = if self.config.word_wrap && state.prefix_rows.len() > line_idx {
                    state.prefix_rows[line_idx] as usize
                } else {
                    line_idx
                };
                let line_y = bounds.y + self.config.padding.top + visual_row as f32 * line_h;
                let is_active = line_idx == cursor_line;
                let color = if is_active {
                    self.p.text_primary
                } else {
                    muted_num
                };

                adv_text::Renderer::fill_text(
                    renderer,
                    adv_text::Text {
                        content: (line_idx + 1).to_string(),
                        bounds: Size::new(inner_text_w, line_h),
                        size: Pixels(self.config.font_size),
                        line_height: text::LineHeight::Absolute(Pixels(line_h)),
                        font: theme::CODE_FONT,
                        horizontal_alignment: alignment::Horizontal::Right,
                        vertical_alignment: alignment::Vertical::Top,
                        shaping: text::Shaping::Basic,
                        wrapping: text::Wrapping::None,
                    },
                    Point::new(text_right_x, line_y),
                    color,
                    gutter_rect,
                );
            }
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
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) -> event::Status {
        let Some(child_layout) = layout.children().next() else {
            return event::Status::Ignored;
        };
        let bounds = layout.bounds();
        let child_bounds = child_layout.bounds();
        let line_count = self.content.line_count().max(1);
        let g_w = gutter_width(line_count, self.config.font_size);
        let sticky_gutter_x = bounds.x.max(viewport.x);
        let gutter_right_x = sticky_gutter_x + g_w;

        let state = tree.state.downcast_mut::<GutterLayoutState>();
        let in_gutter = cursor.position().is_some_and(|pos| {
            pos.x >= sticky_gutter_x
                && pos.x < gutter_right_x
                && pos.y >= bounds.y
                && pos.y <= bounds.y + bounds.height
        });

        let mapped_cursor = match &event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if in_gutter && let Some(pos) = cursor.position() {
                    state.dragging = true;
                    mouse::Cursor::Available(Point::new(
                        child_bounds.x + self.config.padding.left,
                        pos.y,
                    ))
                } else {
                    if cursor.is_over(child_bounds) {
                        state.dragging = true;
                    }
                    cursor
                }
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.dragging = false;
                cursor
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if in_gutter && let Some(pos) = cursor.position() {
                    if state.dragging {
                        mouse::Cursor::Available(Point::new(
                            child_bounds.x + self.config.padding.left,
                            pos.y,
                        ))
                    } else {
                        mouse::Cursor::Unavailable
                    }
                } else {
                    cursor
                }
            }
            _ => {
                if in_gutter {
                    mouse::Cursor::Unavailable
                } else {
                    cursor
                }
            }
        };

        self.editor.as_widget_mut().on_event(
            &mut tree.children[0],
            event,
            child_layout,
            mapped_cursor,
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
        let bounds = layout.bounds();
        let line_count = self.content.line_count().max(1);
        let g_w = gutter_width(line_count, self.config.font_size);
        let sticky_gutter_x = bounds.x.max(viewport.x);
        let gutter_right_x = sticky_gutter_x + g_w;

        if cursor.position().is_some_and(|pos| {
            pos.x >= sticky_gutter_x
                && pos.x < gutter_right_x
                && pos.y >= bounds.y
                && pos.y <= bounds.y + bounds.height
        }) {
            return mouse::Interaction::Pointer;
        }

        self.editor.as_widget().mouse_interaction(
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
            self.editor.as_widget().operate(
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
    ) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        let child_layout = layout.children().next()?;
        self.editor.as_widget_mut().overlay(
            &mut tree.children[0],
            child_layout,
            renderer,
            translation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gutter_width_keeps_stable_3_digit_min_and_expands_for_large_files() {
        let w1 = gutter_width(1, 13.0);
        let w99 = gutter_width(99, 13.0);
        let w999 = gutter_width(999, 13.0);
        let w1000 = gutter_width(1_000, 13.0);
        let w10000 = gutter_width(10_000, 13.0);
        let w100000 = gutter_width(100_000, 13.0);

        assert_eq!(w1, w99);
        assert_eq!(w99, w999);
        assert!(w1 >= GUTTER_MIN_WIDTH);
        assert!(w1000 > w999);
        assert!(w10000 > w1000);
        assert!(w100000 > w10000);
        assert!(gutter_width(50, 18.0) > gutter_width(50, 13.0));
    }

    #[test]
    fn line_visual_cols_expands_tab_stops_accurately() {
        assert_eq!(line_visual_cols(""), 0);
        assert_eq!(line_visual_cols("fn main() {}"), 12);
        assert_eq!(line_visual_cols("\tfoo"), 8 + 3);
        assert_eq!(line_visual_cols("ab\tcd"), 8 + 2);
        assert_eq!(line_visual_cols("12345678\tx"), 16 + 1);
        assert_eq!(line_visual_cols("xin chào"), 8);
    }

    #[test]
    fn visible_unwrapped_range_clips_and_pads_safely() {
        assert_eq!(visible_unwrapped_line_range(0, 0.0, 400.0, 12.0, 20.0), 0..0);
        assert_eq!(
            visible_unwrapped_line_range(500, 0.0, 100.0, 12.0, 20.0),
            0..6
        );
        // Scrolled to y = 212 -> first exact line = (212 - 12)/20 = 10 -> padded to 9
        // bottom = 212 + 100 - 12 = 300 -> 300/20 = 15 -> padded to 16
        assert_eq!(
            visible_unwrapped_line_range(500, 212.0, 100.0, 12.0, 20.0),
            9..16
        );
        // Clamped at end of file
        assert_eq!(
            visible_unwrapped_line_range(12, 200.0, 200.0, 12.0, 20.0),
            8..12
        );
    }

    #[test]
    fn visible_wrapped_range_uses_prefix_rows() {
        // Line 0: 1 row (0..1)
        // Line 1: 3 rows (1..4)
        // Line 2: 1 row (4..5)
        // Line 3: 2 rows (5..7)
        // Line 4: 1 row (7..8)
        let prefix = [0u32, 1, 4, 5, 7, 8];
        // Viewport showing visual rows 2..5 (y = 40..100 with top_pad = 0, line_h = 20)
        // Padded rows: 1..6 -> lines 1, 2, 3 (and maybe 4 at boundary)
        let range = visible_wrapped_line_range(&prefix, 40.0, 60.0, 0.0, 20.0);
        assert!(range.contains(&1));
        assert!(range.contains(&2));
        assert!(range.contains(&3));
        assert!(!range.contains(&0));
    }

    #[test]
    fn ensure_cursor_scroll_y_stays_noop_when_visible_and_scrolls_when_outside() {
        // Viewport [100..500], line_h = 20, top_pad = 12, margin = 30
        // Row 10 -> y = [212..232] -> comfortably inside [130..470]
        assert_eq!(
            compute_ensure_cursor_scroll_y(100.0, 400.0, 10, 20.0, 12.0),
            None
        );

        // Row 3 -> y = [72..92] -> above 130 -> scroll up to 72 - 30 = 42
        assert_eq!(
            compute_ensure_cursor_scroll_y(100.0, 400.0, 3, 20.0, 12.0),
            Some(42.0)
        );

        // Row 0 -> y = [12..32] -> clamps to 0.0
        assert_eq!(
            compute_ensure_cursor_scroll_y(100.0, 400.0, 0, 20.0, 12.0),
            Some(0.0)
        );

        // Row 25 -> y = [512..532] -> below 470 -> scroll down to 532 + 30 - 400 = 162
        assert_eq!(
            compute_ensure_cursor_scroll_y(100.0, 400.0, 25, 20.0, 12.0),
            Some(162.0)
        );
    }

    #[test]
    fn centered_scroll_y_clamps_near_top_and_centers_deep_lines() {
        assert_eq!(compute_centered_scroll_y(600.0, 0, 20.0, 12.0), 0.0);
        assert_eq!(compute_centered_scroll_y(600.0, 2, 20.0, 12.0), 0.0);
        // Row 50 -> y = 1012 -> 1012 - 210 = 802
        assert_eq!(compute_centered_scroll_y(600.0, 50, 20.0, 12.0), 802.0);
    }
}
