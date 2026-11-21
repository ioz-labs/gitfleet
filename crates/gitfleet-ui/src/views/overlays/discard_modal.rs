use crate::app::{DiscardModalState, GitFleetApp, Message};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use crate::views::{parent_dir, slim_scroll};
use gitfleet_git::DiscardTarget;
use iced::widget::{
    Space, button, checkbox, column, container, horizontal_rule, radio, row, text,
};
use iced::{Alignment, Element, Length};

impl GitFleetApp {
    pub(crate) fn view_discard_modal(
        &self,
        p: Palette,
        modal: &DiscardModalState,
    ) -> Element<'_, Message> {
        let total = modal.files.len();
        let selected_count = modal.files.iter().filter(|f| f.selected).count();
        let all_selected = total > 0 && selected_count == total;

        let target_row = row![
            text("Revert to:")
                .size(type_scale::BODY)
                .color(p.text_secondary),
            radio(
                "Index",
                DiscardTarget::Index,
                Some(modal.target),
                Message::SetDiscardTarget,
            )
            .size(14)
            .text_size(type_scale::BODY),
            radio(
                "HEAD",
                DiscardTarget::Head,
                Some(modal.target),
                Message::SetDiscardTarget,
            )
            .size(14)
            .text_size(type_scale::BODY),
            Space::with_width(Length::Fill),
            text(match modal.target {
                DiscardTarget::Index => "Reverts working tree to Index (keeps staged changes)",
                DiscardTarget::Head => "Resets both staged & working tree back to HEAD",
            })
            .size(type_scale::CAPTION)
            .color(p.text_muted),
        ]
        .spacing(space::MD)
        .align_y(Alignment::Center);

        let table_header = container(
            row![
                checkbox("", all_selected)
                    .size(13)
                    .on_toggle(Message::ToggleDiscardAll),
                theme::clipped_line(
                    text("Name").size(type_scale::BADGE).color(p.text_muted),
                    Length::FillPortion(42),
                ),
                theme::clipped_line(
                    text("State").size(type_scale::BADGE).color(p.text_muted),
                    Length::Fixed(96.0),
                ),
                theme::clipped_line(
                    text("Directory")
                        .size(type_scale::BADGE)
                        .color(p.text_muted),
                    Length::FillPortion(58),
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([5, 8])
        .width(Length::Fill)
        .style(theme::dock_subtoolbar_box(p));

        let mut list = column![].spacing(1).width(Length::Fill);
        for (idx, entry) in modal.files.iter().enumerate() {
            let fname = entry
                .path
                .rsplit('/')
                .next()
                .unwrap_or(&entry.path)
                .to_string();
            let dir = parent_dir(&entry.path);
            let dir_display = if dir.is_empty() { "." } else { dir };
            let lower_state = entry.state_label.to_lowercase();
            let state_color = if entry.is_untracked {
                p.accent_fg
            } else if lower_state.contains("delet")
                || lower_state.contains("conflict")
                || lower_state.contains("(-)")
            {
                p.danger_fg
            } else if lower_state.contains("renam") || lower_state.contains("copi") {
                p.violet_fg
            } else if entry.is_added_staged || lower_state.contains("added") {
                p.success_fg
            } else {
                p.warning_fg
            };

            list = list.push(
                container(
                    row![
                        checkbox("", entry.selected)
                            .size(13)
                            .on_toggle(move |v| Message::ToggleDiscardFile(idx, v)),
                        theme::clipped_line(
                            row![
                                icons::file_diff(12.0, p.danger_fg),
                                text(fname)
                                    .size(type_scale::BODY)
                                    .color(p.text_primary),
                            ]
                            .spacing(6)
                            .align_y(Alignment::Center),
                            Length::FillPortion(42),
                        ),
                        theme::clipped_line(
                            text(entry.state_label.clone())
                                .size(type_scale::CAPTION)
                                .color(state_color),
                            Length::Fixed(96.0),
                        ),
                        theme::clipped_line(
                            text(dir_display.to_string())
                                .size(type_scale::CAPTION)
                                .color(p.text_muted),
                            Length::FillPortion(58),
                        ),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([4, 8])
                .width(Length::Fill),
            );
        }

        let files_table = container(
            column![
                table_header,
                horizontal_rule(1).style(theme::subtle_rule(p)),
                slim_scroll(list, p).height(Length::Fixed(230.0)),
            ]
            .width(Length::Fill),
        )
        .width(Length::Fill)
        .style(theme::section_panel_box(p));

        let discard_btn = if selected_count > 0 {
            button(
                text(format!("Discard ({selected_count})")).size(type_scale::BODY),
            )
            .padding(pad::BTN_MD)
            .style(theme::danger_btn(p))
            .on_press(Message::ConfirmDiscardModal)
        } else {
            button(text("Discard").size(type_scale::BODY))
                .padding(pad::BTN_MD)
                .style(theme::subtle_btn(p))
        };

        let body = column![
            column![
                text("Discard local or staged changes")
                    .size(type_scale::TITLE)
                    .color(p.text_primary),
                text("Select the files for which changes should be discarded and whether to set them back to Index or HEAD state.")
                    .size(type_scale::BODY)
                    .color(p.text_secondary),
            ]
            .spacing(4),
            target_row,
            files_table,
            row![
                text("Warning: discarded local edits and deleted untracked files cannot be undone.")
                    .size(type_scale::CAPTION)
                    .color(p.danger_fg),
                Space::with_width(Length::Fill),
                discard_btn,
                button(text("Cancel").size(type_scale::BODY))
                    .padding(pad::BTN_MD)
                    .style(theme::subtle_btn(p))
                    .on_press(Message::CloseDiscardModal),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        ]
        .spacing(space::MD);

        theme::modal_shell(
            p,
            layout::MODAL_MD,
            icons::undo(icon_size::MD, p.danger_fg),
            "Discard",
            Some(theme::badge(
                format!("{selected_count}/{total} selected"),
                p.danger_bg,
                p.danger_fg,
                p.border_subtle,
                false,
            )),
            body,
            Message::CloseDiscardModal,
        )
    }
}
