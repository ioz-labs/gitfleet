use crate::app::{GitFleetApp, Message, PendingConfirm};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use crate::views::{slim_scroll, tip};
use gitfleet_git::GitFlowKind;
use iced::widget::{Space, button, column, container, row, text, text_input, tooltip};
use iced::{Alignment, Color, Element, Length};

fn flow_kind_colors(kind: GitFlowKind, p: Palette) -> (Color, Color) {
    match kind {
        GitFlowKind::Feature => (p.accent_fg, p.fill_selected),
        GitFlowKind::Release => (p.success_fg, p.success_bg),
        GitFlowKind::Hotfix => (p.danger_fg, p.danger_bg),
        GitFlowKind::Bugfix => (p.warning_fg, p.warning_bg),
    }
}

impl GitFleetApp {
    pub(crate) fn view_git_flow_modal(&self, p: Palette) -> Element<'_, Message> {
        let is_init = self.git_flow_state.initialized;
        let show_config_form = !is_init || self.git_flow_editing_config;

        let main_b = self.git_flow_state.main_or_default();
        let dev_b = self.git_flow_state.develop_or_default();

        let header_badge: Option<Element<'_, Message>> = if is_init {
            Some(
                theme::badge(
                    format!("{main_b}  /  {dev_b}"),
                    p.fill_selected,
                    p.accent_fg,
                    p.border_subtle,
                    true,
                ),
            )
        } else {
            None
        };

        let body: Element<'_, Message> = if show_config_form {
            let can_save = !self.git_flow_main_input.trim().is_empty()
                && !self.git_flow_develop_input.trim().is_empty();

            let branches_row = row![
                column![
                    theme::field_label(p, "Main / Production branch"),
                    text_input("main", &self.git_flow_main_input)
                        .on_input(Message::GitFlowMainChanged)
                        .padding(pad::INPUT)
                        .size(type_scale::BODY)
                        .width(Length::Fill)
                        .style(theme::dark_input(p)),
                ]
                .spacing(space::XXS)
                .width(Length::FillPortion(1)),
                column![
                    theme::field_label(p, "Develop / Integration branch"),
                    text_input("develop", &self.git_flow_develop_input)
                        .on_input(Message::GitFlowDevelopChanged)
                        .padding(pad::INPUT)
                        .size(type_scale::BODY)
                        .width(Length::Fill)
                        .style(theme::dark_input(p)),
                ]
                .spacing(space::XXS)
                .width(Length::FillPortion(1)),
            ]
            .spacing(space::SM)
            .width(Length::Fill);

            let prefix_row_1 = row![
                column![
                    theme::field_label(p, "Feature prefix"),
                    text_input("feature/", &self.git_flow_feature_prefix)
                        .on_input(|v| Message::GitFlowPrefixChanged(GitFlowKind::Feature, v))
                        .padding(pad::INPUT_COMPACT)
                        .size(type_scale::CAPTION)
                        .width(Length::Fill)
                        .style(theme::dark_input(p)),
                ]
                .spacing(space::XXS)
                .width(Length::FillPortion(1)),
                column![
                    theme::field_label(p, "Release prefix"),
                    text_input("release/", &self.git_flow_release_prefix)
                        .on_input(|v| Message::GitFlowPrefixChanged(GitFlowKind::Release, v))
                        .padding(pad::INPUT_COMPACT)
                        .size(type_scale::CAPTION)
                        .width(Length::Fill)
                        .style(theme::dark_input(p)),
                ]
                .spacing(space::XXS)
                .width(Length::FillPortion(1)),
            ]
            .spacing(space::SM)
            .width(Length::Fill);

            let prefix_row_2 = row![
                column![
                    theme::field_label(p, "Hotfix prefix"),
                    text_input("hotfix/", &self.git_flow_hotfix_prefix)
                        .on_input(|v| Message::GitFlowPrefixChanged(GitFlowKind::Hotfix, v))
                        .padding(pad::INPUT_COMPACT)
                        .size(type_scale::CAPTION)
                        .width(Length::Fill)
                        .style(theme::dark_input(p)),
                ]
                .spacing(space::XXS)
                .width(Length::FillPortion(1)),
                column![
                    theme::field_label(p, "Bugfix prefix"),
                    text_input("bugfix/", &self.git_flow_bugfix_prefix)
                        .on_input(|v| Message::GitFlowPrefixChanged(GitFlowKind::Bugfix, v))
                        .padding(pad::INPUT_COMPACT)
                        .size(type_scale::CAPTION)
                        .width(Length::Fill)
                        .style(theme::dark_input(p)),
                ]
                .spacing(space::XXS)
                .width(Length::FillPortion(1)),
            ]
            .spacing(space::SM)
            .width(Length::Fill);

            let mut actions_row = row![].spacing(space::SM).align_y(Alignment::Center);
            if is_init {
                actions_row = actions_row.push(
                    button(
                        row![
                            icons::trash(icon_size::XS, p.danger_fg),
                            text("Disable Git Flow…")
                                .size(type_scale::BODY)
                                .color(p.danger_fg)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(5)
                        .align_y(Alignment::Center),
                    )
                    .padding(pad::BTN_MD)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::RequestConfirm(PendingConfirm::disable_git_flow())),
                );
            }
            actions_row = actions_row.push(Space::with_width(Length::Fill));
            if is_init {
                actions_row = actions_row.push(
                    button(
                        text("Cancel")
                            .size(type_scale::BODY)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding(pad::BTN_MD)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::ToggleGitFlowEditConfig),
                );
            }
            actions_row = actions_row.push(
                button(
                    text(if is_init {
                        "Save Configuration"
                    } else {
                        "Initialize Git Flow"
                    })
                    .size(type_scale::BODY)
                    .wrapping(text::Wrapping::None),
                )
                .padding(pad::BTN_MD)
                .style(theme::primary_btn(p))
                .on_press_maybe(can_save.then_some(Message::InitGitFlow)),
            );

            theme::section_card(
                p,
                if is_init {
                    "Edit Git Flow Configuration"
                } else {
                    "Initialize Git Flow"
                },
                Some(if is_init {
                    "Update main/develop branches and prefixes, or disable Git Flow for this repository (existing branches are never deleted)."
                } else {
                    "Configure your main and develop branches for Feature / Release / Hotfix / Bugfix workflows."
                }),
                None,
                column![branches_row, prefix_row_1, prefix_row_2, actions_row].spacing(space::MD),
            )
        } else {
            let kinds = [
                ("Feature", GitFlowKind::Feature),
                ("Release", GitFlowKind::Release),
                ("Hotfix", GitFlowKind::Hotfix),
                ("Bugfix", GitFlowKind::Bugfix),
            ];
            let mut kind_row = row![].spacing(space::XS).align_y(Alignment::Center);
            for (lbl, k) in kinds {
                let active = self.git_flow_kind == k;
                kind_row = kind_row.push(
                    button(
                        text(lbl)
                            .size(type_scale::BODY)
                            .wrapping(text::Wrapping::None),
                    )
                    .padding(pad::BTN_SM)
                    .style(theme::filter_pill_btn(p, active))
                    .on_press(Message::GitFlowKindChanged(k)),
                );
            }

            let cur_prefix = self.git_flow_state.prefix_for(self.git_flow_kind);
            let cur_base = self.git_flow_state.base_branch_for(self.git_flow_kind);
            let cur_target = self.git_flow_state.target_branch_for(self.git_flow_kind);
            let trimmed_name = self.git_flow_name_input.trim();
            let preview_branch = if trimmed_name.is_empty() {
                format!("{cur_prefix}<name>")
            } else if trimmed_name.starts_with(cur_prefix) {
                trimmed_name.to_string()
            } else {
                format!("{cur_prefix}{trimmed_name}")
            };
            let has_name = !trimmed_name.is_empty();

            let config_controls = row![
                tip(
                    button(
                        row![
                            icons::pencil(icon_size::XS, p.text_secondary),
                            text("Edit Config")
                                .size(type_scale::CAPTION)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                    )
                    .padding(pad::BTN_SM)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::ToggleGitFlowEditConfig),
                    "Edit main/develop branches and prefixes",
                    tooltip::Position::Top,
                    p,
                ),
                tip(
                    button(icons::trash(icon_size::XS, p.danger_fg))
                        .padding([5, 7])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::RequestConfirm(PendingConfirm::disable_git_flow())),
                    "Disable / remove Git Flow configuration from this repo",
                    tooltip::Position::Top,
                    p,
                ),
            ]
            .spacing(space::XS)
            .align_y(Alignment::Center);

            let workflow_card = theme::section_card(
                p,
                "Branch Workflow",
                Some("Start a new Git Flow branch or finish & merge an existing one."),
                Some(config_controls.into()),
                column![
                    kind_row,
                    column![
                        theme::field_label(p, "Branch or release name"),
                        text_input(
                            "Name (e.g. login-oauth or 1.2.0)…",
                            &self.git_flow_name_input
                        )
                        .on_input(Message::GitFlowNameChanged)
                        .on_submit(Message::StartGitFlow)
                        .padding(pad::INPUT)
                        .size(type_scale::BODY)
                        .width(Length::Fill)
                        .style(theme::dark_input(p)),
                        text(format!(
                            "Branch: {preview_branch}  ·  base: {cur_base}  →  finish into: {cur_target}"
                        ))
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(p.text_muted),
                    ]
                    .spacing(space::XXS),
                    row![
                        Space::with_width(Length::Fill),
                        button(
                            text("Finish & Merge")
                                .size(type_scale::BODY)
                                .wrapping(text::Wrapping::None),
                        )
                        .padding(pad::BTN_MD)
                        .style(theme::subtle_btn(p))
                        .on_press_maybe(has_name.then_some(Message::FinishGitFlow)),
                        button(
                            text("Start Branch")
                                .size(type_scale::BODY)
                                .wrapping(text::Wrapping::None),
                        )
                        .padding(pad::BTN_MD)
                        .style(theme::primary_btn(p))
                        .on_press_maybe(has_name.then_some(Message::StartGitFlow)),
                    ]
                    .spacing(space::SM),
                ]
                .spacing(space::MD),
            );

            // Active local Git Flow branches with quick Finish / Rename / Cancel actions
            let active_flow_branches: Vec<(GitFlowKind, &gitfleet_git::BranchInfo)> = self
                .branches
                .iter()
                .filter(|b| !b.is_remote)
                .filter_map(|b| {
                    self.git_flow_state
                        .classify_branch(&b.name)
                        .map(|(kind, _)| (kind, b))
                })
                .collect();

            let active_count = active_flow_branches.len();
            let active_body: Element<'_, Message> = if active_flow_branches.is_empty() {
                text("No active feature / release / hotfix / bugfix branches in this repository.")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted)
                    .into()
            } else {
                let mut rows_col = column![].spacing(space::XS);
                for (kind, b) in active_flow_branches {
                    let (fg, bg) = flow_kind_colors(kind, p);
                    let target_b = self.git_flow_state.target_branch_for(kind).to_string();
                    let base_b = self.git_flow_state.base_branch_for(kind).to_string();
                    let mut left_info = row![
                        theme::badge(kind.label(), bg, fg, Color { a: 0.45, ..fg }, false),
                        text(b.name.clone())
                            .size(type_scale::BODY)
                            .color(p.text_primary),
                    ]
                    .spacing(space::XS)
                    .align_y(Alignment::Center);
                    if b.is_head {
                        left_info = left_info.push(theme::badge(
                            "HEAD",
                            p.fill_selected,
                            p.accent_fg,
                            p.accent_primary,
                            true,
                        ));
                    }

                    let item_row = container(
                        row![
                            theme::clipped_line(left_info, Length::Fill),
                            tip(
                                button(
                                    row![
                                        icons::git_merge(icon_size::XS, p.success_fg),
                                        text("Finish")
                                            .size(type_scale::CAPTION)
                                            .color(p.success_fg)
                                            .wrapping(text::Wrapping::None),
                                    ]
                                    .spacing(4)
                                    .align_y(Alignment::Center),
                                )
                                .padding([4, 8])
                                .style(theme::subtle_btn(p))
                                .on_press(Message::FinishGitFlowBranch(kind, b.name.clone())),
                                format!("Merge '{}' into '{target_b}' and delete it", b.name),
                                tooltip::Position::Top,
                                p,
                            ),
                            tip(
                                button(icons::pencil(icon_size::XS, p.text_secondary))
                                    .padding([5, 6])
                                    .style(theme::subtle_btn(p))
                                    .on_press(Message::OpenRenameBranchModal(Some(
                                        b.name.clone(),
                                    ))),
                                format!("Rename branch '{}'", b.name),
                                tooltip::Position::Top,
                                p,
                            ),
                            tip(
                                button(icons::trash(icon_size::XS, p.danger_fg))
                                    .padding([5, 6])
                                    .style(theme::subtle_btn(p))
                                    .on_press(Message::RequestAbortGitFlowBranch {
                                        branch: b.name.clone(),
                                        base_branch: base_b,
                                    }),
                                format!(
                                    "Cancel / delete '{}' without merging",
                                    b.name
                                ),
                                tooltip::Position::Top,
                                p,
                            ),
                        ]
                        .spacing(space::XS)
                        .align_y(Alignment::Center),
                    )
                    .padding([6, 8])
                    .width(Length::Fill)
                    .style(theme::card_box(p));
                    rows_col = rows_col.push(item_row);
                }
                container(slim_scroll(rows_col, p).height(Length::Shrink))
                    .max_height(210.0)
                    .width(Length::Fill)
                    .into()
            };

            let active_card = theme::section_card(
                p,
                "Active Flow Branches",
                Some("Finish & merge, rename, or cancel (delete without merging) active flow branches."),
                Some(theme::badge(
                    format!("{active_count}"),
                    p.surface_card,
                    if active_count > 0 {
                        p.accent_fg
                    } else {
                        p.text_muted
                    },
                    p.border_subtle,
                    false,
                )),
                active_body,
            );

            column![workflow_card, active_card].spacing(space::MD).into()
        };

        theme::modal_shell(
            p,
            layout::MODAL_MD,
            icons::branch(icon_size::MD, p.accent_fg),
            "Git Flow",
            header_badge,
            body,
            Message::CloseOverlays,
        )
    }
}
