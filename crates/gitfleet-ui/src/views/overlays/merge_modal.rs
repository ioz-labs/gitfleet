use crate::app::{GitFleetApp, Message};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use crate::views::slim_scroll;
use gitfleet_git::MergeMode;
use iced::widget::{Space, button, column, container, row, text};
use iced::{Alignment, Element, Length};

impl GitFleetApp {
    pub(crate) fn view_merge_modal(&self, p: Palette) -> Element<'_, Message> {
        let mut branch_list = column![].spacing(space::XXS);
        for b in &self.branches {
            if b.is_head {
                continue;
            }
            let selected = self.merge_target_branch.as_deref() == Some(b.name.as_str());
            branch_list = branch_list.push(
                button(
                    row![
                        icons::branch(
                            icon_size::XS,
                            if b.is_remote {
                                p.violet_fg
                            } else {
                                p.accent_fg
                            }
                        ),
                        text(b.name.clone())
                            .size(type_scale::BODY)
                            .color(p.text_primary),
                        Space::with_width(Length::Fill),
                        if b.is_remote {
                            theme::badge(
                                "remote",
                                p.surface_card,
                                p.violet_fg,
                                p.border_subtle,
                                false,
                            )
                        } else {
                            Space::with_width(Length::Shrink).into()
                        }
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding(pad::ROW)
                .style(theme::repo_row_btn(p, selected))
                .on_press(Message::SelectMergeTarget(b.name.clone())),
            );
        }

        let forecast_box: Element<'_, Message> = if let Some(target) = &self.merge_target_branch {
            let merge_actions = |clean: bool| -> Element<'_, Message> {
                let primary_merge = button(text("Create Merge-Commit").size(type_scale::BODY))
                    .padding(pad::BTN_MD)
                    .style(move |t, s| {
                        if clean {
                            theme::primary_btn(p)(t, s)
                        } else {
                            theme::subtle_btn(p)(t, s)
                        }
                    })
                    .on_press(Message::ExecuteMerge(target.clone(), MergeMode::MergeCommit));

                let wt_merge = button(text("Merge to Working Tree").size(type_scale::BODY))
                    .padding(pad::BTN_MD)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::ExecuteMerge(target.clone(), MergeMode::WorkingTree));

                let squash_merge = button(text("Squash-Merge").size(type_scale::BODY))
                    .padding(pad::BTN_MD)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::ExecuteMerge(target.clone(), MergeMode::Squash));

                let secondary_row = row![
                    button(text("Fast-Forward Merge").size(type_scale::CAPTION))
                        .padding([4, 9])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::FastForwardMergeBranch(target.clone())),
                    button(text(format!("Rebase onto '{target}'")).size(type_scale::CAPTION))
                        .padding([4, 9])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::ExecuteRebase(target.clone())),
                ]
                .spacing(space::SM)
                .align_y(Alignment::Center);

                column![
                    text(
                        "Create Merge-Commit commits automatically if clean. Merge to Working Tree prepares a merge commit without committing. Squash-Merge stages all changes as a single normal commit."
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
                        format!("How to merge from '{target}'?"),
                        Some("In-memory merge-tree forecast found 0 conflicts."),
                        Some(theme::badge(
                            "Clean",
                            p.success_bg,
                            p.success_fg,
                            p.border_subtle,
                            false,
                        )),
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
                            "You can still merge and resolve conflicts interactively in the Conflict Solver.",
                        ),
                        Some(theme::badge(
                            format!("{} conflict(s)", fc.conflicting_paths.len()),
                            p.danger_bg,
                            p.danger_fg,
                            p.border_subtle,
                            false,
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

        let body = column![
            theme::section_card(
                p,
                "Target Branch",
                None,
                None,
                slim_scroll(branch_list, p).height(Length::Fixed(190.0)),
            ),
            forecast_box,
        ]
        .spacing(space::MD);

        theme::modal_shell(
            p,
            layout::MODAL_MD,
            icons::git_merge(icon_size::MD, p.accent_fg),
            "Merge / Rebase + Live Forecast",
            None,
            body,
            Message::CloseOverlays,
        )
    }
}
