use crate::app::{GitFleetApp, Message};
use crate::icons;
use crate::theme::{self, Palette, icon_size, pad, space, type_scale};
use crate::views::{slim_scroll_h, tip};
use iced::widget::{Space, button, column, container, row, text, tooltip};
use iced::{Alignment, Element, Length};

impl GitFleetApp {
    pub(crate) fn view_command_log_body(&self, p: Palette) -> Element<'_, Message> {
        let entries = self.service.command_log.list();

        let mut toolbar_row = row![
            icons::terminal(icon_size::SM, p.text_muted),
            text("Git Command Output")
                .size(type_scale::BODY)
                .color(p.text_secondary),
            theme::badge(
                format!("{} entries", entries.len()),
                p.surface_card,
                p.text_muted,
                p.border_subtle,
                false
            ),
            Space::with_width(Length::Fill),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);

        if !entries.is_empty() {
            let all_cmds: String = entries
                .iter()
                .rev()
                .take(80)
                .map(|e| e.command.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            toolbar_row = toolbar_row.push(
                button(
                    row![
                        icons::copy(icon_size::XS, p.text_secondary),
                        text("Copy All").size(type_scale::BODY),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding(pad::BTN_SM)
                .style(theme::subtle_btn(p))
                .on_press(Message::CopyText("command log".into(), all_cmds)),
            );
        }

        toolbar_row = toolbar_row.push(
            button(
                row![
                    icons::trash(icon_size::XS, p.text_secondary),
                    text("Clear Log").size(type_scale::BODY),
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .padding(pad::BTN_SM)
            .style(theme::subtle_btn(p))
            .on_press(Message::ClearCommandLog),
        );

        let toolbar = theme::dock_toolbar(p, toolbar_row);

        let body: Element<'_, Message> = if entries.is_empty() {
            theme::empty_state(
                p,
                icons::terminal(icon_size::LG, p.text_muted),
                "No Git commands recorded yet",
                "Git operations executed by GitFleet will appear here with exit status and duration.",
                None,
            )
        } else {
            let mut rows = column![].spacing(space::XXS);
            let mut max_cmd_chars: usize = 40;
            for e in entries.iter().rev().take(80) {
                max_cmd_chars = max_cmd_chars.max(e.command.chars().count());
                if let Some(err_preview) = &e.stderr_preview {
                    max_cmd_chars = max_cmd_chars.max(err_preview.chars().count());
                }
                let ok = e.exit_code == Some(0);
                let copy_btn = tip(
                    button(icons::copy(icon_size::XS, p.text_secondary))
                        .padding([3, 6])
                        .style(theme::subtle_btn(p))
                        .on_press(Message::CopyText("git command".into(), e.command.clone())),
                    "Copy command",
                    tooltip::Position::Left,
                    p,
                );

                let main_line = row![
                    theme::badge(
                        if ok { "OK" } else { "ERR" },
                        if ok { p.success_bg } else { p.danger_bg },
                        if ok { p.success_fg } else { p.danger_fg },
                        p.border_subtle,
                        true
                    ),
                    text(e.command.clone())
                        .size(type_scale::CODE)
                        .font(theme::CODE_FONT)
                        .color(p.text_primary)
                        .wrapping(text::Wrapping::None),
                    Space::with_width(Length::Fill),
                    text(format!("{} ms", e.duration_ms))
                        .size(type_scale::CAPTION)
                        .font(theme::CODE_FONT)
                        .color(p.text_muted)
                        .wrapping(text::Wrapping::None),
                    copy_btn,
                ]
                .spacing(space::SM)
                .align_y(Alignment::Center);

                let mut entry_col = column![main_line].spacing(3).width(Length::Fill);
                if !ok
                    && let Some(err_preview) = &e.stderr_preview
                    && !err_preview.trim().is_empty()
                {
                    entry_col = entry_col.push(
                        row![
                            Space::with_width(Length::Fixed(42.0)),
                            text(err_preview.clone())
                                .size(type_scale::CAPTION)
                                .font(theme::CODE_FONT)
                                .color(p.danger_fg)
                                .wrapping(text::Wrapping::None),
                        ]
                        .align_y(Alignment::Center),
                    );
                }

                rows = rows.push(
                    container(entry_col)
                        .padding([5, 8])
                        .width(Length::Fill)
                        .style(theme::section_panel_box(p)),
                );
            }
            let min_w = (max_cmd_chars as f32 * 7.6 + 180.0).clamp(460.0, 2600.0);
            container(slim_scroll_h(rows, min_w, p))
                .padding(pad::DOCK_BODY)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };

        column![toolbar, body]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
