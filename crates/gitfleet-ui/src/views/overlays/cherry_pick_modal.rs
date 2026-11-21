use crate::app::{CherryPickModalState, GitFleetApp, Message};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use crate::views::tip;
use iced::widget::{Space, button, checkbox, column, container, row, text, tooltip};
use iced::{Alignment, Element, Length};

impl GitFleetApp {
    pub(crate) fn view_cherry_pick_modal(
        &self,
        p: Palette,
        modal: &CherryPickModalState,
    ) -> Element<'_, Message> {
        let count = modal.revisions.len();
        let heading = if count > 1 {
            format!("Do you want to cherry-pick {count} commits?")
        } else {
            "Do you want to cherry-pick?".to_string()
        };
        let subtitle = if count > 1 {
            "This will cherry-pick the selected commits into the Working Tree."
        } else {
            "This will cherry-pick the selected commit into the Working Tree."
        };

        let body = column![
            row![
                container(icons::cherry_pick(icon_size::MD + 2.0, p.success_fg))
                    .padding(8)
                    .style(theme::pill_box(p.success_bg, p.success_fg, p.success_fg)),
                column![
                    text(heading)
                        .size(type_scale::TITLE)
                        .color(p.text_primary),
                    text(subtitle)
                        .size(type_scale::BODY)
                        .color(p.text_secondary),
                ]
                .spacing(4),
            ]
            .spacing(space::MD)
            .align_y(Alignment::Center),
            container(
                text(modal.detail.clone())
                    .size(type_scale::CODE)
                    .font(theme::CODE_FONT)
                    .color(p.text_primary),
            )
            .padding([8, 12])
            .width(Length::Fill)
            .style(theme::section_panel_box(p)),
            checkbox(
                "Append source commit ID to commit message (-x)",
                modal.append_source_id,
            )
            .on_toggle(Message::ToggleCherryPickAppendId)
            .size(14)
            .text_size(type_scale::BODY),
            row![
                text("Esc to cancel")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
                Space::with_width(Length::Fill),
                tip(
                    button(text("Cherry-Pick & Commit").size(type_scale::BODY))
                        .padding(pad::BTN_MD)
                        .style(theme::primary_btn(p))
                        .on_press(Message::ConfirmCherryPick { commit: true }),
                    "Apply and immediately create commit(s)",
                    tooltip::Position::Top,
                    p,
                ),
                tip(
                    button(text("Cherry-Pick").size(type_scale::BODY))
                        .padding(pad::BTN_MD)
                        .style(theme::subtle_btn(p))
                        .on_press(Message::ConfirmCherryPick { commit: false }),
                    "Apply changes into Working Tree / Index without committing (--no-commit)",
                    tooltip::Position::Top,
                    p,
                ),
                button(text("Cancel").size(type_scale::BODY))
                    .padding(pad::BTN_MD)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::CloseCherryPickModal),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        ]
        .spacing(space::MD);

        theme::modal_shell(
            p,
            layout::MODAL_SM + 48.0,
            icons::cherry_pick(icon_size::MD, p.accent_fg),
            "Cherry-Pick",
            Some(theme::badge(
                format!("{count} target{}", if count == 1 { "" } else { "s" }),
                p.surface_card,
                p.text_secondary,
                p.border_subtle,
                false,
            )),
            body,
            Message::CloseCherryPickModal,
        )
    }
}
