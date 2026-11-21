use crate::app::{GitFleetApp, Message};
use crate::icons;
use crate::theme::{self, Palette, icon_size, pad, space, type_scale};
use gitfleet_security::IgnoreLayer;
use iced::widget::{Space, button, column, container, row, text, text_editor};
use iced::{Alignment, Color, Element, Length};

impl GitFleetApp {
    pub(crate) fn view_ignore_drawer(&self, p: Palette) -> Element<'_, Message> {
        let toolbar = theme::dock_toolbar(
            p,
            row![
                icons::layers(icon_size::SM, p.text_muted),
                button(text(".gitignore (Root)").size(type_scale::BODY))
                    .padding(pad::BTN_SM)
                    .style(theme::filter_pill_btn(
                        p,
                        self.ignore_layer == IgnoreLayer::RootGitignore
                    ))
                    .on_press(Message::SelectIgnoreLayer(IgnoreLayer::RootGitignore)),
                button(text(".git/info/exclude (Personal)").size(type_scale::BODY))
                    .padding(pad::BTN_SM)
                    .style(theme::filter_pill_btn(
                        p,
                        self.ignore_layer == IgnoreLayer::PersonalExclude
                    ))
                    .on_press(Message::SelectIgnoreLayer(IgnoreLayer::PersonalExclude)),
                button(text("Global Excludes").size(type_scale::BODY))
                    .padding(pad::BTN_SM)
                    .style(theme::filter_pill_btn(
                        p,
                        self.ignore_layer == IgnoreLayer::GlobalExclude
                    ))
                    .on_press(Message::SelectIgnoreLayer(IgnoreLayer::GlobalExclude)),
                Space::with_width(Length::Fill),
                button(
                    row![
                        icons::save(icon_size::XS, Color::WHITE),
                        text("Save Ignore Layer").size(type_scale::BODY),
                    ]
                    .spacing(space::XS)
                    .align_y(Alignment::Center),
                )
                .padding(pad::BTN_SM)
                .style(theme::primary_btn(p))
                .on_press(Message::SaveIgnoreLayer),
            ]
            .spacing(space::SM)
            .align_y(Alignment::Center),
        );

        let editor = container(
            text_editor(&self.ignore_editor)
                .on_action(Message::IgnoreEditorAction)
                .font(theme::CODE_FONT)
                .size(type_scale::CODE)
                .height(Length::Fill)
                .style(theme::dark_editor(p)),
        )
        .padding(pad::DOCK_BODY)
        .width(Length::Fill)
        .height(Length::Fill);

        column![toolbar, editor]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
