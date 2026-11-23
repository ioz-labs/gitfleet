//! Crisp horizontal `UnderlineTabs` bar with `2px` accent bottom indicator.
#![allow(dead_code)]

use crate::theme::components::bottom_underline;
use crate::theme::fonts::{CODE_FONT, UI_FONT, UI_FONT_MEDIUM};
use crate::theme::palette::Palette;
use crate::theme::tokens::{radius, space, type_scale};
use iced::widget::{button, container, row, text};
use iced::{Alignment, Background, Border, Color, Element};

pub struct UnderlineTab<'a, V> {
    pub value: V,
    pub label: &'a str,
    pub icon: Option<Element<'a, ()>>,
    pub count: Option<usize>,
    pub alert_color: Option<Color>,
}

impl<'a, V> UnderlineTab<'a, V> {
    pub fn new(value: V, label: &'a str) -> Self {
        Self {
            value,
            label,
            icon: None,
            count: None,
            alert_color: None,
        }
    }

    pub fn count(mut self, count: usize) -> Self {
        self.count = Some(count);
        self
    }

    pub fn alert(mut self, color: Color) -> Self {
        self.alert_color = Some(color);
        self
    }
}

pub fn underline_tabs<'a, M, V>(
    p: Palette,
    tabs: impl IntoIterator<Item = UnderlineTab<'a, V>>,
    active: V,
    on_select: impl Fn(V) -> M + 'a,
) -> Element<'a, M>
where
    M: Clone + 'a,
    V: Copy + PartialEq + 'a,
{
    let mut strip = row![].spacing(space::XS).align_y(Alignment::Center);

    for tab in tabs {
        let is_active = tab.value == active;
        let mut inner = row![].spacing(space::XXS).align_y(Alignment::Center);
        inner = inner.push(
            text(tab.label)
                .size(type_scale::CAPTION)
                .font(if is_active { UI_FONT_MEDIUM } else { UI_FONT }),
        );
        if let Some(cnt) = tab.count
            && cnt > 0
        {
            let badge_fg = tab.alert_color.unwrap_or(if is_active {
                p.accent_fg
            } else {
                p.text_secondary
            });
            let badge_bg = if let Some(ac) = tab.alert_color {
                Color { a: 0.16, ..ac }
            } else if is_active {
                p.fill_selected
            } else {
                p.surface_card
            };
            inner = inner.push(
                container(
                    text(cnt.to_string())
                        .size(type_scale::BADGE)
                        .font(CODE_FONT)
                        .color(badge_fg),
                )
                .padding([1, 5])
                .style(move |_| container::Style {
                    background: Some(Background::Color(badge_bg)),
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: radius::BADGE.into(),
                    },
                    ..Default::default()
                }),
            );
        }

        let btn = button(inner)
            .padding([6, 8])
            .on_press(on_select(tab.value))
            .style(move |_, status| {
                let hovered = matches!(status, button::Status::Hovered);
                let fg = if is_active || hovered {
                    p.text_primary
                } else {
                    p.text_muted
                };
                let bg = if hovered && !is_active {
                    p.fill_hover
                } else {
                    Color::TRANSPARENT
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    text_color: fg,
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: radius::BADGE.into(),
                    },
                    ..Default::default()
                }
            });

        let indicator = if is_active {
            p.focus_ring
        } else {
            Color::TRANSPARENT
        };
        strip = strip.push(bottom_underline(btn, indicator, 2.0));
    }

    strip.into()
}
