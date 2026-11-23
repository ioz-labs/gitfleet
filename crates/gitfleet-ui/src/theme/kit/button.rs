//! Variant-driven button primitive (`shadcn/ui`-style `UiButton` for pure-Rust `iced`).
#![allow(dead_code)]

use crate::theme::fonts::{CODE_FONT, UI_FONT, UI_FONT_MEDIUM};
use crate::theme::palette::{Palette, on_accent_ink};
use crate::theme::tokens::{pad, radius, space, type_scale};
use iced::widget::{button, container, row, text};
use iced::{Alignment, Background, Border, Color, Element, Length};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Ghost,
    Subtle,
    Destructive,
    OutlineAccent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonSize {
    Xs,
    Sm,
    Md,
    IconSm,
    IconMd,
}

pub fn variant_button_style(
    p: Palette,
    variant: ButtonVariant,
    active: bool,
) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_, status| {
        let disabled = matches!(status, button::Status::Disabled);
        let hovered = matches!(status, button::Status::Hovered);
        let pressed = matches!(status, button::Status::Pressed);

        let (bg, fg, border_color, border_w) = match variant {
            ButtonVariant::Primary => {
                let ink = on_accent_ink(p.accent_primary);
                if disabled {
                    (p.border_subtle, p.text_muted, Color::TRANSPARENT, 0.0)
                } else if pressed {
                    (p.fill_active, Color::WHITE, p.accent_primary, 1.0)
                } else if hovered {
                    (p.accent_hover, on_accent_ink(p.accent_hover), Color::TRANSPARENT, 0.0)
                } else {
                    (p.accent_primary, ink, Color::TRANSPARENT, 0.0)
                }
            }
            ButtonVariant::Secondary => {
                if disabled {
                    (p.surface_panel, p.text_muted, p.border_subtle, 1.0)
                } else if pressed {
                    (p.fill_active, p.text_primary, p.accent_primary, 1.0)
                } else if active {
                    (p.fill_selected, p.accent_fg, p.accent_primary, 1.0)
                } else if hovered {
                    (p.fill_hover, p.text_primary, p.border_strong, 1.0)
                } else {
                    (p.surface_card, p.text_primary, p.border_default, 1.0)
                }
            }
            ButtonVariant::Subtle => {
                if disabled {
                    (Color::TRANSPARENT, p.text_muted, p.border_subtle, 1.0)
                } else if pressed {
                    (p.fill_active, p.text_primary, p.border_strong, 1.0)
                } else if active {
                    (p.fill_selected, p.accent_fg, p.border_default, 1.0)
                } else if hovered {
                    (p.fill_hover, p.text_primary, p.border_default, 1.0)
                } else {
                    (p.surface_panel, p.text_secondary, p.border_subtle, 1.0)
                }
            }
            ButtonVariant::Ghost => {
                if disabled {
                    (Color::TRANSPARENT, p.text_muted, Color::TRANSPARENT, 0.0)
                } else if pressed {
                    (p.fill_active, p.text_primary, Color::TRANSPARENT, 0.0)
                } else if active {
                    (p.fill_selected, p.accent_fg, Color::TRANSPARENT, 0.0)
                } else if hovered {
                    (p.fill_hover, p.text_primary, Color::TRANSPARENT, 0.0)
                } else {
                    (Color::TRANSPARENT, p.text_secondary, Color::TRANSPARENT, 0.0)
                }
            }
            ButtonVariant::Destructive => {
                let base = if p.is_dark {
                    Color::from_rgb(0.82, 0.20, 0.27)
                } else {
                    Color::from_rgb(0.80, 0.14, 0.22)
                };
                let hov = if p.is_dark {
                    Color::from_rgb(0.90, 0.26, 0.33)
                } else {
                    Color::from_rgb(0.72, 0.11, 0.18)
                };
                if disabled {
                    (p.border_subtle, p.text_muted, Color::TRANSPARENT, 0.0)
                } else if pressed {
                    (p.danger_bg, p.danger_fg, p.danger_fg, 1.0)
                } else if hovered {
                    (hov, Color::WHITE, Color::TRANSPARENT, 0.0)
                } else {
                    (base, Color::WHITE, Color::TRANSPARENT, 0.0)
                }
            }
            ButtonVariant::OutlineAccent => {
                if disabled {
                    (Color::TRANSPARENT, p.text_muted, p.border_subtle, 1.0)
                } else if pressed {
                    (p.fill_active, p.accent_fg, p.accent_primary, 1.0)
                } else if hovered {
                    (p.fill_selected, p.accent_fg, p.accent_hover, 1.0)
                } else {
                    (
                        Color {
                            a: 0.12,
                            ..p.accent_primary
                        },
                        p.accent_fg,
                        p.accent_primary,
                        1.0,
                    )
                }
            }
        };

        button::Style {
            background: Some(Background::Color(bg)),
            text_color: fg,
            border: Border {
                color: border_color,
                width: border_w,
                radius: radius::CONTROL.into(),
            },
            ..Default::default()
        }
    }
}

use std::borrow::Cow;

pub struct UiButton<'a, M> {
    p: Palette,
    label: Option<Cow<'a, str>>,
    icon: Option<Element<'a, M>>,
    shortcut: Option<&'a str>,
    badge: Option<String>,
    variant: ButtonVariant,
    size: ButtonSize,
    active: bool,
    width: Length,
    on_press: Option<M>,
}

impl<'a, M: Clone + 'a> UiButton<'a, M> {
    pub fn new(p: Palette, label: impl Into<Cow<'a, str>>) -> Self {
        Self {
            p,
            label: Some(label.into()),
            icon: None,
            shortcut: None,
            badge: None,
            variant: ButtonVariant::Secondary,
            size: ButtonSize::Sm,
            active: false,
            width: Length::Shrink,
            on_press: None,
        }
    }

    pub fn icon_only(p: Palette, icon: impl Into<Element<'a, M>>) -> Self {
        Self {
            p,
            label: None,
            icon: Some(icon.into()),
            shortcut: None,
            badge: None,
            variant: ButtonVariant::Ghost,
            size: ButtonSize::IconMd,
            active: false,
            width: Length::Shrink,
            on_press: None,
        }
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn icon(mut self, icon: impl Into<Element<'a, M>>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn shortcut(mut self, shortcut: &'a str) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    pub fn badge(mut self, badge: impl Into<String>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    pub fn on_press(mut self, msg: M) -> Self {
        self.on_press = Some(msg);
        self
    }

    pub fn on_press_maybe(mut self, msg: Option<M>) -> Self {
        self.on_press = msg;
        self
    }

    pub fn into_element(self) -> Element<'a, M> {
        let p = self.p;
        let (padding, text_sz) = match self.size {
            ButtonSize::Xs => (pad::BTN_XS, type_scale::CAPTION),
            ButtonSize::Sm => (pad::BTN_SM, type_scale::CAPTION),
            ButtonSize::Md => (pad::BTN_MD, type_scale::BODY),
            ButtonSize::IconSm => (pad::BTN_ICON_SM, type_scale::CAPTION),
            ButtonSize::IconMd => (pad::BTN_ICON, type_scale::BODY),
        };

        let mut content_row = row![].spacing(space::XS).align_y(Alignment::Center);
        if let Some(ic) = self.icon {
            content_row = content_row.push(ic);
        }
        if let Some(lbl) = self.label {
            let font = if matches!(
                self.variant,
                ButtonVariant::Primary | ButtonVariant::Destructive | ButtonVariant::OutlineAccent
            ) || self.active
            {
                UI_FONT_MEDIUM
            } else {
                UI_FONT
            };
            content_row = content_row.push(text(lbl).size(text_sz).font(font));
        }
        if let Some(bdg) = self.badge {
            let pill = container(text(bdg).size(type_scale::BADGE).font(CODE_FONT))
                .padding([1, 5])
                .style(move |_| container::Style {
                    background: Some(Background::Color(Color {
                        a: 0.22,
                        ..p.surface_app
                    })),
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: radius::BADGE.into(),
                    },
                    ..Default::default()
                });
            content_row = content_row.push(pill);
        }
        if let Some(sc) = self.shortcut {
            content_row = content_row.push(
                text(sc)
                    .size(type_scale::BADGE)
                    .font(CODE_FONT)
                    .color(p.text_muted),
            );
        }

        let btn = button(content_row)
            .padding(padding)
            .width(self.width)
            .style(variant_button_style(p, self.variant, self.active));

        if let Some(msg) = self.on_press {
            btn.on_press(msg).into()
        } else {
            btn.into()
        }
    }
}

impl<'a, M: Clone + 'a> From<UiButton<'a, M>> for Element<'a, M> {
    fn from(b: UiButton<'a, M>) -> Self {
        b.into_element()
    }
}
