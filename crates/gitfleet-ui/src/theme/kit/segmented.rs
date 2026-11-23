//! Unified `SegmentedControl` primitive replacing inconsistent pill/toggle buttons.
#![allow(dead_code)]

use crate::theme::fonts::{CODE_FONT, UI_FONT, UI_FONT_MEDIUM};
use crate::theme::palette::Palette;
use crate::theme::tokens::{radius, space, type_scale};
use iced::widget::{button, container, row, text};
use iced::{Alignment, Background, Border, Color, Element};

pub struct SegmentItem<'a, V> {
    pub value: V,
    pub label: &'a str,
    pub icon: Option<Element<'a, ()>>,
    pub count: Option<usize>,
}

impl<'a, V> SegmentItem<'a, V> {
    pub fn new(value: V, label: &'a str) -> Self {
        Self {
            value,
            label,
            icon: None,
            count: None,
        }
    }

    pub fn count(mut self, count: usize) -> Self {
        self.count = Some(count);
        self
    }
}

pub fn segmented_control<'a, M, V>(
    p: Palette,
    items: impl IntoIterator<Item = SegmentItem<'a, V>>,
    active: V,
    on_select: impl Fn(V) -> M + 'a,
) -> Element<'a, M>
where
    M: Clone + 'a,
    V: Copy + PartialEq + 'a,
{
    let mut track = row![].spacing(2).align_y(Alignment::Center);

    for item in items {
        let is_active = item.value == active;
        let mut inner = row![].spacing(space::XXS).align_y(Alignment::Center);
        inner = inner.push(
            text(item.label)
                .size(type_scale::CAPTION)
                .font(if is_active { UI_FONT_MEDIUM } else { UI_FONT }),
        );
        if let Some(cnt) = item.count {
            inner = inner.push(
                text(cnt.to_string())
                    .size(type_scale::BADGE)
                    .font(CODE_FONT)
                    .color(if is_active { p.accent_fg } else { p.text_muted }),
            );
        }

        let seg_btn = button(inner)
            .padding([3, 9])
            .on_press(on_select(item.value))
            .style(move |_, status| {
                let hovered = matches!(status, button::Status::Hovered);
                let pressed = matches!(status, button::Status::Pressed);
                let bg = if is_active {
                    p.surface_card
                } else if pressed {
                    p.fill_active
                } else if hovered {
                    p.fill_hover
                } else {
                    Color::TRANSPARENT
                };
                let fg = if is_active || hovered {
                    p.text_primary
                } else {
                    p.text_muted
                };
                let border_color = if is_active {
                    p.border_default
                } else {
                    Color::TRANSPARENT
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    text_color: fg,
                    border: Border {
                        color: border_color,
                        width: if is_active { 1.0 } else { 0.0 },
                        radius: (radius::CONTROL - 1.0).into(),
                    },
                    ..Default::default()
                }
            });

        track = track.push(seg_btn);
    }

    container(track)
        .padding(2)
        .style(move |_| container::Style {
            background: Some(Background::Color(p.surface_input)),
            border: Border {
                color: p.border_subtle,
                width: 1.0,
                radius: radius::CONTROL.into(),
            },
            ..Default::default()
        })
        .into()
}
