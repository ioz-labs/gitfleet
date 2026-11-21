mod components;
mod dock;
mod palette;
mod styles;
pub mod tokens;

pub use components::*;
pub use dock::*;
pub use palette::*;
pub use styles::*;
pub use tokens::{icon_size, layout, pad, radius, space, type_scale};

use iced::Font;
#[cfg(target_os = "windows")]
pub const CODE_FONT: Font = Font::with_name("Consolas");

#[cfg(not(target_os = "windows"))]
pub const CODE_FONT: Font = Font::MONOSPACE;

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
