use crate::app::{GitFleetApp, Message, RenameBranchModalState};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use iced::widget::{Space, button, column, row, text, text_input};
use iced::{Alignment, Element, Length};

impl GitFleetApp {
    pub(crate) fn view_rename_branch_modal<'a>(
        &'a self,
        p: Palette,
        modal: &'a RenameBranchModalState,
    ) -> Element<'a, Message> {
        let trimmed = modal.new_name.trim();
        let can_submit = !trimmed.is_empty() && trimmed != modal.old_name;

        let body = column![
            row![
                text("Current branch:")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
                theme::badge(
                    modal.old_name.clone(),
                    p.surface_card,
                    p.accent_fg,
                    p.border_subtle,
                    true,
                ),
            ]
            .spacing(space::XS)
            .align_y(Alignment::Center),
            text_input("New branch name…", &modal.new_name)
                .id(self.rename_branch_input_id.clone())
                .on_input(Message::RenameBranchInputChanged)
                .on_submit(Message::SubmitRenameBranch)
                .padding(pad::INPUT)
                .size(type_scale::BODY)
                .width(Length::Fill)
                .style(theme::dark_input(p)),
            row![
                Space::with_width(Length::Fill),
                button(text("Cancel").size(type_scale::BODY))
                    .padding(pad::BTN_MD)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::CloseOverlays),
                button(text("Rename Branch").size(type_scale::BODY))
                    .padding(pad::BTN_MD)
                    .style(theme::primary_btn(p))
                    .on_press_maybe(can_submit.then_some(Message::SubmitRenameBranch)),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        ]
        .spacing(space::MD);

        theme::modal_shell(
            p,
            layout::MODAL_SM,
            icons::branch(icon_size::MD, p.accent_fg),
            "Rename Local Branch",
            None,
            body,
            Message::CloseOverlays,
        )
    }
}
