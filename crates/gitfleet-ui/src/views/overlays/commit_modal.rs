use crate::app::{CommitModalScope, CommitModalState, GitFleetApp, Message};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use crate::views::{parent_dir, slim_scroll};
use iced::widget::{
    Space, button, checkbox, column, container, horizontal_rule, radio, row, text, text_editor,
};
use iced::{Alignment, Color, Element, Length};

fn state_badge_color(label: &str, p: Palette) -> Color {
    match label {
        "Added" => p.success_fg,
        "Modified" => p.warning_fg,
        "Deleted" | "Conflict" => p.danger_fg,
        "Renamed" | "Copied" => p.violet_fg,
        "Untracked" => p.accent_fg,
        _ => p.text_secondary,
    }
}

impl GitFleetApp {
    pub(crate) fn view_commit_modal<'a>(
        &'a self,
        p: Palette,
        modal: &'a CommitModalState,
    ) -> Element<'a, Message> {
        let active_files = modal.active_files();
        let total = active_files.len();
        let selected_count = active_files.iter().filter(|f| f.selected).count();
        let all_selected = total > 0 && selected_count == total;
        let has_msg = !self.commit_modal_editor.text().trim().is_empty();
        let can_submit = has_msg && (modal.amend || selected_count > 0);

        let staged_radio = radio(
            format!("Staged Changes ({})", modal.staged_files.len()),
            CommitModalScope::Staged,
            Some(modal.scope),
            Message::CommitModalSetScope,
        )
        .size(14)
        .text_size(type_scale::BODY);

        let local_radio = radio(
            format!("Local Changes ({})", modal.local_files.len()),
            CommitModalScope::Local,
            Some(modal.scope),
            Message::CommitModalSetScope,
        )
        .size(14)
        .text_size(type_scale::BODY);

        let scope_row = row![
            staged_radio,
            local_radio,
            Space::with_width(Length::Fill),
            theme::badge(
                format!("{selected_count} / {total} file(s)"),
                p.surface_card,
                if selected_count > 0 {
                    p.accent_fg
                } else {
                    p.text_muted
                },
                p.border_subtle,
                false,
            ),
        ]
        .spacing(12)
        .align_y(Alignment::Center);

        let table_header = container(
            row![
                checkbox("", all_selected)
                    .size(13)
                    .spacing(0)
                    .on_toggle(Message::CommitModalToggleAll),
                text("Name")
                    .size(type_scale::BADGE)
                    .color(p.text_muted)
                    .width(Length::FillPortion(46)),
                text("State")
                    .size(type_scale::BADGE)
                    .color(p.text_muted)
                    .width(Length::Fixed(78.0)),
                text("Relative Directory")
                    .size(type_scale::BADGE)
                    .color(p.text_muted)
                    .width(Length::FillPortion(54)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([5, 8])
        .width(Length::Fill)
        .style(theme::table_col_header_box(p));

        let mut list_col = column![].spacing(2).width(Length::Fill);
        if modal.loading && active_files.is_empty() {
            list_col = list_col.push(
                container(
                    text("Loading changed files…")
                        .size(type_scale::CAPTION)
                        .color(p.text_muted),
                )
                .padding([12, 10]),
            );
        } else if active_files.is_empty() {
            let empty_msg = match modal.scope {
                CommitModalScope::Staged => {
                    "No staged files. Switch to Local Changes to commit working tree changes."
                }
                CommitModalScope::Local => "Working tree is clean — no local changes to commit.",
            };
            list_col = list_col.push(
                container(
                    text(empty_msg)
                        .size(type_scale::CAPTION)
                        .color(p.text_muted),
                )
                .padding([12, 10]),
            );
        } else {
            for (idx, f) in active_files.iter().enumerate() {
                let file_name = f.path.rsplit('/').next().unwrap_or(&f.path).to_string();
                let dir = parent_dir(&f.path);
                let dir_display = if dir.is_empty() { "." } else { dir };
                let state_color = state_badge_color(&f.state_label, p);

                list_col = list_col.push(
                    container(
                        row![
                            checkbox("", f.selected)
                                .size(13)
                                .spacing(0)
                                .on_toggle(move |v| Message::CommitModalToggleFile(idx, v)),
                            theme::clipped_line(
                                row![
                                    icons::file_code(12.0, state_color),
                                    text(file_name)
                                        .size(type_scale::BODY)
                                        .color(p.text_primary),
                                ]
                                .spacing(5)
                                .align_y(Alignment::Center),
                                Length::FillPortion(46),
                            ),
                            text(f.state_label.clone())
                                .size(type_scale::CAPTION)
                                .color(state_color)
                                .width(Length::Fixed(78.0))
                                .wrapping(text::Wrapping::None),
                            theme::clipped_line(
                                text(dir_display.to_string())
                                    .size(type_scale::CAPTION)
                                    .color(p.text_muted),
                                Length::FillPortion(54),
                            ),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                    )
                    .padding([3, 8])
                    .width(Length::Fill),
                );
            }
        }

        let files_box = container(
            column![
                table_header,
                slim_scroll(list_col, p).height(Length::Fixed(165.0)),
            ]
            .spacing(2)
            .width(Length::Fill),
        )
        .width(Length::Fill)
        .style(theme::section_panel_box(p));

        let msg_editor = text_editor(&self.commit_modal_editor)
            .placeholder("Write a clear commit message…")
            .on_action(Message::CommitModalMessageAction)
            .size(type_scale::BODY)
            .height(Length::Fixed(110.0))
            .style(theme::dark_editor(p));

        let footer = row![
            checkbox("Amend last commit", modal.amend)
                .size(14)
                .text_size(type_scale::CAPTION)
                .on_toggle(Message::CommitModalAmendToggled),
            Space::with_width(Length::Fill),
            button(text("Cancel").size(type_scale::BODY))
                .padding(pad::BTN_MD)
                .style(theme::subtle_btn(p))
                .on_press(Message::CloseOverlays),
            button(
                row![
                    icons::check(icon_size::XS, p.text_primary),
                    text("Commit").size(type_scale::BODY),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding(pad::BTN_MD)
            .style(theme::subtle_btn(p))
            .on_press_maybe(can_submit.then_some(Message::SubmitCommitModal {
                push_after: false,
            })),
            button(
                row![
                    icons::push(icon_size::XS, Color::WHITE),
                    text("Commit & Push").size(type_scale::BODY),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding(pad::BTN_MD)
            .style(theme::primary_btn(p))
            .on_press_maybe(can_submit.then_some(Message::SubmitCommitModal {
                push_after: true,
            })),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);

        let body = column![
            text("Select the files you want to commit and provide a commit message.")
                .size(type_scale::CAPTION)
                .color(p.text_muted),
            scope_row,
            files_box,
            text("Commit Message:")
                .size(type_scale::CAPTION)
                .color(p.text_secondary),
            msg_editor,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            footer,
        ]
        .spacing(space::SM);

        theme::modal_shell(
            p,
            layout::MODAL_LG,
            Element::from(icons::check(icon_size::MD, p.success_fg)),
            "Commit local or staged changes",
            Some(theme::badge(
                modal.repo_name.clone(),
                p.surface_card,
                p.accent_fg,
                p.border_subtle,
                false,
            )),
            body,
            Message::CloseOverlays,
        )
    }
}
