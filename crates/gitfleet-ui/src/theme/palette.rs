use iced::{Color, Theme};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub surface_app: Color,
    pub surface_titlebar: Color,
    pub surface_toolbar: Color,
    pub surface_sidebar: Color,
    pub surface_panel: Color,
    pub surface_card: Color,
    pub surface_input: Color,

    pub border_subtle: Color,
    pub border_default: Color,
    pub border_strong: Color,

    pub fill_selected: Color,
    pub fill_hover: Color,
    pub fill_active: Color,

    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_muted: Color,

    pub accent_primary: Color,
    pub accent_hover: Color,
    pub accent_fg: Color,

    pub success_fg: Color,
    pub success_bg: Color,

    pub warning_fg: Color,
    pub warning_bg: Color,

    pub danger_fg: Color,
    pub danger_bg: Color,

    pub violet_fg: Color,
}

pub const DARK: Palette = Palette {
    surface_app: Color::from_rgb(0.067, 0.086, 0.106),      // #11161b
    surface_titlebar: Color::from_rgb(0.059, 0.078, 0.098), // #0f1419
    surface_toolbar: Color::from_rgb(0.078, 0.110, 0.133),  // #141c22
    surface_sidebar: Color::from_rgb(0.071, 0.094, 0.118),  // #12181e
    surface_panel: Color::from_rgb(0.086, 0.118, 0.145),    // #161e25
    surface_card: Color::from_rgb(0.102, 0.141, 0.176),     // #1a242d
    surface_input: Color::from_rgb(0.063, 0.090, 0.114),    // #10171d

    border_subtle: Color::from_rgb(0.110, 0.169, 0.212),    // #1c2b36
    border_default: Color::from_rgb(0.141, 0.192, 0.231),   // #24313b
    border_strong: Color::from_rgb(0.192, 0.251, 0.298),    // #31404c

    fill_selected: Color::from_rgb(0.082, 0.141, 0.200),    // #152433
    fill_hover: Color::from_rgb(0.102, 0.165, 0.220),       // #1a2a38
    fill_active: Color::from_rgb(0.125, 0.224, 0.298),      // #20394c

    text_primary: Color::from_rgb(0.910, 0.929, 0.949),     // #e8edf2
    text_secondary: Color::from_rgb(0.718, 0.784, 0.839),   // #b7c8d6
    text_muted: Color::from_rgb(0.498, 0.576, 0.635),       // #7f93a2

    accent_primary: Color::from_rgb(0.157, 0.471, 0.816),   // #2878d0
    accent_hover: Color::from_rgb(0.224, 0.549, 0.902),     // #398ce6
    accent_fg: Color::from_rgb(0.557, 0.780, 1.000),        // #8ec7ff

    success_fg: Color::from_rgb(0.561, 0.839, 0.659),       // #8fd6a8
    success_bg: Color::from_rgb(0.102, 0.184, 0.141),       // #1a2f24

    warning_fg: Color::from_rgb(0.941, 0.831, 0.541),       // #f0d48a
    warning_bg: Color::from_rgb(0.165, 0.141, 0.078),       // #2a2414

    danger_fg: Color::from_rgb(1.000, 0.608, 0.588),        // #ff9b96
    danger_bg: Color::from_rgb(0.165, 0.090, 0.090),        // #2a1717

    violet_fg: Color::from_rgb(0.769, 0.710, 0.992),        // #c4b5fd
};

pub const LIGHT: Palette = Palette {
    surface_app: Color::from_rgb(0.945, 0.957, 0.969),      // #f1f4f7
    surface_titlebar: Color::from_rgb(0.925, 0.941, 0.957), // #ecf0f4
    surface_toolbar: Color::from_rgb(0.965, 0.973, 0.980),  // #f6f8fa
    surface_sidebar: Color::from_rgb(0.949, 0.961, 0.973),  // #f2f5f8
    surface_panel: Color::from_rgb(0.988, 0.992, 0.996),    // #fcfdfe
    surface_card: Color::from_rgb(1.000, 1.000, 1.000),     // #ffffff
    surface_input: Color::from_rgb(1.000, 1.000, 1.000),    // #ffffff

    border_subtle: Color::from_rgb(0.847, 0.871, 0.890),    // #d8dee3
    border_default: Color::from_rgb(0.796, 0.827, 0.855),   // #cbd3da
    border_strong: Color::from_rgb(0.690, 0.737, 0.780),    // #b0bcc7

    fill_selected: Color::from_rgb(0.875, 0.933, 0.988),    // #dfeefc
    fill_hover: Color::from_rgb(0.910, 0.941, 0.969),       // #e8f0f7
    fill_active: Color::from_rgb(0.831, 0.902, 0.969),      // #d4e6f7

    text_primary: Color::from_rgb(0.102, 0.141, 0.176),     // #1a242d
    text_secondary: Color::from_rgb(0.255, 0.314, 0.365),   // #41505d
    text_muted: Color::from_rgb(0.420, 0.486, 0.545),       // #6b7c8b

    accent_primary: Color::from_rgb(0.133, 0.427, 0.761),   // #226dc2
    accent_hover: Color::from_rgb(0.180, 0.494, 0.839),     // #2e7ed6
    accent_fg: Color::from_rgb(0.094, 0.349, 0.651),        // #1859a6

    success_fg: Color::from_rgb(0.114, 0.478, 0.255),       // #1d7a41
    success_bg: Color::from_rgb(0.886, 0.965, 0.914),       // #e2f6e9

    warning_fg: Color::from_rgb(0.600, 0.392, 0.043),       // #99640b
    warning_bg: Color::from_rgb(0.996, 0.953, 0.847),       // #fef3d8

    danger_fg: Color::from_rgb(0.753, 0.176, 0.165),        // #c02d2a
    danger_bg: Color::from_rgb(0.992, 0.906, 0.902),        // #fde7e6

    violet_fg: Color::from_rgb(0.451, 0.255, 0.780),        // #7341c7
};

impl ThemeMode {
    pub fn palette(self) -> Palette {
        match self {
            Self::Dark => DARK,
            Self::Light => LIGHT,
        }
    }

    pub fn iced_theme(self) -> Theme {
        match self {
            Self::Dark => Theme::Dark,
            Self::Light => Theme::Light,
        }
    }
}
