//! `D1` — Hierarchical Cross-Repository Fleet Search & Replace (`Repo -> File -> Match` + Exact Line Jump).

use crate::app::{GitFleetApp, Message, PendingConfirm};
use crate::icons;
use crate::theme::kit::{ButtonSize, ButtonVariant, UiButton, status_pill};
use crate::theme::{self, Palette, icon_size, pad, space, type_scale};
use crate::views::slim_scroll_h;
use gitfleet_core::RepoId;
use gitfleet_search::SearchMatch;
use iced::widget::{Space, button, checkbox, column, container, row, text, text_input};
use iced::{Alignment, Element, Length};
use std::collections::BTreeMap;

impl GitFleetApp {
    pub(crate) fn view_search_drawer(&self, p: Palette) -> Element<'_, Message> {
        let top_row = row![
            button(if self.search_show_advanced {
                icons::chevron_down(icon_size::XS, p.accent_fg)
            } else {
                icons::chevron_right(icon_size::XS, p.text_secondary)
            })
            .padding([5, 6])
            .style(theme::vscode_icon_btn(p, self.search_show_advanced))
            .on_press(Message::ToggleFleetSearchAdvanced),
            icons::search(icon_size::SM, p.text_muted),
            text_input(
                "Search across ALL workspace repositories… (Enter)",
                &self.search_query
            )
            .on_input(Message::SearchQueryChanged)
            .on_submit(Message::RunFleetSearch)
            .padding(pad::INPUT_COMPACT)
            .size(type_scale::BODY)
            .width(Length::Fill)
            .style(theme::dark_input(p)),
            checkbox("Aa", self.search_case_sensitive)
                .on_toggle(Message::SearchCaseToggled)
                .size(14)
                .text_size(type_scale::CAPTION)
                .style(theme::tree_checkbox(p)),
            checkbox("Word", self.search_whole_word)
                .on_toggle(Message::SearchWholeWordToggled)
                .size(14)
                .text_size(type_scale::CAPTION)
                .style(theme::tree_checkbox(p)),
            checkbox(".*", self.search_regex)
                .on_toggle(Message::SearchRegexToggled)
                .size(14)
                .text_size(type_scale::CAPTION)
                .style(theme::tree_checkbox(p)),
            if self.search_running {
                UiButton::new(p, "Cancel")
                    .variant(ButtonVariant::Destructive)
                    .size(ButtonSize::Sm)
                    .on_press(Message::CancelFleetSearch)
                    .into_element()
            } else {
                UiButton::new(p, "Search Fleet")
                    .variant(ButtonVariant::Primary)
                    .size(ButtonSize::Sm)
                    .icon(icons::search(
                        icon_size::XS,
                        theme::on_accent_ink(p.accent_primary),
                    ))
                    .on_press(Message::RunFleetSearch)
                    .into_element()
            }
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);

        let mut toolbar_col = column![top_row].spacing(space::XS).width(Length::Fill);

        if self.search_show_advanced {
            let can_replace = !self.search_running
                && !self.search_query.trim().is_empty()
                && !self.search_results.is_empty();
            let adv_row = row![
                Space::with_width(Length::Fixed(24.0)),
                text_input("Replace across fleet…", &self.search_replace_text)
                    .on_input(Message::FleetReplaceTextChanged)
                    .padding(pad::INPUT_COMPACT)
                    .size(type_scale::CAPTION)
                    .width(Length::FillPortion(36))
                    .style(theme::dark_input(p)),
                text_input("Include globs (e.g. *.rs, src/**)", &self.search_include_glob)
                    .on_input(Message::FleetIncludeGlobChanged)
                    .on_submit(Message::RunFleetSearch)
                    .padding(pad::INPUT_COMPACT)
                    .size(type_scale::CAPTION)
                    .width(Length::FillPortion(32))
                    .style(theme::dark_input(p)),
                text_input("Exclude globs (e.g. *.lock, dist/**)", &self.search_exclude_glob)
                    .on_input(Message::FleetExcludeGlobChanged)
                    .on_submit(Message::RunFleetSearch)
                    .padding(pad::INPUT_COMPACT)
                    .size(type_scale::CAPTION)
                    .width(Length::FillPortion(32))
                    .style(theme::dark_input(p)),
                UiButton::new(p, format!("Replace All ({})", self.search_results.len()))
                    .variant(ButtonVariant::OutlineAccent)
                    .size(ButtonSize::Xs)
                    .on_press_maybe(can_replace.then(|| {
                        Message::RequestConfirm(PendingConfirm::fleet_replace_all(
                            &self.search_query,
                            &self.search_replace_text,
                            self.search_results.len(),
                        ))
                    })),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center);
            toolbar_col = toolbar_col.push(adv_row);
        }

        let toolbar = theme::dock_toolbar(p, toolbar_col);

        let body: Element<'_, Message> = if self.search_results.is_empty() && !self.search_running {
            theme::empty_state(
                p,
                icons::search(icon_size::LG, p.text_muted),
                "Search & Replace across your entire Fleet",
                "Press Enter to search all repositories, or expand '>' for Include/Exclude globs and Fleet Replace.",
                None,
            )
        } else {
            // Group hierarchically: Repo -> File -> Vec<&SearchMatch>
            let mut by_repo: BTreeMap<String, (RepoId, BTreeMap<String, Vec<&SearchMatch>>)> =
                BTreeMap::new();
            let mut max_line_chars: usize = 56;

            for m in &self.search_results {
                let entry = by_repo
                    .entry(m.repo_name.clone())
                    .or_insert_with(|| (m.repo_id.clone(), BTreeMap::new()));
                entry
                    .1
                    .entry(m.file_path.clone())
                    .or_default()
                    .push(m);
            }

            let mut tree_col = column![].spacing(3);
            for (repo_name, (repo_id, files_map)) in by_repo {
                let repo_collapsed = self.collapsed_search_repos.contains(&repo_id);
                let repo_match_cnt: usize = files_map.values().map(Vec::len).sum();
                let repo_id_toggle = repo_id.clone();

                tree_col = tree_col.push(
                    button(
                        row![
                            if repo_collapsed {
                                icons::chevron_right(12.0, p.text_muted)
                            } else {
                                icons::chevron_down(12.0, p.text_secondary)
                            },
                            icons::folder(icon_size::XS, p.accent_fg),
                            text(repo_name)
                                .size(type_scale::BODY)
                                .font(theme::UI_FONT_MEDIUM)
                                .color(p.text_primary),
                            status_pill(
                                format!("{repo_match_cnt} in {} file(s)", files_map.len()),
                                p.accent_fg,
                                p.fill_selected,
                            ),
                        ]
                        .spacing(space::XS)
                        .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .padding([4, 8])
                    .style(theme::repo_row_btn(p, false))
                    .on_press(Message::ToggleFleetSearchRepoNode(repo_id_toggle)),
                );

                if repo_collapsed {
                    continue;
                }

                for (file_path, file_matches) in files_map {
                    tree_col = tree_col.push(
                        container(
                            row![
                                Space::with_width(Length::Fixed(18.0)),
                                icons::file_code(12.0, p.text_muted),
                                text(file_path.clone())
                                    .size(type_scale::CAPTION)
                                    .font(theme::CODE_FONT)
                                    .color(p.warning_fg),
                                status_pill(
                                    file_matches.len().to_string(),
                                    p.text_muted,
                                    p.surface_card,
                                ),
                            ]
                            .spacing(space::XS)
                            .align_y(Alignment::Center),
                        )
                        .padding([2, 8]),
                    );

                    for m in file_matches {
                        let preview = theme::sanitize_code_line(&m.line_preview);
                        max_line_chars = max_line_chars.max(preview.chars().count() + 18);
                        tree_col = tree_col.push(
                            button(
                                row![
                                    Space::with_width(Length::Fixed(36.0)),
                                    text(format!(":{}", m.line_number))
                                        .size(type_scale::CAPTION)
                                        .font(theme::CODE_FONT)
                                        .color(p.accent_fg),
                                    text(preview)
                                        .size(type_scale::CODE)
                                        .font(theme::CODE_FONT)
                                        .color(p.text_secondary)
                                        .wrapping(text::Wrapping::None),
                                ]
                                .spacing(space::SM)
                                .align_y(Alignment::Center),
                            )
                            .width(Length::Fill)
                            .padding([3, 8])
                            .style(theme::repo_row_btn(p, false))
                            .on_press(Message::JumpToSearchMatch(
                                m.repo_id.clone(),
                                m.file_path.clone(),
                                m.line_number,
                            )),
                        );
                    }
                }
            }

            let min_w = (max_line_chars as f32 * 7.5 + 80.0).clamp(460.0, 2400.0);
            container(slim_scroll_h(tree_col, min_w, p))
                .padding(pad::DOCK_BODY)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };

        column![toolbar, body]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
