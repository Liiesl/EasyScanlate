//! Aurora-synced accent colors: loading bar, progress bars, dots, icons.
//!
//! Single source of truth so the accent always follows the aurora background
//! theme (`aurora_color`, `is_dark`). Derivation is brightened-hue: keep the
//! aurora hue, boost saturation/value for visibility, auto-darken in light
//! mode for contrast. Achromatic auroras fall back to sky blue.

use iced::Color;

use crate::background::{AuroraConfig, color_to_hsv, hsv_to_color};

/// Fallback accent when the aurora color is achromatic (grey/white/black).
/// Legacy hard-coded blue `#5CBEFF`.
pub const FALLBACK_ACCENT: Color = Color::from_rgb8(92, 190, 255);
/// Darkened fallback for light mode (sky-700 `#0284C7`).
const FALLBACK_ACCENT_LIGHT: Color = Color::from_rgb8(2, 132, 199);

const DARK_S_MIN: u8 = 180;
const DARK_V: u8 = 242;
const LIGHT_S_MIN: u8 = 170;
const LIGHT_V: u8 = 175;

/// Accent derived from an explicit aurora config (no store read).
pub fn aurora_accent(cfg: &AuroraConfig) -> Color {
    let (h_raw, s_raw, _v_raw) = color_to_hsv(cfg.color);
    // Achromatic (grey/white/black): no hue to keep, use fallback.
    if h_raw == -1 || s_raw < 15 {
        return if cfg.is_dark {
            FALLBACK_ACCENT
        } else {
            FALLBACK_ACCENT_LIGHT
        };
    }
    if cfg.is_dark {
        let s = s_raw.max(DARK_S_MIN);
        hsv_to_color(h_raw, s, DARK_V)
    } else {
        let s = s_raw.max(LIGHT_S_MIN);
        hsv_to_color(h_raw, s, LIGHT_V)
    }
}

/// Accent read live from the settings store (re-renders on theme change).
pub fn accent() -> Color {
    aurora_accent(&AuroraConfig::from_store())
}

/// Hover variant: lighten in dark mode, darken in light mode.
pub fn aurora_accent_hover(cfg: &AuroraConfig) -> Color {
    let c = aurora_accent(cfg);
    if cfg.is_dark {
        lighten(c, 0.12)
    } else {
        darken(c, 0.10)
    }
}

/// Hover variant read live from the store.
pub fn accent_hover() -> Color {
    let cfg = AuroraConfig::from_store();
    aurora_accent_hover(&cfg)
}

/// Accent with custom alpha (e.g. badges, active-dot fills).
pub fn aurora_accent_translucent(cfg: &AuroraConfig, alpha: f32) -> Color {
    let c = aurora_accent(cfg);
    Color { a: alpha.clamp(0.0, 1.0), ..c }
}

/// Translucent accent read live from the store.
pub fn accent_translucent(alpha: f32) -> Color {
    let cfg = AuroraConfig::from_store();
    aurora_accent_translucent(&cfg, alpha)
}

/// Progress-bar track: visible on both dark and light auroras.
pub fn aurora_track(cfg: &AuroraConfig) -> Color {
    if cfg.is_dark {
        Color::from_rgba8(255, 255, 255, 0.12)
    } else {
        Color::from_rgba8(0, 0, 0, 0.12)
    }
}

/// Track read live from the store.
pub fn track() -> Color {
    aurora_track(&AuroraConfig::from_store())
}

/// Loading-bar gradient pair `(edge, core)`: transparent void → accent.
/// The void edge is fully transparent so the bar melts into the aurora
/// instead of stamping opaque black.
pub fn aurora_loading_pair(cfg: &AuroraConfig) -> (Color, Color) {
    (Color::from_rgba8(17, 24, 39, 0.0), aurora_accent(cfg))
}

/// Loading pair read live from the store.
pub fn loading_pair() -> (Color, Color) {
    aurora_loading_pair(&AuroraConfig::from_store())
}

/// Loading-bar / progress label color, mode-aware for contrast.
pub fn aurora_label(cfg: &AuroraConfig) -> Color {
    if cfg.is_dark {
        Color::from_rgb8(148, 163, 184) // slate-400
    } else {
        Color::from_rgb8(71, 85, 105) // slate-600
    }
}

/// Label read live from the store.
pub fn label() -> Color {
    aurora_label(&AuroraConfig::from_store())
}

/// Canonical progress-bar style synced with the aurora theme.
pub fn progress_style(cfg: &AuroraConfig) -> iced::widget::progress_bar::Style {
    iced::widget::progress_bar::Style {
        background: aurora_track(cfg).into(),
        bar: aurora_accent(cfg).into(),
        border: iced::Border::default().rounded(crate::scale::s(3.0)),
    }
}

/// Progress style read live from the store (for style closures).
pub fn progress_style_live() -> iced::widget::progress_bar::Style {
    let cfg = AuroraConfig::from_store();
    progress_style(&cfg)
}

fn lighten(c: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::from_rgb(
        c.r + (1.0 - c.r) * t,
        c.g + (1.0 - c.g) * t,
        c.b + (1.0 - c.b) * t,
    )
}

fn darken(c: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::from_rgb(c.r * (1.0 - t), c.g * (1.0 - t), c.b * (1.0 - t))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(hex: (u8, u8, u8), is_dark: bool) -> AuroraConfig {
        AuroraConfig {
            color: Color::from_rgb8(hex.0, hex.1, hex.2),
            blob_count: 2,
            is_dark,
            schema: crate::background::AuroraSchema::Analogous,
        }
    }

    #[test]
    fn preserves_hue() {
        let c = cfg((59, 6, 0), true);
        let (h_before, _, _) = color_to_hsv(c.color);
        let acc = aurora_accent(&c);
        let (h_after, _, _) = color_to_hsv(acc);
        assert_eq!(h_before, h_after);
    }

    #[test]
    fn light_is_darker_than_dark() {
        let dark = aurora_accent(&cfg((100, 50, 200), true));
        let light = aurora_accent(&cfg((100, 50, 200), false));
        let lum = |c: Color| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
        assert!(lum(light) < lum(dark));
    }

    #[test]
    fn grey_falls_back() {
        let dark = aurora_accent(&cfg((128, 128, 128), true));
        let [r, g, b, _] = dark.into_rgba8();
        assert_eq!((r, g, b), (92, 190, 255));
        let light = aurora_accent(&cfg((128, 128, 128), false));
        let [r, g, b, _] = light.into_rgba8();
        assert_eq!((r, g, b), (2, 132, 199));
    }

    #[test]
    fn translucent_keeps_rgb() {
        let c = cfg((59, 6, 0), true);
        let solid = aurora_accent(&c);
        let trans = aurora_accent_translucent(&c, 0.15);
        assert!((trans.a - 0.15).abs() < 0.001);
        assert_eq!((trans.r, trans.g, trans.b), (solid.r, solid.g, solid.b));
    }
}
