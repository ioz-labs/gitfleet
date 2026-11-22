use crate::app::{GitFleetApp, Message, RenameGroupModalState};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use iced::widget::{Space, button, column, row, text, text_input};
use iced::{Alignment, Element, Length};

impl GitFleetApp {
    pub(crate) fn view_rename_group_modal<'a>(
        &'a self,
        p: Palette,
        modal: &'a RenameGroupModalState,
    ) -> Element<'a, Message> {
        let ws = self.service.workspace.current();
        let old_norm = gitfleet_workspace::normalize_group_path(&modal.old_name);
        let new_norm = gitfleet_workspace::normalize_group_path(&modal.new_name);
        let repo_count = gitfleet_workspace::count_repos_in_group(&ws, &old_norm);

        let old_prefix = format!("{old_norm}/");
        let validation_err: Option<String> = if new_norm.is_empty() {
            None
        } else if new_norm == old_norm {
            None
        } else if new_norm.starts_with(&old_prefix) {
            Some("Cannot rename a group into one of its own subgroups.".into())
        } else if ws.groups.iter().any(|g| g == &new_norm)
            || ws
                .repos
                .iter()
                .any(|r| r.group.as_deref() == Some(new_norm.as_str()))
        {
            Some(format!("Group '{new_norm}' already exists."))
        } else {
            None
        };

        let can_submit = !new_norm.is_empty() && new_norm != old_norm && validation_err.is_none();
        let display_err = validation_err.or_else(|| modal.error.clone());

        let mut input = text_input("New group name (e.g. work/backend)…", &modal.new_name)
            .id(self.rename_group_input_id.clone())
            .on_input(Message::RenameGroupInputChanged)
            .padding(pad::INPUT)
            .size(type_scale::BODY)
            .width(Length::Fill)
            .style(theme::dark_input(p));
        if can_submit {
            input = input.on_submit(Message::SubmitRenameGroup);
        }

        let mut current_row = row![
            text("Current group:")
                .size(type_scale::CAPTION)
                .color(p.text_muted),
            theme::badge(
                old_norm,
                p.surface_card,
                p.accent_fg,
                p.border_subtle,
                true,
            ),
        ]
        .spacing(space::XS)
        .align_y(Alignment::Center);

        if repo_count > 0 {
            current_row = current_row.push(theme::badge(
                format!("{repo_count} repo{}", if repo_count == 1 { "" } else { "s" }),
                p.surface_card,
                p.text_muted,
                p.border_subtle,
                false,
            ));
        }

        let mut body = column![
            current_row,
            input,
            text("Use '/' to nest or move into a parent group. Subgroups, repositories, and secret key bindings update automatically.")
                .size(type_scale::CAPTION)
                .color(p.text_muted),
        ]
        .spacing(space::MD);

        if let Some(err) = display_err {
            body = body.push(
                row![
                    icons::alert_triangle(icon_size::XS, p.danger_fg),
                    text(err).size(type_scale::CAPTION).color(p.danger_fg),
                ]
                .spacing(space::XS)
                .align_y(Alignment::Center),
            );
        }

        body = body.push(
            row![
                text("Esc to cancel")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
                Space::with_width(Length::Fill),
                button(text("Cancel").size(type_scale::BODY))
                    .padding(pad::BTN_MD)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::CloseOverlays),
                button(text("Rename Group").size(type_scale::BODY))
                    .padding(pad::BTN_MD)
                    .style(theme::primary_btn(p))
                    .on_press_maybe(can_submit.then_some(Message::SubmitRenameGroup)),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        );

        theme::modal_shell(
            p,
            layout::MODAL_SM,
            icons::folder(icon_size::MD, p.accent_fg),
            "Rename Group",
            None,
            body,
            Message::CloseOverlays,
        )
    }
}
