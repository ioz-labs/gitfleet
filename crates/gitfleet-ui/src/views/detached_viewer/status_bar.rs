//! Contextual Diff/Preview sub-bar and bottom Code Editor status bar with dropdown menu triggers.

use super::state::{
    DetachedAction, DetachedViewMode, DetachedViewerState, StudioStatusMenu, StudioTab,
};
use crate::app::Message;
use crate::icons;
use crate::theme::{self, Palette, ThemeMode, space, type_scale};
use crate::views::tip;
use iced::widget::{Space, button, container, row, text, tooltip};
use iced::{Alignment, Element, Length, window};

// ── Contextual Mode Sub-bar (Diff / Preview) ─────────────────────────────────

pub(crate) fn render_mode_subbar<'a>(
    win_id: window::Id,
    tab: &'a StudioTab,
    p: Palette,
) -> Option<Element<'a, Message>> {
    let is_text_diff = matches!(
        tab.mode,
        DetachedViewMode::UnifiedDiff | DetachedViewMode::SideBySideDiff
    );
    if is_text_diff && !tab.diff_parsed.raw.is_empty() {
        let stats = tab.diff_parsed.stats;
        let has_changes = stats.added_blocks + stats.removed_blocks + stats.modified_blocks > 0;
        let mut bar = row![].spacing(6).align_y(Alignment::Center);

        if has_changes {
            let mut stat_row = row![].spacing(5).align_y(Alignment::Center);
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
            bar = bar.push(
                container(stat_row)
                    .padding([2, 6])
                    .style(theme::section_panel_box(p)),
            );
        }

        if !tab.diff_parsed.blocks.is_empty() {
            bar = bar.push(
                button(
                    text("Compact")
                        .size(type_scale::CAPTION)
                        .wrapping(text::Wrapping::None),
                )
                .padding([2, 8])
                .style(theme::filter_pill_btn(p, tab.diff_compact))
                .on_press(Message::Detached(win_id, DetachedAction::ToggleCompact)),
            );

            if has_changes {
                bar = bar
                    .push(tip(
                        button(icons::push(12.0, p.text_secondary))
                            .padding([2, 6])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::Detached(win_id, DetachedAction::JumpChange(-1))),
                        "Previous Change",
                        tooltip::Position::Bottom,
                        p,
                    ))
                    .push(tip(
                        button(icons::pull(12.0, p.text_secondary))
                            .padding([2, 6])
                            .style(theme::subtle_btn(p))
                            .on_press(Message::Detached(win_id, DetachedAction::JumpChange(1))),
                        "Next Change",
                        tooltip::Position::Bottom,
                        p,
                    ));
            }
        }

        return Some(bar.width(Length::Fill).into());
    }

    if tab.mode == DetachedViewMode::Preview
        && (tab.preview_state.has_rich_view() || tab.preview_state.copyable_text().is_some())
    {
        let mut bar = row![].spacing(6).align_y(Alignment::Center);
        if tab.preview_state.has_rich_view() {
            bar = bar.push(
                button(
                    text(if tab.preview_raw_mode {
                        "Rendered"
                    } else {
                        "Select Text"
                    })
                    .size(type_scale::CAPTION)
                    .wrapping(text::Wrapping::None),
                )
                .padding([2, 8])
                .style(theme::filter_pill_btn(p, tab.preview_raw_mode))
                .on_press(Message::Detached(win_id, DetachedAction::TogglePreviewRaw)),
            );
        }
        if tab.preview_state.copyable_text().is_some() {
            let has_sel = tab
                .preview_editor
                .selection()
                .is_some_and(|s| !s.is_empty());
            bar = bar.push(
                button(
                    row![
                        icons::copy(
                            12.0,
                            if has_sel {
                                p.accent_fg
                            } else {
                                p.text_secondary
                            },
                        ),
                        text(if has_sel { "Copy Sel" } else { "Copy" })
                            .size(type_scale::CAPTION)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([2, 8])
                .style(theme::filter_pill_btn(p, has_sel))
                .on_press(Message::Detached(win_id, DetachedAction::CopyPreviewText)),
            );
        }
        return Some(bar.width(Length::Fill).into());
    }

    None
}

// ── Bottom Code Editor Status Bar ────────────────────────────────────────────

pub(crate) fn render_studio_status_bar<'a>(
    win_id: window::Id,
    v: &'a DetachedViewerState,
    theme_mode: ThemeMode,
    p: Palette,
) -> Element<'a, Message> {
    let note_color = if v.status_is_error {
        p.danger_fg
    } else if v.loading || v.search_running {
        p.accent_fg
    } else {
        p.text_muted
    };

    let sidebar_toggle = tip(
        button(icons::split_horizontal(
            12.0,
            if v.sidebar_open {
                p.accent_fg
            } else {
                p.text_muted
            },
        ))
        .padding([2, 5])
        .style(theme::vscode_status_item(p))
        .on_press(Message::Detached(win_id, DetachedAction::ToggleSidebar)),
        "Toggle Sidebar (Ctrl+B)",
        tooltip::Position::Top,
        p,
    );

    let mut right = row![].spacing(space::XS).align_y(Alignment::Center);

    if let Some(tab) = v.active_tab() {
        let active_menu = v.status_menu.map(|(k, _)| k);
        if tab.is_editable() {
            let (ln, col) = tab.editor_content.cursor_position();
            let eol_open = active_menu == Some(StudioStatusMenu::Eol);
            let enc_open = active_menu == Some(StudioStatusMenu::Encoding);
            right = right
                .push(tip(
                    button(
                        text(format!("Ln {}, Col {}", ln + 1, col + 1))
                            .size(type_scale::BADGE)
                            .font(theme::CODE_FONT)
                            .color(if v.goto_line_visible {
                                p.accent_fg
                            } else {
                                p.text_secondary
                            })
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([1, 5])
                    .style(theme::vscode_status_item(p))
                    .on_press(Message::Detached(win_id, DetachedAction::OpenGoToLine)),
                    "Go to Line / Column (Ctrl+G)",
                    tooltip::Position::Top,
                    p,
                ))
                .push(tip(
                    button(
                        row![
                            text(if tab.indent_use_tabs {
                                "Tabs".to_string()
                            } else {
                                format!("Spaces: {}", tab.indent_spaces)
                            })
                            .size(type_scale::BADGE)
                            .color(if active_menu == Some(StudioStatusMenu::Indent) {
                                p.accent_fg
                            } else {
                                p.text_muted
                            })
                            .wrapping(text::Wrapping::None),
                            icons::chevron_down(10.0, p.text_muted),
                        ]
                        .spacing(3)
                        .align_y(Alignment::Center),
                    )
                    .padding([1, 5])
                    .style(theme::vscode_status_item(p))
                    .on_press(Message::Detached(
                        win_id,
                        DetachedAction::OpenStatusMenu(StudioStatusMenu::Indent),
                    )),
                    "Select Indentation (Spaces / Tabs)",
                    tooltip::Position::Top,
                    p,
                ))
                .push(tip(
                    button(
                        row![
                            text(tab.encoding.label())
                                .size(type_scale::BADGE)
                                .color(if enc_open {
                                    p.accent_fg
                                } else {
                                    p.text_secondary
                                })
                                .wrapping(text::Wrapping::None),
                            icons::chevron_down(10.0, p.text_muted),
                        ]
                        .spacing(3)
                        .align_y(Alignment::Center),
                    )
                    .padding([1, 5])
                    .style(theme::vscode_status_item(p))
                    .on_press(Message::Detached(
                        win_id,
                        DetachedAction::OpenStatusMenu(StudioStatusMenu::Encoding),
                    )),
                    "Select File Encoding (Save / Reopen)",
                    tooltip::Position::Top,
                    p,
                ))
                .push(tip(
                    button(
                        row![
                            text(tab.line_ending.label())
                                .size(type_scale::BADGE)
                                .font(theme::CODE_FONT)
                                .color(if eol_open {
                                    p.accent_fg
                                } else {
                                    p.text_secondary
                                })
                                .wrapping(text::Wrapping::None),
                            icons::chevron_down(10.0, p.text_muted),
                        ]
                        .spacing(3)
                        .align_y(Alignment::Center),
                    )
                    .padding([1, 5])
                    .style(theme::vscode_status_item(p))
                    .on_press(Message::Detached(
                        win_id,
                        DetachedAction::OpenStatusMenu(StudioStatusMenu::Eol),
                    )),
                    "Select End of Line Sequence (LF / CRLF)",
                    tooltip::Position::Top,
                    p,
                ));
        }

        // Clickable Syntax Highlighting Language Menu
        let lang_col = tab.language.badge_color(theme_mode);
        let syntax_open = active_menu == Some(StudioStatusMenu::Syntax);
        right = right.push(tip(
            button(
                row![
                    container(Space::new(Length::Fixed(6.0), Length::Fixed(6.0)))
                        .style(theme::dot_box(lang_col)),
                    text(tab.language.label())
                        .size(type_scale::BADGE)
                        .color(if syntax_open {
                            p.accent_fg
                        } else {
                            p.text_primary
                        })
                        .wrapping(text::Wrapping::None),
                    icons::chevron_down(10.0, p.text_muted),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            )
            .padding([1, 6])
            .style(theme::vscode_status_item(p))
            .on_press(Message::Detached(
                win_id,
                DetachedAction::OpenStatusMenu(StudioStatusMenu::Syntax),
            )),
            "Select Syntax Highlighting Language",
            tooltip::Position::Top,
            p,
        ));
    }

    // Word Wrap toggle & Font zoom controls
    right = right
        .push(tip(
            button(
                text(if v.editor_word_wrap { "Wrap" } else { "No Wrap" })
                    .size(type_scale::BADGE)
                    .color(if v.editor_word_wrap {
                        p.accent_fg
                    } else {
                        p.text_muted
                    })
                    .wrapping(text::Wrapping::None),
            )
            .padding([1, 5])
            .style(theme::vscode_status_item(p))
            .on_press(Message::Detached(win_id, DetachedAction::ToggleWordWrap)),
            "Toggle Word Wrap (Alt+Z)",
            tooltip::Position::Top,
            p,
        ))
        .push(
            row![
                tip(
                    button(text("−").size(type_scale::BADGE).color(p.text_muted))
                        .padding([1, 4])
                        .style(theme::vscode_status_item(p))
                        .on_press(Message::Detached(
                            win_id,
                            DetachedAction::AdjustFontSize(-1.0),
                        )),
                    "Decrease font size (Ctrl+-)",
                    tooltip::Position::Top,
                    p,
                ),
                text(format!("{:.0}px", v.editor_font_size))
                    .size(type_scale::BADGE)
                    .font(theme::CODE_FONT)
                    .color(p.text_muted)
                    .wrapping(text::Wrapping::None),
                tip(
                    button(text("+").size(type_scale::BADGE).color(p.text_muted))
                        .padding([1, 4])
                        .style(theme::vscode_status_item(p))
                        .on_press(Message::Detached(
                            win_id,
                            DetachedAction::AdjustFontSize(1.0),
                        )),
                    "Increase font size (Ctrl+=)",
                    tooltip::Position::Top,
                    p,
                ),
            ]
            .spacing(1)
            .align_y(Alignment::Center),
        );

    container(
        row![
            sidebar_toggle,
            theme::clipped_line(
                text(&v.status_note)
                    .size(type_scale::BADGE)
                    .color(note_color)
                    .wrapping(text::Wrapping::None),
                Length::Fill,
            ),
            right,
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .width(Length::Fill),
    )
    .padding([3, 10])
    .width(Length::Fill)
    .style(theme::vscode_status_bar(p))
    .into()
}
