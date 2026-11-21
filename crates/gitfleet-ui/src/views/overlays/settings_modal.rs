use crate::app::{GitFleetApp, Message};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use crate::views::{slim_scroll, truncate_chars};
use iced::widget::{Space, button, checkbox, column, row, text};
use iced::{Alignment, Element, Length};

impl GitFleetApp {
    pub(crate) fn view_settings_modal(&self, p: Palette) -> Element<'_, Message> {
        let s = self.service.settings();
        let interval = s.auto_fetch_interval_secs.unwrap_or(300);

        // 1. Terminal Profile & Font settings
        let profiles = self.service.terminal.profiles();
        let default_pid = self.default_terminal_profile_id();
        let mut profile_row = row![].spacing(space::XS).align_y(Alignment::Center);
        for prof in &profiles {
            let selected = prof.id == default_pid;
            profile_row = profile_row.push(
                button(text(prof.label.clone()).size(type_scale::BODY))
                    .padding(pad::BTN_SM)
                    .style(theme::filter_pill_btn(p, selected))
                    .on_press(Message::SetDefaultTerminalProfile(prof.id.clone())),
            );
        }

        let configured_font = s.terminal_font_family.as_deref();
        let mut font_buttons = column![].spacing(space::XS);
        let mut cur_font_row = row![
            button(
                text(format!(
                    "Auto ({})",
                    truncate_chars(&self.terminal_fonts.default_font, 24)
                ))
                .size(type_scale::CAPTION)
            )
            .padding(pad::BTN_SM)
            .style(theme::filter_pill_btn(p, configured_font.is_none()))
            .on_press(Message::SetTerminalFontFamily(None)),
        ]
        .spacing(space::XS)
        .align_y(Alignment::Center);
        let mut in_row = 1usize;
        for fam in self.terminal_fonts.available.iter().take(11) {
            if in_row >= 3 {
                font_buttons = font_buttons.push(cur_font_row);
                cur_font_row = row![].spacing(space::XS).align_y(Alignment::Center);
                in_row = 0;
            }
            let selected = configured_font.is_some_and(|c| c.eq_ignore_ascii_case(fam));
            cur_font_row = cur_font_row.push(
                button(text(truncate_chars(fam, 24)).size(type_scale::CAPTION))
                    .padding(pad::BTN_SM)
                    .style(theme::filter_pill_btn(p, selected))
                    .on_press(Message::SetTerminalFontFamily(Some(fam.clone()))),
            );
            in_row += 1;
        }
        if in_row > 0 {
            font_buttons = font_buttons.push(cur_font_row);
        }

        let mut size_row = row![theme::field_label(p, "Font size:")]
            .spacing(space::XS)
            .align_y(Alignment::Center);
        for sz in [11u16, 12, 13, 14, 15, 16] {
            size_row = size_row.push(
                button(text(format!("{sz}pt")).size(type_scale::CAPTION))
                    .padding(pad::BTN_SM)
                    .style(theme::filter_pill_btn(p, s.terminal_font_size == sz))
                    .on_press(Message::SetTerminalFontSize(sz)),
            );
        }

        let terminal_card = theme::section_card(
            p,
            "Embedded Terminal",
            Some("Default shell profile and Nerd Font rendering (auto-detected from Windows Terminal / OS)."),
            None,
            column![
                theme::field_label(p, "Default Profile"),
                profile_row,
                theme::field_label(p, "Terminal Font Family"),
                font_buttons,
                size_row,
            ]
            .spacing(space::SM),
        );

        // 2. Automation & Repository Hygiene
        let automation_card = theme::section_card(
            p,
            "Automation & Pre-commit Hygiene",
            Some("Filesystem watcher, background fetch schedule, and oversized file guard."),
            None,
            column![
                checkbox(
                    "Watch active repository filesystem automatically (notify)",
                    s.watch_filesystem,
                )
                .on_toggle(Message::ToggleFsWatcher)
                .size(15)
                .text_size(type_scale::BODY),
                checkbox(
                    "Scheduled background Fetch across workspace",
                    s.scheduled_fetch_enabled,
                )
                .on_toggle(Message::ToggleScheduledFetch)
                .size(15)
                .text_size(type_scale::BODY),
                row![
                    theme::field_label(p, "Fetch interval:"),
                    button(text("1 min").size(type_scale::BODY))
                        .padding(pad::BTN_SM)
                        .style(theme::filter_pill_btn(p, interval == 60))
                        .on_press(Message::SetFetchInterval(60)),
                    button(text("5 min").size(type_scale::BODY))
                        .padding(pad::BTN_SM)
                        .style(theme::filter_pill_btn(p, interval == 300))
                        .on_press(Message::SetFetchInterval(300)),
                    button(text("15 min").size(type_scale::BODY))
                        .padding(pad::BTN_SM)
                        .style(theme::filter_pill_btn(p, interval == 900))
                        .on_press(Message::SetFetchInterval(900)),
                ]
                .spacing(space::XS)
                .align_y(Alignment::Center),
                row![
                    theme::field_label(p, "Oversized file warning threshold:"),
                    button(text("1 MB").size(type_scale::BODY))
                        .padding(pad::BTN_SM)
                        .style(theme::filter_pill_btn(p, s.oversized_threshold_mb == 1))
                        .on_press(Message::SetOversizedThreshold(1)),
                    button(text("5 MB").size(type_scale::BODY))
                        .padding(pad::BTN_SM)
                        .style(theme::filter_pill_btn(p, s.oversized_threshold_mb == 5))
                        .on_press(Message::SetOversizedThreshold(5)),
                    button(text("20 MB").size(type_scale::BODY))
                        .padding(pad::BTN_SM)
                        .style(theme::filter_pill_btn(p, s.oversized_threshold_mb == 20))
                        .on_press(Message::SetOversizedThreshold(20)),
                ]
                .spacing(space::XS)
                .align_y(Alignment::Center),
            ]
            .spacing(space::SM),
        );

        // 3. Git Environment & External Tools
        let mut tools_col = column![].spacing(space::XXS);
        if self.detected_tools.is_empty() {
            tools_col = tools_col.push(
                text("No external diff/merge tools detected on PATH.")
                    .size(type_scale::BODY)
                    .color(p.text_muted),
            );
        } else {
            for t in &self.detected_tools {
                tools_col = tools_col.push(
                    row![
                        text(t.name.clone())
                            .size(type_scale::BODY)
                            .color(p.accent_fg),
                        Space::with_width(Length::Fill),
                        text(t.path.display().to_string())
                            .size(type_scale::CAPTION)
                            .font(theme::CODE_FONT)
                            .color(p.text_muted),
                    ]
                    .align_y(Alignment::Center),
                );
            }
        }

        let env_card = theme::section_card(
            p,
            "Environment & Diagnostics",
            Some("Detected Git CLI and external diff/editor binaries on PATH."),
            Some(theme::badge(
                self.git_version.clone(),
                p.success_bg,
                p.success_fg,
                p.border_subtle,
                true,
            )),
            column![
                tools_col,
                row![
                    button(text("Export Diagnostics JSON…").size(type_scale::BODY))
                        .padding(pad::BTN_MD)
                        .style(theme::primary_btn(p))
                        .on_press(Message::PickExportDiagnostics),
                    button(text("Open Active Repo in Explorer").size(type_scale::BODY))
                        .padding(pad::BTN_MD)
                        .style(theme::subtle_btn(p))
                        .on_press(Message::RevealSelectedInExplorer),
                ]
                .spacing(space::SM),
            ]
            .spacing(space::MD),
        );

        let body = slim_scroll(
            column![terminal_card, automation_card, env_card].spacing(space::MD),
            p,
        )
        .height(Length::Shrink);

        theme::modal_shell(
            p,
            layout::MODAL_LG,
            icons::settings(icon_size::MD, p.accent_fg),
            "Settings & Diagnostics",
            None,
            body,
            Message::CloseOverlays,
        )
    }
}
