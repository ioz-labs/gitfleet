//! VS Code-style Search & Replace sidebar with include/exclude filters, regex/case/word toggles,
//! grouped match snippets, and per-match / per-file / replace-all actions.

use super::sidebar_explorer::lang_tag;
use super::state::{DetachedAction, DetachedViewerState};
use crate::app::Message;
use crate::icons;
use crate::syntax::CodeLanguage;
use crate::theme::{self, Palette, ThemeMode, type_scale};
use crate::views::{parent_dir, slim_scroll, tip, truncate_chars};
use iced::widget::{
    Space, button, column, container, horizontal_rule, row, text, text_input, tooltip,
};
use iced::{Alignment, Color, Element, Length, window};

// ── Sidebar 2: VS Code-style Search & Replace ────────────────────────────────

pub(crate) fn render_search_sidebar<'a>(
    win_id: window::Id,
    v: &'a DetachedViewerState,
    theme_mode: ThemeMode,
    p: Palette,
) -> Element<'a, Message> {
    let root_label = v
        .search_root_name
        .as_deref()
        .unwrap_or("No folder selected");
    let root_tooltip = v
        .search_root
        .as_deref()
        .map(|r| r.display().to_string())
        .unwrap_or_else(|| "Select a folder to search".into());

    let scope_header = row![
        text("Search")
            .size(type_scale::CAPTION)
            .color(p.text_primary)
            .wrapping(text::Wrapping::None),
        Space::with_width(Length::Fill),
        tip(
            button(
                row![
                    icons::folder(11.0, p.accent_fg),
                    text(truncate_chars(root_label, 18))
                        .size(type_scale::BADGE)
                        .color(p.text_secondary)
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            )
            .padding([2, 6])
            .style(theme::subtle_btn(p))
            .on_press(Message::Detached(win_id, DetachedAction::PickSearchRoot)),
            format!("Scope: {root_tooltip} (click to change)"),
            tooltip::Position::Bottom,
            p,
        ),
    ]
    .spacing(5)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let replace_toggle_icon: Element<'a, Message> = if v.search_show_replace {
        icons::chevron_down(12.0, p.text_secondary).into()
    } else {
        icons::chevron_right(12.0, p.text_secondary).into()
    };

    let search_row = row![
        tip(
            button(replace_toggle_icon)
                .padding([5, 4])
                .style(theme::vscode_icon_btn(p, false))
                .on_press(Message::Detached(
                    win_id,
                    DetachedAction::ToggleSearchReplaceRow,
                )),
            "Toggle Replace",
            tooltip::Position::Right,
            p,
        ),
        text_input("Search files & content…", &v.search_query)
            .id(v.search_input_id.clone())
            .on_input(move |s| Message::Detached(win_id, DetachedAction::SearchQueryChanged(s)))
            .on_submit(Message::Detached(win_id, DetachedAction::RunSearch))
            .padding([5, 8])
            .size(type_scale::CAPTION)
            .width(Length::Fill)
            .style(theme::dark_input(p)),
        tip(
            button(icons::search(12.0, Color::WHITE))
                .padding([5, 8])
                .style(theme::primary_btn(p))
                .on_press(Message::Detached(win_id, DetachedAction::RunSearch)),
            "Run Search (Enter)",
            tooltip::Position::Bottom,
            p,
        ),
    ]
    .spacing(4)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let opt_pill = |label: &'static str,
                    active: bool,
                    hint: &'static str,
                    act: DetachedAction|
     -> Element<'a, Message> {
        tip(
            button(
                text(label)
                    .size(type_scale::BADGE)
                    .font(theme::CODE_FONT)
                    .color(if active { p.accent_fg } else { p.text_muted })
                    .wrapping(text::Wrapping::None),
            )
            .padding([2, 6])
            .style(theme::filter_pill_btn(p, active))
            .on_press(Message::Detached(win_id, act)),
            hint,
            tooltip::Position::Bottom,
            p,
        )
    };

    let options_row = row![
        Space::with_width(Length::Fixed(22.0)),
        opt_pill(
            "Aa",
            v.search_options.case_sensitive,
            "Match Case",
            DetachedAction::ToggleSearchCase,
        ),
        opt_pill(
            "ab",
            v.search_options.whole_word,
            "Match Whole Word",
            DetachedAction::ToggleSearchWholeWord,
        ),
        opt_pill(
            ".*",
            v.search_options.regex,
            "Use Regular Expression",
            DetachedAction::ToggleSearchRegex,
        ),
        opt_pill(
            ".git",
            v.search_use_gitignore,
            "Respect .gitignore rules",
            DetachedAction::ToggleSearchGitignore,
        ),
        Space::with_width(Length::Fill),
        tip(
            button(icons::more_horizontal(
                12.0,
                if v.search_show_filters {
                    p.accent_fg
                } else {
                    p.text_muted
                },
            ))
            .padding([2, 6])
            .style(theme::filter_pill_btn(p, v.search_show_filters))
            .on_press(Message::Detached(
                win_id,
                DetachedAction::ToggleSearchFilters,
            )),
            "Toggle files to include / exclude",
            tooltip::Position::Bottom,
            p,
        ),
    ]
    .spacing(3)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let mut form_col = column![scope_header, search_row, options_row]
        .spacing(5)
        .width(Length::Fill);

    if v.search_show_replace {
        let can_replace_all =
            !v.search_query.is_empty() && !v.search_result.groups.is_empty() && !v.search_running;

        let all_btn = {
            let b = button(
                text("All")
                    .size(type_scale::BADGE)
                    .wrapping(text::Wrapping::None),
            )
            .padding([5, 8]);
            if can_replace_all {
                b.style(theme::subtle_btn(p)).on_press(Message::Detached(
                    win_id,
                    DetachedAction::RequestReplaceAllInSearch,
                ))
            } else {
                b.style(theme::vscode_icon_btn(p, false))
            }
        };

        form_col = form_col.push(
            row![
                Space::with_width(Length::Fixed(22.0)),
                text_input("Replace…", &v.search_replace_text)
                    .on_input(move |s| {
                        Message::Detached(win_id, DetachedAction::SearchReplaceChanged(s))
                    })
                    .on_submit(Message::Detached(
                        win_id,
                        DetachedAction::RequestReplaceAllInSearch,
                    ))
                    .padding([5, 8])
                    .size(type_scale::CAPTION)
                    .width(Length::Fill)
                    .style(theme::dark_input(p)),
                tip(
                    all_btn,
                    "Replace All across matched files",
                    tooltip::Position::Bottom,
                    p,
                ),
            ]
            .spacing(4)
            .align_y(Alignment::Center)
            .width(Length::Fill),
        );

        if v.search_confirm_replace_all {
            let file_cnt = v.search_result.groups.len();
            let match_cnt = v.search_result.total_matches;
            form_col = form_col.push(
                container(
                    row![
                        theme::clipped_line(
                            text(format!("Replace {match_cnt} in {file_cnt} file(s)?"))
                                .size(type_scale::BADGE)
                                .color(p.warning_fg)
                                .wrapping(text::Wrapping::None),
                            Length::Fill,
                        ),
                        button(
                            text("Replace")
                                .size(type_scale::BADGE)
                                .wrapping(text::Wrapping::None),
                        )
                        .padding([2, 8])
                        .style(theme::danger_btn(p))
                        .on_press(Message::Detached(
                            win_id,
                            DetachedAction::ConfirmReplaceAllInSearch,
                        )),
                        button(
                            text("Cancel")
                                .size(type_scale::BADGE)
                                .wrapping(text::Wrapping::None),
                        )
                        .padding([2, 6])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::Detached(
                            win_id,
                            DetachedAction::CancelReplaceAllInSearch,
                        )),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                )
                .padding([4, 8])
                .width(Length::Fill)
                .style(theme::pill_box(p.warning_bg, p.warning_fg, p.border_default)),
            );
        }
    }

    if v.search_show_filters {
        form_col = form_col.push(
            container(
                column![
                    text("Files to include")
                        .size(type_scale::BADGE)
                        .color(p.text_muted),
                    text_input("e.g. src/**, *.rs, crates/ui", &v.search_include)
                        .on_input(move |s| {
                            Message::Detached(win_id, DetachedAction::SearchIncludeChanged(s))
                        })
                        .on_submit(Message::Detached(win_id, DetachedAction::RunSearch))
                        .padding([4, 7])
                        .size(type_scale::CAPTION)
                        .width(Length::Fill)
                        .style(theme::dark_input(p)),
                    text("Files to exclude")
                        .size(type_scale::BADGE)
                        .color(p.text_muted),
                    text_input("e.g. target/**, dist, *.lock", &v.search_exclude)
                        .on_input(move |s| {
                            Message::Detached(win_id, DetachedAction::SearchExcludeChanged(s))
                        })
                        .on_submit(Message::Detached(win_id, DetachedAction::RunSearch))
                        .padding([4, 7])
                        .size(type_scale::CAPTION)
                        .width(Length::Fill)
                        .style(theme::dark_input(p)),
                ]
                .spacing(3)
                .width(Length::Fill),
            )
            .padding([5, 6])
            .width(Length::Fill)
            .style(theme::section_panel_box(p)),
        );
    }

    let summary_text = if v.search_running {
        "Searching…".to_string()
    } else if let Some(err) = &v.search_result.error {
        err.clone()
    } else if !v.search_has_run {
        "Press Enter to search".to_string()
    } else if v.search_query.is_empty() {
        format!("{} file(s)", v.search_result.matched_files.len())
    } else {
        let trunc = if v.search_result.truncated { "+" } else { "" };
        format!(
            "{}{trunc} result(s) in {} file(s)",
            v.search_result.total_matches,
            v.search_result.groups.len()
        )
    };

    let mut summary_row = row![
        theme::clipped_line(
            text(summary_text)
                .size(type_scale::BADGE)
                .color(if v.search_result.error.is_some() {
                    p.danger_fg
                } else if v.search_running {
                    p.accent_fg
                } else {
                    p.text_muted
                })
                .wrapping(text::Wrapping::None),
            Length::Fill,
        ),
    ]
    .spacing(4)
    .align_y(Alignment::Center)
    .width(Length::Fill);

    if v.search_result.groups.len() > 1 {
        let all_collapsed = v
            .search_result
            .groups
            .iter()
            .all(|g| v.search_collapsed_groups.contains(&g.file_path));
        summary_row = summary_row.push(
            button(
                text(if all_collapsed { "Expand" } else { "Collapse" })
                    .size(type_scale::BADGE)
                    .color(p.text_muted)
                    .wrapping(text::Wrapping::None),
            )
            .padding([1, 5])
            .style(theme::vscode_icon_btn(p, false))
            .on_press(Message::Detached(
                win_id,
                DetachedAction::CollapseExpandAllSearchGroups,
            )),
        );
    }

    let active_file_path = v.active_tab().and_then(|t| t.file_path.as_ref());
    let mut results_col = column![].spacing(2).width(Length::Fill);

    if !v.search_result.matched_files.is_empty() {
        let files_chev: Element<'a, Message> = if v.search_files_collapsed {
            icons::chevron_right(11.0, p.text_muted).into()
        } else {
            icons::chevron_down(11.0, p.text_muted).into()
        };
        results_col = results_col.push(
            button(
                row![
                    files_chev,
                    text(format!("Files ({})", v.search_result.matched_files.len()))
                        .size(type_scale::BADGE)
                        .color(p.text_muted)
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(4)
                .align_y(Alignment::Center)
                .width(Length::Fill),
            )
            .padding([3, 4])
            .width(Length::Fill)
            .style(theme::vscode_icon_btn(p, false))
            .on_press(Message::Detached(
                win_id,
                DetachedAction::ToggleSearchFilesSection,
            )),
        );

        if !v.search_files_collapsed {
            let max_show = if v.search_query.is_empty() { 120 } else { 30 };
            for (rel, abs) in v.search_result.matched_files.iter().take(max_show) {
                let fname = rel.rsplit('/').next().unwrap_or(rel);
                let dir = parent_dir(rel);
                let is_open = active_file_path == Some(abs);
                let lang = CodeLanguage::from_path(abs);

                let mut file_row = row![
                    container(lang_tag(lang, theme_mode)).width(Length::Fixed(20.0)),
                    text(fname)
                        .size(type_scale::CAPTION)
                        .color(if is_open { p.accent_fg } else { p.text_primary })
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(4)
                .align_y(Alignment::Center);

                if !dir.is_empty() {
                    file_row = file_row.push(
                        text(dir)
                            .size(type_scale::BADGE)
                            .color(p.text_muted)
                            .wrapping(text::Wrapping::None),
                    );
                }

                results_col = results_col.push(
                    button(theme::clipped_line(file_row, Length::Fill))
                        .padding([3, 8])
                        .width(Length::Fill)
                        .style(theme::repo_row_btn(p, is_open))
                        .on_press(Message::Detached(
                            win_id,
                            DetachedAction::OpenSearchFile(abs.clone()),
                        )),
                );
            }
        }
    }

    if !v.search_result.groups.is_empty() {
        if !v.search_result.matched_files.is_empty() {
            results_col = results_col.push(
                container(horizontal_rule(1).style(theme::subtle_rule(p))).padding([4, 0]),
            );
        }

        for group in &v.search_result.groups {
            let collapsed = v.search_collapsed_groups.contains(&group.file_path);
            let fname = group
                .file_path
                .rsplit('/')
                .next()
                .unwrap_or(&group.file_path);
            let dir = parent_dir(&group.file_path);
            let is_open = active_file_path == Some(&group.abs_path);
            let lang = CodeLanguage::from_path(&group.abs_path);

            let chev: Element<'a, Message> = if collapsed {
                icons::chevron_right(11.0, p.text_muted).into()
            } else {
                icons::chevron_down(11.0, p.text_muted).into()
            };

            let mut label_row = row![
                container(lang_tag(lang, theme_mode)).width(Length::Fixed(20.0)),
                text(fname)
                    .size(type_scale::CAPTION)
                    .color(if is_open { p.accent_fg } else { p.text_primary })
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(3)
            .align_y(Alignment::Center);

            if !dir.is_empty() {
                label_row = label_row.push(
                    text(dir)
                        .size(type_scale::BADGE)
                        .color(p.text_muted)
                        .wrapping(text::Wrapping::None),
                );
            }

            let mut hdr_row = row![
                button(chev)
                    .padding([3, 3])
                    .style(theme::vscode_icon_btn(p, false))
                    .on_press(Message::Detached(
                        win_id,
                        DetachedAction::ToggleSearchGroup(group.file_path.clone()),
                    )),
                button(theme::clipped_line(label_row, Length::Fill))
                    .padding([3, 4])
                    .width(Length::Fill)
                    .style(theme::repo_row_btn(p, is_open))
                    .on_press(Message::Detached(
                        win_id,
                        DetachedAction::OpenSearchFile(group.abs_path.clone()),
                    )),
                theme::badge(
                    format!("{}", group.matches.len()),
                    p.surface_card,
                    p.text_muted,
                    p.border_subtle,
                    false,
                ),
            ]
            .spacing(2)
            .align_y(Alignment::Center)
            .width(Length::Fill);

            if v.search_show_replace && !v.search_query.is_empty() {
                hdr_row = hdr_row.push(tip(
                    button(icons::swap_arrows(11.0, p.accent_fg))
                        .padding([2, 4])
                        .style(theme::vscode_icon_btn(p, false))
                        .on_press(Message::Detached(
                            win_id,
                            DetachedAction::ReplaceInSearchFile(group.abs_path.clone()),
                        )),
                    format!("Replace {} match(es) in {fname}", group.matches.len()),
                    tooltip::Position::Left,
                    p,
                ));
            }

            results_col = results_col.push(hdr_row);

            if !collapsed {
                for m in &group.matches {
                    let snippet =
                        render_highlighted_snippet(&m.line_text, m.col_start, m.col_end, p);
                    let mut match_row = row![
                        button(
                            row![
                                text(format!("{:>4}", m.line_number))
                                    .size(type_scale::BADGE)
                                    .font(theme::CODE_FONT)
                                    .color(p.text_muted)
                                    .wrapping(text::Wrapping::None),
                                snippet,
                            ]
                            .spacing(6)
                            .align_y(Alignment::Center)
                            .width(Length::Fill),
                        )
                        .padding([2, 6])
                        .width(Length::Fill)
                        .style(theme::vscode_icon_btn(p, false))
                        .on_press(Message::Detached(
                            win_id,
                            DetachedAction::OpenSearchMatch {
                                path: group.abs_path.clone(),
                                line_number: m.line_number,
                                col_start: m.col_start,
                                col_end: m.col_end,
                            },
                        )),
                    ]
                    .spacing(2)
                    .align_y(Alignment::Center)
                    .width(Length::Fill);

                    if v.search_show_replace && !v.search_query.is_empty() {
                        match_row = match_row.push(tip(
                            button(
                                text("→")
                                    .size(type_scale::BADGE)
                                    .font(theme::CODE_FONT)
                                    .color(p.text_muted),
                            )
                            .padding([1, 4])
                            .style(theme::vscode_icon_btn(p, false))
                            .on_press(Message::Detached(
                                win_id,
                                DetachedAction::ReplaceSingleSearchMatch {
                                    path: group.abs_path.clone(),
                                    line_number: m.line_number,
                                },
                            )),
                            format!("Replace on line {}", m.line_number),
                            tooltip::Position::Left,
                            p,
                        ));
                    }

                    results_col = results_col.push(container(match_row).padding(iced::Padding {
                        top: 0.0,
                        right: 0.0,
                        bottom: 0.0,
                        left: 10.0,
                    }));
                }
            }
        }
    } else if v.search_has_run && !v.search_running && v.search_result.matched_files.is_empty() {
        results_col = results_col.push(
            container(
                text("No matching files or content.")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
            )
            .padding([12, 8]),
        );
    }

    container(
        column![
            form_col,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            summary_row,
            slim_scroll(results_col, p).height(Length::Fill),
        ]
        .spacing(6)
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .padding(8)
    .width(Length::Fixed(316.0))
    .height(Length::Fill)
    .style(theme::sidebar_box(p))
    .into()
}

fn render_highlighted_snippet<'a>(
    line: &str,
    col_start: usize,
    col_end: usize,
    p: Palette,
) -> Element<'a, Message> {
    let chars: Vec<char> = line.chars().collect();
    let leading_ws = chars.iter().take_while(|c| c.is_whitespace()).count();
    let trimmed = &chars[leading_ws..];

    let rel_start = col_start.saturating_sub(leading_ws).min(trimmed.len());
    let rel_end = col_end
        .saturating_sub(leading_ws)
        .clamp(rel_start, trimmed.len());

    let (window_start, prefix_ellipsis) = if rel_start > 18 {
        (rel_start.saturating_sub(10), true)
    } else {
        (0, false)
    };
    let window_end = (window_start + 72).min(trimmed.len()).max(rel_end);

    let before: String = trimmed[window_start..rel_start].iter().collect();
    let matched: String = trimmed[rel_start..rel_end].iter().collect();
    let after: String = trimmed[rel_end..window_end].iter().collect();

    let mut r = row![].spacing(0).align_y(Alignment::Center);
    if prefix_ellipsis {
        r = r.push(
            text("…")
                .size(type_scale::CAPTION)
                .font(theme::CODE_FONT)
                .color(p.text_muted)
                .wrapping(text::Wrapping::None),
        );
    }
    if !before.is_empty() {
        r = r.push(
            text(before)
                .size(type_scale::CAPTION)
                .font(theme::CODE_FONT)
                .color(p.text_secondary)
                .wrapping(text::Wrapping::None),
        );
    }
    if !matched.is_empty() {
        r = r.push(
            container(
                text(matched)
                    .size(type_scale::CAPTION)
                    .font(theme::CODE_FONT)
                    .color(p.text_primary)
                    .wrapping(text::Wrapping::None),
            )
            .padding([0, 2])
            .style(theme::pill_box(
                Color {
                    a: 0.28,
                    ..p.warning_fg
                },
                p.text_primary,
                Color::TRANSPARENT,
            )),
        );
    }
    if !after.is_empty() {
        r = r.push(
            text(after)
                .size(type_scale::CAPTION)
                .font(theme::CODE_FONT)
                .color(p.text_secondary)
                .wrapping(text::Wrapping::None),
        );
    }

    theme::clipped_line(r, Length::Fill)
}
