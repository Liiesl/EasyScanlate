//! Central UI font scaling. Every text size, padding, margin, border radius
//! and gap that has a connection to a font derives from `ui_font_size`.
//! Window chrome (`GAP`, `OUTER_PADDING`, modal shell, viewer constants) stays
//! fixed and never goes through here.

use scanlateit_settings::Settings;

pub const DEFAULT_FONT_SIZE: f32 = 12.0;
pub const DEFAULT_FONT_SIZE_U32: u32 = 12;
pub const MIN_FONT_SIZE: u32 = 8;
pub const MAX_FONT_SIZE: u32 = 30;

#[inline]
pub fn clamp_font_size(v: u32) -> u32 {
    v.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
}

#[inline]
pub fn font_size() -> u32 {
    let raw = scanlateit_settings::get(|s| s.ui_font_size);
    clamp_font_size(raw)
}

#[inline]
pub fn scale() -> f32 {
    font_size() as f32 / DEFAULT_FONT_SIZE
}

/// Scale a design-time `base` size (at 12pt) to the current font size.
/// Use for every `.size()`, `.text_size()`, `Padding`, `spacing`, `Border`
/// radius/width, `Length::Fixed`, `Space` dimensions that are font-adjacent.
#[inline]
pub fn s(base: f32) -> f32 {
    base * scale()
}

/// Alias for `s`.
#[inline]
pub fn scaled(base: f32) -> f32 {
    s(base)
}

/// Parse a free-typed font-size string like VS Code's setting: integer only,
/// fallback to current value when half-typed/invalid, clamped to range when
/// committed via `clamp_font_size`.
pub fn parse_font_size(input: &str) -> Option<u32> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<u32>().ok()
}

pub fn set_font_size(v: u32) {
    let v = clamp_font_size(v);
    let _ = scanlateit_settings::modify(|s: &mut Settings| s.ui_font_size = v);
}
