//! In-file Find & Replace toolbar inside GitFleet Studio's active tab.

use super::state::{DetachedAction, DetachedViewMode, DetachedViewerState, StudioTab};
use crate::app::Message;
use crate::icons;
use crate::theme::{self, Palette, type_scale};
use crate::views::{slim_scroll, tip, truncate_chars};
use iced::widget::{Space, button, column, container, row, text, text_input, tooltip};
use iced::{Alignment, Element, Length, window};

// ── In-File Find & Replace Bar ───────────────────────────────────────────────

pub(crate) fn render_in_file_find_bar<'a>(
    win_id: window::Id,
    v: &'a DetachedViewerState,
    tab: &'a StudioTab,
    p: Palette,
) -> Element<'a, Message> {
    let can_replace = tab.is_editable() && tab.mode == DetachedViewMode::Edit;
    let chev: Element<'a, Message> = if v.in_file_show_replace && can_replace {
        icons::chevron_down(11.0, p.text_secondary).into()
    } else {
        icons::chevron_right(11.0, p.text_secondary).into()
    };

    let opt_btn = |label: &'static str,
                   active: bool,
                   hint: &'static str,
                   act: DetachedAction|
     -> Element<'a, Message> {
        tip(
            button(
                text(label)
                    .size(type_scale::BADGE)
                    .font(theme::CODE_FONT)
                    .color(if active { p.accent_fg } else { p.text_muted })
                    .wrapping(text::Wrapping::None),
            )
            .padding([3, 6])
            .style(theme::filter_pill_btn(p, active))
            .on_press(Message::Detached(win_id, act)),
            hint,
            tooltip::Position::Bottom,
            p,
        )
    };

    let counter_label = if let Some(err) = &v.in_file_error {
        truncate_chars(err, 22)
    } else if v.in_file_query.is_empty() {
        String::new()
    } else if v.in_file_matches.is_empty() {
        "No results".into()
    } else {
        let cur = v.in_file_active_idx.map(|i| i + 1).unwrap_or(1);
        format!("{cur} / {}", v.in_file_matches.len())
    };
    let counter_color = if v.in_file_error.is_some()
        || (!v.in_file_query.is_empty() && v.in_file_matches.is_empty())
    {
        p.danger_fg
    } else {
        p.text_muted
    };

    let mut row1 = row![].spacing(4).align_y(Alignment::Center).width(Length::Fill);

    if can_replace {
        row1 = row1.push(tip(
            button(chev)
                .padding([4, 4])
                .style(theme::vscode_icon_btn(p, false))
                .on_press(Message::Detached(
                    win_id,
                    DetachedAction::ToggleInFileReplace,
                )),
            "Toggle Replace (Ctrl+H)",
            tooltip::Position::Bottom,
            p,
        ));
    }

    row1 = row1
        .push(
            text_input("Find in file…", &v.in_file_query)
                .id(v.in_file_input_id.clone())
                .on_input(move |s| Message::Detached(win_id, DetachedAction::InFileQueryChanged(s)))
                .on_submit(Message::Detached(win_id, DetachedAction::InFileJump(1)))
                .padding([4, 8])
                .size(type_scale::CAPTION)
                .width(Length::FillPortion(3))
                .style(theme::dark_input(p)),
        )
        .push(opt_btn(
            "Aa",
            v.in_file_options.case_sensitive,
            "Match Case",
            DetachedAction::ToggleInFileCase,
        ))
        .push(opt_btn(
            "ab",
            v.in_file_options.whole_word,
            "Match Whole Word",
            DetachedAction::ToggleInFileWholeWord,
        ))
        .push(opt_btn(
            ".*",
            v.in_file_options.regex,
            "Use Regular Expression",
            DetachedAction::ToggleInFileRegex,
        ))
        .push(
            container(
                text(counter_label)
                    .size(type_scale::BADGE)
                    .font(theme::CODE_FONT)
                    .color(counter_color)
                    .wrapping(text::Wrapping::None),
            )
            .padding([0, 6]),
        )
        .push(tip(
            button(icons::chevron_up(12.0, p.text_secondary))
                .padding([3, 6])
                .style(theme::subtle_btn(p))
                .on_press(Message::Detached(win_id, DetachedAction::InFileJump(-1))),
            "Previous Match",
            tooltip::Position::Bottom,
            p,
        ))
        .push(tip(
            button(icons::chevron_down(12.0, p.text_secondary))
                .padding([3, 6])
                .style(theme::subtle_btn(p))
                .on_press(Message::Detached(win_id, DetachedAction::InFileJump(1))),
            "Next Match (Enter)",
            tooltip::Position::Bottom,
            p,
        ))
        .push(tip(
            button(icons::close_x(11.0, p.text_muted))
                .padding([3, 5])
                .style(theme::vscode_icon_btn(p, false))
                .on_press(Message::Detached(win_id, DetachedAction::CloseInFileFind)),
            "Close (Esc)",
            tooltip::Position::Bottom,
            p,
        ));

    let mut col = column![row1].spacing(5).width(Length::Fill);

    if can_replace && v.in_file_show_replace {
        let has_matches = !v.in_file_matches.is_empty();
        let rep_base = button(
            text("Replace")
                .size(type_scale::CAPTION)
                .wrapping(text::Wrapping::None),
        )
        .padding([3, 9]);
        let rep_btn = if has_matches {
            rep_base.style(theme::subtle_btn(p)).on_press(Message::Detached(
                win_id,
                DetachedAction::InFileReplaceCurrent,
            ))
        } else {
            rep_base.style(theme::vscode_icon_btn(p, false))
        };

        let rep_all_base = button(
            text("Replace All")
                .size(type_scale::CAPTION)
                .wrapping(text::Wrapping::None),
        )
        .padding([3, 9]);
        let rep_all_btn = if has_matches {
            rep_all_base
                .style(theme::primary_btn(p))
                .on_press(Message::Detached(win_id, DetachedAction::InFileReplaceAll))
        } else {
            rep_all_base.style(theme::vscode_icon_btn(p, false))
        };

        col = col.push(
            row![
                Space::with_width(Length::Fixed(22.0)),
                text_input("Replace in file…", &v.in_file_replace)
                    .id(v.in_file_replace_input_id.clone())
                    .on_input(move |s| {
                        Message::Detached(win_id, DetachedAction::InFileReplaceChanged(s))
                    })
                    .on_submit(Message::Detached(
                        win_id,
                        DetachedAction::InFileReplaceCurrent,
                    ))
                    .padding([4, 8])
                    .size(type_scale::CAPTION)
                    .width(Length::FillPortion(3))
                    .style(theme::dark_input(p)),
                rep_btn,
                rep_all_btn,
                Space::with_width(Length::FillPortion(1)),
            ]
            .spacing(5)
            .align_y(Alignment::Center)
            .width(Length::Fill),
        );
    }

    if tab.mode == DetachedViewMode::Preview && !v.in_file_matches.is_empty() {
        let mut jump_col = column![].spacing(1).width(Length::Fill);
        for (idx, m) in v.in_file_matches.iter().take(60).enumerate() {
            let active = v.in_file_active_idx == Some(idx);
            jump_col = jump_col.push(
                button(
                    row![
                        text(format!("L{:<4}", m.line_index + 1))
                            .size(type_scale::BADGE)
                            .font(theme::CODE_FONT)
                            .color(if active { p.accent_fg } else { p.text_muted })
                            .wrapping(text::Wrapping::None),
                        theme::clipped_line(
                            text(&m.line_preview)
                                .size(type_scale::CAPTION)
                                .font(theme::CODE_FONT)
                                .color(p.text_secondary)
                                .wrapping(text::Wrapping::None),
                            Length::Fill,
                        ),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([2, 8])
                .width(Length::Fill)
                .style(theme::repo_row_btn(p, active))
                .on_press(Message::Detached(
                    win_id,
                    DetachedAction::InFileJumpIndex(idx),
                )),
            );
        }
        col = col.push(
            container(slim_scroll(jump_col, p).height(Length::Fixed(92.0)))
                .padding([2, 0])
                .width(Length::Fill),
        );
    }

    container(
        container(col)
            .padding([6, 10])
            .width(Length::Fill)
            .style(theme::studio_find_box(p)),
    )
    .padding([6, 10])
    .width(Length::Fill)
    .style(theme::studio_breadcrumb_bar(p))
    .into()
}
