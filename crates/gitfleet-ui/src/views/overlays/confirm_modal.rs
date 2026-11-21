use crate::app::{GitFleetApp, Message, PendingConfirm};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use iced::widget::{Space, button, column, container, opaque, row, text};
use iced::{Alignment, Element, Length};

impl GitFleetApp {
    pub(crate) fn view_confirm_modal(
        &self,
        p: Palette,
        confirm: &PendingConfirm,
    ) -> Element<'_, Message> {
        let accent = if confirm.is_danger {
            p.danger_fg
        } else {
            p.accent_fg
        };
        let accent_bg = if confirm.is_danger {
            p.danger_bg
        } else {
            p.fill_selected
        };
        let icon_el: Element<'_, Message> = if confirm.is_danger {
            icons::alert_triangle(icon_size::MD + 2.0, accent).into()
        } else {
            icons::sparkle(icon_size::MD + 2.0, accent).into()
        };

        let mut body = column![
            row![
                container(icon_el)
                    .padding(7)
                    .style(theme::pill_box(accent_bg, accent, accent)),
                text(confirm.title.clone())
                    .size(type_scale::TITLE)
                    .color(p.text_primary),
            ]
            .spacing(space::MD)
            .align_y(Alignment::Center),
            text(confirm.message.clone())
                .size(type_scale::BODY)
                .color(p.text_secondary),
        ]
        .spacing(space::MD);

        if let Some(detail) = &confirm.detail {
            body = body.push(
                container(
                    text(detail.clone())
                        .size(type_scale::CODE)
                        .font(theme::CODE_FONT)
                        .color(p.text_primary),
                )
                .padding([8, 12])
                .width(Length::Fill)
                .style(theme::section_panel_box(p)),
            );
        }

        let confirm_button = if confirm.is_danger {
            button(text(confirm.confirm_label.clone()).size(type_scale::BODY))
                .padding(pad::BTN_MD)
                .style(theme::danger_btn(p))
                .on_press(Message::ConfirmAccepted)
        } else {
            button(text(confirm.confirm_label.clone()).size(type_scale::BODY))
                .padding(pad::BTN_MD)
                .style(theme::primary_btn(p))
                .on_press(Message::ConfirmAccepted)
        };

        body = body.push(
            row![
                text("Esc to cancel")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
                Space::with_width(Length::Fill),
                button(text("Cancel").size(type_scale::BODY))
                    .padding(pad::BTN_MD)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::ConfirmCancelled),
                confirm_button,
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        );

        let modal = opaque(
            container(body)
                .padding(pad::MODAL)
                .width(Length::Fixed(layout::MODAL_SM))
                .style(theme::modal_box(p)),
        );

        let centered = container(modal)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(theme::backdrop_box);

        theme::overlay_backdrop(centered, Message::ConfirmCancelled)
    }
}
