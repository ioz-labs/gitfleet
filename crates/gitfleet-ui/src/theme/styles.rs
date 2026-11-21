use crate::theme::Palette;
use iced::widget::{button, container, rule, scrollable, text_editor, text_input};
use iced::{Background, Border, Color, Shadow, Theme, Vector};
pub fn app_bg(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.surface_app)),
        text_color: Some(p.text_primary),
        ..Default::default()
    }
}

pub fn titlebar_box(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.surface_titlebar)),
        text_color: Some(p.text_primary),
        ..Default::default()
    }
}

pub fn toolbar_box(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.surface_toolbar)),
        text_color: Some(p.text_primary),
        border: Border {
            color: p.border_subtle,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

pub fn sidebar_box(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.surface_sidebar)),
        text_color: Some(p.text_primary),
        border: Border {
            color: p.border_subtle,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

pub fn panel_box(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.surface_panel)),
        text_color: Some(p.text_primary),
        border: Border {
            color: p.border_subtle,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

pub fn card_box(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.surface_card)),
        text_color: Some(p.text_primary),
        border: Border {
            color: p.border_default,
            width: 1.0,
            radius: crate::theme::radius::MD.into(),
        },
        ..Default::default()
    }
}

pub fn section_panel_box(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.surface_panel)),
        text_color: Some(p.text_primary),
        border: Border {
            color: p.border_subtle,
            width: 1.0,
            radius: crate::theme::radius::MD.into(),
        },
        ..Default::default()
    }
}

pub fn subtle_rule(p: Palette) -> impl Fn(&Theme) -> rule::Style {
    move |_| rule::Style {
        color: p.border_subtle,
        width: 1,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
    }
}

pub fn section_header_box(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.surface_titlebar)),
        text_color: Some(p.text_secondary),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

pub fn table_col_header_box(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.surface_sidebar)),
        text_color: Some(p.text_muted),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

pub fn modal_box(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.surface_card)),
        text_color: Some(p.text_primary),
        border: Border {
            color: p.border_strong,
            width: 1.0,
            radius: crate::theme::radius::LG.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.52),
            offset: Vector::new(0.0, 12.0),
            blur_radius: 32.0,
        },
    }
}

pub fn tooltip_box(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.surface_titlebar)),
        text_color: Some(p.text_primary),
        border: Border {
            color: p.border_strong,
            width: 1.0,
            radius: 5.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: Vector::new(0.0, 3.0),
            blur_radius: 10.0,
        },
    }
}

pub fn context_menu_box(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.surface_panel)),
        text_color: Some(p.text_primary),
        border: Border {
            color: p.border_strong,
            width: 1.0,
            radius: 7.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.48),
            offset: Vector::new(0.0, 6.0),
            blur_radius: 18.0,
        },
    }
}

pub fn context_menu_item(
    p: Palette,
    danger: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered => {
                if danger {
                    p.danger_bg
                } else {
                    p.fill_selected
                }
            }
            button::Status::Pressed => p.fill_active,
            _ => Color::TRANSPARENT,
        };
        let fg = if danger {
            p.danger_fg
        } else {
            p.text_primary
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: fg,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        }
    }
}

pub fn danger_btn(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Active => Color::from_rgb(0.80, 0.22, 0.24),
            button::Status::Hovered => Color::from_rgb(0.88, 0.27, 0.29),
            button::Status::Pressed => Color::from_rgb(0.68, 0.17, 0.19),
            button::Status::Disabled => p.border_default,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: Color::WHITE,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 5.0.into(),
            },
            ..Default::default()
        }
    }
}

pub fn backdrop_box(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::from_rgba(0.02, 0.04, 0.06, 0.58))),
        ..Default::default()
    }
}

pub fn pill_box(bg: Color, fg: Color, border: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(bg)),
        text_color: Some(fg),
        border: Border {
            color: border,
            width: 1.0,
            radius: 999.0.into(),
        },
        ..Default::default()
    }
}

pub fn dot_box(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(color)),
        border: Border {
            color,
            width: 0.0,
            radius: 999.0.into(),
        },
        ..Default::default()
    }
}

pub fn avatar_box(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(color)),
        text_color: Some(Color::WHITE),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    }
}

/// Computes SmartGit-style 2-letter uppercase author initials and a deterministic vibrant badge color.
pub fn author_avatar_info(author: &str) -> (String, Color) {
    const AVATAR_PALETTE: [Color; 8] = [
        Color::from_rgb(0.659, 0.333, 0.788), // SmartGit Orchid Purple (#a855c9)
        Color::from_rgb(0.196, 0.494, 0.808), // Ocean Blue
        Color::from_rgb(0.165, 0.620, 0.451), // Emerald
        Color::from_rgb(0.804, 0.478, 0.165), // Amber Orange
        Color::from_rgb(0.780, 0.290, 0.369), // Rose Crimson
        Color::from_rgb(0.149, 0.608, 0.678), // Teal
        Color::from_rgb(0.412, 0.376, 0.827), // Indigo
        Color::from_rgb(0.514, 0.639, 0.216), // Olive Green
    ];

    let words: Vec<&str> = author.split_whitespace().collect();
    let initials = match words.len() {
        0 => "??".to_string(),
        1 => words[0]
            .chars()
            .take(2)
            .collect::<String>()
            .to_uppercase(),
        _ => {
            let first = words[0].chars().next().unwrap_or('?');
            let last = words[words.len() - 1].chars().next().unwrap_or('?');
            format!("{first}{last}").to_uppercase()
        }
    };

    let mut hash: usize = 5381;
    for b in author.to_lowercase().bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(b as usize);
    }
    let color = AVATAR_PALETTE[hash % AVATAR_PALETTE.len()];
    (initials, color)
}

pub fn diff_line_box(bg: Color, fg: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(bg)),
        text_color: Some(fg),
        ..Default::default()
    }
}

pub fn toolbar_btn(p: Palette, active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Active => {
                if active {
                    p.fill_selected
                } else {
                    Color::TRANSPARENT
                }
            }
            button::Status::Hovered => p.fill_hover,
            button::Status::Pressed => p.fill_active,
            button::Status::Disabled => Color::TRANSPARENT,
        };
        let border_color = if active {
            p.accent_primary
        } else if matches!(status, button::Status::Hovered) {
            p.border_default
        } else {
            Color::TRANSPARENT
        };
        let text_color = match status {
            button::Status::Disabled => p.text_muted,
            _ if active => p.accent_fg,
            _ => p.text_primary,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color,
            border: Border {
                color: border_color,
                width: 1.0,
                radius: 5.0.into(),
            },
            ..Default::default()
        }
    }
}

pub fn primary_btn(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Active => p.accent_primary,
            button::Status::Hovered => p.accent_hover,
            button::Status::Pressed => p.fill_active,
            button::Status::Disabled => p.border_default,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: Color::WHITE,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 5.0.into(),
            },
            ..Default::default()
        }
    }
}

pub fn subtle_btn(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Active => p.surface_card,
            button::Status::Hovered => p.fill_hover,
            button::Status::Pressed => p.fill_active,
            button::Status::Disabled => p.surface_panel,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: match status {
                button::Status::Disabled => p.text_muted,
                _ => p.text_secondary,
            },
            border: Border {
                color: p.border_default,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        }
    }
}

pub fn repo_row_btn(p: Palette, selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
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
                color: if selected {
                    p.accent_primary
                } else {
                    Color::TRANSPARENT
                },
                width: if selected { 1.0 } else { 0.0 },
                radius: 5.0.into(),
            },
            ..Default::default()
        }
    }
}

#[allow(dead_code)]
pub fn tab_btn(p: Palette, active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Active => {
                if active {
                    p.fill_selected
                } else {
                    Color::TRANSPARENT
                }
            }
            button::Status::Hovered => p.fill_hover,
            button::Status::Pressed => p.fill_active,
            button::Status::Disabled => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: if active { p.accent_fg } else { p.text_secondary },
            border: Border {
                color: if active {
                    p.accent_primary
                } else {
                    Color::TRANSPARENT
                },
                width: if active { 1.0 } else { 0.0 },
                radius: 5.0.into(),
            },
            ..Default::default()
        }
    }
}

pub fn filter_pill_btn(p: Palette, active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = if active {
            p.fill_selected
        } else if matches!(status, button::Status::Hovered) {
            p.fill_hover
        } else {
            p.surface_panel
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: if active { p.accent_fg } else { p.text_muted },
            border: Border {
                color: if active {
                    p.accent_primary
                } else {
                    p.border_subtle
                },
                width: 1.0,
                radius: 999.0.into(),
            },
            ..Default::default()
        }
    }
}

pub fn dark_input(p: Palette) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, status| {
        let border_color = match status {
            text_input::Status::Focused => p.accent_primary,
            text_input::Status::Hovered => p.border_strong,
            _ => p.border_default,
        };
        text_input::Style {
            background: Background::Color(p.surface_input),
            border: Border {
                color: border_color,
                width: 1.0,
                radius: 5.0.into(),
            },
            icon: p.text_muted,
            placeholder: p.text_muted,
            value: p.text_primary,
            selection: p.fill_active,
        }
    }
}

pub fn dark_editor(p: Palette) -> impl Fn(&Theme, text_editor::Status) -> text_editor::Style {
    move |_, status| {
        let border_color = match status {
            text_editor::Status::Focused => p.accent_primary,
            text_editor::Status::Hovered => p.border_strong,
            _ => p.border_subtle,
        };
        text_editor::Style {
            background: Background::Color(p.surface_input),
            border: Border {
                color: border_color,
                width: 1.0,
                radius: 4.0.into(),
            },
            icon: p.text_muted,
            placeholder: p.text_muted,
            value: p.text_primary,
            selection: p.fill_active,
        }
    }
}

pub fn slim_vertical_direction() -> scrollable::Direction {
    scrollable::Direction::Vertical(
        scrollable::Scrollbar::new()
            .width(5)
            .scroller_width(5)
            .margin(1)
            .spacing(2),
    )
}

pub fn slim_horizontal_direction() -> scrollable::Direction {
    scrollable::Direction::Horizontal(
        scrollable::Scrollbar::new()
            .width(4)
            .scroller_width(4)
            .margin(1)
            .spacing(2),
    )
}

pub fn slim_both_direction() -> scrollable::Direction {
    scrollable::Direction::Both {
        vertical: scrollable::Scrollbar::new()
            .width(6)
            .scroller_width(6)
            .margin(1),
        horizontal: scrollable::Scrollbar::new()
            .width(6)
            .scroller_width(6)
            .margin(1),
    }
}

pub fn slim_scrollable(p: Palette) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
    move |_, status| {
        let (v_color, h_color) = match status {
            scrollable::Status::Active => (p.border_strong, p.border_strong),
            scrollable::Status::Hovered {
                is_horizontal_scrollbar_hovered,
                is_vertical_scrollbar_hovered,
            } => (
                if is_vertical_scrollbar_hovered {
                    p.text_muted
                } else {
                    p.border_strong
                },
                if is_horizontal_scrollbar_hovered {
                    p.text_muted
                } else {
                    p.border_strong
                },
            ),
            scrollable::Status::Dragged {
                is_horizontal_scrollbar_dragged,
                is_vertical_scrollbar_dragged,
            } => (
                if is_vertical_scrollbar_dragged {
                    p.accent_primary
                } else {
                    p.border_strong
                },
                if is_horizontal_scrollbar_dragged {
                    p.accent_primary
                } else {
                    p.border_strong
                },
            ),
        };

        let make_rail = |color: Color| scrollable::Rail {
            background: None,
            border: Border::default(),
            scroller: scrollable::Scroller {
                color,
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 999.0.into(),
                },
            },
        };

        scrollable::Style {
            container: container::Style::default(),
            vertical_rail: make_rail(v_color),
            horizontal_rail: make_rail(h_color),
            gap: None,
        }
    }
}
