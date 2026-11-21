use crate::app::{GitFleetApp, Message};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use crate::views::slim_scroll;
use iced::widget::{Space, button, column, horizontal_rule, row, text, text_input};
use iced::{Alignment, Color, Element, Length};

impl GitFleetApp {
    pub(crate) fn view_branch_popover(&self, p: Palette) -> Element<'_, Message> {
        let q = self.branch_filter_query.trim().to_lowercase();
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

        let mut modal_col = column![
            text_input("Filter or create new branch…", &self.branch_filter_query)
                .id(self.branch_filter_id.clone())
                .on_input(Message::BranchFilterChanged)
                .on_submit(Message::CreateBranchSubmitted)
                .padding(pad::INPUT)
                .size(type_scale::BODY)
                .width(Length::Fill)
                .style(theme::dark_input(p)),
        ]
        .spacing(space::SM);

        if !q.is_empty() {
            modal_col = modal_col.push(
                button(
                    row![
                        icons::plus(icon_size::XS, Color::WHITE),
                        text(format!(
                            "Create & Checkout '{}'",
                            self.branch_filter_query.trim()
                        ))
                        .size(type_scale::BODY),
                    ]
                    .spacing(space::XS)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding(pad::BTN_MD)
                .style(theme::primary_btn(p))
                .on_press(Message::CreateBranchSubmitted),
            );
        }

        modal_col = modal_col
            .push(horizontal_rule(1))
            .push(slim_scroll(list, p).height(Length::Fixed(280.0)));

        theme::modal_shell(
            p,
            layout::MODAL_SM,
            icons::branch(icon_size::MD, p.accent_fg),
            "Switch or Create Branch",
            None,
            modal_col,
            Message::CloseOverlays,
        )
    }
}
