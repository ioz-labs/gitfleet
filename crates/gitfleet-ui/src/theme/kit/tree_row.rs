//! `TreeRow` focus discipline, `2px` active-pane focus bar, and monospace kbd/status pills.
#![allow(dead_code)]

use crate::theme::fonts::CODE_FONT;
use crate::theme::palette::Palette;
use crate::theme::tokens::{layout, radius, type_scale};
use iced::widget::{Space, button, container, text};
use iced::{Background, Border, Color, Element, Length, Theme};

/// Calm tree-row button style: eliminates noisy simultaneous `1px` blue outlines across panes.
/// Selected rows use `fill_selected`; active keyboard focus is communicated via `focus_accent_bar`.
pub fn tree_row_btn_style(
    p: Palette,
    selected: bool,
    _pane_focused: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Active => {
                if selected {
                    p.fill_selected
                } else {
                    Color::TRANSPARENT
                }
            }
            button::Status::Hovered => {
                if selected {
                    p.fill_selected
                } else {
                    p.fill_hover
                }
            }
            button::Status::Pressed => p.fill_active,
            button::Status::Disabled => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: p.text_primary,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: radius::CONTROL.into(),
            },
            ..Default::default()
        }
    }
}

/// Renders a crisp `2px` vertical left bar: bright `focus_ring` when the owning pane has keyboard
/// focus, subtle `border_strong` when selected in an unfocused pane, or invisible otherwise.
pub fn focus_accent_bar<'a, M: 'a>(
    p: Palette,
    selected: bool,
    pane_focused: bool,
    height: f32,
) -> Element<'a, M> {
    let color = if selected && pane_focused {
        p.focus_ring
    } else if selected {
        p.border_strong
    } else {
        Color::TRANSPARENT
    };
    container(Space::new(layout::FOCUS_BAR_WIDTH, height))
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 1.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// Renders a crisp `2px` pane-header top/left indicator showing which pane holds keyboard focus.
pub fn pane_focus_dot<'a, M: 'a>(p: Palette, focused: bool) -> Element<'a, M> {
    let color = if focused {
        p.focus_ring
    } else {
        Color::TRANSPARENT
    };
    container(Space::new(6.0, 6.0))
        .style(move |_| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 999.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// Monospace keyboard shortcut chip (`Ctrl+K`, `Alt+1`, `F2`, `Esc`) using `JetBrains Mono`.
pub fn kbd_pill<'a, M: 'a>(p: Palette, shortcut: &'a str) -> Element<'a, M> {
    container(
        text(shortcut)
            .size(type_scale::BADGE)
            .font(CODE_FONT)
            .color(p.text_muted),
    )
    .padding([1, 5])
    .width(Length::Shrink)
    .style(move |_| container::Style {
        background: Some(Background::Color(p.surface_input)),
        border: Border {
            color: p.border_subtle,
            width: 1.0,
            radius: radius::BADGE.into(),
        },
        ..Default::default()
    })
    .into()
}

/// Crisp `3px`-radius status pill badge (`M`, `A`, `+3`, `Verified`).
pub fn status_pill<'a, M: 'a>(
    label: impl text::IntoFragment<'a>,
    fg: Color,
    bg: Color,
) -> Element<'a, M> {
    container(text(label).size(type_scale::BADGE).font(CODE_FONT).color(fg))
        .padding([1, 5])
        .style(move |_| container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: Color { a: 0.28, ..fg },
                width: 1.0,
                radius: radius::BADGE.into(),
            },
            ..Default::default()
        })
        .into()
}
