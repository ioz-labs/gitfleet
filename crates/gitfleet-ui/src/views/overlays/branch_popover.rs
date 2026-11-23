use super::fuzzy::{fuzzy_match, render_fuzzy_text};
use crate::app::{GitFleetApp, Message};
use crate::icons;
use crate::theme::kit::{ModalShell, ModalSize, focus_accent_bar, kbd_pill, status_pill};
use crate::theme::{self, Palette, icon_size, pad, space, type_scale};
use crate::views::slim_scroll;
use gitfleet_git::BranchInfo;
use iced::widget::{Space, button, checkbox, column, horizontal_rule, row, text, text_input};
use iced::{Alignment, Element, Length};

impl GitFleetApp {
    pub(crate) fn build_filtered_popover_branches(&self) -> Vec<(&BranchInfo, Vec<usize>)> {
        let q = self.branch_filter_query.trim();
        if q.is_empty() {
            let mut list: Vec<(&BranchInfo, Vec<usize>)> =
                self.branches.iter().map(|b| (b, Vec::new())).collect();
            // Local branches first (HEAD on top), then remote branches
            list.sort_by(|(a, _), (b, _)| {
                a.is_remote
                    .cmp(&b.is_remote)
                    .then_with(|| b.is_head.cmp(&a.is_head))
                    .then_with(|| a.name.cmp(&b.name))
            });
            return list;
        }

        let mut scored: Vec<(i32, &BranchInfo, Vec<usize>)> = self
            .branches
            .iter()
            .filter_map(|b| {
                fuzzy_match(q, &b.name).map(|(mut score, indices)| {
                    if !b.is_remote {
                        score += 12;
                    }
                    if b.is_head {
                        score += 8;
                    }
                    (score, b, indices)
                })
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        scored.into_iter().map(|(_, b, idx)| (b, idx)).collect()
    }

    pub(crate) fn view_branch_popover(&self, p: Palette) -> Element<'_, Message> {
        let trimmed = self.branch_filter_query.trim();
        let exact_exists = !trimmed.is_empty()
            && self
                .branches
                .iter()
                .any(|b| !b.is_remote && b.name.eq_ignore_ascii_case(trimmed));
        let can_create = !trimmed.is_empty() && !trimmed.ends_with('/') && !exact_exists;

        let base_label = self
            .branch_create_base
            .clone()
            .or_else(|| {
                self.branches
                    .iter()
                    .find(|b| !b.is_remote && b.is_head)
                    .map(|b| b.name.clone())
            })
            .unwrap_or_else(|| "HEAD".to_string());

        let filtered = self.build_filtered_popover_branches();
        let active_idx = if filtered.is_empty() {
            0
        } else {
            self.branch_popover_selected_idx.min(filtered.len() - 1)
        };

        let mut list = column![].spacing(2);
        if filtered.is_empty() {
            list = list.push(
                text("No matching local or remote branches — press Enter above to create a new branch.")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
            );
        } else {
            for (idx, (b, matched)) in filtered.into_iter().enumerate() {
                let is_sel = idx == active_idx;
                let msg = if b.is_remote {
                    Message::CheckoutRemoteBranch(b.name.clone())
                } else {
                    Message::CheckoutBranch(b.name.clone())
                };
                let icon_col = if b.is_head {
                    p.success_fg
                } else if b.is_remote {
                    p.violet_fg
                } else {
                    p.accent_fg
                };

                let mut trailing = row![].spacing(4).align_y(Alignment::Center);
                if b.is_head {
                    trailing = trailing.push(status_pill("HEAD", p.success_fg, p.success_bg));
                } else if b.is_remote {
                    trailing = trailing.push(status_pill("remote", p.violet_fg, p.surface_card));
                }

                list = list.push(
                    button(
                        row![
                            focus_accent_bar(p, is_sel, true, 16.0),
                            icons::branch(icon_size::XS, icon_col),
                            theme::clipped_line(
                                render_fuzzy_text(&b.name, &matched, p.text_primary, p.accent_fg),
                                Length::Fill,
                            ),
                            trailing,
                        ]
                        .spacing(space::SM)
                        .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .padding([5, 8])
                    .style(theme::repo_row_btn(p, is_sel || b.is_head))
                    .on_press(msg),
                );
            }
        }

        let create_label = if trimmed.is_empty() {
            if self.branch_create_checkout {
                "Create & Checkout New Branch".to_string()
            } else {
                "Create New Branch".to_string()
            }
        } else if exact_exists {
            format!("Branch '{trimmed}' already exists")
        } else if self.branch_create_checkout {
            format!("Create & Checkout '{trimmed}'")
        } else {
            format!("Create Branch '{trimmed}'")
        };

        let create_btn = button(
            row![
                icons::plus(icon_size::XS, theme::on_accent_ink(p.accent_primary)),
                text(create_label)
                    .size(type_scale::BODY)
                    .font(theme::UI_FONT_MEDIUM),
            ]
            .spacing(space::XS)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding(pad::BTN_MD)
        .style(theme::primary_btn(p))
        .on_press_maybe(can_create.then_some(Message::CreateBranchSubmitted));

        let create_options = row![
            checkbox("Check out after creating", self.branch_create_checkout)
                .size(13)
                .text_size(type_scale::CAPTION)
                .style(theme::tree_checkbox(p))
                .on_toggle(Message::ToggleCreateBranchCheckout),
            Space::with_width(Length::Fill),
            text("Base:").size(type_scale::BADGE).color(p.text_muted),
            status_pill(base_label, p.accent_fg, p.fill_selected),
        ]
        .spacing(space::XS)
        .align_y(Alignment::Center);

        let modal_col = column![
            row![
                text_input(
                    "Fuzzy filter local/remote branches or type a new branch name…",
                    &self.branch_filter_query
                )
                .id(self.branch_filter_id.clone())
                .on_input(Message::BranchFilterChanged)
                .on_submit(Message::BranchPopoverConfirmSelected)
                .padding(pad::INPUT)
                .size(type_scale::BODY)
                .width(Length::Fill)
                .style(theme::dark_input(p)),
                kbd_pill(p, "↑↓"),
                kbd_pill(p, "Enter"),
            ]
            .spacing(space::XS)
            .align_y(Alignment::Center),
            create_options,
            create_btn,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            slim_scroll(list, p).height(Length::Fixed(260.0)),
        ]
        .spacing(space::SM);

        let title = if self.branch_create_base.is_some() {
            "Create Branch"
        } else {
            "Quick Switch or Create Branch"
        };

        ModalShell::new(p, title, modal_col)
            .subtitle("Navigate local & remote branches with ↑/↓ and press Enter to switch.")
            .icon(icons::branch(icon_size::MD, p.accent_fg))
            .size(ModalSize::Md)
            .scrollable_body(false)
            .on_close(Message::CloseOverlays)
            .into_element()
    }
}
