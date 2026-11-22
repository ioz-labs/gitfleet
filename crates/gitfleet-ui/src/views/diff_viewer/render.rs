//! Virtualized Unified Diff, Side-by-Side Diff, and Rich File Preview renderers
//! shared between the Host Diff Panel and GitFleet Studio.

use super::layout::{DiffLayoutCache, SideRow, UnifiedRow, visible_row_range};
use super::selection::row_selection_range;
use super::widgets::{
    DiffConnectorCanvas, DiffOverviewBar, DiffRowSelConfig, change_colors, diff_code_row_box,
    hidden_gap_bar, render_highlighted_code,
};
use super::{
    CONNECTOR_W, DIFF_LINE_H, DIFF_OVERSCAN_PX, DIFF_SEP_H, LINE_NUM_W, OVERVIEW_BAR_W,
    TextDiffRenderCtx,
};
use crate::app::{DiffSelectionSide, DiffTextSelection, Message, PendingConfirm};
use crate::icons;
use crate::preview::{PreviewHighlightSettings, PreviewHighlighter, PreviewState};
use crate::syntax::TokenFormat;
use crate::theme::{self, Palette, ThemeMode, type_scale};
use crate::views::slim_scroll_h;
use gitfleet_git::DiffChangeKind;
use iced::advanced::text as adv_text;
use iced::widget::canvas::Canvas;
use iced::widget::{Space, button, column, container, row, text, text_editor};
use iced::{Alignment, Color, Element, Length, Padding, alignment};
use std::ops::Range;

fn render_side_row<'a>(
    item: &SideRow,
    syntax_spans: &[(Range<usize>, TokenFormat)],
    side: DiffSelectionSide,
    row_idx: usize,
    selection: Option<DiffTextSelection>,
    p: Palette,
) -> Element<'a, Message> {
    match item {
        SideRow::Code {
            line_no,
            text: code,
            spans,
            kind,
            is_first,
            is_last,
            top_seam,
            bottom_seam,
        } => {
            let (bg, border_c, fg) = match kind {
                Some(k) => {
                    let (b, a, t) = change_colors(*k, p);
                    (b, Some(a), t)
                }
                None => (Color::TRANSPARENT, None, p.text_secondary),
            };
            let top_border = if *is_first { border_c } else { None };
            let bottom_border = if *is_last { border_c } else { None };
            let top_seam_c = top_seam.map(|k| change_colors(k, p).1);
            let bot_seam_c = bottom_seam.map(|k| change_colors(k, p).1);

            let ln_str = line_no.map(|n| n.to_string()).unwrap_or_default();
            let gutter = container(
                text(ln_str)
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

            let code_cell = container(render_highlighted_code(
                code,
                spans,
                syntax_spans,
                fg,
                border_c,
            ))
            .width(Length::Fill)
            .height(Length::Fixed(DIFF_LINE_H))
            .padding(Padding {
                top: 0.0,
                right: 8.0,
                bottom: 0.0,
                left: 8.0,
            })
            .align_y(alignment::Vertical::Center);

            let char_len = code.chars().count();
            let sel_cfg = DiffRowSelConfig {
                side,
                row_idx,
                code_start_x: LINE_NUM_W + 8.0,
                line_text: code.clone(),
                sel_range: row_selection_range(selection, side, row_idx, char_len),
                is_dragging_same_side: selection.is_some_and(|s| s.dragging && s.side == side),
                sel_bg: Color {
                    a: 0.42,
                    ..p.accent_primary
                },
                sel_overlay: Color {
                    a: 0.16,
                    ..p.accent_hover
                },
            };

            diff_code_row_box(
                bg,
                top_border,
                bottom_border,
                top_seam_c,
                bot_seam_c,
                LINE_NUM_W,
                p.border_subtle,
                sel_cfg,
                row![gutter, code_cell].align_y(Alignment::Center),
            )
        }
        SideRow::Hidden {
            gap_id,
            hidden_count,
            can_expand_top,
            can_expand_bottom,
        } => hidden_gap_bar(*gap_id, *hidden_count, *can_expand_top, *can_expand_bottom, p),
        SideRow::Separator(hdr) => container(
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
        .style(theme::diff_line_box(p.fill_selected, p.accent_fg))
        .into(),
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
                let (bg, sign_color, fg) = match kind {
                    Some(DiffChangeKind::Added) => (p.success_bg, p.success_fg, p.text_primary),
                    Some(DiffChangeKind::Removed) => (p.danger_bg, p.danger_fg, p.text_primary),
                    Some(DiffChangeKind::Modified) => (p.warning_bg, p.warning_fg, p.text_primary),
                    None => (Color::TRANSPARENT, p.text_muted, p.text_secondary),
                };
                let hl_accent = kind.map(|k| change_colors(k, p).1);
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
                        hl_accent,
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
    theme::responsive_hscroll_synced(
        lines_col,
        min_w,
        ctx.unified_scroll_id,
        Message::DiffUnifiedScrolled,
        p,
    )
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
    let left_vh = ctx.left_viewport_h.max(600.0);
    let right_vh = ctx.right_viewport_h.max(600.0);

    let (left_range, left_top_spacer, left_bot_spacer) = visible_row_range(
        &cache.left_y_offsets,
        ctx.left_scroll_y,
        left_vh,
        DIFF_OVERSCAN_PX,
    );
    let mut left_col = column![Space::with_height(Length::Fixed(left_top_spacer))].spacing(0);
    for idx in left_range {
        let item = &layout.left_rows[idx];
        let syntax_spans = cache
            .left_syntax
            .get(idx)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        left_col = left_col.push(render_side_row(
            item,
            syntax_spans,
            DiffSelectionSide::Left,
            idx,
            ctx.selection,
            p,
        ));
    }
    left_col = left_col.push(Space::with_height(Length::Fixed(left_bot_spacer)));

    let (right_range, right_top_spacer, right_bot_spacer) = visible_row_range(
        &cache.right_y_offsets,
        ctx.right_scroll_y,
        right_vh,
        DIFF_OVERSCAN_PX,
    );
    let mut right_col = column![Space::with_height(Length::Fixed(right_top_spacer))].spacing(0);
    for idx in right_range {
        let item = &layout.right_rows[idx];
        let syntax_spans = cache
            .right_syntax
            .get(idx)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        right_col = right_col.push(render_side_row(
            item,
            syntax_spans,
            DiffSelectionSide::Right,
            idx,
            ctx.selection,
            p,
        ));
    }
    right_col = right_col.push(Space::with_height(Length::Fixed(right_bot_spacer)));

    let left_min_w =
        (layout.max_left_chars as f32 * 8.0 + LINE_NUM_W + 30.0).clamp(240.0, 2600.0);
    let right_min_w =
        (layout.max_right_chars as f32 * 8.0 + LINE_NUM_W + 30.0).clamp(240.0, 2600.0);
    let shared_min_w = left_min_w.max(right_min_w);

    let left_overview: Element<'_, Message> = Canvas::new(DiffOverviewBar {
        spans: layout.connectors.clone(),
        is_left: true,
        total_h: layout.total_left_h,
        p,
    })
    .width(Length::Fixed(OVERVIEW_BAR_W))
    .height(Length::Fill)
    .into();

    let right_overview: Element<'_, Message> = Canvas::new(DiffOverviewBar {
        spans: layout.connectors.clone(),
        is_left: false,
        total_h: layout.total_right_h,
        p,
    })
    .width(Length::Fixed(OVERVIEW_BAR_W))
    .height(Length::Fill)
    .into();

    let left_pane = container(theme::responsive_hscroll_synced(
        left_col,
        shared_min_w,
        ctx.left_scroll_id,
        Message::DiffLeftScrolled,
        p,
    ))
    .width(Length::FillPortion(1))
    .height(Length::Fill);

    let connector: Element<'_, Message> = Canvas::new(DiffConnectorCanvas {
        spans: layout.connectors.clone(),
        left_scroll_y: ctx.left_scroll_y,
        right_scroll_y: ctx.right_scroll_y,
        p,
    })
    .width(Length::Fixed(CONNECTOR_W))
    .height(Length::Fill)
    .into();

    let right_pane = container(theme::responsive_hscroll_synced(
        right_col,
        shared_min_w,
        ctx.right_scroll_id,
        Message::DiffRightScrolled,
        p,
    ))
    .width(Length::FillPortion(1))
    .height(Length::Fill);

    row![left_overview, left_pane, connector, right_pane, right_overview]
        .spacing(0)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

pub(crate) fn render_file_preview<'a>(
    state: &'a PreviewState,
    editor: &'a text_editor::Content,
    raw_mode: bool,
    theme_mode: ThemeMode,
    p: Palette,
) -> Element<'a, Message> {
    if state.uses_selectable_editor(raw_mode) {
        let mut col = column![].spacing(6).height(Length::Fill);
        match state {
            PreviewState::Json {
                valid,
                error: Some(err),
                ..
            } if !valid => {
                col = col.push(
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
                col = col.push(
                    text("Valid JSON — formatted (select text & Ctrl+C to copy)")
                        .size(type_scale::CAPTION)
                        .color(p.success_fg),
                );
            }
            PreviewState::Config { kind, content } => {
                col = col.push(
                    text(format!(
                        "{kind} Preview  •  {} lines  (select & Ctrl+C to copy)",
                        content.lines().count()
                    ))
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
                );
            }
            PreviewState::Code { lang, content } => {
                col = col.push(
                    text(format!(
                        "{lang}  •  {} lines  (select & Ctrl+C to copy)",
                        content.lines().count()
                    ))
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
                );
            }
            _ => {}
        }

        let hl_settings = PreviewHighlightSettings {
            kind: state.highlight_kind(),
            theme_mode,
        };
        let ed = text_editor(editor)
            .on_action(Message::PreviewEditorAction)
            .highlight_with::<PreviewHighlighter>(hl_settings, |color, _theme| {
                adv_text::highlighter::Format {
                    color: Some(*color),
                    font: None,
                }
            })
            .font(theme::CODE_FONT)
            .size(type_scale::CODE)
            .height(Length::Fill)
            .style(theme::dark_editor(p));

        return col.push(ed).into();
    }

    let content = state.view(p, theme_mode, Message::PreviewLinkClicked);
    slim_scroll_h(content, state.preferred_min_width(), p)
}
