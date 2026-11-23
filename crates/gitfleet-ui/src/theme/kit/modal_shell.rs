//! Standardized `ModalShell` (`8px` radius, hairline header/footer dividers, `Esc` kbd hint, backdrop).
#![allow(dead_code)]

use crate::icons;
use crate::theme::fonts::UI_FONT_SEMIBOLD;
use crate::theme::kit::button::{ButtonSize, ButtonVariant, UiButton};
use crate::theme::kit::tree_row::kbd_pill;
use crate::theme::palette::Palette;
use crate::theme::styles::{backdrop_box, modal_box, slim_scrollable, slim_vertical_direction, subtle_rule};
use crate::theme::tokens::{icon_size, layout, pad, space, type_scale};
use iced::widget::{Space, column, container, horizontal_rule, mouse_area, opaque, row, scrollable, text};
use iced::{Alignment, Element, Length};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModalSize {
    Sm,
    Md,
    Lg,
    Xl,
    Custom(f32),
}

impl ModalSize {
    pub const fn width(self) -> f32 {
        match self {
            Self::Sm => layout::MODAL_SM,
            Self::Md => layout::MODAL_MD,
            Self::Lg => layout::MODAL_LG,
            Self::Xl => layout::MODAL_XL,
            Self::Custom(w) => w,
        }
    }
}

pub struct ModalShell<'a, M> {
    p: Palette,
    title: &'a str,
    subtitle: Option<&'a str>,
    icon: Option<Element<'a, M>>,
    size: ModalSize,
    max_height: f32,
    scrollable_body: bool,
    body: Element<'a, M>,
    footer: Option<Element<'a, M>>,
    on_close: Option<M>,
}

impl<'a, M: Clone + 'a> ModalShell<'a, M> {
    pub fn new(p: Palette, title: &'a str, body: impl Into<Element<'a, M>>) -> Self {
        Self {
            p,
            title,
            subtitle: None,
            icon: None,
            size: ModalSize::Md,
            max_height: layout::MODAL_MAX_HEIGHT,
            scrollable_body: true,
            body: body.into(),
            footer: None,
            on_close: None,
        }
    }

    pub fn subtitle(mut self, subtitle: &'a str) -> Self {
        self.subtitle = Some(subtitle);
        self
    }

    pub fn icon(mut self, icon: impl Into<Element<'a, M>>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn size(mut self, size: ModalSize) -> Self {
        self.size = size;
        self
    }

    pub fn max_height(mut self, max_height: f32) -> Self {
        self.max_height = max_height;
        self
    }

    pub fn scrollable_body(mut self, scrollable_body: bool) -> Self {
        self.scrollable_body = scrollable_body;
        self
    }

    pub fn footer(mut self, footer: impl Into<Element<'a, M>>) -> Self {
        self.footer = Some(footer.into());
        self
    }

    pub fn on_close(mut self, on_close: M) -> Self {
        self.on_close = Some(on_close);
        self
    }

    pub fn into_element(self) -> Element<'a, M> {
        let p = self.p;
        let mut title_col = column![
            text(self.title)
                .size(type_scale::HEADING)
                .font(UI_FONT_SEMIBOLD)
                .color(p.text_primary)
        ]
        .spacing(2);
        if let Some(sub) = self.subtitle {
            title_col = title_col.push(text(sub).size(type_scale::CAPTION).color(p.text_muted));
        }

        let mut header_row = row![].spacing(space::SM).align_y(Alignment::Center);
        if let Some(ic) = self.icon {
            header_row = header_row.push(ic);
        }
        header_row = header_row.push(title_col.width(Length::Fill));
        header_row = header_row.push(kbd_pill(p, "Esc"));
        if let Some(close_msg) = self.on_close.clone() {
            header_row = header_row.push(
                UiButton::icon_only(p, icons::close_x(icon_size::SM, p.text_secondary))
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::IconSm)
                    .on_press(close_msg),
            );
        }

        let header = container(header_row)
            .padding([12, pad::MODAL])
            .width(Length::Fill);

        let body_el: Element<'a, M> = if self.scrollable_body {
            scrollable(
                container(self.body)
                    .padding([14, pad::MODAL])
                    .width(Length::Fill),
            )
            .direction(slim_vertical_direction())
            .style(slim_scrollable(p))
            .into()
        } else {
            container(self.body)
                .padding([14, pad::MODAL])
                .width(Length::Fill)
                .into()
        };

        let mut card_col = column![header, horizontal_rule(1).style(subtle_rule(p)), body_el]
            .spacing(0)
            .width(Length::Fill);

        if let Some(ft) = self.footer {
            card_col = card_col
                .push(horizontal_rule(1).style(subtle_rule(p)))
                .push(
                    container(
                        row![Space::with_width(Length::Fill), ft]
                            .spacing(space::SM)
                            .align_y(Alignment::Center),
                    )
                    .padding([12, pad::MODAL])
                    .width(Length::Fill),
                );
        }

        let card = opaque(
            container(card_col)
                .width(Length::Fixed(self.size.width()))
                .max_height(self.max_height)
                .style(modal_box(p)),
        );

        let centered = container(card)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(backdrop_box);

        if let Some(close_msg) = self.on_close {
            mouse_area(centered).on_press(close_msg).into()
        } else {
            centered.into()
        }
    }
}

impl<'a, M: Clone + 'a> From<ModalShell<'a, M>> for Element<'a, M> {
    fn from(shell: ModalShell<'a, M>) -> Self {
        shell.into_element()
    }
}
