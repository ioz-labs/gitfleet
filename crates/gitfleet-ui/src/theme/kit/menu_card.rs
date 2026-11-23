//! Crisp `MenuCard` context menu builder with aligned `16px` icon column and right-aligned shortcuts.
#![allow(dead_code)]

use crate::icons;
use crate::theme::fonts::{CODE_FONT, UI_FONT, UI_FONT_MEDIUM};
use crate::theme::palette::Palette;
use crate::theme::styles::{context_menu_box, context_menu_item, subtle_rule};
use crate::theme::tokens::{space, type_scale};
use iced::widget::{Space, button, column, container, horizontal_rule, opaque, row, text};
use iced::{Alignment, Element, Length};

pub enum MenuEntry<'a, M> {
    Header(&'a str),
    Separator,
    Action {
        icon: Option<Element<'a, M>>,
        label: String,
        shortcut: Option<&'a str>,
        danger: bool,
        on_press: Option<M>,
    },
    Submenu {
        icon: Option<Element<'a, M>>,
        label: String,
        open: bool,
        on_trigger: M,
    },
}

impl<'a, M> MenuEntry<'a, M> {
    pub fn action(label: impl Into<String>, on_press: M) -> Self {
        Self::Action {
            icon: None,
            label: label.into(),
            shortcut: None,
            danger: false,
            on_press: Some(on_press),
        }
    }

    pub fn icon(mut self, icon: impl Into<Element<'a, M>>) -> Self {
        match &mut self {
            Self::Action { icon: slot, .. } | Self::Submenu { icon: slot, .. } => {
                *slot = Some(icon.into());
            }
            _ => {}
        }
        self
    }

    pub fn shortcut(mut self, shortcut: &'a str) -> Self {
        if let Self::Action { shortcut: slot, .. } = &mut self {
            *slot = Some(shortcut);
        }
        self
    }

    pub fn danger(mut self) -> Self {
        if let Self::Action { danger, .. } = &mut self {
            *danger = true;
        }
        self
    }
}

pub fn menu_card<'a, M: Clone + 'a>(
    p: Palette,
    width: f32,
    entries: impl IntoIterator<Item = MenuEntry<'a, M>>,
) -> Element<'a, M> {
    let mut col = column![].spacing(1).width(Length::Fill);

    for entry in entries {
        match entry {
            MenuEntry::Header(title) => {
                col = col.push(
                    container(
                        text(title)
                            .size(type_scale::BADGE)
                            .font(UI_FONT_MEDIUM)
                            .color(p.text_muted),
                    )
                    .padding([5, 10]),
                );
            }
            MenuEntry::Separator => {
                col = col.push(
                    container(horizontal_rule(1).style(subtle_rule(p)))
                        .padding([3, 6])
                        .width(Length::Fill),
                );
            }
            MenuEntry::Action {
                icon,
                label,
                shortcut,
                danger,
                on_press,
            } => {
                let icon_slot: Element<'a, M> = match icon {
                    Some(ic) => container(ic)
                        .width(Length::Fixed(16.0))
                        .center_x(Length::Fixed(16.0))
                        .into(),
                    None => Space::new(16.0, 14.0).into(),
                };
                let mut r = row![
                    icon_slot,
                    text(label)
                        .size(type_scale::BODY)
                        .font(UI_FONT)
                        .width(Length::Fill),
                ]
                .spacing(space::SM)
                .align_y(Alignment::Center);

                if let Some(sc) = shortcut {
                    r = r.push(
                        text(sc)
                            .size(type_scale::BADGE)
                            .font(CODE_FONT)
                            .color(if danger { p.danger_fg } else { p.text_muted }),
                    );
                }

                let btn = button(r)
                    .padding([5, 10])
                    .width(Length::Fill)
                    .style(context_menu_item(p, danger));

                if let Some(msg) = on_press {
                    col = col.push(btn.on_press(msg));
                } else {
                    col = col.push(btn);
                }
            }
            MenuEntry::Submenu {
                icon,
                label,
                open,
                on_trigger,
            } => {
                let icon_slot: Element<'a, M> = match icon {
                    Some(ic) => container(ic)
                        .width(Length::Fixed(16.0))
                        .center_x(Length::Fixed(16.0))
                        .into(),
                    None => Space::new(16.0, 14.0).into(),
                };
                let r = row![
                    icon_slot,
                    text(label)
                        .size(type_scale::BODY)
                        .font(if open { UI_FONT_MEDIUM } else { UI_FONT })
                        .width(Length::Fill),
                    icons::chevron_right(12.0, if open { p.accent_fg } else { p.text_muted }),
                ]
                .spacing(space::SM)
                .align_y(Alignment::Center);

                let btn = button(r)
                    .padding([5, 10])
                    .width(Length::Fill)
                    .on_press(on_trigger)
                    .style(context_menu_item(p, false));
                col = col.push(btn);
            }
        }
    }

    opaque(
        container(col)
            .padding(4)
            .width(Length::Fixed(width))
            .style(context_menu_box(p)),
    )
}
