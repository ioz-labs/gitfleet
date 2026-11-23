use super::fuzzy::{fuzzy_match, render_fuzzy_text};
use crate::app::{GitFleetApp, Message};
use crate::icons;
use crate::theme::kit::{ModalShell, ModalSize, focus_accent_bar, status_pill};
use crate::theme::{self, Palette, icon_size, pad, space, type_scale};
use crate::views::slim_scroll;
use gitfleet_git::MergeMode;
use iced::widget::{Space, button, column, container, row, text, text_input};
use iced::{Alignment, Element, Length};

impl GitFleetApp {
    pub(crate) fn view_merge_modal(&self, p: Palette) -> Element<'_, Message> {
        let q = self.merge_branch_filter.trim();
        let mut branch_list = column![].spacing(space::XXS);
        for b in &self.branches {
            if b.is_head {
                continue;
            }
            let matched = if q.is_empty() {
                Vec::new()
            } else if let Some((_, m)) = fuzzy_match(q, &b.name) {
                m
            } else {
                continue;
            };
            let selected = self.merge_target_branch.as_deref() == Some(b.name.as_str());
            branch_list = branch_list.push(
                button(
                    row![
                        focus_accent_bar(p, selected, true, 16.0),
                        icons::branch(
                            icon_size::XS,
                            if b.is_remote {
                                p.violet_fg
                            } else {
                                p.accent_fg
                            }
                        ),
                        theme::clipped_line(
                            render_fuzzy_text(&b.name, &matched, p.text_primary, p.accent_fg),
                            Length::Fill,
                        ),
                        if b.is_remote {
                            status_pill("remote", p.violet_fg, p.surface_card)
                        } else {
                            Space::with_width(Length::Shrink).into()
                        }
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding([5, 8])
                .style(theme::repo_row_btn(p, selected))
                .on_press(Message::SelectMergeTarget(b.name.clone())),
            );
        }

        let forecast_box: Element<'_, Message> = if let Some(target) = &self.merge_target_branch {
            let merge_actions = |clean: bool| -> Element<'_, Message> {
                let primary_merge = button(text("Merge Commit (--no-ff)").size(type_scale::CAPTION))
                    .padding(pad::BTN_MD)
                    .style(move |t, s| {
                        if clean {
                            theme::primary_btn(p)(t, s)
                        } else {
                            theme::subtle_btn(p)(t, s)
                        }
                    })
                    .on_press(Message::ExecuteMerge(target.clone(), MergeMode::MergeCommit));

                let wt_merge = button(text("Stage Only (--no-commit)").size(type_scale::CAPTION))
                    .padding(pad::BTN_MD)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::ExecuteMerge(target.clone(), MergeMode::WorkingTree));

                let squash_merge = button(text("Squash (--squash)").size(type_scale::CAPTION))
                    .padding(pad::BTN_MD)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::ExecuteMerge(target.clone(), MergeMode::Squash));

                let secondary_row = row![
                    button(text("Fast-Forward (--ff-only)").size(type_scale::CAPTION))
                        .padding([4, 10])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::FastForwardMergeBranch(target.clone())),
                    button(text(format!("Rebase onto '{target}'")).size(type_scale::CAPTION))
                        .padding([4, 10])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::ExecuteRebase(target.clone())),
                ]
                .spacing(space::SM)
                .align_y(Alignment::Center);

                column![
                    text(
                        "Merge Commit preserves branch topology (──●──). Squash condenses incoming commits into Working Tree. Rebase replays your local commits linearly atop target."
                    )
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
                    row![primary_merge, wt_merge, squash_merge]
                        .spacing(space::SM)
                        .align_y(Alignment::Center),
                    secondary_row,
                ]
                .spacing(space::SM)
                .into()
            };

            if let Some(fc) = &self.merge_forecast {
                if fc.clean {
                    theme::section_card(
                        p,
                        format!("Forecast merging '{target}'"),
                        Some("In-memory git merge-tree simulation detected 0 conflicts."),
                        Some(status_pill("0 Conflicts", p.success_fg, p.success_bg)),
                        merge_actions(true),
                    )
                } else {
                    let mut c_col = column![].spacing(space::XXS);
                    for path in fc.conflicting_paths.iter().take(8) {
                        c_col = c_col.push(
                            row![
                                icons::alert_triangle(icon_size::XS, p.warning_fg),
                                text(path.clone())
                                    .size(type_scale::CODE)
                                    .font(theme::CODE_FONT)
                                    .color(p.warning_fg),
                            ]
                            .spacing(space::XS)
                            .align_y(Alignment::Center),
                        );
                    }
                    if fc.conflicting_paths.len() > 8 {
                        c_col = c_col.push(
                            text(format!(
                                "… and {} more conflicting file(s)",
                                fc.conflicting_paths.len() - 8
                            ))
                            .size(type_scale::CAPTION)
                            .color(p.text_muted),
                        );
                    }
                    c_col = c_col
                        .push(Space::with_height(Length::Fixed(4.0)))
                        .push(merge_actions(false));
                    theme::section_card(
                        p,
                        format!("Predicted conflicts merging '{target}'"),
                        Some(
                            "You can still merge and resolve conflicts interactively in the 3-Way Conflict Solver.",
                        ),
                        Some(status_pill(
                            format!("{} conflict(s)", fc.conflicting_paths.len()),
                            p.danger_fg,
                            p.danger_bg,
                        )),
                        c_col,
                    )
                }
            } else {
                container(
                    text(format!(
                        "Running in-memory merge-tree forecast for '{target}'…"
                    ))
                    .size(type_scale::BODY)
                    .color(p.text_muted),
                )
                .padding(pad::CARD)
                .width(Length::Fill)
                .style(theme::section_panel_box(p))
                .into()
            }
        } else {
            container(
                text("Select a target branch above to preview a zero-touch Merge / Rebase forecast.")
                    .size(type_scale::BODY)
                    .color(p.text_muted),
            )
            .padding(pad::CARD)
            .width(Length::Fill)
            .style(theme::section_panel_box(p))
            .into()
        };

        let target_card = theme::section_card(
            p,
            "Target Branch",
            None,
            None,
            column![
                text_input("Filter local or remote branches…", &self.merge_branch_filter)
                    .on_input(Message::MergeBranchFilterChanged)
                    .padding(pad::INPUT_COMPACT)
                    .size(type_scale::BODY)
                    .width(Length::Fill)
                    .style(theme::dark_input(p)),
                slim_scroll(branch_list, p).height(Length::Fixed(185.0)),
            ]
            .spacing(space::SM),
        );

        let body = column![target_card, forecast_box].spacing(space::MD);

        ModalShell::new(p, "Merge / Rebase + Live Forecast", body)
            .subtitle("Simulate merges in memory before touching your working tree.")
            .icon(icons::git_merge(icon_size::MD, p.accent_fg))
            .size(ModalSize::Md)
            .on_close(Message::CloseOverlays)
            .into_element()
    }
}
