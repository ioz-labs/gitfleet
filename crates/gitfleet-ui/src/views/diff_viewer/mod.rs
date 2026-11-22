//! Diff, Rich Preview, and Inline Editor panel for the Host window, plus shared
//! modular diff engine (`inline_diff`, `layout`, `selection`, `widgets`, `render`).

pub(crate) mod inline_diff;
pub(crate) mod layout;
pub(crate) mod render;
pub(crate) mod selection;
pub(crate) mod widgets;

#[cfg(test)]
mod tests;

pub(crate) use layout::{
    DiffLayoutCache, build_diff_layout_cache, map_scroll_y, parse_and_cache_diff,
};
pub(crate) use render::{
    render_file_preview, render_side_by_side_diff_with, render_unified_diff_with,
};
pub(crate) use selection::{
    diff_line_text_at, diff_row_count, extract_selected_diff_text, word_bounds_at,
};

use crate::app::{BottomViewMode, DiffTextSelection, GitFleetApp, Message};
use crate::icons;
use crate::theme::{self, Palette, type_scale};
use crate::views::tip;
use gitfleet_git::ParsedDiff;
use iced::widget::{
    Space, button, column, container, horizontal_rule, row, scrollable, text, text_editor, tooltip,
};
use iced::{Alignment, Color, Element, Length};

pub(crate) const DIFF_LINE_H: f32 = 21.0;
pub(crate) const DIFF_HIDDEN_H: f32 = 24.0;
pub(crate) const DIFF_SEP_H: f32 = 26.0;
pub(crate) const DIFF_CONTEXT_LINES: usize = 3;
pub(crate) const DIFF_EXPAND_STEP: usize = 20;
pub(crate) const LINE_NUM_W: f32 = 48.0;
pub(crate) const CONNECTOR_W: f32 = 28.0;
pub(crate) const OVERVIEW_BAR_W: f32 = 5.0;
/// Overscan buffer above & below the visible diff viewport (~17 lines) so wheel scrolling stays seamless.
pub(crate) const DIFF_OVERSCAN_PX: f32 = 360.0;

pub(crate) struct TextDiffRenderCtx<'a> {
    pub(crate) parsed: &'a ParsedDiff,
    pub(crate) cache: &'a DiffLayoutCache,
    pub(crate) is_working_file: bool,
    pub(crate) is_staged: bool,
    pub(crate) diff_title: &'a str,
    pub(crate) selection: Option<DiffTextSelection>,
    pub(crate) left_scroll_id: scrollable::Id,
    pub(crate) right_scroll_id: scrollable::Id,
    pub(crate) unified_scroll_id: scrollable::Id,
    pub(crate) left_scroll_y: f32,
    pub(crate) right_scroll_y: f32,
    pub(crate) unified_scroll_y: f32,
    pub(crate) left_viewport_h: f32,
    pub(crate) right_viewport_h: f32,
    pub(crate) unified_viewport_h: f32,
}

impl GitFleetApp {
    pub(crate) fn view_bottom_viewer(&self, p: Palette) -> Element<'_, Message> {
        let preview_badge = self.preview_state.kind_badge();
        let is_diff_mode = matches!(
            self.bottom_mode,
            BottomViewMode::UnifiedDiff | BottomViewMode::SideBySideDiff
        );
        let stats = self.diff_parsed.stats;
        let has_changes = stats.added_blocks + stats.removed_blocks + stats.modified_blocks > 0;

        let mut header = row![
            theme::clipped_line(
                text(self.diff_title.clone())
                    .size(type_scale::BODY)
                    .color(p.text_primary),
                Length::Fill,
            ),
            if self.bottom_mode == BottomViewMode::Preview && !preview_badge.is_empty() {
                Element::from(
                    container(
                        text(preview_badge)
                            .size(type_scale::BADGE)
                            .color(p.accent_fg)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([2, 7])
                    .style(theme::pill_box(p.fill_selected, p.accent_fg, p.border_default)),
                )
            } else {
                Space::with_width(Length::Shrink).into()
            },
            if self.editor_dirty {
                Element::from(
                    row![
                        container(Space::new(6, 6)).style(theme::dot_box(p.warning_fg)),
                        text("unsaved")
                            .size(type_scale::CAPTION)
                            .color(p.warning_fg)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
            } else {
                Space::with_width(Length::Shrink).into()
            },
        ]
        .spacing(5)
        .align_y(Alignment::Center);

        if is_diff_mode && has_changes {
            let mut stat_row = row![].spacing(4).align_y(Alignment::Center);
            if stats.added_blocks > 0 {
                stat_row = stat_row.push(
                    text(format!("+{}", stats.added_blocks))
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(p.success_fg)
                        .wrapping(text::Wrapping::None),
                );
            }
            if stats.removed_blocks > 0 {
                stat_row = stat_row.push(
                    text(format!("-{}", stats.removed_blocks))
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(p.danger_fg)
                        .wrapping(text::Wrapping::None),
                );
            }
            if stats.modified_blocks > 0 {
                stat_row = stat_row.push(
                    text(format!("~{}", stats.modified_blocks))
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(p.warning_fg)
                        .wrapping(text::Wrapping::None),
                );
            }
            header = header.push(tip(
                container(stat_row)
                    .padding([2, 6])
                    .style(theme::section_panel_box(p)),
                "Change blocks: +Added  -Removed  ~Modified",
                tooltip::Position::Top,
                p,
            ));
        }

        if self.conflict_file_path.is_some() {
            let unresolved = self.conflict_parsed.unresolved_count();
            let resolve_label = if unresolved > 0 {
                format!("Resolve ({unresolved})")
            } else {
                "Resolve ✓".to_string()
            };
            header = header.push(tip(
                button(
                    text(resolve_label)
                        .size(type_scale::CAPTION)
                        .color(if self.bottom_mode == BottomViewMode::ResolveConflicts {
                            p.accent_fg
                        } else if unresolved > 0 {
                            p.danger_fg
                        } else {
                            p.success_fg
                        })
                        .wrapping(text::Wrapping::None),
                )
                .padding([3, 8])
                .style(theme::filter_pill_btn(
                    p,
                    self.bottom_mode == BottomViewMode::ResolveConflicts,
                ))
                .on_press(Message::SetBottomViewMode(BottomViewMode::ResolveConflicts)),
                "Interactive Conflict Solver (Ours / Theirs / Both / Manual Edit)",
                tooltip::Position::Top,
                p,
            ));
        }

        let is_image_file = self.diff_image.is_some()
            || self
                .active_file_path
                .as_deref()
                .is_some_and(crate::views::image_diff::is_image_diff_path);
        let is_raster_only = self
            .diff_image
            .as_ref()
            .is_some_and(|d| !d.is_svg)
            || self
                .active_file_path
                .as_deref()
                .is_some_and(crate::views::image_diff::is_raster_image_path);
        let is_csv_file = self.diff_csv.is_some()
            || self
                .active_file_path
                .as_deref()
                .is_some_and(crate::views::csv_diff::is_csv_diff_path);

        if is_image_file {
            header = header.push(tip(
                button(
                    row![
                        icons::image_frame(
                            12.0,
                            if self.bottom_mode == BottomViewMode::ImageDiff {
                                p.accent_fg
                            } else {
                                p.text_muted
                            }
                        ),
                        text("Image Diff")
                            .size(type_scale::CAPTION)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([3, 8])
                .style(theme::filter_pill_btn(
                    p,
                    self.bottom_mode == BottomViewMode::ImageDiff,
                ))
                .on_press(Message::SetBottomViewMode(BottomViewMode::ImageDiff)),
                "Visual Image Diff (2-up, Blink, Swipe, Onion Skin & Pixel Diff)",
                tooltip::Position::Top,
                p,
            ));
        }

        if is_csv_file {
            header = header.push(tip(
                button(
                    row![
                        icons::table_grid(
                            12.0,
                            if self.bottom_mode == BottomViewMode::CsvDiff {
                                p.accent_fg
                            } else {
                                p.text_muted
                            }
                        ),
                        text("Table")
                            .size(type_scale::CAPTION)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([3, 8])
                .style(theme::filter_pill_btn(
                    p,
                    self.bottom_mode == BottomViewMode::CsvDiff,
                ))
                .on_press(Message::SetBottomViewMode(BottomViewMode::CsvDiff)),
                "Spreadsheet Table Diff with cell-level highlights",
                tooltip::Position::Top,
                p,
            ));
        }

        if !is_raster_only {
            header = header
                .push(tip(
                    button(
                        text("Unified")
                            .size(type_scale::CAPTION)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([3, 8])
                    .style(theme::filter_pill_btn(
                        p,
                        self.bottom_mode == BottomViewMode::UnifiedDiff,
                    ))
                    .on_press(Message::SetBottomViewMode(BottomViewMode::UnifiedDiff)),
                    "Unified Diff with Line Numbers & Hunk Staging",
                    tooltip::Position::Top,
                    p,
                ))
                .push(tip(
                    button(
                        text("Side by Side")
                            .size(type_scale::CAPTION)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([3, 8])
                    .style(theme::filter_pill_btn(
                        p,
                        self.bottom_mode == BottomViewMode::SideBySideDiff,
                    ))
                    .on_press(Message::SetBottomViewMode(BottomViewMode::SideBySideDiff)),
                    "SmartGit-style Side-by-Side Diff (Synced Vertical & Horizontal Scroll + Connectors)",
                    tooltip::Position::Top,
                    p,
                ));
        }

        if is_diff_mode {
            header = header.push(tip(
                button(
                    text("Compact")
                        .size(type_scale::CAPTION)
                        .wrapping(text::Wrapping::None),
                )
                .padding([3, 8])
                .style(theme::filter_pill_btn(p, self.diff_compact))
                .on_press(Message::ToggleDiffCompact),
                if self.diff_compact {
                    "Compact mode ON (hides unchanged lines — click to show full file)"
                } else {
                    "Compact mode OFF (showing full file — click to collapse unchanged lines)"
                },
                tooltip::Position::Top,
                p,
            ));
            if has_changes {
                header = header
                    .push(tip(
                        button(icons::push(12.0, p.text_secondary))
                            .padding([4, 6])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::DiffJumpChange(-1)),
                        "Previous Change",
                        tooltip::Position::Top,
                        p,
                    ))
                    .push(tip(
                        button(icons::pull(12.0, p.text_secondary))
                            .padding([4, 6])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::DiffJumpChange(1)),
                        "Next Change",
                        tooltip::Position::Top,
                        p,
                    ));
            }
        }

        header = header.push(tip(
            button(
                text("Preview")
                    .size(type_scale::CAPTION)
                    .wrapping(text::Wrapping::None),
            )
            .padding([3, 8])
            .style(theme::filter_pill_btn(
                p,
                self.bottom_mode == BottomViewMode::Preview,
            ))
            .on_press(Message::SetBottomViewMode(BottomViewMode::Preview)),
            "Selectable Rich Preview (.md, images, SVG, JSON, CSV, configs, code)",
            tooltip::Position::Top,
            p,
        ));

        if !is_raster_only {
            header = header.push(tip(
                button(
                    text("Edit")
                        .size(type_scale::CAPTION)
                        .wrapping(text::Wrapping::None),
                )
                .padding([3, 8])
                .style(theme::filter_pill_btn(
                    p,
                    self.bottom_mode == BottomViewMode::Edit,
                ))
                .on_press(Message::SetBottomViewMode(BottomViewMode::Edit)),
                "Live File Editor (Ctrl+S to save)",
                tooltip::Position::Top,
                p,
            ));
        }

        if self.bottom_mode == BottomViewMode::Preview {
            if self.preview_state.has_rich_view() {
                header = header.push(tip(
                    button(
                        text(if self.preview_raw_mode {
                            "Rendered"
                        } else {
                            "Select Text"
                        })
                        .size(type_scale::CAPTION)
                        .wrapping(text::Wrapping::None),
                    )
                    .padding([3, 8])
                    .style(theme::filter_pill_btn(p, self.preview_raw_mode))
                    .on_press(Message::TogglePreviewRaw),
                    "Switch between rich rendered view and selectable raw text",
                    tooltip::Position::Top,
                    p,
                ));
            }
            if self.preview_state.copyable_text().is_some() {
                let has_preview_sel = self
                    .preview_editor
                    .selection()
                    .is_some_and(|s| !s.is_empty());
                header = header.push(tip(
                    button(
                        row![
                            icons::copy(
                                12.0,
                                if has_preview_sel {
                                    p.accent_fg
                                } else {
                                    p.text_secondary
                                }
                            ),
                            text(if has_preview_sel { "Copy Sel" } else { "Copy" })
                                .size(type_scale::CAPTION)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                    )
                    .padding([3, 8])
                    .style(theme::filter_pill_btn(p, has_preview_sel))
                    .on_press(Message::CopyPreviewText),
                    "Copy selected preview text (Ctrl+C), or full preview content",
                    tooltip::Position::Top,
                    p,
                ));
            }
        }

        if self.bottom_mode == BottomViewMode::Edit {
            header = header
                .push(
                    button(text("Open File…").size(type_scale::CAPTION))
                        .padding([3, 8])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::PickOpenIgnoredFile),
                )
                .push(
                    button(text("Save As…").size(type_scale::CAPTION))
                        .padding([3, 8])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::PickSaveDocAs),
                )
                .push(
                    button(
                        row![
                            icons::save(13.0, Color::WHITE),
                            text("Save (Ctrl+S)").size(type_scale::CAPTION)
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                    )
                    .padding([3, 9])
                    .style(theme::primary_btn(p))
                    .on_press_maybe(self.editor_dirty.then_some(Message::SaveDocument)),
                );
        }

        let body: Element<'_, Message> = match self.bottom_mode {
            BottomViewMode::UnifiedDiff => self.view_unified_diff(p),
            BottomViewMode::SideBySideDiff => self.view_side_by_side_diff(p),
            BottomViewMode::ImageDiff => self.view_image_diff(p),
            BottomViewMode::CsvDiff => self.view_csv_diff(p),
            BottomViewMode::Preview => self.view_file_preview(p),
            BottomViewMode::Edit => self.view_file_editor(p),
            BottomViewMode::ResolveConflicts => self.view_conflict_solver(p),
        };

        column![
            header,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            body
        ]
        .spacing(6)
        .height(Length::Fill)
        .into()
    }

    pub(crate) fn view_unified_diff(&self, p: Palette) -> Element<'_, Message> {
        render_unified_diff_with(self.host_text_diff_ctx(), p)
    }

    pub(crate) fn view_side_by_side_diff(&self, p: Palette) -> Element<'_, Message> {
        render_side_by_side_diff_with(self.host_text_diff_ctx(), p)
    }

    fn host_text_diff_ctx(&self) -> TextDiffRenderCtx<'_> {
        TextDiffRenderCtx {
            parsed: &self.diff_parsed,
            cache: &self.diff_cache,
            is_working_file: self.diff_is_working_file,
            is_staged: self.diff_is_staged,
            diff_title: &self.diff_title,
            selection: self.diff_selection,
            left_scroll_id: self.diff_left_scroll_id.clone(),
            right_scroll_id: self.diff_right_scroll_id.clone(),
            unified_scroll_id: self.diff_unified_scroll_id.clone(),
            left_scroll_y: self.diff_left_scroll_y,
            right_scroll_y: self.diff_right_scroll_y,
            unified_scroll_y: self.diff_unified_scroll_y,
            left_viewport_h: self.diff_left_viewport_h,
            right_viewport_h: self.diff_right_viewport_h,
            unified_viewport_h: self.diff_unified_viewport_h,
        }
    }

    pub(crate) fn view_file_preview(&self, p: Palette) -> Element<'_, Message> {
        render_file_preview(
            &self.preview_state,
            &self.preview_editor,
            &self.preview_scroll,
            crate::views::code_editor::NumberedEditorConfig::preview(type_scale::CODE, false),
            self.preview_raw_mode,
            self.theme_mode,
            p,
        )
    }

    pub(crate) fn view_file_editor(&self, p: Palette) -> Element<'_, Message> {
        if self.open_doc_id.is_none() {
            return container(
                text("Select any file from 'All Files' or 'Changed' to edit it directly.")
                    .size(type_scale::BODY)
                    .color(p.text_muted),
            )
            .padding(10)
            .into();
        }
        let hl_settings = crate::syntax::CodeHighlightSettings {
            language: self.open_doc_language,
            theme_mode: self.theme_mode,
            search_query: String::new(),
            search_options: gitfleet_search::SearchOptions::default(),
            active_match_line: None,
            active_match_cols: None,
        };
        let ed = text_editor(&self.editor_content)
            .on_action(Message::EditorAction)
            .highlight_with::<crate::syntax::CodeSyntaxHighlighter>(
                hl_settings,
                crate::syntax::format_code_token,
            );
        crate::views::code_editor::numbered_code_editor(
            &self.editor_content,
            ed,
            &self.editor_scroll,
            crate::views::code_editor::NumberedEditorConfig::host_edit(),
            p,
            Message::EditorScrolled,
        )
    }
}
