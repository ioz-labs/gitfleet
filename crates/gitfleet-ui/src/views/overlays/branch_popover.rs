use crate::app::{GitFleetApp, Message};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use crate::views::slim_scroll;
use iced::widget::{Space, button, checkbox, column, horizontal_rule, row, text, text_input};
use iced::{Alignment, Color, Element, Length};

impl GitFleetApp {
    pub(crate) fn view_branch_popover(&self, p: Palette) -> Element<'_, Message> {
        let trimmed = self.branch_filter_query.trim();
        let q = trimmed.to_lowercase();
        let exact_exists = !q.is_empty()
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

        let mut list = column![].spacing(space::XXS);
        for b in &self.branches {
            if b.is_remote {
                continue;
            }
            if !q.is_empty() && !b.name.to_lowercase().contains(&q) {
                continue;
            }
            list = list.push(
                button(
                    row![
                        icons::branch(
                            icon_size::XS,
                            if b.is_head {
                                p.success_fg
                            } else {
                                p.text_muted
                            }
                        ),
                        text(b.name.clone())
                            .size(type_scale::BODY)
                            .color(p.text_primary),
                        Space::with_width(Length::Fill),
                        if b.is_head {
                            theme::badge(
                                "current",
                                p.success_bg,
                                p.success_fg,
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
                .style(theme::repo_row_btn(p, b.is_head))
                .on_press(Message::CheckoutBranch(b.name.clone())),
            );
        }

        let create_label = if trimmed.is_empty() {
            if self.branch_create_checkout {
                "Add & Checkout Branch".to_string()
            } else {
                "Add Branch".to_string()
            }
        } else if exact_exists {
            format!("Branch '{trimmed}' already exists")
        } else if self.branch_create_checkout {
            format!("Add & Checkout '{trimmed}'")
        } else {
            format!("Add Branch '{trimmed}'")
        };

        let create_btn = button(
            row![
                icons::plus(icon_size::XS, Color::WHITE),
                text(create_label).size(type_scale::BODY),
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
                .on_toggle(Message::ToggleCreateBranchCheckout),
            Space::with_width(Length::Fill),
            text("From:")
                .size(type_scale::BADGE)
                .color(p.text_muted),
            theme::badge(
                base_label,
                p.surface_card,
                p.accent_fg,
                p.border_subtle,
                true,
            ),
        ]
        .spacing(space::XS)
        .align_y(Alignment::Center);

        let modal_col = column![
            text_input(
                "New branch name (e.g. feat/login) or filter existing…",
                &self.branch_filter_query
            )
            .id(self.branch_filter_id.clone())
            .on_input(Message::BranchFilterChanged)
            .on_submit(Message::CreateBranchSubmitted)
            .padding(pad::INPUT)
            .size(type_scale::BODY)
            .width(Length::Fill)
            .style(theme::dark_input(p)),
            create_options,
            create_btn,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            slim_scroll(list, p).height(Length::Fixed(240.0)),
        ]
        .spacing(space::SM);

        let title = if self.branch_create_base.is_some() {
            "Add Branch"
        } else {
            "Add or Switch Branch"
        };

        theme::modal_shell(
            p,
            layout::MODAL_SM,
            icons::branch(icon_size::MD, p.accent_fg),
            title,
            None,
            modal_col,
            Message::CloseOverlays,
        )
    }
}
