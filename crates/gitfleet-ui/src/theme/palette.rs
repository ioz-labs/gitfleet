//! Layer 2 Semantic Color Tokens (`SemanticTheme` / `Palette`) and WCAG contrast utilities.
#![allow(dead_code)]

use iced::{Color, Theme};

/// Light vs Dark polarity used by syntax highlighters, markdown viewers, and terminal ANSI ramps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeMode {
    Dark,
    Light,
}

/// Complete semantic token table consumed by all views and UI kit widgets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub is_dark: bool,

    // Surface hierarchy (flat 1px-separated planes)
    pub surface_app: Color,
    pub surface_titlebar: Color,
    pub surface_toolbar: Color,
    pub surface_sidebar: Color,
    pub surface_panel: Color,
    pub surface_card: Color,
    pub surface_input: Color,

    // Hairline borders & focus discipline
    pub border_subtle: Color,
    pub border_default: Color,
    pub border_strong: Color,
    pub focus_ring: Color,
    pub splitter_idle: Color,
    pub splitter_active: Color,

    // Interactive state fills
    pub fill_selected: Color,
    pub fill_hover: Color,
    pub fill_active: Color,

    // Typography hierarchy
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_muted: Color,

    // Brand / primary accent
    pub accent_primary: Color,
    pub accent_hover: Color,
    pub accent_fg: Color,

    // Semantic status colors
    pub success_fg: Color,
    pub success_bg: Color,
    pub warning_fg: Color,
    pub warning_bg: Color,
    pub danger_fg: Color,
    pub danger_bg: Color,
    pub violet_fg: Color,

    // WCAG-compliant Diff viewer alpha surfaces
    pub diff_add_line_bg: Color,
    pub diff_add_word_bg: Color,
    pub diff_del_line_bg: Color,
    pub diff_del_word_bg: Color,
    pub diff_hunk_bg: Color,
    pub diff_gutter_bg: Color,

    // Git file & graph signal tokens
    pub git_modified: Color,
    pub git_added: Color,
    pub git_deleted: Color,
    pub git_renamed: Color,
    pub git_untracked: Color,
    pub git_conflict: Color,
}

/// Alias emphasizing the 3-layer Design System nomenclature.
pub type SemanticTheme = Palette;

/// Const helper converting `0xRRGGBB` hex integers to `iced::Color`.
pub const fn hex_rgb(rgb: u32) -> Color {
    let r = ((rgb >> 16) & 0xFF) as f32 / 255.0;
    let g = ((rgb >> 8) & 0xFF) as f32 / 255.0;
    let b = (rgb & 0xFF) as f32 / 255.0;
    Color::from_rgb(r, g, b)
}

/// Const helper converting `0xRRGGBB` + explicit alpha (`0.0..=1.0`) to `iced::Color`.
pub const fn hex_rgba(rgb: u32, alpha: f32) -> Color {
    let r = ((rgb >> 16) & 0xFF) as f32 / 255.0;
    let g = ((rgb >> 8) & 0xFF) as f32 / 255.0;
    let b = (rgb & 0xFF) as f32 / 255.0;
    Color::from_rgba(r, g, b, alpha)
}

/// Default Dark preset (*Precision Obsidian* — neutral deep slate + cobalt hairline precision).
pub const DARK: Palette = Palette {
    is_dark: true,

    surface_app: hex_rgb(0x0B0E14),
    surface_titlebar: hex_rgb(0x0D1117),
    surface_toolbar: hex_rgb(0x11161F),
    surface_sidebar: hex_rgb(0x0F131B),
    surface_panel: hex_rgb(0x121721),
    surface_card: hex_rgb(0x171E2B),
    surface_input: hex_rgb(0x0C1017),

    border_subtle: hex_rgb(0x1D2432),
    border_default: hex_rgb(0x263042),
    border_strong: hex_rgb(0x36445D),
    focus_ring: hex_rgb(0x3B82F6),
    splitter_idle: hex_rgb(0x1D2432),
    splitter_active: hex_rgb(0x3B82F6),

    fill_selected: hex_rgb(0x17253B),
    fill_hover: hex_rgb(0x1A2230),
    fill_active: hex_rgb(0x20324E),

    text_primary: hex_rgb(0xEDF2F7),
    text_secondary: hex_rgb(0xA9B6C8),
    text_muted: hex_rgb(0x708096),

    accent_primary: hex_rgb(0x2563EB),
    accent_hover: hex_rgb(0x3B82F6),
    accent_fg: hex_rgb(0x93C5FD),

    success_fg: hex_rgb(0x6EE7B7),
    success_bg: hex_rgb(0x112922),
    warning_fg: hex_rgb(0xFCD34D),
    warning_bg: hex_rgb(0x292111),
    danger_fg: hex_rgb(0xFDA4AF),
    danger_bg: hex_rgb(0x2B151B),
    violet_fg: hex_rgb(0xC4B5FD),

    diff_add_line_bg: hex_rgba(0x10B981, 0.13),
    diff_add_word_bg: hex_rgba(0x10B981, 0.32),
    diff_del_line_bg: hex_rgba(0xF43F5E, 0.14),
    diff_del_word_bg: hex_rgba(0xF43F5E, 0.34),
    diff_hunk_bg: hex_rgba(0x3B82F6, 0.12),
    diff_gutter_bg: hex_rgb(0x0E121A),

    git_modified: hex_rgb(0xFBBF24),
    git_added: hex_rgb(0x34D399),
    git_deleted: hex_rgb(0xFB7185),
    git_renamed: hex_rgb(0x60A5FA),
    git_untracked: hex_rgb(0x2DD4BF),
    git_conflict: hex_rgb(0xF43F5E),
};

/// High-contrast Light preset (*Daylight Crisp* — architectural paper + deep slate ink).
pub const LIGHT: Palette = Palette {
    is_dark: false,

    surface_app: hex_rgb(0xF1F5F9),
    surface_titlebar: hex_rgb(0xEEF2F6),
    surface_toolbar: hex_rgb(0xF8FAFC),
    surface_sidebar: hex_rgb(0xF4F7FA),
    surface_panel: hex_rgb(0xFFFFFF),
    surface_card: hex_rgb(0xFFFFFF),
    surface_input: hex_rgb(0xFFFFFF),

    border_subtle: hex_rgb(0xE2E8F0),
    border_default: hex_rgb(0xCBD5E1),
    border_strong: hex_rgb(0x94A3B8),
    focus_ring: hex_rgb(0x2563EB),
    splitter_idle: hex_rgb(0xE2E8F0),
    splitter_active: hex_rgb(0x2563EB),

    fill_selected: hex_rgb(0xE0ECFF),
    fill_hover: hex_rgb(0xEDF2F7),
    fill_active: hex_rgb(0xCFE2FF),

    text_primary: hex_rgb(0x0F172A),
    text_secondary: hex_rgb(0x334155),
    text_muted: hex_rgb(0x64748B),

    accent_primary: hex_rgb(0x2563EB),
    accent_hover: hex_rgb(0x1D4ED8),
    accent_fg: hex_rgb(0x1E40AF),

    success_fg: hex_rgb(0x15803D),
    success_bg: hex_rgb(0xDCFCE7),
    warning_fg: hex_rgb(0xB45309),
    warning_bg: hex_rgb(0xFEF3C7),
    danger_fg: hex_rgb(0xBE123C),
    danger_bg: hex_rgb(0xFFE4E6),
    violet_fg: hex_rgb(0x6D28D9),

    diff_add_line_bg: hex_rgba(0x16A34A, 0.11),
    diff_add_word_bg: hex_rgba(0x16A34A, 0.27),
    diff_del_line_bg: hex_rgba(0xE11D48, 0.10),
    diff_del_word_bg: hex_rgba(0xE11D48, 0.25),
    diff_hunk_bg: hex_rgba(0x2563EB, 0.09),
    diff_gutter_bg: hex_rgb(0xF8FAFC),

    git_modified: hex_rgb(0xD97706),
    git_added: hex_rgb(0x16A34A),
    git_deleted: hex_rgb(0xE11D48),
    git_renamed: hex_rgb(0x2563EB),
    git_untracked: hex_rgb(0x0D9488),
    git_conflict: hex_rgb(0xDC2626),
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

/// Computes WCAG 2.1 relative luminance (`0.0..=1.0`) for an opaque sRGB color.
pub fn relative_luminance(c: Color) -> f32 {
    fn channel(v: f32) -> f32 {
        let v = v.clamp(0.0, 1.0);
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
}

/// Composites a translucent `fg` color onto an opaque `bg` surface.
pub fn composite_over(fg: Color, bg: Color) -> Color {
    let a = fg.a.clamp(0.0, 1.0);
    Color::from_rgb(
        fg.r * a + bg.r * (1.0 - a),
        fg.g * a + bg.g * (1.0 - a),
        fg.b * a + bg.b * (1.0 - a),
    )
}

/// Computes WCAG 2.1 contrast ratio (`1.0..=21.0`) between two opaque sRGB colors.
pub fn contrast_ratio(fg: Color, bg: Color) -> f32 {
    let l1 = relative_luminance(fg);
    let l2 = relative_luminance(bg);
    let (bright, dark) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    (bright + 0.05) / (dark + 0.05)
}

/// Picks crisp readable button label ink (`#0B0E14` vs `#FFFFFF`) for any accent fill.
pub fn on_accent_ink(accent_bg: Color) -> Color {
    if relative_luminance(accent_bg) > 0.40 {
        hex_rgb(0x0B0E14)
    } else {
        Color::WHITE
    }
}
