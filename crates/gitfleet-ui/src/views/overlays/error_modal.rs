use crate::app::{ActionErrorState, GitFleetApp, Message};
use crate::icons;
use crate::theme::{self, Palette, icon_size, layout, pad, space, type_scale};
use crate::views::slim_scroll;
use iced::widget::{Space, button, column, container, horizontal_rule, opaque, row, text};
use iced::{Alignment, Background, Border, Color, Element, Length};

impl GitFleetApp {
    pub(crate) fn view_error_modal(
        &self,
        p: Palette,
        err: &ActionErrorState,
    ) -> Element<'_, Message> {
        let mut header_left = row![
            container(icons::x_circle(icon_size::MD + 2.0, p.danger_fg))
                .padding(7)
                .style(theme::pill_box(
                    p.danger_bg,
                    p.danger_fg,
                    Color {
                        a: 0.7,
                        ..p.danger_fg
                    }
                )),
            column![
                text(err.title.clone())
                    .size(type_scale::TITLE)
                    .color(p.text_primary),
                if let Some(ctx) = &err.context {
                    Element::from(
                        text(ctx.clone())
                            .size(type_scale::CAPTION)
                            .font(theme::CODE_FONT)
                            .color(p.text_muted),
                    )
                } else {
                    Element::from(Space::with_height(Length::Shrink))
                },
            ]
            .spacing(2),
        ]
        .spacing(space::MD)
        .align_y(Alignment::Center);

        header_left = header_left
            .push(Space::with_width(Length::Fill))
            .push(
                button(icons::close_x(icon_size::SM, p.text_secondary))
                    .padding(pad::BTN_ICON)
                    .style(theme::vscode_icon_btn(p, false))
                    .on_press(Message::CloseErrorModal),
            );

        let mut body = column![
            header_left,
            horizontal_rule(1).style(theme::subtle_rule(p)),
            text(err.summary.clone())
                .size(type_scale::BODY)
                .color(p.text_primary),
        ]
        .spacing(space::MD);

        if let Some(detail) = &err.detail
            && !detail.trim().is_empty()
        {
            let detail_scroll = slim_scroll(
                container(
                    text(detail.clone())
                        .size(type_scale::CODE)
                        .font(theme::CODE_FONT)
                        .color(p.danger_fg),
                )
                .padding([10, 12])
                .width(Length::Fill),
                p,
            )
            .height(Length::Shrink);

            let detail_box = container(detail_scroll)
                .width(Length::Fill)
                .max_height(240.0)
                .style(move |_| container::Style {
                    background: Some(Background::Color(p.surface_input)),
                    border: Border {
                        color: Color {
                            a: 0.45,
                            ..p.danger_fg
                        },
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    ..Default::default()
                });

            body = body.push(
                column![
                    theme::field_label(p, "Error Details"),
                    detail_box,
                ]
                .spacing(space::XXS),
            );
        }

        let copy_payload = match (&err.context, &err.detail) {
            (Some(ctx), Some(det)) => {
                format!("{} ({})\n{}\n\n{}", err.title, ctx, err.summary, det)
            }
            (None, Some(det)) => format!("{}\n{}\n\n{}", err.title, err.summary, det),
            (Some(ctx), None) => format!("{} ({})\n{}", err.title, ctx, err.summary),
            (None, None) => format!("{}\n{}", err.title, err.summary),
        };

        let mut footer = row![
            text("Esc to close")
                .size(type_scale::CAPTION)
                .color(p.text_muted),
            Space::with_width(Length::Fill),
        ]
        .spacing(space::SM)
        .align_y(Alignment::Center);

        if err.show_output_shortcut {
            footer = footer.push(
                button(
                    row![
                        icons::terminal(13.0, p.text_secondary),
                        text("View Output").size(type_scale::BODY),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding(pad::BTN_MD)
                .style(theme::subtle_btn(p))
                .on_press(Message::OpenErrorCommandOutput),
            );
        }

        footer = footer
            .push(
                button(
                    row![
                        icons::copy(13.0, p.text_secondary),
                        text("Copy Error").size(type_scale::BODY),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding(pad::BTN_MD)
                .style(theme::subtle_btn(p))
                .on_press(Message::CopyText("error details".into(), copy_payload)),
            )
            .push(
                button(text("Close").size(type_scale::BODY))
                    .padding(pad::BTN_MD)
                    .style(theme::danger_btn(p))
                    .on_press(Message::CloseErrorModal),
            );

        body = body
            .push(horizontal_rule(1).style(theme::subtle_rule(p)))
            .push(footer);

        let modal = opaque(
            container(body)
                .padding(pad::MODAL)
                .width(Length::Fixed(layout::MODAL_MD))
                .max_height(layout::MODAL_MAX_HEIGHT)
                .style(theme::modal_box(p)),
        );

        let centered = container(modal)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(theme::backdrop_box);

        theme::overlay_backdrop(centered, Message::CloseErrorModal)
    }
}
