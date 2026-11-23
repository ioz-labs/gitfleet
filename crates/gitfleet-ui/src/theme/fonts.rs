//! Bundled cross-platform UI & Code typography (`Inter` v4.1 + `JetBrains Mono`).
//!
//! Embedding static OpenType/TrueType weights guarantees crisp, identical glyph metrics
//! (including full Latin-Ext and Vietnamese diacritics) on Windows, macOS, and Linux.
#![allow(dead_code)]

use iced::Font;
use iced::font::{Family, Style, Weight};

pub const INTER_REGULAR: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Regular.ttf");
pub const INTER_MEDIUM: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Medium.ttf");
pub const INTER_SEMIBOLD: &[u8] = include_bytes!("../../../../assets/fonts/Inter-SemiBold.ttf");

pub const JETBRAINS_MONO_REGULAR: &[u8] =
    include_bytes!("../../../../assets/fonts/JetBrainsMono-Regular.ttf");
pub const JETBRAINS_MONO_MEDIUM: &[u8] =
    include_bytes!("../../../../assets/fonts/JetBrainsMono-Medium.ttf");
pub const JETBRAINS_MONO_BOLD: &[u8] =
    include_bytes!("../../../../assets/fonts/JetBrainsMono-Bold.ttf");

/// All embedded font slices registered with the `iced` daemon at startup.
pub const ALL_BUNDLED_FONTS: [&[u8]; 6] = [
    INTER_REGULAR,
    INTER_MEDIUM,
    INTER_SEMIBOLD,
    JETBRAINS_MONO_REGULAR,
    JETBRAINS_MONO_MEDIUM,
    JETBRAINS_MONO_BOLD,
];

/// Primary UI sans-serif font (`Inter` Regular 400).
pub const UI_FONT: Font = Font::with_name("Inter");

/// Medium-weight UI font (`Inter` Medium 500) for tabs, headers, and active controls.
pub const UI_FONT_MEDIUM: Font = Font {
    family: Family::Name("Inter"),
    weight: Weight::Medium,
    stretch: iced::font::Stretch::Normal,
    style: Style::Normal,
};

/// SemiBold UI font (`Inter` SemiBold 600) for modal titles, active repo names, and primary CTAs.
pub const UI_FONT_SEMIBOLD: Font = Font {
    family: Family::Name("Inter"),
    weight: Weight::Semibold,
    stretch: iced::font::Stretch::Normal,
    style: Style::Normal,
};

/// Primary monospace font (`JetBrains Mono` Regular 400) for diffs, code editors, SHAs, and kbd hints.
pub const CODE_FONT: Font = Font::with_name("JetBrains Mono");

/// Medium-weight monospace font (`JetBrains Mono` Medium 500).
pub const CODE_FONT_MEDIUM: Font = Font {
    family: Family::Name("JetBrains Mono"),
    weight: Weight::Medium,
    stretch: iced::font::Stretch::Normal,
    style: Style::Normal,
};

/// Bold monospace font (`JetBrains Mono` Bold 700).
pub const CODE_FONT_BOLD: Font = Font {
    family: Family::Name("JetBrains Mono"),
    weight: Weight::Bold,
    stretch: iced::font::Stretch::Normal,
    style: Style::Normal,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_fonts_parse_and_expose_expected_families() {
        let mut db = fontdb::Database::new();
        for bytes in ALL_BUNDLED_FONTS {
            assert!(bytes.len() > 50_000, "unexpectedly small font slice");
            db.load_font_data(bytes.to_vec());
        }
        let families: Vec<String> = db
            .faces()
            .flat_map(|f| f.families.iter().map(|(name, _)| name.clone()))
            .collect();
        assert!(
            families.iter().any(|n| n == "Inter"),
            "Inter family missing in bundled fonts: {families:?}"
        );
        assert!(
            families.iter().any(|n| n == "JetBrains Mono"),
            "JetBrains Mono family missing in bundled fonts: {families:?}"
        );
    }
}
