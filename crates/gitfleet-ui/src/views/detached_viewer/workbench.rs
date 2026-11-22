//! Main Studio editor workbench: tab bar, breadcrumb/mode bar, code editor canvas,
//! diff/preview hosts, and empty welcome screen.

use super::find_bar::render_in_file_find_bar;
use super::io::map_text_diff_msg;
use super::sidebar_explorer::lang_tag;
use super::state::{
    DetachedAction, DetachedViewMode, DetachedViewerState, StudioSidebarPanel, StudioTab,
};
use super::status_bar::render_mode_subbar;
use crate::app::Message;
use crate::icons;
use crate::syntax::{CodeHighlightSettings, CodeSyntaxHighlighter, format_code_token};
use crate::theme::{self, Palette, ThemeMode, icon_size, type_scale};
use crate::views::csv_diff::render_csv_diff;
use crate::views::diff_viewer::{
    TextDiffRenderCtx, render_file_preview, render_side_by_side_diff_with,
    render_unified_diff_with,
};
use crate::views::image_diff::render_image_diff;
use crate::views::{tip, truncate_chars};
use iced::widget::{
    Space, button, column, container, horizontal_rule, row, scrollable, text, text_editor,
    tooltip,
};
use iced::{Alignment, Color, Element, Length, keyboard, window};

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
        DetachedViewMode::Edit => render_studio_code_editor(win_id, v, tab, theme_mode, p),
        DetachedViewMode::Preview => container(
            render_file_preview(
                &tab.preview_state,
                &tab.preview_editor,
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
                    right_scroll_id: tab.diff_right_scroll_id.clone(),
                    unified_scroll_id: tab.diff_unified_scroll_id.clone(),
                    left_scroll_y: tab.diff_left_scroll_y,
                    right_scroll_y: tab.diff_right_scroll_y,
                    unified_scroll_y: tab.diff_unified_scroll_y,
                    left_viewport_h: tab.diff_left_viewport_h,
                    right_viewport_h: tab.diff_right_viewport_h,
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
                    right_scroll_id: tab.diff_right_scroll_id.clone(),
                    unified_scroll_id: tab.diff_unified_scroll_id.clone(),
                    left_scroll_y: tab.diff_left_scroll_y,
                    right_scroll_y: tab.diff_right_scroll_y,
                    unified_scroll_y: tab.diff_unified_scroll_y,
                    left_viewport_h: tab.diff_left_viewport_h,
                    right_viewport_h: tab.diff_right_viewport_h,
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

        let tab_pill = row![
            button(inner)
                .padding([6, 8])
                .style(theme::studio_tab_btn(p, is_active))
                .on_press(Message::Detached(win_id, DetachedAction::SelectTab(idx))),
            close_btn,
        ]
        .spacing(1)
        .align_y(Alignment::Center);

        tabs_row = tabs_row.push(tab_pill);
    }

    tabs_row = tabs_row.push(tip(
        button(icons::plus(12.0, p.text_muted))
            .padding([5, 7])
            .style(theme::vscode_icon_btn(p, false))
            .on_press(Message::Detached(win_id, DetachedAction::PickSingleFile)),
        "Open File (Ctrl+O)",
        tooltip::Position::Bottom,
        p,
    ));

    let tabs_scroll = scrollable(tabs_row)
        .direction(theme::slim_horizontal_direction())
        .style(theme::slim_scrollable(p))
        .width(Length::Fill);

    let mut right_actions = row![].spacing(4).align_y(Alignment::Center);

    if let Some(tab) = v.active_tab() {
        if tab.is_editable() && tab.dirty {
            right_actions = right_actions.push(tip(
                button(
                    row![
                        icons::save(11.0, Color::WHITE),
                        text("Save")
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

    let mut crumb_row = row![].spacing(4).align_y(Alignment::Center);
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

    // Minimal mode pills on the right of breadcrumb strip
    let is_raster_only = tab.diff_image.as_ref().is_some_and(|d| !d.is_svg);
    let mut modes = row![].spacing(3).align_y(Alignment::Center);

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

    if tab.dirty {
        modes = modes.push(tip(
            button(icons::undo(12.0, p.warning_fg))
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

    let active_match = if v.in_file_find_open {
        v.in_file_active_idx
            .and_then(|i| v.in_file_matches.get(i))
    } else {
        None
    };
    let hl_settings = CodeHighlightSettings {
        language: tab.language,
        theme_mode,
        search_query: if v.in_file_find_open {
            v.in_file_query.clone()
        } else {
            String::new()
        },
        search_options: v.in_file_options,
        active_match_line: active_match.map(|m| m.line_index),
        active_match_cols: active_match.map(|m| (m.col_start, m.col_end)),
    };

    let wrapping = if v.editor_word_wrap {
        text::Wrapping::WordOrGlyph
    } else {
        text::Wrapping::None
    };

    text_editor(&tab.editor_content)
        .on_action(move |a| Message::Detached(win_id, DetachedAction::EditorAction(a)))
        .font(theme::CODE_FONT)
        .size(v.editor_font_size)
        .line_height(text::LineHeight::Relative(1.48))
        .padding([12, 16])
        .wrapping(wrapping)
        .height(Length::Fill)
        .highlight_with::<CodeSyntaxHighlighter>(hl_settings, format_code_token)
        .key_binding(move |key_press| {
            // Let window-level shortcuts handle Ctrl+S / Ctrl+F / Ctrl+H / Ctrl+W / Ctrl+O / Ctrl+B / Ctrl+Shift+E / Ctrl+=/- / Alt+Z cleanly once
            if key_press.modifiers.command()
                && matches!(
                    key_press.key.as_ref(),
                    keyboard::Key::Character("s")
                        | keyboard::Key::Character("S")
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
                        | keyboard::Key::Character("=")
                        | keyboard::Key::Character("+")
                        | keyboard::Key::Character("-")
                )
            {
                return None;
            }
            if key_press.modifiers.alt()
                && matches!(
                    key_press.key.as_ref(),
                    keyboard::Key::Character("z") | keyboard::Key::Character("Z")
                )
            {
                return None;
            }
            // Real code-editor Tab indentation!
            if matches!(key_press.key.as_ref(), keyboard::Key::Named(keyboard::key::Named::Tab))
                && !key_press.modifiers.command()
                && !key_press.modifiers.alt()
                && !key_press.modifiers.shift()
            {
                return Some(text_editor::Binding::Custom(Message::Detached(
                    win_id,
                    DetachedAction::InsertIndent,
                )));
            }
            text_editor::Binding::from_key_press(key_press)
        })
        .style(theme::studio_editor(p))
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
                icons::file_code(13.0, Color::WHITE),
                text("Open File")
                    .size(type_scale::CAPTION)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([6, 14])
        .style(theme::primary_btn(p))
        .on_press(Message::Detached(win_id, DetachedAction::PickSingleFile)),
        button(
            row![
                icons::search(13.0, p.text_primary),
                text("Search & Replace")
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
            DetachedAction::OpenAndFocusSearchSidebar,
        )),
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
                shortcut_row("Ctrl+O", "Open file into new tab"),
                shortcut_row("Ctrl+Shift+F", "Search & replace across folder"),
                shortcut_row("Ctrl+F / Ctrl+H", "Find & replace inside open file"),
                shortcut_row("Ctrl+B", "Toggle Explorer / Search sidebar"),
                shortcut_row("Ctrl+S", "Save active file"),
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
