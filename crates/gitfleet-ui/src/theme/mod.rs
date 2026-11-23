mod components;
mod dock;
pub mod fonts;
pub mod kit;
pub mod palette;
pub mod presets;
mod styles;
pub mod tokens;

pub use components::*;
pub use dock::*;
#[allow(unused_imports)]
pub use fonts::{
    ALL_BUNDLED_FONTS, CODE_FONT, CODE_FONT_BOLD, CODE_FONT_MEDIUM, UI_FONT, UI_FONT_MEDIUM,
    UI_FONT_SEMIBOLD,
};
#[allow(unused_imports)]
pub use palette::{
    DARK, LIGHT, Palette, SemanticTheme, ThemeMode, composite_over, contrast_ratio, hex_rgb,
    hex_rgba, on_accent_ink, relative_luminance,
};
#[allow(unused_imports)]
pub use presets::{
    CATPPUCCIN_MOCHA, GITHUB_DIMMED, TOKYO_NIGHT, ThemeDefinition, ThemePreset,
    active_custom_palette, export_palette_to_json, load_custom_theme_from_json,
    load_custom_theme_from_path, set_active_custom_palette,
};
pub use styles::*;
pub use tokens::{icon_size, layout, pad, radius, space, tree, type_scale};

/// Replaces ASCII tabs with 4 spaces and strips CR so monospace rendering never draws missing-glyph boxes.
pub fn sanitize_code_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + 8);
    for ch in line.chars() {
        match ch {
            '\t' => out.push_str("    "),
            '\r' => {}
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}
