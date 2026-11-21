//! Centralized GitFleet Design System tokens (typography, spacing, padding, radii, layout).
//!
//! All bottom dock tabs, modals, overlays, and chrome components should draw sizes and spacing
//! from this module rather than hardcoding magic numbers.
#![allow(dead_code)]

/// Typography scale (in logical pixels).
pub mod type_scale {
    pub const TITLE: f32 = 16.0;
    pub const HEADING: f32 = 14.0;
    pub const SUBHEADING: f32 = 13.0;
    pub const BODY: f32 = 12.0;
    pub const CODE: f32 = 12.0;
    pub const CAPTION: f32 = 11.0;
    pub const BADGE: f32 = 10.5;
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
    pub const BTN_SM: [u16; 2] = [5, 10];
    pub const BTN_MD: [u16; 2] = [7, 14];
    pub const BTN_ICON: [u16; 2] = [5, 6];
    pub const INPUT: [u16; 2] = [7, 10];
    pub const INPUT_COMPACT: [u16; 2] = [6, 9];
    pub const CARD: u16 = 12;
    pub const MODAL: u16 = 20;
    pub const DOCK_TOOLBAR: [u16; 2] = [8, 14];
    pub const DOCK_BODY: [u16; 2] = [12, 14];
    pub const PILL: [u16; 2] = [2, 8];
    pub const ROW: [u16; 2] = [6, 10];
}

/// Border radii scale.
pub mod radius {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 6.0;
    pub const MD: f32 = 8.0;
    pub const LG: f32 = 12.0;
}

/// Standard vector SVG icon sizes.
pub mod icon_size {
    pub const XS: f32 = 12.0;
    pub const SM: f32 = 14.0;
    pub const MD: f32 = 16.0;
    pub const LG: f32 = 22.0;
}

/// Modal widths, bottom dock sizing, and chrome dimensions.
pub mod layout {
    pub const MODAL_SM: f32 = 480.0;
    pub const MODAL_MD: f32 = 600.0;
    pub const MODAL_LG: f32 = 760.0;
    pub const MODAL_XL: f32 = 860.0;
    pub const MODAL_MAX_HEIGHT: f32 = 760.0;

    pub const DOCK_DEFAULT_HEIGHT: f32 = 320.0;
    pub const DOCK_MIN_HEIGHT: f32 = 180.0;
    pub const STATUS_BAR_HEIGHT: f32 = 26.0;
    pub const TERMINAL_SIDEBAR_WIDTH: f32 = 184.0;
    pub const TERMINAL_MENU_WIDTH: f32 = 324.0;
}
