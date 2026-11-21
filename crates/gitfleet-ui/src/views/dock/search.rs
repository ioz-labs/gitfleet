use crate::app::{GitFleetApp, Message};
use crate::icons;
use crate::theme::{self, Palette, icon_size, pad, space, type_scale};
use crate::views::slim_scroll_h;
use iced::widget::{button, checkbox, column, container, row, text, text_input};
use iced::{Alignment, Color, Element, Length};

impl GitFleetApp {
    pub(crate) fn view_search_drawer(&self, p: Palette) -> Element<'_, Message> {
        let toolbar = theme::dock_toolbar(
            p,
            row![
                icons::search(icon_size::SM, p.text_muted),
                text_input(
                    "Search across ALL workspace repositories… (Enter)",
                    &self.search_query
                )
                .on_input(Message::SearchQueryChanged)
                .on_submit(Message::RunFleetSearch)
                .padding(pad::INPUT_COMPACT)
                .size(type_scale::BODY)
                .width(Length::Fill)
                .style(theme::dark_input(p)),
                checkbox("Case", self.search_case_sensitive)
                    .on_toggle(Message::SearchCaseToggled)
                    .size(15)
                    .text_size(type_scale::BODY),
                checkbox("Regex", self.search_regex)
                    .on_toggle(Message::SearchRegexToggled)
                    .size(15)
                    .text_size(type_scale::BODY),
                if self.search_running {
                    Element::from(
                        button(
                            text("Cancel")
                                .size(type_scale::BODY)
                                .color(p.danger_fg),
                        )
                        .padding(pad::BTN_SM)
                        .style(theme::subtle_btn(p))
                        .on_press(Message::CancelFleetSearch),
                    )
                } else {
                    Element::from(
                        button(
                            row![
                                icons::search(icon_size::XS, Color::WHITE),
                                text("Search Fleet").size(type_scale::BODY),
                            ]
                            .spacing(space::XS)
                            .align_y(Alignment::Center),
                        )
                        .padding(pad::BTN_SM)
                        .style(theme::primary_btn(p))
                        .on_press(Message::RunFleetSearch),
                    )
                }
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        );

        let body: Element<'_, Message> =
            if self.search_results.is_empty() && !self.search_running {
                theme::empty_state(
                    p,
                    icons::search(icon_size::LG, p.text_muted),
                    "Search across your entire workspace",
                    "Type a keyword or regex above and press Enter to search every repository at once.",
                    None,
                )
            } else {
                let mut list = column![].spacing(space::XXS);
                let mut max_line_chars: usize = 48;
                for m in self.search_results.iter().take(200) {
                    let preview = theme::sanitize_code_line(&m.line_preview);
                    max_line_chars = max_line_chars.max(
                        m.repo_name.chars().count()
                            + m.file_path.chars().count()
                            + preview.chars().count()
                            + 8,
                    );
                    list = list.push(
                        button(
                            row![
                                theme::badge(
                                    m.repo_name.clone(),
                                    p.fill_selected,
                                    p.accent_fg,
                                    p.border_default,
                                    false
                                ),
                                text(format!("{}:{}", m.file_path, m.line_number))
                                    .size(type_scale::CODE)
                                    .font(theme::CODE_FONT)
                                    .color(p.warning_fg),
                                text(preview)
                                    .size(type_scale::CODE)
                                    .font(theme::CODE_FONT)
                                    .color(p.text_secondary)
                                    .wrapping(text::Wrapping::None),
                            ]
                            .spacing(space::SM)
                            .align_y(Alignment::Center),
                        )
                        .width(Length::Fill)
                        .padding(pad::ROW)
                        .style(theme::repo_row_btn(p, false))
                        .on_press(Message::JumpToSearchMatch(
                            m.repo_id.clone(),
                            m.file_path.clone(),
                        )),
                    );
                }
                let min_w = (max_line_chars as f32 * 7.4 + 90.0).clamp(460.0, 2400.0);
                container(slim_scroll_h(list, min_w, p))
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
