use crate::app::{GitFleetApp, Message};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use gitfleet_git::GitFlowKind;
use iced::widget::{Space, button, column, row, text, text_input};
use iced::{Element, Length};

impl GitFleetApp {
    pub(crate) fn view_git_flow_modal(&self, p: Palette) -> Element<'_, Message> {
        let body: Element<'_, Message> = if !self.git_flow_state.initialized {
            theme::section_card(
                p,
                "Initialize Git Flow",
                Some("Configure your main and develop branches for Feature / Release / Hotfix workflows."),
                None,
                column![
                    column![
                        theme::field_label(p, "Main branch"),
                        text_input("main", &self.git_flow_main_input)
                            .on_input(Message::GitFlowMainChanged)
                            .padding(pad::INPUT)
                            .size(type_scale::BODY)
                            .width(Length::Fill)
                            .style(theme::dark_input(p)),
                    ]
                    .spacing(space::XXS),
                    column![
                        theme::field_label(p, "Develop branch"),
                        text_input("develop", &self.git_flow_develop_input)
                            .on_input(Message::GitFlowDevelopChanged)
                            .padding(pad::INPUT)
                            .size(type_scale::BODY)
                            .width(Length::Fill)
                            .style(theme::dark_input(p)),
                    ]
                    .spacing(space::XXS),
                    row![
                        Space::with_width(Length::Fill),
                        button(text("Initialize Git Flow").size(type_scale::BODY))
                            .padding(pad::BTN_MD)
                            .style(theme::primary_btn(p))
                            .on_press(Message::InitGitFlow),
                    ],
                ]
                .spacing(space::MD),
            )
        } else {
            let kinds = [
                ("Feature", GitFlowKind::Feature),
                ("Release", GitFlowKind::Release),
                ("Hotfix", GitFlowKind::Hotfix),
            ];
            let mut kind_row = row![].spacing(space::XS);
            for (lbl, k) in kinds {
                let active =
                    std::mem::discriminant(&self.git_flow_kind) == std::mem::discriminant(&k);
                kind_row = kind_row.push(
                    button(text(lbl).size(type_scale::BODY))
                        .padding(pad::BTN_SM)
                        .style(theme::filter_pill_btn(p, active))
                        .on_press(Message::GitFlowKindChanged(k)),
                );
            }

            let main_b = self
                .git_flow_state
                .main_branch
                .as_deref()
                .unwrap_or("main");
            let dev_b = self
                .git_flow_state
                .develop_branch
                .as_deref()
                .unwrap_or("develop");

            theme::section_card(
                p,
                "Branch Workflow",
                None,
                Some(theme::badge(
                    format!("{main_b}  /  {dev_b}"),
                    p.fill_selected,
                    p.accent_fg,
                    p.border_subtle,
                    true,
                )),
                column![
                    kind_row,
                    column![
                        theme::field_label(p, "Branch or release name"),
                        text_input(
                            "Name (e.g. login-oauth or 1.2.0)…",
                            &self.git_flow_name_input
                        )
                        .on_input(Message::GitFlowNameChanged)
                        .on_submit(Message::StartGitFlow)
                        .padding(pad::INPUT)
                        .size(type_scale::BODY)
                        .width(Length::Fill)
                        .style(theme::dark_input(p)),
                    ]
                    .spacing(space::XXS),
                    row![
                        Space::with_width(Length::Fill),
                        button(text("Finish & Merge").size(type_scale::BODY))
                            .padding(pad::BTN_MD)
                            .style(theme::subtle_btn(p))
                            .on_press(Message::FinishGitFlow),
                        button(text("Start Branch").size(type_scale::BODY))
                            .padding(pad::BTN_MD)
                            .style(theme::primary_btn(p))
                            .on_press(Message::StartGitFlow),
                    ]
                    .spacing(space::SM),
                ]
                .spacing(space::MD),
            )
        };

        theme::modal_shell(
            p,
            layout::MODAL_MD,
            icons::branch(icon_size::MD, p.accent_fg),
            "Git Flow",
            None,
            body,
            Message::CloseOverlays,
        )
    }
}
