use crate::app::{GitFleetApp, Message, PendingConfirm};
use crate::icons;
use crate::theme::kit::{ModalShell, ModalSize};
use crate::theme::{self, Palette, icon_size, pad, space, type_scale};
use crate::views::slim_scroll;
use gitfleet_git::PullStrategy;
use iced::widget::{
    Space, button, checkbox, column, container, horizontal_rule, radio, row, text, text_input,
};
use iced::{Alignment, Element, Length};

#[derive(Debug, Clone)]
pub(crate) struct SaveStashModalState {
    pub(crate) message: String,
    pub(crate) include_untracked: bool,
    pub(crate) keep_index: bool,
    pub(crate) selected_only: bool,
    pub(crate) selected_paths: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ApplyStashModalState {
    pub(crate) selected_index: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct PullSyncModalState {
    pub(crate) strategy: PullStrategy,
    pub(crate) auto_stash: bool,
    pub(crate) push_after: bool,
}

impl GitFleetApp {
    pub(crate) fn view_save_stash_modal(
        &self,
        p: Palette,
        modal: &SaveStashModalState,
    ) -> Element<'_, Message> {
        let st = self.active_status();
        let branch = st.and_then(|s| s.branch.as_deref()).unwrap_or("HEAD");
        let staged = st.map(|s| s.staged_count).unwrap_or(0);
        let unstaged = st.map(|s| s.unstaged_count).unwrap_or(0);
        let untracked = st.map(|s| s.untracked_count).unwrap_or(0);
        let effective_total = if modal.selected_only && !modal.selected_paths.is_empty() {
            modal.selected_paths.len()
        } else if modal.include_untracked {
            staged + unstaged + untracked
        } else {
            staged + unstaged
        };

        let summary_strip = container(
            row![
                icons::stash_save(14.0, p.accent_fg),
                text(format!(
                    "Branch {branch}  ·  {staged} staged  ·  {unstaged} modified  ·  {untracked} untracked"
                ))
                .size(type_scale::CAPTION)
                .color(p.text_secondary),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        )
        .padding([7, 10])
        .width(Length::Fill)
        .style(theme::section_panel_box(p));

        let msg_field = column![
            text("Stash Message (optional)")
                .size(type_scale::CAPTION)
                .font(theme::UI_FONT_MEDIUM)
                .color(p.text_secondary),
            text_input(
                &format!("WIP on {branch}: describe work in progress…"),
                &modal.message,
            )
            .id(self.save_stash_input_id.clone())
            .on_input(Message::SaveStashMessageChanged)
            .on_submit(Message::ConfirmSaveStashModal)
            .padding(pad::INPUT)
            .size(type_scale::BODY)
            .width(Length::Fill)
            .style(theme::dark_input(p)),
        ]
        .spacing(space::XXS);

        let mut options_col = column![
            checkbox(
                "Include untracked files (-u)",
                modal.include_untracked || modal.selected_only,
            )
            .size(14)
            .text_size(type_scale::BODY)
            .on_toggle_maybe(
                (!modal.selected_only).then_some(Message::SaveStashToggleUntracked),
            ),
            checkbox(
                "Keep staged changes in Index (--keep-index)",
                modal.keep_index && !modal.selected_only,
            )
            .size(14)
            .text_size(type_scale::BODY)
            .on_toggle_maybe(
                (!modal.selected_only).then_some(Message::SaveStashToggleKeepIndex),
            ),
        ]
        .spacing(space::XS);

        if !modal.selected_paths.is_empty() {
            options_col = options_col.push(
                checkbox(
                    format!(
                        "Stash only selected files ({} file(s))",
                        modal.selected_paths.len()
                    ),
                    modal.selected_only,
                )
                .size(14)
                .text_size(type_scale::BODY)
                .on_toggle(Message::SaveStashToggleSelectedOnly),
            );
        }

        let can_save = effective_total > 0;
        let on_accent = theme::on_accent_ink(p.accent_primary);
        let footer = row![
            text(if can_save {
                format!("{effective_total} file(s) will be saved to stash@{{0}}")
            } else {
                "No matching tracked/untracked changes to stash".to_string()
            })
            .size(type_scale::CAPTION)
            .color(if can_save { p.text_muted } else { p.warning_fg }),
            Space::with_width(Length::Fill),
            button(text("Cancel").size(type_scale::BODY))
                .padding(pad::BTN_MD)
                .style(theme::subtle_btn(p))
                .on_press(Message::CloseSaveStashModal),
            button(
                row![
                    icons::stash_save(
                        13.0,
                        if can_save { on_accent } else { p.text_muted },
                    ),
                    text("Save Stash").size(type_scale::BODY).color(if can_save {
                        on_accent
                    } else {
                        p.text_muted
                    }),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding(pad::BTN_MD)
            .style(theme::primary_btn(p))
            .on_press_maybe(can_save.then_some(Message::ConfirmSaveStashModal)),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);

        let body = column![
            summary_strip,
            msg_field,
            container(options_col)
                .padding([10, 12])
                .width(Length::Fill)
                .style(theme::section_panel_box(p)),
            horizontal_rule(1).style(theme::subtle_rule(p)),
            footer,
        ]
        .spacing(space::MD);

        ModalShell::new(p, "Save Stash", body)
            .subtitle("Shelve uncommitted changes safely so you can switch branches, rebase, or pull cleanly.")
            .icon(icons::stash_save(icon_size::MD, p.accent_fg))
            .size(ModalSize::Sm)
            .on_close(Message::CloseSaveStashModal)
            .into_element()
    }

    pub(crate) fn view_apply_stash_modal(
        &self,
        p: Palette,
        modal: &ApplyStashModalState,
    ) -> Element<'_, Message> {
        let sel_idx = modal
            .selected_index
            .min(self.stashes.len().saturating_sub(1));

        let mut list_col = column![].spacing(3).width(Length::Fill);
        if self.stashes.is_empty() {
            list_col = list_col.push(
                container(
                    text("No stashes saved in this repository.")
                        .size(type_scale::BODY)
                        .color(p.text_muted),
                )
                .padding(14),
            );
        } else {
            for entry in &self.stashes {
                let idx = entry.index;
                let selected = idx == sel_idx;
                let row_btn = button(
                    row![
                        container(
                            text(entry.ref_name.clone())
                                .size(type_scale::BADGE)
                                .font(theme::CODE_FONT)
                                .color(if selected { p.accent_fg } else { p.text_secondary }),
                        )
                        .padding([2, 6])
                        .style(theme::pill_box(
                            p.surface_card,
                            if selected {
                                p.accent_primary
                            } else {
                                p.border_subtle
                            },
                            p.text_secondary,
                        )),
                        theme::clipped_line(
                            text(entry.message.clone())
                                .size(type_scale::BODY)
                                .color(p.text_primary),
                            Length::Fill,
                        ),
                        text(entry.date_relative.clone())
                            .size(type_scale::BADGE)
                            .color(p.text_muted)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(space::SM)
                    .align_y(Alignment::Center),
                )
                .padding([6, 8])
                .width(Length::Fill)
                .style(theme::tab_btn(p, selected))
                .on_press(Message::SelectApplyStashIndex(idx));

                let inspect_btn = button(
                    text("Inspect")
                        .size(type_scale::BADGE)
                        .color(p.accent_fg)
                        .wrapping(text::Wrapping::None),
                )
                .padding([4, 8])
                .style(theme::subtle_btn(p))
                .on_press(Message::InspectStashFromModal(idx));

                list_col = list_col.push(
                    row![row_btn, inspect_btn]
                        .spacing(space::XS)
                        .align_y(Alignment::Center),
                );
            }
        }

        let stash_box = container(slim_scroll(list_col, p).height(Length::Fixed(210.0)))
            .padding(6)
            .width(Length::Fill)
            .style(theme::section_panel_box(p));

        let safety_note = container(
            row![
                icons::shield(13.0, p.success_fg),
                text("Conflict safety: if applying or popping conflicts with local files, Git preserves the stash entry intact and GitFleet opens the 3-Way Conflict Solver.")
                    .size(type_scale::CAPTION)
                    .color(p.text_secondary),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        )
        .padding([7, 10])
        .width(Length::Fill)
        .style(theme::section_panel_box(p));

        let selected_entry = self
            .stashes
            .iter()
            .find(|s| s.index == sel_idx)
            .or_else(|| self.stashes.first());
        let has_stash = selected_entry.is_some();
        let drop_confirm = selected_entry.map(|s| {
            Message::RequestConfirm(PendingConfirm::drop_stash(
                s.index,
                &s.ref_name,
                &s.message,
            ))
        });
        let on_accent = theme::on_accent_ink(p.accent_primary);
        let footer = row![
            button(
                row![
                    icons::trash(12.0, if has_stash { p.danger_fg } else { p.text_muted }),
                    text("Drop")
                        .size(type_scale::BODY)
                        .color(if has_stash { p.danger_fg } else { p.text_muted }),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding(pad::BTN_MD)
            .style(theme::subtle_btn(p))
            .on_press_maybe(drop_confirm),
            Space::with_width(Length::Fill),
            button(text("Cancel").size(type_scale::BODY))
                .padding(pad::BTN_MD)
                .style(theme::subtle_btn(p))
                .on_press(Message::CloseApplyStashModal),
            button(text("Apply & Keep").size(type_scale::BODY))
                .padding(pad::BTN_MD)
                .style(theme::subtle_btn(p))
                .on_press_maybe(
                    has_stash.then_some(Message::ConfirmApplyStashModal { pop: false }),
                ),
            button(
                row![
                    icons::stash_apply(
                        13.0,
                        if has_stash { on_accent } else { p.text_muted },
                    ),
                    text("Pop Stash (Apply & Drop)")
                        .size(type_scale::BODY)
                        .color(if has_stash { on_accent } else { p.text_muted }),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding(pad::BTN_MD)
            .style(theme::primary_btn(p))
            .on_press_maybe(has_stash.then_some(Message::ConfirmApplyStashModal { pop: true })),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);

        let body = column![
            stash_box,
            safety_note,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            footer,
        ]
        .spacing(space::MD);

        ModalShell::new(p, "Apply / Pop Stash", body)
            .subtitle("Restore shelved changes back into your working tree.")
            .icon(icons::stash_apply(icon_size::MD, p.accent_fg))
            .size(ModalSize::Md)
            .on_close(Message::CloseApplyStashModal)
            .into_element()
    }

    pub(crate) fn view_pull_sync_modal(
        &self,
        p: Palette,
        modal: &PullSyncModalState,
    ) -> Element<'_, Message> {
        let st = self.active_status();
        let branch = st.and_then(|s| s.branch.as_deref()).unwrap_or("HEAD");
        let upstream = st
            .and_then(|s| s.upstream.as_deref())
            .unwrap_or("origin (tracking)");
        let behind = st.map(|s| s.behind).unwrap_or(0);
        let ahead = st.map(|s| s.ahead).unwrap_or(0);
        let dirty_count = st
            .map(|s| s.staged_count + s.unstaged_count + s.untracked_count)
            .unwrap_or(0);

        let status_strip = container(
            row![
                icons::branch(14.0, p.accent_fg),
                text(format!("{branch}  →  {upstream}"))
                    .size(type_scale::BODY)
                    .font(theme::CODE_FONT)
                    .color(p.text_primary),
                Space::with_width(Length::Fill),
                text(format!("↓ {behind} incoming"))
                    .size(type_scale::CAPTION)
                    .color(if behind > 0 { p.accent_fg } else { p.text_muted }),
                text("·").size(type_scale::CAPTION).color(p.text_muted),
                text(format!("↑ {ahead} outgoing"))
                    .size(type_scale::CAPTION)
                    .color(if ahead > 0 { p.success_fg } else { p.text_muted }),
                text("·").size(type_scale::CAPTION).color(p.text_muted),
                text(if dirty_count > 0 {
                    format!("{dirty_count} uncommitted")
                } else {
                    "Clean working tree".to_string()
                })
                .size(type_scale::CAPTION)
                .color(if dirty_count > 0 {
                    p.warning_fg
                } else {
                    p.text_muted
                }),
            ]
            .spacing(space::XS)
            .align_y(Alignment::Center),
        )
        .padding([8, 12])
        .width(Length::Fill)
        .style(theme::section_panel_box(p));

        let strategy_card = |strat: PullStrategy, title: &'static str, desc: &'static str| {
            let active = modal.strategy == strat;
            button(
                row![
                    radio("", strat, Some(modal.strategy), Message::PullSyncSetStrategy).size(14),
                    column![
                        text(title)
                            .size(type_scale::BODY)
                            .font(theme::UI_FONT_MEDIUM)
                            .color(if active { p.accent_fg } else { p.text_primary }),
                        text(desc)
                            .size(type_scale::CAPTION)
                            .color(p.text_secondary),
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                ]
                .spacing(space::SM)
                .align_y(Alignment::Center),
            )
            .padding([8, 10])
            .width(Length::Fill)
            .style(theme::tab_btn(p, active))
            .on_press(Message::PullSyncSetStrategy(strat))
        };

        let strategies_col = column![
            text("Integrate Upstream Commits")
                .size(type_scale::CAPTION)
                .font(theme::UI_FONT_MEDIUM)
                .color(p.text_secondary),
            strategy_card(
                PullStrategy::FastForwardOnly,
                "Fast-Forward Only (--ff-only)",
                "Advances branch pointer cleanly without creating merge commits; aborts if branches diverged.",
            ),
            strategy_card(
                PullStrategy::Rebase,
                "Rebase Local Commits (--rebase)",
                "Replays your unpushed local commits linearly on top of incoming upstream commits.",
            ),
            strategy_card(
                PullStrategy::Merge,
                "Merge Upstream (--no-rebase)",
                "Integrates diverged upstream commits with a standard merge commit.",
            ),
        ]
        .spacing(space::XS);

        let options_box = container(
            column![
                checkbox(
                    if dirty_count > 0 {
                        format!(
                            "Auto-Stash uncommitted changes ({dirty_count} file(s): Stash → Pull → Stash Pop)"
                        )
                    } else {
                        "Auto-Stash uncommitted changes before pulling".to_string()
                    },
                    modal.auto_stash,
                )
                .size(14)
                .text_size(type_scale::BODY)
                .on_toggle(Message::PullSyncToggleAutoStash),
                text("If incoming commits modify the same lines as your uncommitted edits, GitFleet keeps stash@{0} intact as a backup and opens the 3-Way Conflict Solver automatically.")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
                horizontal_rule(1).style(theme::subtle_rule(p)),
                checkbox(
                    if ahead > 0 {
                        format!("Push {ahead} outgoing commit(s) after Pull succeeds (Sync)")
                    } else {
                        "Push local branch to upstream after Pull succeeds (Sync)".to_string()
                    },
                    modal.push_after,
                )
                .size(14)
                .text_size(type_scale::BODY)
                .on_toggle(Message::PullSyncTogglePushAfter),
            ]
            .spacing(space::XS),
        )
        .padding([10, 12])
        .width(Length::Fill)
        .style(theme::section_panel_box(p));

        let on_accent = theme::on_accent_ink(p.accent_primary);
        let action_label = if modal.push_after {
            "Sync (Pull & Push)"
        } else {
            "Pull Now"
        };

        let footer = row![
            button(
                row![
                    icons::fetch(13.0, p.text_secondary),
                    text("Fetch Only").size(type_scale::BODY),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding(pad::BTN_MD)
            .style(theme::subtle_btn(p))
            .on_press(Message::PullSyncFetchOnly),
            Space::with_width(Length::Fill),
            button(text("Cancel").size(type_scale::BODY))
                .padding(pad::BTN_MD)
                .style(theme::subtle_btn(p))
                .on_press(Message::ClosePullSyncModal),
            button(
                row![
                    if modal.push_after {
                        icons::sync_arrows(13.0, on_accent)
                    } else {
                        icons::pull(13.0, on_accent)
                    },
                    text(action_label).size(type_scale::BODY).color(on_accent),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding(pad::BTN_MD)
            .style(theme::primary_btn(p))
            .on_press(Message::ConfirmPullSyncModal),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);

        let title = if modal.push_after {
            "Sync Repository"
        } else {
            "Pull from Remote"
        };

        let body = column![
            status_strip,
            strategies_col,
            options_box,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            footer,
        ]
        .spacing(space::MD);

        ModalShell::new(p, title, body)
            .subtitle("Choose integration strategy and Auto-Stash protection for local working tree edits.")
            .icon(if modal.push_after {
                icons::sync_arrows(icon_size::MD, p.accent_fg)
            } else {
                icons::pull(icon_size::MD, p.accent_fg)
            })
            .size(ModalSize::Md)
            .on_close(Message::ClosePullSyncModal)
            .into_element()
    }
}
