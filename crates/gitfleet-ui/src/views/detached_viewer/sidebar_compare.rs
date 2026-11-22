//! Dedicated local two-file Compare sidebar panel for GitFleet Studio.

use super::io::file_display_name;
use super::state::{DetachedAction, DetachedViewerState};
use crate::app::Message;
use crate::icons;
use crate::theme::{self, Palette, type_scale};
use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Element, Length, window};
use std::path::Path;

// ── Sidebar 3: Clean Dedicated Compare Panel ─────────────────────────────────

pub(crate) fn render_compare_sidebar<'a>(
    win_id: window::Id,
    v: &'a DetachedViewerState,
    p: Palette,
) -> Element<'a, Message> {
    let slot_card = |label: &'static str, path: Option<&Path>, act: DetachedAction| {
        let fname = path
            .map(file_display_name)
            .unwrap_or_else(|| "Not selected".into());
        let full = path
            .map(|fp| fp.display().to_string())
            .unwrap_or_else(|| "Pick any local code, image, or CSV file".into());

        container(
            column![
                text(label).size(type_scale::BADGE).color(p.text_muted),
                theme::clipped_line(
                    text(fname)
                        .size(type_scale::CAPTION)
                        .color(if path.is_some() {
                            p.text_primary
                        } else {
                            p.text_muted
                        })
                        .wrapping(text::Wrapping::None),
                    Length::Fill,
                ),
                theme::clipped_line(
                    text(full)
                        .size(type_scale::BADGE)
                        .color(p.text_muted)
                        .wrapping(text::Wrapping::None),
                    Length::Fill,
                ),
                button(
                    text("Choose File…")
                        .size(type_scale::CAPTION)
                        .wrapping(text::Wrapping::None),
                )
                .padding([4, 10])
                .style(theme::subtle_btn(p))
                .on_press(Message::Detached(win_id, act)),
            ]
            .spacing(5)
            .width(Length::Fill),
        )
        .padding(10)
        .width(Length::Fill)
        .style(theme::section_panel_box(p))
    };

    let both_set = v.compare_left.is_some() && v.compare_right.is_some();
    let swap_btn = {
        let b = button(
            row![
                icons::swap_arrows(12.0, p.text_secondary),
                text("Swap")
                    .size(type_scale::CAPTION)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .padding([5, 10]);
        if both_set {
            b.style(theme::subtle_btn(p))
                .on_press(Message::Detached(win_id, DetachedAction::SwapCompareFiles))
        } else {
            b.style(theme::vscode_icon_btn(p, false))
        }
    };

    let reload_btn = {
        let b = button(
            row![
                icons::refresh(12.0, p.text_secondary),
                text("Reload")
                    .size(type_scale::CAPTION)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        )
        .padding([5, 10]);
        if both_set {
            b.style(theme::subtle_btn(p))
                .on_press(Message::Detached(win_id, DetachedAction::ReloadFiles))
        } else {
            b.style(theme::vscode_icon_btn(p, false))
        }
    };

    container(
        column![
            text("Compare Files")
                .size(type_scale::CAPTION)
                .color(p.text_primary),
            text("Compare two local text, image, or CSV files side by side.")
                .size(type_scale::BADGE)
                .color(p.text_muted),
            slot_card(
                "Left (Before)",
                v.compare_left.as_deref(),
                DetachedAction::PickLeftFile,
            ),
            slot_card(
                "Right (After)",
                v.compare_right.as_deref(),
                DetachedAction::PickRightFile,
            ),
            row![swap_btn, reload_btn]
                .spacing(6)
                .align_y(Alignment::Center),
        ]
        .spacing(10)
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .padding(10)
    .width(Length::Fixed(264.0))
    .height(Length::Fill)
    .style(theme::sidebar_box(p))
    .into()
}
