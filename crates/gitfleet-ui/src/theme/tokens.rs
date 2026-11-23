//! Centralized GitFleet Design System scale tokens (typography, 4px spacing, 3-tier radii, layout).
//!
//! Layer 2 scale tokens: all panes, bottom dock tabs, modals, overlays, and kit components draw
//! dimensions from this module rather than hardcoding magic numbers.
#![allow(dead_code)]

/// Typography scale (in logical pixels, strict 12px minimum across the entire app).
pub mod type_scale {
    pub const TITLE: f32 = 18.0;
    pub const HEADING: f32 = 15.0;
    pub const SUBHEADING: f32 = 14.0;
    pub const BODY: f32 = 13.0;
    pub const CODE: f32 = 13.0;
    pub const CAPTION: f32 = 12.0;
    pub const BADGE: f32 = 12.0;
}

/// 4px-grid spacing scale.
pub mod space {
    pub const XXS: f32 = 4.0;
    pub const XS: f32 = 6.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 20.0;
}

/// Reusable padding presets (`[vertical, horizontal]` or uniform `u16`).
pub mod pad {
    pub const BTN_XS: [u16; 2] = [3, 8];
    pub const BTN_SM: [u16; 2] = [5, 10];
    pub const BTN_MD: [u16; 2] = [7, 14];
    pub const BTN_ICON_SM: [u16; 2] = [4, 5];
    pub const BTN_ICON: [u16; 2] = [5, 6];
    pub const INPUT: [u16; 2] = [7, 10];
    pub const INPUT_COMPACT: [u16; 2] = [5, 9];
    pub const CARD: u16 = 12;
    pub const MODAL: u16 = 18;
    pub const DOCK_TOOLBAR: [u16; 2] = [6, 12];
    pub const DOCK_BODY: [u16; 2] = [10, 14];
    pub const PILL: [u16; 2] = [2, 7];
    pub const ROW: [u16; 2] = [5, 10];
}

/// Strict 3-tier border-radius discipline (+ `0px` for edge-to-edge split panes).
///
/// - `NONE` (`0px`): Split panes, dock headers, ribbon, status bar (eliminates box-in-box borders).
/// - `BADGE` / `XS` (`3px`): Status badges, counters, shortcut kbd chips.
/// - `CONTROL` / `SM` (`5px`): Buttons, inputs, segmented controls, tree row highlights.
/// - `SURFACE` / `MD` / `LG` (`8px`): Modals, floating popovers, context menu cards, elevated cards.
pub mod radius {
    pub const NONE: f32 = 0.0;
    pub const BADGE: f32 = 3.0;
    pub const CONTROL: f32 = 5.0;
    pub const SURFACE: f32 = 8.0;

    // Semantic aliases preserved for existing callers mapped onto the 3-tier system:
    pub const XS: f32 = BADGE;
    pub const SM: f32 = CONTROL;
    pub const MD: f32 = SURFACE;
    pub const LG: f32 = SURFACE;
}

/// Standard vector SVG icon sizes.
pub mod icon_size {
    pub const XS: f32 = 13.0;
    pub const SM: f32 = 15.0;
    pub const MD: f32 = 17.0;
    pub const LG: f32 = 24.0;
}

/// Chrome dimensions, modal widths, splitter geometry, and dock sizing.
pub mod layout {
    pub const RIBBON_HEIGHT: f32 = 48.0;
    pub const PANE_HEADER_HEIGHT: f32 = 30.0;
    pub const STATUS_BAR_HEIGHT: f32 = 26.0;

    /// Invisible cursor hit area for `HairlineSplitter` (`7px`), painting a crisp `1px` line inside.
    pub const SPLITTER_HIT_SIZE: f32 = 7.0;
    pub const SPLITTER_LINE_SIZE: f32 = 1.0;
    pub const FOCUS_BAR_WIDTH: f32 = 2.0;

    pub const MODAL_SM: f32 = 480.0;
    pub const MODAL_MD: f32 = 640.0;
    pub const MODAL_LG: f32 = 800.0;
    pub const MODAL_XL: f32 = 920.0;
    pub const MODAL_MAX_HEIGHT: f32 = 760.0;

    pub const DOCK_DEFAULT_HEIGHT: f32 = 320.0;
    pub const DOCK_MIN_HEIGHT: f32 = 180.0;
    pub const DOCK_RIGHT_DEFAULT_WIDTH: f32 = 460.0;
    pub const DOCK_RIGHT_MIN_WIDTH: f32 = 300.0;

    pub const TERMINAL_SIDEBAR_WIDTH: f32 = 196.0;
    pub const TERMINAL_MENU_WIDTH: f32 = 344.0;
}

/// Shared horizontal & vertical grid geometry for sidebar trees (Repositories, Branches, Scan preview)
/// so parent groups, subgroups, repositories, submodules, and branches align on identical columns.
pub mod tree {
    pub const BASE_LEFT: f32 = 4.0;
    pub const INDENT_STEP: f32 = 16.0;
    pub const CHEVRON_W: f32 = 15.0;
    pub const LEAD_W: f32 = 15.0;
    pub const CHEVRON_GAP: f32 = 5.0;
    pub const ROW_BTN_GAP: f32 = 2.0;
    pub const ROW_PAD_V: f32 = 3.5;
    pub const ROW_PAD_H: f32 = 6.0;
    /// Visual gap from the lead icon/checkbox right edge to the row label (`ROW_BTN_GAP + ROW_PAD_H`).
    pub const LABEL_GAP: f32 = ROW_BTN_GAP + ROW_PAD_H;
    pub const ROW_SPACING: f32 = 1.5;
    pub const CHECKBOX_SIZE: f32 = 14.0;
    pub const CHEVRON_ICON_SIZE: f32 = 12.0;
    pub const LEAD_ICON_SIZE: f32 = 14.0;

    /// Left edge of the chevron slot for a tree node at `depth` (`0` = root level).
    pub const fn chevron_x(depth: usize) -> f32 {
        BASE_LEFT + (depth as f32) * INDENT_STEP
    }

    /// Left edge of the lead icon / checkbox slot for a tree node at `depth`.
    pub const fn lead_x(depth: usize) -> f32 {
        chevron_x(depth) + CHEVRON_W + CHEVRON_GAP
    }

    /// Left edge where the row content (status dot / HEAD marker / label) starts at `depth`.
    pub const fn content_x(depth: usize) -> f32 {
        lead_x(depth) + LEAD_W + LABEL_GAP
    }
}

#[cfg(test)]
mod tests {
    use super::{layout, radius, type_scale};

    #[test]
    fn enforces_12px_minimum_type_scale() {
        for sz in [
            type_scale::BADGE,
            type_scale::CAPTION,
            type_scale::CODE,
            type_scale::BODY,
            type_scale::SUBHEADING,
            type_scale::HEADING,
            type_scale::TITLE,
        ] {
            assert!(sz >= 12.0, "type_scale token must be >= 12px, got {sz}");
        }
        assert!(type_scale::BODY > type_scale::CAPTION);
        assert!(type_scale::SUBHEADING > type_scale::BODY);
        assert!(type_scale::HEADING > type_scale::SUBHEADING);
        assert!(type_scale::TITLE > type_scale::HEADING);
    }

    #[test]
    fn enforces_three_tier_radius_scale() {
        assert_eq!(radius::NONE, 0.0);
        assert_eq!(radius::BADGE, 3.0);
        assert_eq!(radius::CONTROL, 5.0);
        assert_eq!(radius::SURFACE, 8.0);
        assert!(layout::SPLITTER_HIT_SIZE >= 6.0);
        assert_eq!(layout::SPLITTER_LINE_SIZE, 1.0);
    }
}
