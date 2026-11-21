use crate::app::{GitFleetApp, Message};
use crate::icons;
use crate::theme::{self, Palette, icon_size, pad, space, type_scale};
use crate::views::slim_scroll_h;
use iced::widget::{Space, button, column, container, row, text};
use iced::{Alignment, Element, Length};

impl GitFleetApp {
    pub(crate) fn view_command_log_body(&self, p: Palette) -> Element<'_, Message> {
        let entries = self.service.command_log.list();

        let toolbar = theme::dock_toolbar(
            p,
            row![
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
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        );

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
                let ok = e.exit_code == Some(0);
                rows = rows.push(
                    container(
                        row![
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
                                .color(p.text_primary),
                            Space::with_width(Length::Fill),
                            text(format!("{} ms", e.duration_ms))
                                .size(type_scale::CAPTION)
                                .font(theme::CODE_FONT)
                                .color(p.text_muted),
                        ]
                        .spacing(space::SM)
                        .align_y(Alignment::Center),
                    )
                    .padding([5, 8])
                    .width(Length::Fill)
                    .style(theme::section_panel_box(p)),
                );
            }
            let min_w = (max_cmd_chars as f32 * 7.4 + 130.0).clamp(420.0, 2400.0);
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
