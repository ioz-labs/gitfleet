use crate::app::{GitFleetApp, Message, RenameWorkspaceModalState};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use crate::views::truncate_chars;
use iced::widget::{Space, button, column, row, text, text_input};
use iced::{Alignment, Element, Length};

impl GitFleetApp {
    pub(crate) fn view_rename_workspace_modal<'a>(
        &'a self,
        p: Palette,
        modal: &'a RenameWorkspaceModalState,
    ) -> Element<'a, Message> {
        let trimmed = modal.new_name.trim();
        let can_submit = !trimmed.is_empty() && trimmed != modal.old_name;

        let mut meta_col = column![
            row![
                text("Current workspace:")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
                theme::clipped_line(
                    theme::badge(
                        modal.old_name.clone(),
                        p.surface_card,
                        p.accent_fg,
                        p.border_subtle,
                        false,
                    ),
                    Length::Fill,
                ),
            ]
            .spacing(space::XS)
            .align_y(Alignment::Center),
        ]
        .spacing(space::XXS);

        if let Some(hint) = &modal.file_hint {
            meta_col = meta_col.push(
                text(truncate_chars(hint, 52))
                    .size(type_scale::CAPTION)
                    .font(theme::CODE_FONT)
                    .color(p.text_muted),
            );
        }

        let mut body = column![
            meta_col,
            text_input("New workspace name…", &modal.new_name)
                .id(self.rename_workspace_input_id.clone())
                .on_input(Message::RenameWorkspaceInputChanged)
                .on_submit(Message::SubmitRenameWorkspace)
                .padding(pad::INPUT)
                .size(type_scale::BODY)
                .width(Length::Fill)
                .style(theme::dark_input(p)),
        ]
        .spacing(space::MD);

        if trimmed.is_empty() {
            body = body.push(
                text("Workspace name cannot be empty.")
                    .size(type_scale::CAPTION)
                    .color(p.danger_fg),
            );
        }

        body = body.push(
            row![
                Space::with_width(Length::Fill),
                button(text("Cancel").size(type_scale::BODY))
                    .padding(pad::BTN_MD)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::CloseOverlays),
                button(text("Rename Workspace").size(type_scale::BODY))
                    .padding(pad::BTN_MD)
                    .style(theme::primary_btn(p))
                    .on_press_maybe(can_submit.then_some(Message::SubmitRenameWorkspace)),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        );

        theme::modal_shell(
            p,
            layout::MODAL_SM,
            icons::pencil(icon_size::MD, p.accent_fg),
            "Rename Workspace",
            None,
            body,
            Message::CloseOverlays,
        )
    }
}
