//! Layer 1 Theme Presets (5 built-in developer themes + Custom JSON Theme serializer/loader).

use crate::theme::palette::{DARK, LIGHT, Palette, ThemeMode, hex_rgb, hex_rgba};
use iced::Color;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{OnceLock, RwLock};

/// GitHub Dark Dimmed preset (`#1C2128` / `#22272E` / `#2D333B` + soft steel borders).
pub const GITHUB_DIMMED: Palette = Palette {
    is_dark: true,

    surface_app: hex_rgb(0x1C2128),
    surface_titlebar: hex_rgb(0x191E24),
    surface_toolbar: hex_rgb(0x22272E),
    surface_sidebar: hex_rgb(0x1E232A),
    surface_panel: hex_rgb(0x22272E),
    surface_card: hex_rgb(0x2D333B),
    surface_input: hex_rgb(0x1C2128),

    border_subtle: hex_rgb(0x30363D),
    border_default: hex_rgb(0x373E47),
    border_strong: hex_rgb(0x444C56),
    focus_ring: hex_rgb(0x539BF5),
    splitter_idle: hex_rgb(0x30363D),
    splitter_active: hex_rgb(0x539BF5),

    fill_selected: hex_rgb(0x26354A),
    fill_hover: hex_rgb(0x293038),
    fill_active: hex_rgb(0x2E425D),

    text_primary: hex_rgb(0xE6EDF3),
    text_secondary: hex_rgb(0xADBAC7),
    text_muted: hex_rgb(0x768390),

    accent_primary: hex_rgb(0x316DCA),
    accent_hover: hex_rgb(0x539BF5),
    accent_fg: hex_rgb(0x6CB6FF),

    success_fg: hex_rgb(0x6BC46D),
    success_bg: hex_rgb(0x1F3126),
    warning_fg: hex_rgb(0xDAA72F),
    warning_bg: hex_rgb(0x342B1B),
    danger_fg: hex_rgb(0xFF938A),
    danger_bg: hex_rgb(0x362123),
    violet_fg: hex_rgb(0xDCBDFB),

    diff_add_line_bg: hex_rgba(0x46954A, 0.16),
    diff_add_word_bg: hex_rgba(0x46954A, 0.36),
    diff_del_line_bg: hex_rgba(0xE5534B, 0.16),
    diff_del_word_bg: hex_rgba(0xE5534B, 0.36),
    diff_hunk_bg: hex_rgba(0x539BF5, 0.14),
    diff_gutter_bg: hex_rgb(0x1E232A),

    git_modified: hex_rgb(0xE3B341),
    git_added: hex_rgb(0x57AB5A),
    git_deleted: hex_rgb(0xF47067),
    git_renamed: hex_rgb(0x6CB6FF),
    git_untracked: hex_rgb(0x39C5CF),
    git_conflict: hex_rgb(0xFF7B72),
};

/// Catppuccin Mocha preset (`#11111B` / `#181825` / `#1E1E2E` + warm pastel accents).
pub const CATPPUCCIN_MOCHA: Palette = Palette {
    is_dark: true,

    surface_app: hex_rgb(0x11111B),
    surface_titlebar: hex_rgb(0x141420),
    surface_toolbar: hex_rgb(0x181825),
    surface_sidebar: hex_rgb(0x181825),
    surface_panel: hex_rgb(0x1E1E2E),
    surface_card: hex_rgb(0x28283D),
    surface_input: hex_rgb(0x141420),

    border_subtle: hex_rgb(0x2A2B3D),
    border_default: hex_rgb(0x313244),
    border_strong: hex_rgb(0x45475A),
    focus_ring: hex_rgb(0x89B4FA),
    splitter_idle: hex_rgb(0x2A2B3D),
    splitter_active: hex_rgb(0x89B4FA),

    fill_selected: hex_rgb(0x28324B),
    fill_hover: hex_rgb(0x26273A),
    fill_active: hex_rgb(0x313E5E),

    text_primary: hex_rgb(0xCDD6F4),
    text_secondary: hex_rgb(0xBAC2DE),
    text_muted: hex_rgb(0x7F849C),

    accent_primary: hex_rgb(0x89B4FA),
    accent_hover: hex_rgb(0xB4BEFE),
    accent_fg: hex_rgb(0x89B4FA),

    success_fg: hex_rgb(0xA6E3A1),
    success_bg: hex_rgb(0x1E2E28),
    warning_fg: hex_rgb(0xF9E2AF),
    warning_bg: hex_rgb(0x302B22),
    danger_fg: hex_rgb(0xF38BA8),
    danger_bg: hex_rgb(0x321F29),
    violet_fg: hex_rgb(0xCBA6F7),

    diff_add_line_bg: hex_rgba(0x2EA043, 0.14),
    diff_add_word_bg: hex_rgba(0x2EA043, 0.32),
    diff_del_line_bg: hex_rgba(0xE64553, 0.14),
    diff_del_word_bg: hex_rgba(0xE64553, 0.32),
    diff_hunk_bg: hex_rgba(0x89B4FA, 0.12),
    diff_gutter_bg: hex_rgb(0x181825),

    git_modified: hex_rgb(0xF9E2AF),
    git_added: hex_rgb(0xA6E3A1),
    git_deleted: hex_rgb(0xF38BA8),
    git_renamed: hex_rgb(0x89B4FA),
    git_untracked: hex_rgb(0x94E2D5),
    git_conflict: hex_rgb(0xEBA0AC),
};

/// Tokyo Night preset (`#16161E` / `#1A1B26` / `#24283B` + electric indigo accents).
pub const TOKYO_NIGHT: Palette = Palette {
    is_dark: true,

    surface_app: hex_rgb(0x13141C),
    surface_titlebar: hex_rgb(0x16161E),
    surface_toolbar: hex_rgb(0x1A1B26),
    surface_sidebar: hex_rgb(0x16161E),
    surface_panel: hex_rgb(0x1A1B26),
    surface_card: hex_rgb(0x24283B),
    surface_input: hex_rgb(0x14151F),

    border_subtle: hex_rgb(0x24283B),
    border_default: hex_rgb(0x2F3549),
    border_strong: hex_rgb(0x414868),
    focus_ring: hex_rgb(0x7AA2F7),
    splitter_idle: hex_rgb(0x24283B),
    splitter_active: hex_rgb(0x7AA2F7),

    fill_selected: hex_rgb(0x24304E),
    fill_hover: hex_rgb(0x222638),
    fill_active: hex_rgb(0x2D3C63),

    text_primary: hex_rgb(0xDCE3FF),
    text_secondary: hex_rgb(0xA9B1D6),
    text_muted: hex_rgb(0x6E7699),

    accent_primary: hex_rgb(0x7AA2F7),
    accent_hover: hex_rgb(0x8FB4FF),
    accent_fg: hex_rgb(0x7DCFFF),

    success_fg: hex_rgb(0x9ECE6A),
    success_bg: hex_rgb(0x1D2B24),
    warning_fg: hex_rgb(0xE0AF68),
    warning_bg: hex_rgb(0x2E271E),
    danger_fg: hex_rgb(0xF7768E),
    danger_bg: hex_rgb(0x311D26),
    violet_fg: hex_rgb(0xBB9AF7),

    diff_add_line_bg: hex_rgba(0x10B981, 0.13),
    diff_add_word_bg: hex_rgba(0x10B981, 0.30),
    diff_del_line_bg: hex_rgba(0xF43F5E, 0.13),
    diff_del_word_bg: hex_rgba(0xF43F5E, 0.30),
    diff_hunk_bg: hex_rgba(0x7AA2F7, 0.12),
    diff_gutter_bg: hex_rgb(0x16161E),

    git_modified: hex_rgb(0xE0AF68),
    git_added: hex_rgb(0x9ECE6A),
    git_deleted: hex_rgb(0xF7768E),
    git_renamed: hex_rgb(0x7AA2F7),
    git_untracked: hex_rgb(0x73DACA),
    git_conflict: hex_rgb(0xDB4B4B),
};

static ACTIVE_CUSTOM_PALETTE: OnceLock<RwLock<Palette>> = OnceLock::new();

fn custom_palette_slot() -> &'static RwLock<Palette> {
    ACTIVE_CUSTOM_PALETTE.get_or_init(|| RwLock::new(DARK))
}

/// Updates the active runtime palette returned by `ThemePreset::Custom.palette()`.
pub fn set_active_custom_palette(p: Palette) {
    if let Ok(mut guard) = custom_palette_slot().write() {
        *guard = p;
    }
}

/// Returns the currently loaded custom palette (falls back to `DARK` if none loaded).
pub fn active_custom_palette() -> Palette {
    custom_palette_slot().read().map(|g| *g).unwrap_or(DARK)
}

/// Selectable built-in or user-loaded JSON theme presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ThemePreset {
    PrecisionObsidian,
    DaylightCrisp,
    GitHubDimmed,
    CatppuccinMocha,
    TokyoNight,
    Custom,
}

impl Default for ThemePreset {
    fn default() -> Self {
        Self::PrecisionObsidian
    }
}

impl ThemePreset {
    pub const ALL_BUILTINS: [Self; 5] = [
        Self::PrecisionObsidian,
        Self::DaylightCrisp,
        Self::GitHubDimmed,
        Self::CatppuccinMocha,
        Self::TokyoNight,
    ];

    pub const fn id(self) -> &'static str {
        match self {
            Self::PrecisionObsidian => "obsidian",
            Self::DaylightCrisp => "daylight",
            Self::GitHubDimmed => "github-dimmed",
            Self::CatppuccinMocha => "catppuccin-mocha",
            Self::TokyoNight => "tokyo-night",
            Self::Custom => "custom",
        }
    }

    pub fn from_id(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "obsidian" | "dark" | "precision-obsidian" => Self::PrecisionObsidian,
            "daylight" | "light" | "daylight-crisp" => Self::DaylightCrisp,
            "github-dimmed" | "github" | "dimmed" => Self::GitHubDimmed,
            "catppuccin-mocha" | "catppuccin" | "mocha" => Self::CatppuccinMocha,
            "tokyo-night" | "tokyonight" => Self::TokyoNight,
            "custom" => Self::Custom,
            _ => Self::PrecisionObsidian,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::PrecisionObsidian => "Precision Obsidian",
            Self::DaylightCrisp => "Daylight Crisp",
            Self::GitHubDimmed => "GitHub Dimmed",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::TokyoNight => "Tokyo Night",
            Self::Custom => "Custom JSON Theme",
        }
    }

    pub const fn subtitle(self) -> &'static str {
        match self {
            Self::PrecisionObsidian => "Neutral deep slate & cobalt hairline contrast",
            Self::DaylightCrisp => "Architectural paper & crisp slate ink",
            Self::GitHubDimmed => "Soft steel dark canvas for long review sessions",
            Self::CatppuccinMocha => "Soothing warm dark with sapphire & lavender",
            Self::TokyoNight => "Deep midnight indigo & electric sky accents",
            Self::Custom => "User-supplied JSON semantic token file",
        }
    }

    pub fn palette(self) -> Palette {
        match self {
            Self::PrecisionObsidian => DARK,
            Self::DaylightCrisp => LIGHT,
            Self::GitHubDimmed => GITHUB_DIMMED,
            Self::CatppuccinMocha => CATPPUCCIN_MOCHA,
            Self::TokyoNight => TOKYO_NIGHT,
            Self::Custom => active_custom_palette(),
        }
    }

    pub fn mode(self) -> ThemeMode {
        if self.palette().is_dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        }
    }

    /// Toggles cleanly between the user's dark theme and `DaylightCrisp` when clicking the quick sun/moon toggle.
    pub fn toggle_polarity(self) -> Self {
        match self {
            Self::DaylightCrisp => Self::PrecisionObsidian,
            _ if !self.palette().is_dark => Self::PrecisionObsidian,
            _ => Self::DaylightCrisp,
        }
    }
}

/// Human-editable JSON schema for custom GitFleet themes (`#RRGGBB` or `#RRGGBBAA`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeDefinition {
    pub name: String,
    #[serde(default = "default_true")]
    pub is_dark: bool,

    pub surface_app: String,
    pub surface_titlebar: String,
    pub surface_toolbar: String,
    pub surface_sidebar: String,
    pub surface_panel: String,
    pub surface_card: String,
    pub surface_input: String,

    pub border_subtle: String,
    pub border_default: String,
    pub border_strong: String,
    #[serde(default)]
    pub focus_ring: Option<String>,

    pub fill_selected: String,
    pub fill_hover: String,
    pub fill_active: String,

    pub text_primary: String,
    pub text_secondary: String,
    pub text_muted: String,

    pub accent_primary: String,
    pub accent_hover: String,
    pub accent_fg: String,

    pub success_fg: String,
    pub success_bg: String,
    pub warning_fg: String,
    pub warning_bg: String,
    pub danger_fg: String,
    pub danger_bg: String,
    pub violet_fg: String,

    #[serde(default)]
    pub diff_add_line_bg: Option<String>,
    #[serde(default)]
    pub diff_add_word_bg: Option<String>,
    #[serde(default)]
    pub diff_del_line_bg: Option<String>,
    #[serde(default)]
    pub diff_del_word_bg: Option<String>,
}

fn default_true() -> bool {
    true
}

pub fn parse_hex_color(raw: &str) -> Result<Color, String> {
    let hex = raw.trim().trim_start_matches('#');
    match hex.len() {
        6 => {
            let v = u32::from_str_radix(hex, 16)
                .map_err(|_| format!("Invalid 6-digit hex color '{raw}'"))?;
            Ok(hex_rgb(v))
        }
        8 => {
            let v = u32::from_str_radix(hex, 16)
                .map_err(|_| format!("Invalid 8-digit hex color '{raw}'"))?;
            let r = ((v >> 24) & 0xFF) as f32 / 255.0;
            let g = ((v >> 16) & 0xFF) as f32 / 255.0;
            let b = ((v >> 8) & 0xFF) as f32 / 255.0;
            let a = (v & 0xFF) as f32 / 255.0;
            Ok(Color::from_rgba(r, g, b, a))
        }
        _ => Err(format!(
            "Expected #RRGGBB or #RRGGBBAA color string, got '{raw}'"
        )),
    }
}

pub fn format_hex_color(c: Color) -> String {
    let r = (c.r.clamp(0.0, 1.0) * 255.0).round() as u8;
    let g = (c.g.clamp(0.0, 1.0) * 255.0).round() as u8;
    let b = (c.b.clamp(0.0, 1.0) * 255.0).round() as u8;
    let a = (c.a.clamp(0.0, 1.0) * 255.0).round() as u8;
    if a == 255 {
        format!("#{r:02X}{g:02X}{b:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

impl ThemeDefinition {
    pub fn from_palette(name: impl Into<String>, p: Palette) -> Self {
        Self {
            name: name.into(),
            is_dark: p.is_dark,
            surface_app: format_hex_color(p.surface_app),
            surface_titlebar: format_hex_color(p.surface_titlebar),
            surface_toolbar: format_hex_color(p.surface_toolbar),
            surface_sidebar: format_hex_color(p.surface_sidebar),
            surface_panel: format_hex_color(p.surface_panel),
            surface_card: format_hex_color(p.surface_card),
            surface_input: format_hex_color(p.surface_input),
            border_subtle: format_hex_color(p.border_subtle),
            border_default: format_hex_color(p.border_default),
            border_strong: format_hex_color(p.border_strong),
            focus_ring: Some(format_hex_color(p.focus_ring)),
            fill_selected: format_hex_color(p.fill_selected),
            fill_hover: format_hex_color(p.fill_hover),
            fill_active: format_hex_color(p.fill_active),
            text_primary: format_hex_color(p.text_primary),
            text_secondary: format_hex_color(p.text_secondary),
            text_muted: format_hex_color(p.text_muted),
            accent_primary: format_hex_color(p.accent_primary),
            accent_hover: format_hex_color(p.accent_hover),
            accent_fg: format_hex_color(p.accent_fg),
            success_fg: format_hex_color(p.success_fg),
            success_bg: format_hex_color(p.success_bg),
            warning_fg: format_hex_color(p.warning_fg),
            warning_bg: format_hex_color(p.warning_bg),
            danger_fg: format_hex_color(p.danger_fg),
            danger_bg: format_hex_color(p.danger_bg),
            violet_fg: format_hex_color(p.violet_fg),
            diff_add_line_bg: Some(format_hex_color(p.diff_add_line_bg)),
            diff_add_word_bg: Some(format_hex_color(p.diff_add_word_bg)),
            diff_del_line_bg: Some(format_hex_color(p.diff_del_line_bg)),
            diff_del_word_bg: Some(format_hex_color(p.diff_del_word_bg)),
        }
    }

    pub fn to_palette(&self) -> Result<Palette, String> {
        let base = if self.is_dark { DARK } else { LIGHT };
        let accent_primary = parse_hex_color(&self.accent_primary)?;
        let border_subtle = parse_hex_color(&self.border_subtle)?;
        let surface_sidebar = parse_hex_color(&self.surface_sidebar)?;
        let focus_ring = match &self.focus_ring {
            Some(s) => parse_hex_color(s)?,
            None => accent_primary,
        };
        let success_fg = parse_hex_color(&self.success_fg)?;
        let warning_fg = parse_hex_color(&self.warning_fg)?;
        let danger_fg = parse_hex_color(&self.danger_fg)?;
        let accent_fg = parse_hex_color(&self.accent_fg)?;

        Ok(Palette {
            is_dark: self.is_dark,
            surface_app: parse_hex_color(&self.surface_app)?,
            surface_titlebar: parse_hex_color(&self.surface_titlebar)?,
            surface_toolbar: parse_hex_color(&self.surface_toolbar)?,
            surface_sidebar,
            surface_panel: parse_hex_color(&self.surface_panel)?,
            surface_card: parse_hex_color(&self.surface_card)?,
            surface_input: parse_hex_color(&self.surface_input)?,
            border_subtle,
            border_default: parse_hex_color(&self.border_default)?,
            border_strong: parse_hex_color(&self.border_strong)?,
            focus_ring,
            splitter_idle: border_subtle,
            splitter_active: focus_ring,
            fill_selected: parse_hex_color(&self.fill_selected)?,
            fill_hover: parse_hex_color(&self.fill_hover)?,
            fill_active: parse_hex_color(&self.fill_active)?,
            text_primary: parse_hex_color(&self.text_primary)?,
            text_secondary: parse_hex_color(&self.text_secondary)?,
            text_muted: parse_hex_color(&self.text_muted)?,
            accent_primary,
            accent_hover: parse_hex_color(&self.accent_hover)?,
            accent_fg,
            success_fg,
            success_bg: parse_hex_color(&self.success_bg)?,
            warning_fg,
            warning_bg: parse_hex_color(&self.warning_bg)?,
            danger_fg,
            danger_bg: parse_hex_color(&self.danger_bg)?,
            violet_fg: parse_hex_color(&self.violet_fg)?,
            diff_add_line_bg: match &self.diff_add_line_bg {
                Some(s) => parse_hex_color(s)?,
                None => base.diff_add_line_bg,
            },
            diff_add_word_bg: match &self.diff_add_word_bg {
                Some(s) => parse_hex_color(s)?,
                None => base.diff_add_word_bg,
            },
            diff_del_line_bg: match &self.diff_del_line_bg {
                Some(s) => parse_hex_color(s)?,
                None => base.diff_del_line_bg,
            },
            diff_del_word_bg: match &self.diff_del_word_bg {
                Some(s) => parse_hex_color(s)?,
                None => base.diff_del_word_bg,
            },
            diff_hunk_bg: Color {
                a: 0.12,
                ..accent_primary
            },
            diff_gutter_bg: surface_sidebar,
            git_modified: warning_fg,
            git_added: success_fg,
            git_deleted: danger_fg,
            git_renamed: accent_fg,
            git_untracked: base.git_untracked,
            git_conflict: danger_fg,
        })
    }
}

pub fn load_custom_theme_from_json(raw: &str) -> Result<(String, Palette), String> {
    let def: ThemeDefinition =
        serde_json::from_str(raw).map_err(|e| format!("Invalid theme JSON: {e}"))?;
    let p = def.to_palette()?;
    Ok((def.name, p))
}

pub fn load_custom_theme_from_path(path: &Path) -> Result<(String, Palette), String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read theme file '{}': {e}", path.display()))?;
    load_custom_theme_from_json(&raw)
}

pub fn export_palette_to_json(name: &str, p: Palette) -> Result<String, String> {
    let def = ThemeDefinition::from_palette(name, p);
    serde_json::to_string_pretty(&def).map_err(|e| format!("Failed to serialize theme JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::palette::{composite_over, contrast_ratio};

    #[test]
    fn all_builtin_presets_satisfy_wcag_aa_contrast_on_surfaces_and_diffs() {
        for preset in ThemePreset::ALL_BUILTINS {
            let p = preset.palette();
            let panel_cr = contrast_ratio(p.text_primary, p.surface_panel);
            let sidebar_cr = contrast_ratio(p.text_primary, p.surface_sidebar);
            let card_cr = contrast_ratio(p.text_primary, p.surface_card);
            assert!(
                panel_cr >= 7.0,
                "{preset:?} primary text vs surface_panel contrast too low: {panel_cr:.2}"
            );
            assert!(
                sidebar_cr >= 7.0,
                "{preset:?} primary text vs surface_sidebar contrast too low: {sidebar_cr:.2}"
            );
            assert!(
                card_cr >= 6.5,
                "{preset:?} primary text vs surface_card contrast too low: {card_cr:.2}"
            );

            // Verify alpha-composited diff lines & inline word highlights remain WCAG AA readable!
            let add_line_surface = composite_over(p.diff_add_line_bg, p.surface_panel);
            let del_line_surface = composite_over(p.diff_del_line_bg, p.surface_panel);
            let add_word_surface = composite_over(p.diff_add_word_bg, add_line_surface);
            let del_word_surface = composite_over(p.diff_del_word_bg, del_line_surface);

            for (label, bg) in [
                ("diff_add_line", add_line_surface),
                ("diff_del_line", del_line_surface),
                ("diff_add_word", add_word_surface),
                ("diff_del_word", del_word_surface),
            ] {
                let cr = contrast_ratio(p.text_primary, bg);
                assert!(
                    cr >= 4.5,
                    "{preset:?} {label} fails WCAG AA 4.5:1 contrast with text_primary: {cr:.2}"
                );
            }
        }
    }

    #[test]
    fn custom_theme_json_roundtrips_cleanly() {
        for preset in ThemePreset::ALL_BUILTINS {
            let orig = preset.palette();
            let json = export_palette_to_json(preset.label(), orig).expect("serialize JSON");
            let (name, parsed) = load_custom_theme_from_json(&json).expect("parse JSON");
            assert_eq!(name, preset.label());
            assert_eq!(parsed.is_dark, orig.is_dark);
            assert!(
                (parsed.surface_panel.r - orig.surface_panel.r).abs() < 0.01
                    && (parsed.accent_primary.b - orig.accent_primary.b).abs() < 0.01
            );
        }
    }

    #[test]
    fn preset_id_lookup_handles_legacy_dark_light_values() {
        assert_eq!(ThemePreset::from_id("dark"), ThemePreset::PrecisionObsidian);
        assert_eq!(ThemePreset::from_id("light"), ThemePreset::DaylightCrisp);
        assert_eq!(
            ThemePreset::from_id("catppuccin-mocha"),
            ThemePreset::CatppuccinMocha
        );
        assert_eq!(ThemePreset::from_id("tokyo-night"), ThemePreset::TokyoNight);
        assert_eq!(
            ThemePreset::from_id("github-dimmed"),
            ThemePreset::GitHubDimmed
        );
    }
}
