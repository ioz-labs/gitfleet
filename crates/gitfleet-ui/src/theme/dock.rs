use crate::theme::Palette;
use iced::widget::{button, container};
use iced::{Background, Border, Color, Theme};
// ── VS Code-Style Bottom Dock & Status Bar ───────────────────────────────────

pub fn vscode_dock_box(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.surface_sidebar)),
        text_color: Some(p.text_primary),
        ..Default::default()
    }
}

pub fn vscode_dock_header(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.surface_titlebar)),
        text_color: Some(p.text_primary),
        ..Default::default()
    }
}

pub fn dock_subtoolbar_box(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.surface_toolbar)),
        text_color: Some(p.text_primary),
        ..Default::default()
    }
}

pub fn vscode_resize_handle(p: Palette, dragging: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(if dragging {
            p.accent_primary
        } else {
            p.border_default
        })),
        ..Default::default()
    }
}

pub fn pane_splitter(p: Palette, dragging: bool) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(if dragging {
            p.accent_primary
        } else {
            p.surface_app
        })),
        ..Default::default()
    }
}

pub fn vscode_dock_tab(p: Palette, active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let text_color = if active {
            p.text_primary
        } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            p.text_primary
        } else {
            p.text_muted
        };
        button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            text_color,
            border: Border::default(),
            ..Default::default()
        }
    }
}

pub fn vscode_icon_btn(p: Palette, active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered => p.fill_hover,
            button::Status::Pressed => p.fill_active,
            button::Status::Active if active => p.fill_selected,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: if active { p.accent_fg } else { p.text_secondary },
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        }
    }
}

pub fn vscode_status_bar(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.surface_titlebar)),
        text_color: Some(p.text_secondary),
        border: Border::default(),
        ..Default::default()
    }
}

pub fn vscode_status_item(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered => p.fill_hover,
            button::Status::Pressed => p.fill_active,
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: p.text_secondary,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        }
    }
}

pub fn vscode_status_badge(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered => p.accent_hover,
            button::Status::Pressed => p.fill_active,
            _ => p.accent_primary,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: Color::WHITE,
            border: Border::default(),
            ..Default::default()
        }
    }
}
