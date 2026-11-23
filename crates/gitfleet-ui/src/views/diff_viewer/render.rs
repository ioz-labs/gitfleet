//! Virtualized Unified Diff, Side-by-Side Diff, and Rich File Preview renderers
//! shared between the Host Diff Panel and GitFleet Studio.

use super::layout::{DiffLayoutCache, SplitCodeCell, SplitRow, UnifiedRow, visible_row_range};
use super::selection::{diff_code_char_width, row_selection_range};
use super::widgets::{
    DiffOverviewBar, DiffRowSelConfig, OverviewSide, SplitSideVisual, change_colors,
    diff_code_row_box, hidden_gap_bar, render_highlighted_code, split_diff_scroll,
    split_paired_code_row, sticky_x_row,
};
use super::{
    DIFF_HIDDEN_H, DIFF_LINE_H, DIFF_OVERSCAN_PX, DIFF_SEP_H, LINE_NUM_W, OVERVIEW_BAR_W,
    TextDiffRenderCtx,
};
use crate::app::{DiffSelectionSide, DiffTextSelection, Message, PendingConfirm};
use crate::icons;
use crate::preview::PreviewState;
use crate::syntax::TokenFormat;
use crate::theme::{self, Palette, ThemeMode, type_scale};
use crate::views::slim_scroll_h;
use gitfleet_git::DiffChangeKind;
use iced::widget::canvas::Canvas;
use iced::widget::{Space, button, column, container, row, text, text_editor};
use iced::{Alignment, Color, Element, Length, alignment};
use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

fn build_split_side_visual<'a>(
    cell: &SplitCodeCell,
    syntax_spans: &[(Range<usize>, TokenFormat)],
    side: DiffSelectionSide,
    row_idx: usize,
    selection: Option<DiffTextSelection>,
    p: Palette,
) -> SplitSideVisual<'a> {
    let (bg, border_c, word_hl_bg, fg) = match cell.kind {
        Some(DiffChangeKind::Added) => (
            p.diff_add_line_bg,
            Some(p.git_added),
            Some(p.diff_add_word_bg),
            p.text_primary,
        ),
        Some(DiffChangeKind::Removed) => (
            p.diff_del_line_bg,
            Some(p.git_deleted),
            Some(p.diff_del_word_bg),
            p.text_primary,
        ),
        Some(DiffChangeKind::Modified) => (
            Color {
                a: if p.is_dark { 0.13 } else { 0.11 },
                ..p.git_modified
            },
            Some(p.git_modified),
            Some(Color {
                a: if p.is_dark { 0.30 } else { 0.24 },
                ..p.git_modified
            }),
            p.text_primary,
        ),
        None => (Color::TRANSPARENT, None, None, p.text_secondary),
    };
    let top_border = if cell.is_first { border_c } else { None };
    let bottom_border = if cell.is_last { border_c } else { None };
    let code_elem = container(render_highlighted_code(
        &cell.text,
        &cell.spans,
        syntax_spans,
        fg,
        word_hl_bg,
    ))
    .height(Length::Fixed(DIFF_LINE_H))
    .align_y(alignment::Vertical::Center)
    .into();

    let char_len = cell.text.chars().count();
    SplitSideVisual {
        line_no: cell.line_no,
        line_text: cell.text.clone(),
        bg,
        top_border,
        bottom_border,
        sel_range: row_selection_range(selection, side, row_idx, char_len),
        code: code_elem,
    }
}

pub(crate) fn render_unified_diff_with<'a>(
    ctx: TextDiffRenderCtx<'a>,
    p: Palette,
) -> Element<'a, Message> {
    if ctx.parsed.raw.is_empty() {
        return theme::empty_state(
            p,
            icons::file_code(theme::icon_size::LG, p.text_muted),
            "No diff selected",
            "Select a file from Changed, Dir, or All Files to inspect diff, preview, or edit.",
            None,
        );
    }

    let cache: &DiffLayoutCache = ctx.cache;
    let viewport_h = ctx.unified_viewport_h.max(600.0);

    if ctx.parsed.blocks.is_empty() {
        let total_lines = cache.raw_lines.len();
        let total_h = total_lines as f32 * DIFF_LINE_H;
        let first = (((ctx.unified_scroll_y - DIFF_OVERSCAN_PX).max(0.0) / DIFF_LINE_H).floor()
            as usize)
            .min(total_lines);
        let last = (((ctx.unified_scroll_y + viewport_h + DIFF_OVERSCAN_PX) / DIFF_LINE_H).ceil()
            as usize)
            .min(total_lines)
            .max(first);
        let top_spacer = first as f32 * DIFF_LINE_H;
        let bottom_spacer = (total_h - last as f32 * DIFF_LINE_H).max(0.0);

        let mut raw_col = column![Space::with_height(Length::Fixed(top_spacer))].spacing(0);
        let raw_dragging = ctx
            .selection
            .is_some_and(|s| s.dragging && s.side == DiffSelectionSide::Raw);

        for i in first..last {
            let clean = &cache.raw_lines[i];
            let char_len = clean.chars().count();
            let gutter = container(
                text(format!("{}", i + 1))
                    .size(type_scale::CAPTION)
                    .font(theme::CODE_FONT)
                    .color(p.text_muted)
                    .wrapping(text::Wrapping::None),
            )
            .width(Length::Fixed(LINE_NUM_W))
            .height(Length::Fixed(DIFF_LINE_H))
            .padding([0, 6])
            .align_x(alignment::Horizontal::Right)
            .align_y(alignment::Vertical::Center);
            let code_cell = container(
                text(clean.clone())
                    .size(type_scale::CODE)
                    .font(theme::CODE_FONT)
                    .color(p.text_secondary)
                    .wrapping(text::Wrapping::None),
            )
            .width(Length::Fill)
            .height(Length::Fixed(DIFF_LINE_H))
            .padding([0, 8])
            .align_y(alignment::Vertical::Center);
            let sel_cfg = DiffRowSelConfig {
                side: DiffSelectionSide::Raw,
                row_idx: i,
                code_start_x: LINE_NUM_W + 8.0,
                line_text: clean.clone(),
                sel_range: row_selection_range(ctx.selection, DiffSelectionSide::Raw, i, char_len),
                is_dragging_same_side: raw_dragging,
                sel_bg: Color {
                    a: 0.42,
                    ..p.accent_primary
                },
                sel_overlay: Color {
                    a: 0.16,
                    ..p.accent_hover
                },
            };
            raw_col = raw_col.push(diff_code_row_box(
                Color::TRANSPARENT,
                None,
                None,
                None,
                None,
                LINE_NUM_W,
                p.border_subtle,
                sel_cfg,
                row![gutter, code_cell].align_y(Alignment::Center),
            ));
        }
        raw_col = raw_col.push(Space::with_height(Length::Fixed(bottom_spacer)));
        let min_w = (cache.raw_max_chars as f32 * 8.0 + LINE_NUM_W + 32.0).clamp(380.0, 2800.0);
        return theme::responsive_hscroll_synced(
            raw_col,
            min_w,
            ctx.unified_scroll_id,
            Message::DiffUnifiedScrolled,
            p,
        );
    }

    let layout = &cache.layout;
    let (vis_range, top_spacer, bottom_spacer) = visible_row_range(
        &cache.unified_y_offsets,
        ctx.unified_scroll_y,
        viewport_h,
        DIFF_OVERSCAN_PX,
    );

    let mut lines_col = column![Space::with_height(Length::Fixed(top_spacer))].spacing(0);
    let gutter_total = LINE_NUM_W * 2.0 + 16.0;
    let unified_dragging = ctx
        .selection
        .is_some_and(|s| s.dragging && s.side == DiffSelectionSide::Unified);

    for row_idx in vis_range {
        let item = &layout.unified_rows[row_idx];
        let syntax_spans = cache
            .unified_syntax
            .get(row_idx)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        match item {
            UnifiedRow::HunkHeader {
                header,
                patch,
                kind,
            } => {
                let (_, accent, _) = change_colors(*kind, p);
                let mut bar = row![
                    text(header.clone())
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(accent)
                        .wrapping(text::Wrapping::None),
                    Space::with_width(Length::Fill),
                ]
                .spacing(6)
                .align_y(Alignment::Center);

                if ctx.is_working_file {
                    if ctx.is_staged {
                        bar = bar.push(
                            button(text("Unstage Hunk").size(type_scale::BADGE))
                                .padding([2, 8])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::ApplyHunk {
                                    patch: patch.clone(),
                                    cached: true,
                                    reverse: true,
                                }),
                        );
                    } else {
                        bar = bar
                            .push(
                                button(text("Stage Hunk").size(type_scale::BADGE))
                                    .padding([2, 8])
                                    .style(theme::primary_btn(p))
                                    .on_press(Message::ApplyHunk {
                                        patch: patch.clone(),
                                        cached: true,
                                        reverse: false,
                                    }),
                            )
                            .push(
                                button(
                                    text("Discard Hunk")
                                        .size(type_scale::BADGE)
                                        .color(p.danger_fg),
                                )
                                .padding([2, 8])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::RequestConfirm(PendingConfirm::discard_hunk(
                                    ctx.diff_title,
                                    header,
                                    patch.clone(),
                                ))),
                            );
                    }
                }

                lines_col = lines_col.push(
                    container(bar)
                        .width(Length::Fill)
                        .height(Length::Fixed(DIFF_SEP_H))
                        .padding([0, 8])
                        .align_y(alignment::Vertical::Center)
                        .style(theme::diff_line_box(p.fill_selected, accent)),
                );
            }
            UnifiedRow::Code {
                old_line,
                new_line,
                sign,
                text: code,
                spans,
                kind,
            } => {
                let is_del = *sign == '-' || matches!(kind, Some(DiffChangeKind::Removed));
                let is_add = *sign == '+' || matches!(kind, Some(DiffChangeKind::Added));
                let (bg, sign_color, word_hl_bg, fg) = if is_add {
                    (
                        p.diff_add_line_bg,
                        p.git_added,
                        Some(p.diff_add_word_bg),
                        p.text_primary,
                    )
                } else if is_del {
                    (
                        p.diff_del_line_bg,
                        p.git_deleted,
                        Some(p.diff_del_word_bg),
                        p.text_primary,
                    )
                } else if kind.is_some() {
                    (
                        p.diff_hunk_bg,
                        p.accent_fg,
                        Some(p.diff_hunk_bg),
                        p.text_primary,
                    )
                } else {
                    (Color::TRANSPARENT, p.text_muted, None, p.text_secondary)
                };
                let old_s = old_line.map(|n| n.to_string()).unwrap_or_default();
                let new_s = new_line.map(|n| n.to_string()).unwrap_or_default();

                let row_inner = row![
                    container(
                        text(old_s)
                            .size(type_scale::CAPTION)
                            .font(theme::CODE_FONT)
                            .color(p.text_muted)
                            .wrapping(text::Wrapping::None),
                    )
                    .width(Length::Fixed(LINE_NUM_W))
                    .height(Length::Fixed(DIFF_LINE_H))
                    .padding([0, 5])
                    .align_x(alignment::Horizontal::Right)
                    .align_y(alignment::Vertical::Center),
                    container(
                        text(new_s)
                            .size(type_scale::CAPTION)
                            .font(theme::CODE_FONT)
                            .color(p.text_muted)
                            .wrapping(text::Wrapping::None),
                    )
                    .width(Length::Fixed(LINE_NUM_W))
                    .height(Length::Fixed(DIFF_LINE_H))
                    .padding([0, 5])
                    .align_x(alignment::Horizontal::Right)
                    .align_y(alignment::Vertical::Center),
                    container(
                        text(sign.to_string())
                            .size(type_scale::CODE)
                            .font(theme::CODE_FONT)
                            .color(sign_color)
                            .wrapping(text::Wrapping::None),
                    )
                    .width(Length::Fixed(16.0))
                    .height(Length::Fixed(DIFF_LINE_H))
                    .align_x(alignment::Horizontal::Center)
                    .align_y(alignment::Vertical::Center),
                    container(render_highlighted_code(
                        code,
                        spans,
                        syntax_spans,
                        fg,
                        word_hl_bg,
                    ))
                    .width(Length::Fill)
                    .height(Length::Fixed(DIFF_LINE_H))
                    .padding([0, 6])
                    .align_y(alignment::Vertical::Center),
                ]
                .align_y(Alignment::Center);

                let char_len = code.chars().count();
                let sel_cfg = DiffRowSelConfig {
                    side: DiffSelectionSide::Unified,
                    row_idx,
                    code_start_x: gutter_total + 6.0,
                    line_text: code.clone(),
                    sel_range: row_selection_range(
                        ctx.selection,
                        DiffSelectionSide::Unified,
                        row_idx,
                        char_len,
                    ),
                    is_dragging_same_side: unified_dragging,
                    sel_bg: Color {
                        a: 0.42,
                        ..p.accent_primary
                    },
                    sel_overlay: Color {
                        a: 0.16,
                        ..p.accent_hover
                    },
                };

                lines_col = lines_col.push(diff_code_row_box(
                    bg,
                    None,
                    None,
                    None,
                    None,
                    gutter_total,
                    p.border_subtle,
                    sel_cfg,
                    row_inner,
                ));
            }
            UnifiedRow::Hidden {
                gap_id,
                hidden_count,
                can_expand_top,
                can_expand_bottom,
            } => {
                lines_col = lines_col.push(hidden_gap_bar(
                    *gap_id,
                    *hidden_count,
                    *can_expand_top,
                    *can_expand_bottom,
                    false,
                    p,
                ));
            }
            UnifiedRow::Separator(hdr) => {
                lines_col = lines_col.push(
                    container(
                        text(theme::sanitize_code_line(hdr))
                            .size(type_scale::CAPTION)
                            .font(theme::CODE_FONT)
                            .color(p.accent_fg)
                            .wrapping(text::Wrapping::None),
                    )
                    .width(Length::Fill)
                    .height(Length::Fixed(DIFF_SEP_H))
                    .padding([0, 8])
                    .align_y(alignment::Vertical::Center)
                    .style(theme::diff_line_box(p.fill_selected, p.accent_fg)),
                );
            }
        }
    }

    lines_col = lines_col.push(Space::with_height(Length::Fixed(bottom_spacer)));
    let min_w =
        (layout.max_unified_chars as f32 * 8.0 + gutter_total + 40.0).clamp(420.0, 2800.0);
    let scroller = theme::responsive_hscroll_synced(
        lines_col,
        min_w,
        ctx.unified_scroll_id,
        Message::DiffUnifiedScrolled,
        p,
    );
    let overview: Element<'_, Message> = Canvas::new(DiffOverviewBar {
        spans: layout.connectors.clone(),
        side: OverviewSide::Unified,
        total_h: layout.total_unified_h,
        scroll_y: ctx.unified_scroll_y,
        viewport_h,
        active_idx: ctx.active_change,
        p,
    })
    .width(Length::Fixed(OVERVIEW_BAR_W))
    .height(Length::Fill)
    .into();

    row![scroller, overview]
        .spacing(0)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

pub(crate) fn render_side_by_side_diff_with<'a>(
    ctx: TextDiffRenderCtx<'a>,
    p: Palette,
) -> Element<'a, Message> {
    if ctx.parsed.blocks.is_empty() {
        return render_unified_diff_with(ctx, p);
    }

    let cache = ctx.cache;
    let layout = &cache.layout;
    let viewport_h = ctx.left_viewport_h.max(600.0);
    let viewport_w = Rc::new(Cell::new(800.0));

    let (vis_range, top_spacer, bot_spacer) = visible_row_range(
        &cache.split_y_offsets,
        ctx.left_scroll_y,
        viewport_h,
        DIFF_OVERSCAN_PX,
    );

    let dragging_side = ctx
        .selection
        .filter(|s| {
            s.dragging && matches!(s.side, DiffSelectionSide::Left | DiffSelectionSide::Right)
        })
        .map(|s| s.side);

    let mut split_col = column![Space::with_height(Length::Fixed(top_spacer))].spacing(0);
    for idx in vis_range {
        let item = &layout.split_rows[idx];
        match item {
            SplitRow::PairedCode {
                left,
                right,
                block_kind,
                block_is_first,
            } => {
                let left_syn = cache
                    .left_syntax
                    .get(idx)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let right_syn = cache
                    .right_syntax
                    .get(idx)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);

                let left_vis = left.as_ref().map(|cell| {
                    build_split_side_visual(
                        cell,
                        left_syn,
                        DiffSelectionSide::Left,
                        idx,
                        ctx.selection,
                        p,
                    )
                });
                let right_vis = right.as_ref().map(|cell| {
                    build_split_side_visual(
                        cell,
                        right_syn,
                        DiffSelectionSide::Right,
                        idx,
                        ctx.selection,
                        p,
                    )
                });

                split_col = split_col.push(split_paired_code_row(
                    idx,
                    left_vis,
                    right_vis,
                    *block_kind,
                    *block_is_first,
                    dragging_side,
                    Rc::clone(&viewport_w),
                    p,
                ));
            }
            SplitRow::Hidden {
                gap_id,
                hidden_count,
                can_expand_top,
                can_expand_bottom,
            } => {
                split_col = split_col.push(sticky_x_row(
                    DIFF_HIDDEN_H,
                    Rc::clone(&viewport_w),
                    hidden_gap_bar(
                        *gap_id,
                        *hidden_count,
                        *can_expand_top,
                        *can_expand_bottom,
                        true,
                        p,
                    ),
                ));
            }
            SplitRow::Separator(hdr) => {
                split_col = split_col.push(sticky_x_row(
                    DIFF_SEP_H,
                    Rc::clone(&viewport_w),
                    container(
                        text(theme::sanitize_code_line(hdr))
                            .size(type_scale::CAPTION)
                            .font(theme::CODE_FONT)
                            .color(p.accent_fg)
                            .wrapping(text::Wrapping::None),
                    )
                    .width(Length::Fill)
                    .height(Length::Fixed(DIFF_SEP_H))
                    .padding([0, 8])
                    .align_y(alignment::Vertical::Center)
                    .style(theme::diff_line_box(p.fill_selected, p.accent_fg)),
                ));
            }
        }
    }
    split_col = split_col.push(Space::with_height(Length::Fixed(bot_spacer)));

    let char_w = diff_code_char_width().max(7.8);
    let min_code_w = (layout.max_side_chars as f32 * char_w + 32.0).clamp(160.0, 4800.0);

    let split_pane = split_diff_scroll(
        split_col,
        min_code_w,
        viewport_w,
        layout.connectors.clone(),
        ctx.left_scroll_y,
        ctx.active_change,
        ctx.left_scroll_id,
        Message::DiffLeftScrolled,
        p,
    );

    let left_overview: Element<'_, Message> = Canvas::new(DiffOverviewBar {
        spans: layout.connectors.clone(),
        side: OverviewSide::Left,
        total_h: layout.total_split_h,
        scroll_y: ctx.left_scroll_y,
        viewport_h,
        active_idx: ctx.active_change,
        p,
    })
    .width(Length::Fixed(OVERVIEW_BAR_W))
    .height(Length::Fill)
    .into();

    let right_overview: Element<'_, Message> = Canvas::new(DiffOverviewBar {
        spans: layout.connectors.clone(),
        side: OverviewSide::Right,
        total_h: layout.total_split_h,
        scroll_y: ctx.left_scroll_y,
        viewport_h,
        active_idx: ctx.active_change,
        p,
    })
    .width(Length::Fixed(OVERVIEW_BAR_W))
    .height(Length::Fill)
    .into();

    row![left_overview, split_pane, right_overview]
        .spacing(0)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

pub(crate) fn render_file_preview<'a>(
    state: &'a PreviewState,
    _editor: &'a text_editor::Content,
    scroll: &crate::views::code_editor::CodeScrollState,
    config: crate::views::code_editor::NumberedEditorConfig,
    raw_mode: bool,
    theme_mode: ThemeMode,
    p: Palette,
) -> Element<'a, Message> {
    if state.uses_selectable_editor(raw_mode) {
        let mut outer = column![].spacing(6).height(Length::Fill);
        match state {
            PreviewState::Json {
                valid,
                error: Some(err),
                ..
            } if !valid => {
                outer = outer.push(
                    container(
                        text(format!("Invalid JSON: {err}"))
                            .size(type_scale::CAPTION)
                            .color(p.danger_fg),
                    )
                    .padding([4, 8])
                    .width(Length::Fill)
                    .style(theme::pill_box(p.danger_bg, p.danger_fg, p.danger_fg)),
                );
            }
            PreviewState::Json { valid: true, .. } => {
                outer = outer.push(
                    text("Valid JSON — formatted")
                        .size(type_scale::CAPTION)
                        .color(p.success_fg),
                );
            }
            PreviewState::Config { kind, content, .. } => {
                outer = outer.push(
                    text(format!(
                        "{kind} Preview  •  {} lines",
                        content.bytes().filter(|&b| b == b'\n').count() + 1
                    ))
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
                );
            }
            PreviewState::Code { lang, content, .. } => {
                outer = outer.push(
                    text(format!(
                        "{lang}  •  {} lines",
                        content.bytes().filter(|&b| b == b'\n').count() + 1
                    ))
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
                );
            }
            _ => {}
        }

        let Some(doc) = state.highlighted_doc(theme_mode) else {
            return outer.into();
        };

        let row_h = config.line_height_px().max(DIFF_LINE_H).ceil();
        let total_lines = doc.lines.len();
        let total_h = total_lines as f32 * row_h;
        let viewport_h = scroll.viewport_h.max(600.0);
        let first = (((scroll.scroll_y - DIFF_OVERSCAN_PX).max(0.0) / row_h).floor() as usize)
            .min(total_lines);
        let last = (((scroll.scroll_y + viewport_h + DIFF_OVERSCAN_PX) / row_h).ceil() as usize)
            .min(total_lines)
            .max(first);
        let top_spacer = first as f32 * row_h;
        let bottom_spacer = (total_h - last as f32 * row_h).max(0.0);
        let gutter_w = crate::views::code_editor::gutter_width(total_lines, config.font_size);

        let mut lines_col = column![Space::with_height(Length::Fixed(top_spacer))].spacing(0);
        for i in first..last {
            let line = &doc.lines[i];
            let gutter = container(
                text(format!("{}", i + 1))
                    .size(config.font_size.min(type_scale::CAPTION))
                    .font(theme::CODE_FONT)
                    .color(p.text_muted)
                    .wrapping(text::Wrapping::None),
            )
            .width(Length::Fixed(gutter_w))
            .height(Length::Fixed(row_h))
            .padding([0, 8])
            .align_x(alignment::Horizontal::Right)
            .align_y(alignment::Vertical::Center)
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(p.surface_sidebar)),
                ..Default::default()
            });

            let code_el: Element<'_, Message> = if line.spans.is_empty() {
                text(line.text.clone())
                    .size(config.font_size)
                    .font(theme::CODE_FONT)
                    .color(p.text_primary)
                    .wrapping(text::Wrapping::None)
                    .into()
            } else {
                render_highlighted_code(&line.text, &[], &line.spans, p.text_primary, None)
            };

            let code_cell = container(code_el)
                .width(Length::Fill)
                .height(Length::Fixed(row_h))
                .padding([0, 10])
                .align_y(alignment::Vertical::Center);

            let sep = container(Space::with_width(Length::Fixed(1.0)))
                .width(Length::Fixed(1.0))
                .height(Length::Fixed(row_h))
                .style(move |_| container::Style {
                    background: Some(iced::Background::Color(p.border_subtle)),
                    ..Default::default()
                });

            lines_col = lines_col.push(
                row![gutter, sep, code_cell]
                    .spacing(0)
                    .height(Length::Fixed(row_h))
                    .align_y(Alignment::Center),
            );
        }
        lines_col = lines_col.push(Space::with_height(Length::Fixed(bottom_spacer)));

        let min_w = (doc.max_chars as f32 * (config.font_size * 0.62) + gutter_w + 36.0)
            .clamp(380.0, 3200.0);
        let scroller = theme::responsive_hscroll_synced(
            lines_col,
            min_w,
            scroll.id.clone(),
            Message::PreviewScrolled,
            p,
        );

        return outer.push(scroller).into();
    }

    let content = state.view(p, theme_mode, Message::PreviewLinkClicked);
    slim_scroll_h(content, state.preferred_min_width(), p)
}
