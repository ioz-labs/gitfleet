//! `D3` — DevTools-Style Git Command Output Log with Filter Modes & `stdout`/`stderr` Inspector.

use crate::app::{CommandLogFilter, GitFleetApp, Message};
use crate::icons;
use crate::theme::kit::{
    ButtonSize, ButtonVariant, SegmentItem, UiButton, focus_accent_bar, segmented_control,
    status_pill,
};
use crate::theme::{self, Palette, icon_size, pad, space, type_scale};
use crate::views::{slim_scroll, truncate_chars};
use iced::widget::{Space, button, column, container, horizontal_rule, row, text, text_input, vertical_rule};
use iced::{Alignment, Element, Length};

impl GitFleetApp {
    pub(crate) fn view_command_log_body(&self, p: Palette) -> Element<'_, Message> {
        let entries = self.service.command_log.list();
        let q = self.command_log_query.trim().to_lowercase();

        let err_cnt = entries.iter().filter(|e| e.exit_code != Some(0)).count();
        let slow_cnt = entries.iter().filter(|e| e.duration_ms >= 200).count();

        let filtered: Vec<_> = entries
            .iter()
            .rev()
            .filter(|e| match self.command_log_filter {
                CommandLogFilter::All => true,
                CommandLogFilter::Errors => e.exit_code != Some(0),
                CommandLogFilter::Slow => e.duration_ms >= 200,
            })
            .filter(|e| {
                q.is_empty()
                    || e.command.to_lowercase().contains(&q)
                    || e.repo_path
                        .as_deref()
                        .is_some_and(|rp| rp.to_lowercase().contains(&q))
            })
            .take(150)
            .collect();

        let mut toolbar_row = row![
            segmented_control(
                p,
                [
                    SegmentItem::new(CommandLogFilter::All, "All").count(entries.len()),
                    SegmentItem::new(CommandLogFilter::Errors, "Errors").count(err_cnt),
                    SegmentItem::new(CommandLogFilter::Slow, "Slow ≥200ms").count(slow_cnt),
                ],
                self.command_log_filter,
                Message::SetCommandLogFilter,
            ),
            text_input("Filter command or repo…", &self.command_log_query)
                .on_input(Message::CommandLogQueryChanged)
                .padding(pad::INPUT_COMPACT)
                .size(type_scale::CAPTION)
                .width(Length::Fixed(210.0))
                .style(theme::dark_input(p)),
            Space::with_width(Length::Fill),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);

        if !entries.is_empty() {
            let all_cmds: String = filtered
                .iter()
                .map(|e| e.command.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            toolbar_row = toolbar_row.push(
                UiButton::new(p, "Copy Visible")
                    .variant(ButtonVariant::Subtle)
                    .size(ButtonSize::Xs)
                    .icon(icons::copy(icon_size::XS, p.text_secondary))
                    .on_press(Message::CopyText("command log".into(), all_cmds)),
            );
        }
        toolbar_row = toolbar_row.push(
            UiButton::new(p, "Clear")
                .variant(ButtonVariant::Subtle)
                .size(ButtonSize::Xs)
                .icon(icons::trash(icon_size::XS, p.text_secondary))
                .on_press(Message::ClearCommandLog),
        );

        let toolbar = theme::dock_toolbar(p, toolbar_row);

        if entries.is_empty() {
            return column![
                toolbar,
                theme::empty_state(
                    p,
                    icons::terminal(icon_size::LG, p.text_muted),
                    "No Git commands recorded yet",
                    "Every Git operation executed by GitFleet is logged here with timing, exit code, stdout, and stderr.",
                    None,
                ),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
        }

        let selected_entry = self
            .selected_command_log_id
            .as_deref()
            .and_then(|id| filtered.iter().copied().find(|e| e.id == id))
            .or_else(|| filtered.first().copied());

        let mut rows = column![].spacing(2);
        for e in &filtered {
            let ok = e.exit_code == Some(0);
            let is_sel = selected_entry.is_some_and(|sel| sel.id == e.id);
            let repo_leaf = e
                .repo_path
                .as_deref()
                .and_then(|rp| rp.rsplit(['/', '\\']).next())
                .unwrap_or("—");

            rows = rows.push(
                button(
                    row![
                        focus_accent_bar(p, is_sel, true, 16.0),
                        status_pill(
                            if ok { "OK" } else { "ERR" },
                            if ok { p.success_fg } else { p.danger_fg },
                            if ok { p.success_bg } else { p.danger_bg },
                        ),
                        text(truncate_chars(repo_leaf, 16))
                            .size(type_scale::BADGE)
                            .font(theme::CODE_FONT)
                            .color(p.accent_fg),
                        theme::clipped_line(
                            text(e.command.clone())
                                .size(type_scale::CODE)
                                .font(theme::CODE_FONT)
                                .color(p.text_primary),
                            Length::Fill,
                        ),
                        text(format!("{} ms", e.duration_ms))
                            .size(type_scale::BADGE)
                            .font(theme::CODE_FONT)
                            .color(if e.duration_ms >= 200 {
                                p.warning_fg
                            } else {
                                p.text_muted
                            }),
                    ]
                    .spacing(space::XS)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding([4, 8])
                .style(theme::repo_row_btn(p, is_sel))
                .on_press(Message::SelectCommandLogEntry(e.id.clone())),
            );
        }

        let inspector: Element<'_, Message> = if let Some(sel) = selected_entry {
            let ok = sel.exit_code == Some(0);
            let ts = sel.timestamp.format("%H:%M:%S%.3f UTC").to_string();
            let repo_str = sel.repo_path.as_deref().unwrap_or("(global)").to_string();
            let stdout_str = sel
                .stdout_preview
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("(empty stdout)")
                .to_string();
            let stderr_str = sel
                .stderr_preview
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("(empty stderr)")
                .to_string();

            container(
                slim_scroll(
                    column![
                        row![
                            status_pill(
                                format!("exit {}", sel.exit_code.unwrap_or(-1)),
                                if ok { p.success_fg } else { p.danger_fg },
                                if ok { p.success_bg } else { p.danger_bg },
                            ),
                            text(format!("{} ms · {ts}", sel.duration_ms))
                                .size(type_scale::BADGE)
                                .font(theme::CODE_FONT)
                                .color(p.text_muted),
                            Space::with_width(Length::Fill),
                            UiButton::new(p, "Copy Cmd")
                                .variant(ButtonVariant::Subtle)
                                .size(ButtonSize::Xs)
                                .icon(icons::copy(icon_size::XS, p.text_secondary))
                                .on_press(Message::CopyText(
                                    "git command".into(),
                                    sel.command.clone(),
                                )),
                        ]
                        .spacing(space::XS)
                        .align_y(Alignment::Center),
                        text(repo_str)
                            .size(type_scale::BADGE)
                            .font(theme::CODE_FONT)
                            .color(p.accent_fg),
                        container(
                            text(sel.command.clone())
                                .size(type_scale::CAPTION)
                                .font(theme::CODE_FONT)
                                .color(p.text_primary),
                        )
                        .padding([6, 8])
                        .width(Length::Fill)
                        .style(theme::card_box(p)),
                        horizontal_rule(1).style(theme::subtle_rule(p)),
                        text("stderr")
                            .size(type_scale::BADGE)
                            .font(theme::UI_FONT_MEDIUM)
                            .color(if ok { p.text_muted } else { p.danger_fg }),
                        text(stderr_str)
                            .size(type_scale::CAPTION)
                            .font(theme::CODE_FONT)
                            .color(if ok { p.text_secondary } else { p.danger_fg }),
                        horizontal_rule(1).style(theme::subtle_rule(p)),
                        text("stdout")
                            .size(type_scale::BADGE)
                            .font(theme::UI_FONT_MEDIUM)
                            .color(p.text_muted),
                        text(stdout_str)
                            .size(type_scale::CAPTION)
                            .font(theme::CODE_FONT)
                            .color(p.text_secondary),
                    ]
                    .spacing(space::XS),
                    p,
                )
                .height(Length::Fill),
            )
            .padding([8, 12])
            .width(Length::FillPortion(42))
            .height(Length::Fill)
            .into()
        } else {
            container(
                text("No log entries match the current filter.")
                    .size(type_scale::CAPTION)
                    .color(p.text_muted),
            )
            .padding(12)
            .width(Length::FillPortion(42))
            .into()
        };

        column![
            toolbar,
            row![
                container(slim_scroll(rows, p).height(Length::Fill))
                    .padding([6, 8])
                    .width(Length::FillPortion(58))
                    .height(Length::Fill),
                vertical_rule(1).style(theme::subtle_rule(p)),
                inspector,
            ]
            .width(Length::Fill)
            .height(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
