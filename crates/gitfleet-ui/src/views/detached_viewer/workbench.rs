//! Main Studio editor workbench: tab bar, breadcrumb/mode bar, code editor canvas,
//! diff/preview hosts, and empty welcome screen.

use super::find_bar::render_in_file_find_bar;
use super::io::map_text_diff_msg;
use super::sidebar_explorer::lang_tag;
use super::state::{
    DetachedAction, DetachedViewMode, DetachedViewerState, StudioSidebarPanel, StudioTab,
};
use super::status_bar::render_mode_subbar;
use super::symbols::LineGitMark;
use crate::app::Message;
use crate::icons;
use crate::syntax::{CodeHighlightSettings, CodeSyntaxHighlighter, format_code_token};
use crate::theme::{self, Palette, ThemeMode, icon_size, type_scale};
use crate::views::csv_diff::render_csv_diff;
use crate::views::diff_viewer::widgets::render_highlighted_code;
use crate::views::diff_viewer::{
    DIFF_LINE_H, DIFF_OVERSCAN_PX, TextDiffRenderCtx, render_file_preview,
    render_side_by_side_diff_with, render_unified_diff_with,
};
use crate::views::image_diff::render_image_diff;
use crate::views::{tip, truncate_chars};
use iced::widget::{
    Space, button, column, container, horizontal_rule, mouse_area, row, scrollable, text,
    text_editor, tooltip,
};
use iced::{Alignment, Color, Element, Length, keyboard, window};
use std::sync::Arc;

// ── Right Editor Workbench (Tab Bar + Breadcrumb + Find + Canvas) ────────────

pub(crate) fn render_editor_workbench<'a>(
    win_id: window::Id,
    v: &'a DetachedViewerState,
    theme_mode: ThemeMode,
    p: Palette,
) -> Element<'a, Message> {
    let tab_bar = render_tab_bar(win_id, v, theme_mode, p);

    let Some(tab) = v.active_tab() else {
        let welcome = render_welcome_canvas(win_id, v, p);
        return column![
            tab_bar,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            welcome,
        ]
        .spacing(0)
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
    };

    let mut col = column![
        tab_bar,
        horizontal_rule(1).style(theme::subtle_rule(p)),
        render_breadcrumb_bar(win_id, v, tab, p),
        horizontal_rule(1).style(theme::subtle_rule(p)),
    ]
    .spacing(0)
    .width(Length::Fill)
    .height(Length::Fill);

    // Inline unsaved tab close confirmation
    if let Some(pending_id) = v.pending_close_tab
        && pending_id == tab.id
    {
        let banner = container(
            row![
                icons::alert_triangle(13.0, p.warning_fg),
                theme::clipped_line(
                    text(format!("Save changes to {} before closing?", tab.title))
                        .size(type_scale::CAPTION)
                        .color(p.warning_fg)
                        .wrapping(text::Wrapping::None),
                    Length::Fill,
                ),
                button(
                    text("Save")
                        .size(type_scale::BADGE)
                        .wrapping(text::Wrapping::None),
                )
                .padding([3, 10])
                .style(theme::primary_btn(p))
                .on_press(Message::Detached(
                    win_id,
                    DetachedAction::ConfirmCloseTab {
                        tab_id: pending_id,
                        save_first: true,
                    },
                )),
                button(
                    text("Don't Save")
                        .size(type_scale::BADGE)
                        .wrapping(text::Wrapping::None),
                )
                .padding([3, 10])
                .style(theme::danger_btn(p))
                .on_press(Message::Detached(
                    win_id,
                    DetachedAction::ConfirmCloseTab {
                        tab_id: pending_id,
                        save_first: false,
                    },
                )),
                button(
                    text("Cancel")
                        .size(type_scale::BADGE)
                        .wrapping(text::Wrapping::None),
                )
                .padding([3, 8])
                .style(theme::subtle_btn(p))
                .on_press(Message::Detached(win_id, DetachedAction::CancelCloseTab)),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([6, 12])
        .width(Length::Fill)
        .style(theme::pill_box(p.warning_bg, p.warning_fg, p.border_subtle));

        col = col
            .push(container(banner).padding([6, 10]).width(Length::Fill))
            .push(horizontal_rule(1).style(theme::subtle_rule(p)));
    }

    // External disk modification conflict banner (`S8`)
    if tab.disk_conflict {
        let disk_banner = container(
            row![
                icons::alert_triangle(13.0, p.warning_fg),
                theme::clipped_line(
                    text(format!(
                        "'{}' was modified externally on disk while you have unsaved edits.",
                        tab.title
                    ))
                    .size(type_scale::CAPTION)
                    .color(p.warning_fg)
                    .wrapping(text::Wrapping::None),
                    Length::Fill,
                ),
                button(
                    text("Reload from Disk")
                        .size(type_scale::BADGE)
                        .wrapping(text::Wrapping::None),
                )
                .padding([3, 10])
                .style(theme::primary_btn(p))
                .on_press(Message::Detached(win_id, DetachedAction::ReloadFiles)),
                button(
                    text("Keep Mine")
                        .size(type_scale::BADGE)
                        .wrapping(text::Wrapping::None),
                )
                .padding([3, 10])
                .style(theme::subtle_btn(p))
                .on_press(Message::Detached(
                    win_id,
                    DetachedAction::DismissDiskConflict(tab.id),
                )),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([6, 12])
        .width(Length::Fill)
        .style(theme::pill_box(p.warning_bg, p.warning_fg, p.border_subtle));

        col = col
            .push(container(disk_banner).padding([6, 10]).width(Length::Fill))
            .push(horizontal_rule(1).style(theme::subtle_rule(p)));
    }

    // Diff / Preview contextual sub-bar when applicable
    if let Some(sub) = render_mode_subbar(win_id, tab, p) {
        col = col
            .push(
                container(sub)
                    .padding([4, 12])
                    .width(Length::Fill)
                    .style(theme::studio_breadcrumb_bar(p)),
            )
            .push(horizontal_rule(1).style(theme::subtle_rule(p)));
    }

    // In-file Find & Replace strip
    if v.in_file_find_open && matches!(tab.mode, DetachedViewMode::Edit | DetachedViewMode::Preview)
    {
        col = col
            .push(render_in_file_find_bar(win_id, v, tab, p))
            .push(horizontal_rule(1).style(theme::subtle_rule(p)));
    }

    let canvas: Element<'a, Message> = match tab.mode {
        DetachedViewMode::Edit if v.split_right_preview && tab.has_preview_available() => {
            let left_p = ((v.split_ratio.clamp(0.22, 0.78) * 1000.0).round() as u16).max(200);
            let right_p = 1000u16.saturating_sub(left_p).max(200);
            let left_editor = container(render_studio_code_editor(win_id, v, tab, theme_mode, p))
                .width(Length::FillPortion(left_p))
                .height(Length::Fill);
            let right_preview = container(
                render_file_preview(
                    &tab.preview_state,
                    &tab.preview_editor,
                    &tab.preview_scroll,
                    crate::views::code_editor::NumberedEditorConfig::preview(
                        v.editor_font_size,
                        v.editor_word_wrap,
                    ),
                    tab.preview_raw_mode,
                    theme_mode,
                    p,
                )
                .map(move |m| map_text_diff_msg(win_id, m)),
            )
            .padding(8)
            .width(Length::FillPortion(right_p))
            .height(Length::Fill);

            row![
                left_editor,
                theme::kit::hairline_v_splitter(
                    p,
                    v.resizing_split,
                    Message::Detached(win_id, DetachedAction::StartResizeSplit),
                ),
                right_preview,
            ]
            .spacing(0)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        }
        DetachedViewMode::Edit => render_studio_code_editor(win_id, v, tab, theme_mode, p),
        DetachedViewMode::Preview => container(
            render_file_preview(
                &tab.preview_state,
                &tab.preview_editor,
                &tab.preview_scroll,
                crate::views::code_editor::NumberedEditorConfig::preview(
                    v.editor_font_size,
                    v.editor_word_wrap,
                ),
                tab.preview_raw_mode,
                theme_mode,
                p,
            )
            .map(move |m| map_text_diff_msg(win_id, m)),
        )
        .padding(8)
        .width(Length::Fill)
        .height(Length::Fill)
        .into(),
        DetachedViewMode::ImageDiff => container(
            render_image_diff(tab.diff_image.as_ref(), tab.image_state, p, theme_mode)
                .map(move |m| Message::Detached(win_id, DetachedAction::ImageMsg(m))),
        )
        .padding(8)
        .width(Length::Fill)
        .height(Length::Fill)
        .into(),
        DetachedViewMode::CsvDiff => container(
            render_csv_diff(
                tab.diff_csv.as_ref(),
                tab.csv_changes_only,
                tab.csv_ignore_format,
                &tab.csv_filter,
                p,
                theme_mode,
            )
            .map(move |m| Message::Detached(win_id, DetachedAction::CsvMsg(m))),
        )
        .padding(8)
        .width(Length::Fill)
        .height(Length::Fill)
        .into(),
        DetachedViewMode::UnifiedDiff => container(
            render_unified_diff_with(
                TextDiffRenderCtx {
                    parsed: &tab.diff_parsed,
                    cache: &tab.diff_cache,
                    is_working_file: false,
                    is_staged: false,
                    diff_title: &tab.title,
                    selection: tab.diff_selection,
                    left_scroll_id: tab.diff_left_scroll_id.clone(),
                    unified_scroll_id: tab.diff_unified_scroll_id.clone(),
                    left_scroll_y: tab.diff_left_scroll_y,
                    unified_scroll_y: tab.diff_unified_scroll_y,
                    left_viewport_h: tab.diff_left_viewport_h,
                    unified_viewport_h: tab.diff_unified_viewport_h,
                },
                p,
            )
            .map(move |m| map_text_diff_msg(win_id, m)),
        )
        .padding(4)
        .width(Length::Fill)
        .height(Length::Fill)
        .into(),
        DetachedViewMode::SideBySideDiff => container(
            render_side_by_side_diff_with(
                TextDiffRenderCtx {
                    parsed: &tab.diff_parsed,
                    cache: &tab.diff_cache,
                    is_working_file: false,
                    is_staged: false,
                    diff_title: &tab.title,
                    selection: tab.diff_selection,
                    left_scroll_id: tab.diff_left_scroll_id.clone(),
                    unified_scroll_id: tab.diff_unified_scroll_id.clone(),
                    left_scroll_y: tab.diff_left_scroll_y,
                    unified_scroll_y: tab.diff_unified_scroll_y,
                    left_viewport_h: tab.diff_left_viewport_h,
                    unified_viewport_h: tab.diff_unified_viewport_h,
                },
                p,
            )
            .map(move |m| map_text_diff_msg(win_id, m)),
        )
        .padding(4)
        .width(Length::Fill)
        .height(Length::Fill)
        .into(),
    };

    col.push(canvas).into()
}

fn render_tab_bar<'a>(
    win_id: window::Id,
    v: &'a DetachedViewerState,
    theme_mode: ThemeMode,
    p: Palette,
) -> Element<'a, Message> {
    let mut tabs_row = row![].spacing(2).align_y(Alignment::End);

    for (idx, tab) in v.tabs.iter().enumerate() {
        let is_active = v.active_tab == Some(idx);
        let badge: Element<'a, Message> = if tab.left_path.is_some() && tab.right_path.is_some() {
            icons::file_diff(11.0, p.accent_fg).into()
        } else {
            lang_tag(tab.language, theme_mode)
        };

        let mut inner = row![
            badge,
            text(truncate_chars(&tab.title, 26))
                .size(type_scale::CAPTION)
                .color(if is_active {
                    p.text_primary
                } else {
                    p.text_secondary
                })
                .wrapping(text::Wrapping::None),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        if tab.dirty {
            inner = inner.push(
                text("●")
                    .size(9)
                    .color(p.warning_fg)
                    .wrapping(text::Wrapping::None),
            );
        }

        let close_btn = button(icons::close_x(10.0, p.text_muted))
            .padding([2, 3])
            .style(theme::vscode_icon_btn(p, false))
            .on_press(Message::Detached(win_id, DetachedAction::CloseTab(idx)));

        let tab_pill = mouse_area(
            row![
                button(inner)
                    .padding([6, 8])
                    .style(theme::studio_tab_btn(p, is_active))
                    .on_press(Message::Detached(win_id, DetachedAction::SelectTab(idx))),
                close_btn,
            ]
            .spacing(1)
            .align_y(Alignment::Center),
        )
        .on_right_press(Message::Detached(win_id, DetachedAction::OpenTabMenu(idx)));

        tabs_row = tabs_row.push(tab_pill);
    }

    tabs_row = tabs_row
        .push(tip(
            button(icons::plus(12.0, p.text_muted))
                .padding([5, 7])
                .style(theme::vscode_icon_btn(p, false))
                .on_press(Message::Detached(win_id, DetachedAction::NewUntitledTab)),
            "New Scratch Tab (Ctrl+N)",
            tooltip::Position::Bottom,
            p,
        ))
        .push(tip(
            button(icons::folder_open(12.0, p.text_muted))
                .padding([5, 7])
                .style(theme::vscode_icon_btn(p, false))
                .on_press(Message::Detached(win_id, DetachedAction::PickSingleFile)),
            "Open File… (Ctrl+O)",
            tooltip::Position::Bottom,
            p,
        ));

    let tabs_scroll = scrollable(tabs_row)
        .direction(theme::slim_horizontal_direction())
        .style(theme::slim_scrollable(p))
        .width(Length::Fill);

    let dirty_file_tabs = v
        .tabs
        .iter()
        .filter(|t| t.dirty && t.file_path.is_some() && t.doc_id.is_some())
        .count();

    let mut right_actions = row![].spacing(4).align_y(Alignment::Center);

    right_actions = right_actions.push(tip(
        button(
            row![
                icons::search(11.0, p.text_muted),
                text("Quick Open")
                    .size(type_scale::BADGE)
                    .color(p.text_secondary)
                    .wrapping(text::Wrapping::None),
                theme::kit::kbd_pill(p, "Ctrl+P"),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .padding([3, 7])
        .style(theme::subtle_btn(p))
        .on_press(Message::Detached(win_id, DetachedAction::ToggleQuickOpen)),
        "Go to File or :line (Ctrl+P)",
        tooltip::Position::Bottom,
        p,
    ));

    if dirty_file_tabs >= 2 {
        right_actions = right_actions.push(tip(
            button(
                text(format!("Save All ({dirty_file_tabs})"))
                    .size(type_scale::BADGE)
                    .wrapping(text::Wrapping::None),
            )
            .padding([4, 8])
            .style(theme::subtle_btn(p))
            .on_press(Message::Detached(win_id, DetachedAction::SaveAllTabs)),
            "Save all modified tabs (Ctrl+Alt+S)",
            tooltip::Position::Bottom,
            p,
        ));
    }

    if let Some(tab) = v.active_tab() {
        if tab.is_editable() && (tab.dirty || tab.is_untitled) {
            right_actions = right_actions.push(tip(
                button(
                    row![
                        icons::save(11.0, Color::WHITE),
                        text(if tab.is_untitled { "Save As…" } else { "Save" })
                            .size(type_scale::BADGE)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([4, 9])
                .style(theme::primary_btn(p))
                .on_press(Message::Detached(win_id, DetachedAction::SaveEditor)),
                "Save changes (Ctrl+S)",
                tooltip::Position::Bottom,
                p,
            ));
        }

        if tab.is_editable() && !tab.is_untitled {
            right_actions = right_actions.push(tip(
                button(icons::save(12.0, p.text_muted))
                    .padding([4, 6])
                    .style(theme::vscode_icon_btn(p, false))
                    .on_press(Message::Detached(win_id, DetachedAction::SaveTabAs)),
                "Save As… (Ctrl+Shift+S)",
                tooltip::Position::Bottom,
                p,
            ));
        }

        if tab.is_editable() && tab.has_preview_available() {
            right_actions = right_actions.push(tip(
                button(icons::split_horizontal(
                    13.0,
                    if v.split_right_preview {
                        p.accent_fg
                    } else {
                        p.text_muted
                    },
                ))
                .padding([4, 6])
                .style(theme::vscode_icon_btn(p, v.split_right_preview))
                .on_press(Message::Detached(
                    win_id,
                    DetachedAction::ToggleSplitPreview,
                )),
                "Split Live Preview (Ctrl+\\)",
                tooltip::Position::Bottom,
                p,
            ));
        }

        if tab.is_editable() || tab.has_preview_available() {
            right_actions = right_actions.push(tip(
                button(icons::search(
                    13.0,
                    if v.in_file_find_open {
                        p.accent_fg
                    } else {
                        p.text_muted
                    },
                ))
                .padding([4, 6])
                .style(theme::vscode_icon_btn(p, v.in_file_find_open))
                .on_press(Message::Detached(
                    win_id,
                    if v.in_file_find_open {
                        DetachedAction::CloseInFileFind
                    } else {
                        DetachedAction::OpenInFileFind {
                            show_replace: false,
                        }
                    },
                )),
                "Find in file (Ctrl+F)",
                tooltip::Position::Bottom,
                p,
            ));
        }
    }

    container(
        row![tabs_scroll, right_actions]
            .spacing(6)
            .align_y(Alignment::Center)
            .width(Length::Fill),
    )
    .padding(iced::Padding {
        top: 4.0,
        right: 8.0,
        bottom: 0.0,
        left: 6.0,
    })
    .width(Length::Fill)
    .style(theme::studio_tab_bar(p))
    .into()
}

fn render_breadcrumb_bar<'a>(
    win_id: window::Id,
    v: &'a DetachedViewerState,
    tab: &'a StudioTab,
    p: Palette,
) -> Element<'a, Message> {
    let crumb_source = tab
        .rel_path
        .as_deref()
        .unwrap_or(tab.title.as_str());
    let segments: Vec<&str> = crumb_source.split('/').filter(|s| !s.is_empty()).collect();

    let mut crumb_row = row![].spacing(3).align_y(Alignment::Center);
    let can_pick_siblings = v.search_root.is_some() && tab.rel_path.is_some();
    for (idx, seg) in segments.iter().enumerate() {
        let is_last = idx + 1 == segments.len();
        if idx > 0 {
            crumb_row = crumb_row.push(
                text("›")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted)
                    .wrapping(text::Wrapping::None),
            );
        }
        if can_pick_siblings {
            let parent_dir = if idx == 0 {
                String::new()
            } else {
                segments[..idx].join("/")
            };
            crumb_row = crumb_row.push(
                button(
                    text(*seg)
                        .size(type_scale::CAPTION)
                        .color(if is_last {
                            p.text_primary
                        } else {
                            p.text_secondary
                        })
                        .wrapping(text::Wrapping::None),
                )
                .padding([1, 4])
                .style(theme::vscode_status_item(p))
                .on_press(Message::Detached(
                    win_id,
                    DetachedAction::OpenBreadcrumbMenu(parent_dir),
                )),
            );
        } else {
            crumb_row = crumb_row.push(
                text(*seg)
                    .size(type_scale::CAPTION)
                    .color(if is_last {
                        p.text_primary
                    } else {
                        p.text_muted
                    })
                    .wrapping(text::Wrapping::None),
            );
        }
    }

    if tab.is_editable() {
        crumb_row = crumb_row.push(tip(
            button(
                text("@ Symbols")
                    .size(type_scale::BADGE)
                    .font(theme::CODE_FONT)
                    .color(p.accent_fg)
                    .wrapping(text::Wrapping::None),
            )
            .padding([1, 6])
            .style(theme::vscode_status_item(p))
            .on_press(Message::Detached(win_id, DetachedAction::OpenSymbolOutline)),
            "Jump to Symbol in File (Ctrl+Shift+O)",
            tooltip::Position::Bottom,
            p,
        ));
    }

    // Minimal mode pills on the right of breadcrumb strip
    let is_raster_only = tab.diff_image.as_ref().is_some_and(|d| !d.is_svg);
    let mut modes = row![].spacing(3).align_y(Alignment::Center);

    let in_git_repo = v
        .search_root
        .as_deref()
        .is_some_and(|r| r.join(".git").exists());
    if in_git_repo && tab.rel_path.is_some() && tab.diff_parsed.raw.is_empty() && !is_raster_only {
        modes = modes.push(tip(
            button(
                row![
                    icons::file_diff(11.0, p.text_secondary),
                    text("Diff HEAD")
                        .size(type_scale::BADGE)
                        .color(p.text_secondary)
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            )
            .padding([2, 8])
            .style(theme::filter_pill_btn(p, false))
            .on_press(Message::Detached(
                win_id,
                DetachedAction::DiffActiveWithHead,
            )),
            "Compare working file with Git HEAD",
            tooltip::Position::Bottom,
            p,
        ));
    }

    if tab.is_editable() {
        let active = tab.mode == DetachedViewMode::Edit;
        modes = modes.push(
            button(
                text("Code")
                    .size(type_scale::BADGE)
                    .color(if active {
                        p.accent_fg
                    } else {
                        p.text_muted
                    })
                    .wrapping(text::Wrapping::None),
            )
            .padding([2, 8])
            .style(theme::filter_pill_btn(p, active))
            .on_press(Message::Detached(
                win_id,
                DetachedAction::SetMode(DetachedViewMode::Edit),
            )),
        );
    }

    if tab.has_preview_available() {
        let active = tab.mode == DetachedViewMode::Preview;
        modes = modes.push(
            button(
                text("Preview")
                    .size(type_scale::BADGE)
                    .color(if active {
                        p.accent_fg
                    } else {
                        p.text_muted
                    })
                    .wrapping(text::Wrapping::None),
            )
            .padding([2, 8])
            .style(theme::filter_pill_btn(p, active))
            .on_press(Message::Detached(
                win_id,
                DetachedAction::SetMode(DetachedViewMode::Preview),
            )),
        );
    }

    if !is_raster_only && !tab.diff_parsed.raw.is_empty() {
        let split_a = tab.mode == DetachedViewMode::SideBySideDiff;
        let uni_a = tab.mode == DetachedViewMode::UnifiedDiff;
        modes = modes
            .push(
                button(
                    text("Split Diff")
                        .size(type_scale::BADGE)
                        .color(if split_a { p.accent_fg } else { p.text_muted })
                        .wrapping(text::Wrapping::None),
                )
                .padding([2, 8])
                .style(theme::filter_pill_btn(p, split_a))
                .on_press(Message::Detached(
                    win_id,
                    DetachedAction::SetMode(DetachedViewMode::SideBySideDiff),
                )),
            )
            .push(
                button(
                    text("Unified")
                        .size(type_scale::BADGE)
                        .color(if uni_a { p.accent_fg } else { p.text_muted })
                        .wrapping(text::Wrapping::None),
                )
                .padding([2, 8])
                .style(theme::filter_pill_btn(p, uni_a))
                .on_press(Message::Detached(
                    win_id,
                    DetachedAction::SetMode(DetachedViewMode::UnifiedDiff),
                )),
            );
    }

    if tab.diff_image.is_some() {
        let active = tab.mode == DetachedViewMode::ImageDiff;
        modes = modes.push(
            button(
                text("Image Diff")
                    .size(type_scale::BADGE)
                    .color(if active { p.accent_fg } else { p.text_muted })
                    .wrapping(text::Wrapping::None),
            )
            .padding([2, 8])
            .style(theme::filter_pill_btn(p, active))
            .on_press(Message::Detached(
                win_id,
                DetachedAction::SetMode(DetachedViewMode::ImageDiff),
            )),
        );
    }

    if tab.diff_csv.is_some() {
        let active = tab.mode == DetachedViewMode::CsvDiff;
        modes = modes.push(
            button(
                text("Table")
                    .size(type_scale::BADGE)
                    .color(if active { p.accent_fg } else { p.text_muted })
                    .wrapping(text::Wrapping::None),
            )
            .padding([2, 8])
            .style(theme::filter_pill_btn(p, active))
            .on_press(Message::Detached(
                win_id,
                DetachedAction::SetMode(DetachedViewMode::CsvDiff),
            )),
        );
    }

    if tab.is_editable() && tab.mode == DetachedViewMode::Edit {
        let can_undo = !tab.undo_stack.is_empty();
        let can_redo = !tab.redo_stack.is_empty();
        let mut undo_btn = button(icons::undo(
            12.0,
            if can_undo {
                p.text_secondary
            } else {
                p.text_muted
            },
        ))
        .padding([2, 5])
        .style(theme::vscode_icon_btn(p, false));
        if can_undo {
            undo_btn = undo_btn.on_press(Message::Detached(win_id, DetachedAction::Undo));
        }
        let mut redo_btn = button(icons::redo(
            12.0,
            if can_redo {
                p.text_secondary
            } else {
                p.text_muted
            },
        ))
        .padding([2, 5])
        .style(theme::vscode_icon_btn(p, false));
        if can_redo {
            redo_btn = redo_btn.on_press(Message::Detached(win_id, DetachedAction::Redo));
        }
        modes = modes
            .push(tip(undo_btn, "Undo (Ctrl+Z)", tooltip::Position::Bottom, p))
            .push(tip(
                redo_btn,
                "Redo (Ctrl+Y / Ctrl+Shift+Z)",
                tooltip::Position::Bottom,
                p,
            ));
    }

    if tab.dirty && tab.file_path.is_some() {
        modes = modes.push(tip(
            button(icons::refresh(11.0, p.warning_fg))
                .padding([2, 5])
                .style(theme::vscode_icon_btn(p, false))
                .on_press(Message::Detached(win_id, DetachedAction::RevertEditor)),
            "Revert unsaved changes from disk",
            tooltip::Position::Bottom,
            p,
        ));
    }

    if tab.file_path.is_some() || (tab.left_path.is_some() && tab.right_path.is_some()) {
        modes = modes.push(tip(
            button(icons::refresh(11.0, p.text_muted))
                .padding([2, 5])
                .style(theme::vscode_icon_btn(p, false))
                .on_press(Message::Detached(win_id, DetachedAction::ReloadFiles)),
            "Reload from disk",
            tooltip::Position::Bottom,
            p,
        ));
    }

    if let Some(rel) = tab.rel_path.clone()
        && v.search_root.is_some()
        && tab.left_path.is_none()
    {
        modes = modes.push(tip(
            button(icons::trash(11.0, p.danger_fg))
                .padding([2, 5])
                .style(theme::vscode_icon_btn(p, false))
                .on_press(Message::Detached(
                    win_id,
                    DetachedAction::RequestDeleteExplorerEntry {
                        rel_path: rel,
                        is_dir: false,
                    },
                )),
            "Delete this file from disk…",
            tooltip::Position::Bottom,
            p,
        ));
    }

    let _ = v;
    container(
        row![
            theme::clipped_line(crumb_row, Length::Fill),
            modes,
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .width(Length::Fill),
    )
    .padding([4, 12])
    .width(Length::Fill)
    .style(theme::studio_breadcrumb_bar(p))
    .into()
}

fn render_studio_code_editor<'a>(
    win_id: window::Id,
    v: &'a DetachedViewerState,
    tab: &'a StudioTab,
    theme_mode: ThemeMode,
    p: Palette,
) -> Element<'a, Message> {
    if tab.loading_file {
        return container(
            text(format!("Opening {}…", tab.title))
                .size(type_scale::CAPTION)
                .color(p.text_muted),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into();
    }

    if !tab.is_editable() {
        return theme::empty_state(
            p,
            icons::file_code(icon_size::MD, p.text_muted),
            "Binary or read-only file",
            "Switch to Preview to inspect this file.",
            None,
        );
    }

    if tab.is_virtualized_code_mode() {
        return render_virtualized_studio_code(win_id, v, tab, theme_mode, p);
    }

    let active_match = if v.in_file_find_open {
        v.in_file_active_idx
            .and_then(|i| v.in_file_matches.get(i))
    } else {
        None
    };
    let line_offset = tab.edit_window.map(|(s, _)| s).unwrap_or(0);
    let hl_settings = CodeHighlightSettings {
        language: tab.language,
        theme_mode,
        search_query: if v.in_file_find_open {
            v.in_file_query.clone()
        } else {
            String::new()
        },
        search_options: v.in_file_options,
        active_match_line: active_match.and_then(|m| m.line_index.checked_sub(line_offset)),
        active_match_cols: active_match.map(|m| (m.col_start, m.col_end)),
    };

    let ed = text_editor(&tab.editor_content)
        .on_action(move |a| Message::Detached(win_id, DetachedAction::EditorAction(a)))
        .highlight_with::<CodeSyntaxHighlighter>(hl_settings, format_code_token)
        .key_binding(move |key_press| {
            let cmd = key_press.modifiers.command();
            let shift = key_press.modifiers.shift();
            let alt = key_press.modifiers.alt();

            // Window-level shortcuts handled once at window dispatch
            if cmd
                && matches!(
                    key_press.key.as_ref(),
                    keyboard::Key::Character("s")
                        | keyboard::Key::Character("S")
                        | keyboard::Key::Character("p")
                        | keyboard::Key::Character("P")
                        | keyboard::Key::Character("k")
                        | keyboard::Key::Character("K")
                        | keyboard::Key::Character("g")
                        | keyboard::Key::Character("G")
                        | keyboard::Key::Character("n")
                        | keyboard::Key::Character("N")
                        | keyboard::Key::Character("f")
                        | keyboard::Key::Character("F")
                        | keyboard::Key::Character("h")
                        | keyboard::Key::Character("H")
                        | keyboard::Key::Character("w")
                        | keyboard::Key::Character("W")
                        | keyboard::Key::Character("o")
                        | keyboard::Key::Character("O")
                        | keyboard::Key::Character("b")
                        | keyboard::Key::Character("B")
                        | keyboard::Key::Character("e")
                        | keyboard::Key::Character("E")
                        | keyboard::Key::Character("t")
                        | keyboard::Key::Character("T")
                        | keyboard::Key::Character("\\")
                        | keyboard::Key::Character("=")
                        | keyboard::Key::Character("+")
                        | keyboard::Key::Character("-")
                )
            {
                return None;
            }
            if alt
                && matches!(
                    key_press.key.as_ref(),
                    keyboard::Key::Character("z") | keyboard::Key::Character("Z")
                )
            {
                return None;
            }

            // Per-tab Undo / Redo (`S1`)
            if cmd && !alt {
                match key_press.key.as_ref() {
                    keyboard::Key::Character("z") | keyboard::Key::Character("Z") if !shift => {
                        return Some(text_editor::Binding::Custom(Message::Detached(
                            win_id,
                            DetachedAction::Undo,
                        )));
                    }
                    keyboard::Key::Character("z") | keyboard::Key::Character("Z") if shift => {
                        return Some(text_editor::Binding::Custom(Message::Detached(
                            win_id,
                            DetachedAction::Redo,
                        )));
                    }
                    keyboard::Key::Character("y") | keyboard::Key::Character("Y") if !shift => {
                        return Some(text_editor::Binding::Custom(Message::Detached(
                            win_id,
                            DetachedAction::Redo,
                        )));
                    }
                    keyboard::Key::Character("/") | keyboard::Key::Character("?") if !shift => {
                        return Some(text_editor::Binding::Custom(Message::Detached(
                            win_id,
                            DetachedAction::ToggleComment,
                        )));
                    }
                    _ => {}
                }
            }

            // Alt+Up / Alt+Down move lines vertically (`S4`)
            if alt && !cmd && !shift {
                match key_press.key.as_ref() {
                    keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                        return Some(text_editor::Binding::Custom(Message::Detached(
                            win_id,
                            DetachedAction::MoveLine(-1),
                        )));
                    }
                    keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                        return Some(text_editor::Binding::Custom(Message::Detached(
                            win_id,
                            DetachedAction::MoveLine(1),
                        )));
                    }
                    _ => {}
                }
            }

            // Tab / Shift+Tab block indent & outdent (`S4`)
            if matches!(key_press.key.as_ref(), keyboard::Key::Named(keyboard::key::Named::Tab))
                && !cmd
                && !alt
            {
                return Some(text_editor::Binding::Custom(Message::Detached(
                    win_id,
                    if shift {
                        DetachedAction::OutdentSelection
                    } else {
                        DetachedAction::InsertIndent
                    },
                )));
            }

            // Smart Enter auto-indentation & bracket expansion (`S4`)
            if matches!(key_press.key.as_ref(), keyboard::Key::Named(keyboard::key::Named::Enter))
                && !cmd
                && !alt
                && !shift
            {
                return Some(text_editor::Binding::Custom(Message::Detached(
                    win_id,
                    DetachedAction::SmartEnter,
                )));
            }

            // Auto-closing pairs & closing delimiter skip-over (`S4`)
            if !cmd
                && !alt
                && let Some(txt) = key_press.text.as_deref()
                && txt.chars().count() == 1
                && let Some(ch) = txt.chars().next()
                && matches!(ch, '(' | ')' | '[' | ']' | '{' | '}' | '"' | '\'' | '`')
            {
                return Some(text_editor::Binding::Custom(Message::Detached(
                    win_id,
                    DetachedAction::TypeAutoPairChar(ch),
                )));
            }

            text_editor::Binding::from_key_press(key_press)
        });

    let editor_el = crate::views::code_editor::numbered_code_editor_with_marks(
        &tab.editor_content,
        ed,
        &tab.editor_scroll,
        crate::views::code_editor::NumberedEditorConfig::studio(
            v.editor_font_size,
            v.editor_word_wrap,
        )
        .with_indent_step(tab.indent_spaces)
        .with_line_offset(line_offset),
        Some(Arc::clone(&tab.git_line_marks)),
        p,
        move |vp| Message::Detached(win_id, DetachedAction::EditorScrolled(vp)),
    );

    if let Some((win_start, win_end)) = tab.edit_window {
        let slice_banner = container(
            row![
                icons::file_code(13.0, p.accent_fg),
                theme::clipped_line(
                    text(format!(
                        "Editing slice: lines {}–{} of {}  ·  Ctrl+S saves the entire file",
                        win_start + 1,
                        win_end,
                        tab.total_raw_lines
                    ))
                    .size(type_scale::CAPTION)
                    .color(p.text_primary),
                    Length::Fill,
                ),
                button(
                    text("Done Editing Slice")
                        .size(type_scale::BADGE)
                        .wrapping(text::Wrapping::None),
                )
                .padding([3, 10])
                .style(theme::primary_btn(p))
                .on_press(Message::Detached(win_id, DetachedAction::CommitEditWindow)),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([5, 12])
        .width(Length::Fill)
        .style(theme::studio_breadcrumb_bar(p));

        return column![
            slice_banner,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            editor_el
        ]
        .spacing(0)
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
    }

    editor_el
}

fn render_virtualized_studio_code<'a>(
    win_id: window::Id,
    v: &'a DetachedViewerState,
    tab: &'a StudioTab,
    theme_mode: ThemeMode,
    p: Palette,
) -> Element<'a, Message> {
    let cfg = crate::views::code_editor::NumberedEditorConfig::studio(
        v.editor_font_size,
        false,
    );
    let row_h = cfg.line_height_px().max(DIFF_LINE_H).ceil();
    let cur_ln = tab.virtual_cursor_line.min(tab.total_raw_lines.saturating_sub(1));

    let fast_banner = container(
        row![
            icons::file_code(12.0, p.accent_fg),
            theme::clipped_line(
                text(format!(
                    "Fast Large-File Mode ({} lines) — virtualized for instant scrolling · double-click any line to edit",
                    tab.total_raw_lines
                ))
                .size(type_scale::CAPTION)
                .color(p.text_secondary),
                Length::Fill,
            ),
            button(
                text(format!("Edit Around Ln {}", cur_ln + 1))
                    .size(type_scale::BADGE)
                    .wrapping(text::Wrapping::None),
            )
            .padding([3, 10])
            .style(theme::primary_btn(p))
            .on_press(Message::Detached(
                win_id,
                DetachedAction::OpenEditWindowAround(cur_ln),
            )),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([4, 12])
    .width(Length::Fill)
    .style(theme::studio_breadcrumb_bar(p));

    let Some(doc) = tab.preview_state.highlighted_doc(theme_mode) else {
        return column![
            fast_banner,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            Space::with_height(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
    };

    let total_lines = doc.lines.len();
    let total_h = total_lines as f32 * row_h;
    let viewport_h = tab.editor_scroll.viewport_h.max(650.0);
    let first = (((tab.editor_scroll.scroll_y - DIFF_OVERSCAN_PX).max(0.0) / row_h).floor()
        as usize)
        .min(total_lines);
    let last = (((tab.editor_scroll.scroll_y + viewport_h + DIFF_OVERSCAN_PX) / row_h).ceil()
        as usize)
        .min(total_lines)
        .max(first);
    let top_spacer = first as f32 * row_h;
    let bottom_spacer = (total_h - last as f32 * row_h).max(0.0);
    let gutter_w = crate::views::code_editor::gutter_width(total_lines, cfg.font_size);

    let active_search_line = if v.in_file_find_open {
        v.in_file_active_idx
            .and_then(|idx| v.in_file_matches.get(idx))
            .map(|m| m.line_index)
    } else {
        None
    };

    let mut lines_col = column![Space::with_height(Length::Fixed(top_spacer))].spacing(0);
    for i in first..last {
        let line = &doc.lines[i];
        let is_active = i == cur_ln;
        let is_search_hit = active_search_line == Some(i)
            || (v.in_file_find_open
                && !v.in_file_matches.is_empty()
                && v.in_file_matches
                    .binary_search_by_key(&i, |m| m.line_index)
                    .is_ok());

        let git_mark_color = tab.git_line_marks.get(&i).map(|m| match m {
            LineGitMark::Added => p.git_added,
            LineGitMark::Modified => p.git_modified,
            LineGitMark::DeletedAbove => p.git_deleted,
        });

        let gutter_bg = if is_active {
            Color {
                a: 0.42,
                ..p.fill_selected
            }
        } else {
            p.surface_sidebar
        };

        let gutter_num = container(
            text(format!("{}", i + 1))
                .size(cfg.font_size.min(type_scale::CAPTION))
                .font(theme::CODE_FONT)
                .color(if is_active { p.text_primary } else { p.text_muted })
                .wrapping(text::Wrapping::None),
        )
        .width(Length::Fixed((gutter_w - 4.0).max(28.0)))
        .height(Length::Fixed(row_h))
        .padding([0, 6])
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Center)
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(gutter_bg)),
            ..Default::default()
        });

        let mark_strip = container(Space::with_width(Length::Fixed(3.0)))
            .width(Length::Fixed(3.0))
            .height(Length::Fixed(row_h))
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(
                    git_mark_color.unwrap_or(gutter_bg),
                )),
                ..Default::default()
            });

        let sep = container(Space::with_width(Length::Fixed(1.0)))
            .width(Length::Fixed(1.0))
            .height(Length::Fixed(row_h))
            .style(move |_| container::Style {
                background: Some(iced::Background::Color(p.border_subtle)),
                ..Default::default()
            });

        let code_el: Element<'_, Message> = if line.spans.is_empty() {
            text(line.text.clone())
                .size(cfg.font_size)
                .font(theme::CODE_FONT)
                .color(p.text_primary)
                .wrapping(text::Wrapping::None)
                .into()
        } else {
            render_highlighted_code(&line.text, &[], &line.spans, p.text_primary, None)
        };

        let row_bg = if active_search_line == Some(i) {
            Some(Color {
                a: 0.28,
                ..p.warning_fg
            })
        } else if is_search_hit {
            Some(Color {
                a: 0.16,
                ..p.warning_fg
            })
        } else if is_active {
            Some(Color {
                a: 0.28,
                ..p.fill_selected
            })
        } else {
            None
        };

        let code_cell = container(code_el)
            .width(Length::Fill)
            .height(Length::Fixed(row_h))
            .padding([0, 12])
            .align_y(iced::alignment::Vertical::Center)
            .style(move |_| container::Style {
                background: row_bg.map(iced::Background::Color),
                ..Default::default()
            });

        let clickable_row = mouse_area(
            row![gutter_num, mark_strip, sep, code_cell]
                .spacing(0)
                .height(Length::Fixed(row_h))
                .align_y(Alignment::Center),
        )
        .on_press(Message::Detached(
            win_id,
            DetachedAction::VirtualCodeLineClicked(i),
        ));

        lines_col = lines_col.push(clickable_row);
    }
    lines_col = lines_col.push(Space::with_height(Length::Fixed(bottom_spacer)));

    let min_w = (doc.max_chars as f32 * (cfg.font_size * 0.62) + gutter_w + 48.0)
        .clamp(420.0, 3600.0);
    let scroller = container(theme::responsive_hscroll_synced(
        lines_col,
        min_w,
        tab.editor_scroll.id.clone(),
        move |vp| Message::Detached(win_id, DetachedAction::EditorScrolled(vp)),
        p,
    ))
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |_| container::Style {
        background: Some(iced::Background::Color(p.surface_input)),
        ..Default::default()
    });

    column![
        fast_banner,
        horizontal_rule(1).style(theme::subtle_rule(p)),
        scroller
    ]
    .spacing(0)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn render_welcome_canvas<'a>(
    win_id: window::Id,
    v: &'a DetachedViewerState,
    p: Palette,
) -> Element<'a, Message> {
    let shortcut_row = |keys: &'static str, desc: &'static str| {
        row![
            container(
                text(keys)
                    .size(type_scale::BADGE)
                    .font(theme::CODE_FONT)
                    .color(p.accent_fg)
                    .wrapping(text::Wrapping::None),
            )
            .padding([2, 7])
            .style(theme::section_panel_box(p)),
            text(desc)
                .size(type_scale::CAPTION)
                .color(p.text_secondary)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
    };

    let subtitle = match &v.search_root_name {
        Some(name) => format!("Workspace folder: {name}"),
        None => "Lightweight code editor with syntax highlighting, search & replace, and diff."
            .to_string(),
    };

    let actions = row![
        button(
            row![
                icons::file_plus(13.0, Color::WHITE),
                text("New File")
                    .size(type_scale::CAPTION)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([6, 14])
        .style(theme::primary_btn(p))
        .on_press(Message::Detached(win_id, DetachedAction::NewUntitledTab)),
        button(
            row![
                icons::folder_open(13.0, p.text_primary),
                text("Open File")
                    .size(type_scale::CAPTION)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([6, 14])
        .style(theme::subtle_btn(p))
        .on_press(Message::Detached(win_id, DetachedAction::PickSingleFile)),
        button(
            row![
                icons::search(13.0, p.text_primary),
                text("Quick Open")
                    .size(type_scale::CAPTION)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([6, 14])
        .style(theme::subtle_btn(p))
        .on_press(Message::Detached(win_id, DetachedAction::ToggleQuickOpen)),
        button(
            row![
                icons::file_diff(13.0, p.text_primary),
                text("Compare Files")
                    .size(type_scale::CAPTION)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([6, 14])
        .style(theme::subtle_btn(p))
        .on_press(Message::Detached(
            win_id,
            DetachedAction::SetSidebarPanel(StudioSidebarPanel::Compare),
        )),
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    container(
        column![
            icons::file_code(28.0, p.accent_fg),
            text("GitFleet Studio")
                .size(type_scale::TITLE)
                .color(p.text_primary),
            text(subtitle)
                .size(type_scale::BODY)
                .color(p.text_muted),
            Space::with_height(Length::Fixed(8.0)),
            actions,
            Space::with_height(Length::Fixed(12.0)),
            column![
                shortcut_row("Ctrl+P / Ctrl+G", "Quick Open file or jump to :line"),
                shortcut_row("Ctrl+N / Ctrl+O", "New scratch tab or open file"),
                shortcut_row("Ctrl+Z / Ctrl+Y", "Undo & redo per tab"),
                shortcut_row("Ctrl+/ · Alt+↑/↓", "Toggle comment · Move line up/down"),
                shortcut_row("Ctrl+Shift+F", "Search & replace across folder"),
                shortcut_row("Ctrl+S / Ctrl+Shift+S", "Save / Save As"),
            ]
            .spacing(6)
            .align_x(Alignment::Start),
        ]
        .spacing(8)
        .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(iced::alignment::Horizontal::Center)
    .align_y(iced::alignment::Vertical::Center)
    .style(theme::panel_box(p))
    .into()
}
