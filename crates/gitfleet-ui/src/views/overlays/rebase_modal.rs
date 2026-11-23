use crate::app::{GitFleetApp, Message};
use crate::icons;
use crate::theme::kit::{ModalShell, ModalSize};
use crate::theme::{self, Palette, icon_size, pad, space, type_scale};
use crate::views::slim_scroll;
use gitfleet_git::{RebaseAction, RebaseTodoItem};
use iced::widget::{Space, button, column, container, horizontal_rule, row, text, text_input};
use iced::{Alignment, Color, Element, Length};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RebaseActionChoice {
    Pick,
    Reword,
    Squash,
    Fixup,
    Drop,
}

#[derive(Debug, Clone)]
pub(crate) struct RebasePlanRow {
    pub(crate) oid: String,
    pub(crate) short_oid: String,
    pub(crate) summary: String,
    pub(crate) choice: RebaseActionChoice,
    pub(crate) reword_text: String,
}

#[derive(Debug, Clone)]
pub(crate) struct RebaseModalState {
    pub(crate) base_ref: String,
    pub(crate) base_label: String,
    pub(crate) rows: Vec<RebasePlanRow>,
    pub(crate) loading: bool,
    pub(crate) error: Option<String>,
}

impl RebaseModalState {
    pub(crate) fn new_loading(base_ref: String, base_label: String) -> Self {
        Self {
            base_ref,
            base_label,
            rows: Vec::new(),
            loading: true,
            error: None,
        }
    }

    pub(crate) fn populate(&mut self, items: Vec<RebaseTodoItem>) {
        self.loading = false;
        self.error = None;
        self.rows = items
            .into_iter()
            .map(|it| RebasePlanRow {
                oid: it.oid,
                short_oid: it.short_oid,
                reword_text: it.summary.clone(),
                summary: it.summary,
                choice: RebaseActionChoice::Pick,
            })
            .collect();
    }

    pub(crate) fn validation_error(&self) -> Option<&'static str> {
        if self.loading {
            return Some("Loading commits in rebase range…");
        }
        if self.rows.is_empty() {
            return Some("No commits found between the selected base and HEAD.");
        }
        let first_kept = self
            .rows
            .iter()
            .find(|r| r.choice != RebaseActionChoice::Drop);
        if let Some(first) = first_kept
            && matches!(
                first.choice,
                RebaseActionChoice::Squash | RebaseActionChoice::Fixup
            )
        {
            return Some("The first kept commit cannot be Squash or Fixup (no previous commit to meld into).");
        }
        None
    }

    pub(crate) fn to_git_plan(&self) -> Vec<RebaseTodoItem> {
        self.rows
            .iter()
            .map(|r| {
                let action = match r.choice {
                    RebaseActionChoice::Pick => RebaseAction::Pick,
                    RebaseActionChoice::Reword => RebaseAction::Reword(r.reword_text.clone()),
                    RebaseActionChoice::Squash => RebaseAction::Squash,
                    RebaseActionChoice::Fixup => RebaseAction::Fixup,
                    RebaseActionChoice::Drop => RebaseAction::Drop,
                };
                RebaseTodoItem {
                    action,
                    oid: r.oid.clone(),
                    short_oid: r.short_oid.clone(),
                    summary: r.summary.clone(),
                }
            })
            .collect()
    }
}

impl GitFleetApp {
    pub(crate) fn view_interactive_rebase_modal(
        &self,
        p: Palette,
        modal: &RebaseModalState,
    ) -> Element<'_, Message> {
        let kept_cnt = modal
            .rows
            .iter()
            .filter(|r| r.choice != RebaseActionChoice::Drop)
            .count();
        let drop_cnt = modal.rows.len().saturating_sub(kept_cnt);

        let info_strip = container(
            row![
                icons::undo(12.0, p.success_fg),
                text(format!(
                    "Onto {} · {} commit(s) ({} kept, {} dropped) · Pre-rebase HEAD saved to Undo",
                    modal.base_label,
                    modal.rows.len(),
                    kept_cnt,
                    drop_cnt
                ))
                .size(type_scale::CAPTION)
                .color(p.text_secondary),
            ]
            .spacing(space::XS)
            .align_y(Alignment::Center),
        )
        .padding([5, 10])
        .width(Length::Fill)
        .style(theme::section_panel_box(p));

        let mut list_col = column![].spacing(space::XS);
        if modal.loading {
            list_col = list_col.push(
                container(
                    text("Loading commits in rebase range…")
                        .size(type_scale::BODY)
                        .color(p.text_muted),
                )
                .padding(14),
            );
        } else if modal.rows.is_empty() {
            list_col = list_col.push(
                container(
                    text("No commits ahead of the selected base on HEAD.")
                        .size(type_scale::BODY)
                        .color(p.text_muted),
                )
                .padding(14),
            );
        } else {
            let total = modal.rows.len();
            for (idx, item) in modal.rows.iter().enumerate() {
                let is_dropped = item.choice == RebaseActionChoice::Drop;
                let action_pill = |label: &'static str,
                                   choice: RebaseActionChoice,
                                   fg: Color|
                 -> Element<'_, Message> {
                    let active = item.choice == choice;
                    button(
                        text(label)
                            .size(type_scale::BADGE)
                            .color(if active { p.accent_fg } else { fg })
                            .wrapping(text::Wrapping::None),
                    )
                    .padding([2, 7])
                    .style(theme::filter_pill_btn(p, active))
                    .on_press(Message::RebaseSetAction(idx, choice))
                    .into()
                };

                let reorder_btns = row![
                    button(text("↑").size(type_scale::CAPTION))
                        .padding([2, 6])
                        .style(theme::subtle_btn(p))
                        .on_press_maybe((idx > 0).then_some(Message::RebaseMoveRow(idx, -1))),
                    button(text("↓").size(type_scale::CAPTION))
                        .padding([2, 6])
                        .style(theme::subtle_btn(p))
                        .on_press_maybe(
                            (idx + 1 < total).then_some(Message::RebaseMoveRow(idx, 1))
                        ),
                ]
                .spacing(2)
                .align_y(Alignment::Center);

                let top_line = row![
                    text(format!("#{}", idx + 1))
                        .size(type_scale::BADGE)
                        .font(theme::CODE_FONT)
                        .color(p.text_muted),
                    reorder_btns,
                    row![
                        action_pill("Pick", RebaseActionChoice::Pick, p.text_secondary),
                        action_pill("Reword", RebaseActionChoice::Reword, p.success_fg),
                        action_pill("Squash", RebaseActionChoice::Squash, p.violet_fg),
                        action_pill("Fixup", RebaseActionChoice::Fixup, p.warning_fg),
                        action_pill("Drop", RebaseActionChoice::Drop, p.danger_fg),
                    ]
                    .spacing(3)
                    .align_y(Alignment::Center),
                    text(item.short_oid.clone())
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(p.accent_fg),
                    theme::clipped_line(
                        text(item.summary.clone())
                            .size(type_scale::BODY)
                            .color(if is_dropped {
                                p.text_muted
                            } else {
                                p.text_primary
                            }),
                        Length::Fill,
                    ),
                ]
                .spacing(space::XS)
                .align_y(Alignment::Center);

                let mut row_card = column![top_line].spacing(space::XXS);
                if item.choice == RebaseActionChoice::Reword {
                    row_card = row_card.push(
                        text_input("New commit message…", &item.reword_text)
                            .on_input(move |v| Message::RebaseEditMessage(idx, v))
                            .padding(pad::INPUT_COMPACT)
                            .size(type_scale::CAPTION)
                            .width(Length::Fill)
                            .style(theme::dark_input(p)),
                    );
                }

                list_col = list_col.push(
                    container(row_card)
                        .padding([6, 8])
                        .width(Length::Fill)
                        .style(theme::section_panel_box(p)),
                );
            }
        }

        let validation = modal.validation_error();
        let err_banner: Element<'_, Message> = if let Some(err) = modal
            .error
            .as_deref()
            .or(validation.filter(|_| !modal.loading))
        {
            container(
                text(err.to_string())
                    .size(type_scale::CAPTION)
                    .color(p.danger_fg),
            )
            .padding([5, 10])
            .width(Length::Fill)
            .style(theme::pill_box(p.danger_bg, p.danger_fg, p.danger_fg))
            .into()
        } else {
            Space::with_height(Length::Shrink).into()
        };

        let can_submit = !modal.loading && validation.is_none();
        let footer = row![
            text("Oldest commit at top (#1) → newest at bottom, matching git rebase -i")
                .size(type_scale::BADGE)
                .color(p.text_muted),
            Space::with_width(Length::Fill),
            button(text("Cancel").size(type_scale::BODY))
                .padding(pad::BTN_MD)
                .style(theme::subtle_btn(p))
                .on_press(Message::CloseInteractiveRebase),
            button(
                row![
                    icons::branch(13.0, Color::WHITE),
                    text("Start Rebase").size(type_scale::BODY).color(Color::WHITE),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding(pad::BTN_MD)
            .style(theme::primary_btn(p))
            .on_press_maybe(can_submit.then_some(Message::ConfirmInteractiveRebase)),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);

        let body = column![
            info_strip,
            slim_scroll(list_col, p).height(Length::Fixed(330.0)),
            err_banner,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            footer,
        ]
        .spacing(space::SM);

        ModalShell::new(p, "Interactive Rebase", body)
            .subtitle("Reorder, reword, squash, fixup, or drop commits cleanly without opening a terminal editor.")
            .icon(icons::branch(icon_size::MD, p.violet_fg))
            .size(ModalSize::Lg)
            .on_close(Message::CloseInteractiveRebase)
            .into_element()
    }
}
