//! Drop-shadow paint for the text overlay.
//!
//! iced's geometry backend (`Fill`/`Stroke`/`Text`) has no shadow or blur
//! primitive — its [`iced::Shadow`] only applies to rectangular widget
//! backgrounds — so the overlay draws the shadow manually: the same glyph
//! outlines, filled with the shadow color at an offset, *behind* the stroke
//! and fill. Feather has no GPU primitive either, so a soft shadow is faked
//! with concentric 8-tap rings of falling, alpha-normalized weight (a single
//! hard pass when the blur is `0`). The raster export reuses
//! `draw_entries`, so overlay and export stay pixel-identical by construction.

use iced::{Color, Vector};

use easyscanlate_model::EntryStyle;

use crate::color::rgba_to_color;

/// Resolved drop shadow in frame px: base color/offset plus the blur radius.
#[derive(Debug, Clone, Copy)]
pub struct ShadowPaint {
    color: Color,
    offset: Vector,
    blur: f32,
}

impl ShadowPaint {
    /// Builds the shadow paint for `style` at `scale` (frame px per image px):
    /// `None` when the shadow is toggled off, fully transparent, or has
    /// neither distance nor feather. Distance 0 still renders when
    /// feathered: the blur rings spread beyond the glyph as an outer glow
    /// (Photoshop distance-0 behavior).
    pub fn for_style(style: &EntryStyle, scale: f32) -> Option<Self> {
        if !style.shadow_enabled {
            return None;
        }
        if style.shadow_color[3] == 0 {
            return None;
        }
        let distance = (style.shadow_distance * scale).max(0.0);
        let blur = (style.shadow_blur * scale).max(0.0);
        if distance <= 0.0 && blur <= 0.0 {
            return None;
        }
        // Same angle convention as the text gradients: `0` = bottom→top,
        // `90` = left→right (see `gradient::gradient_flow`).
        let internal = style.shadow_angle.to_radians() - std::f32::consts::FRAC_PI_2;
        Some(Self {
            color: rgba_to_color(style.shadow_color),
            offset: Vector::new(internal.cos() * distance, internal.sin() * distance),
            blur,
        })
    }

    /// Fill passes as `(offset, color)`: a single hard pass when the blur is
    /// zero, otherwise the base pass plus concentric 8-tap rings (axis +
    /// diagonal) of falling alpha faking a feathered edge. Ring alphas are
    /// normalized so the passes sum to the base alpha — the core softens as
    /// the blur grows instead of staying hard-edged under a faint halo.
    pub fn passes(&self) -> Vec<(Vector, Color)> {
        if self.blur <= 0.0 {
            return vec![(self.offset, self.color)];
        }
        // Floor at 1 frame px: image px shrink below a pixel at low zoom, and
        // a sub-pixel ring would be invisible (feather "not working").
        let radius = self.blur.max(1.0);
        // ~2px steps; cap at 3 rings (25 passes) to bound glyph fill cost.
        let rings = (radius / 2.0).ceil() as usize;
        let rings = rings.clamp(1, 3);
        const DIAG: f32 = std::f32::consts::FRAC_1_SQRT_2;
        // Gaussian-ish per-tap weights, normalized to preserve total alpha.
        let mut tap_weights = Vec::with_capacity(rings);
        for k in 1..=rings {
            let t = k as f32 / rings as f32;
            tap_weights.push(0.7 * (-2.0 * t * t).exp());
        }
        let total = 1.0 + 8.0 * tap_weights.iter().sum::<f32>();
        let [r, g, b, _] = self.color.into_rgba8();
        let at_weight = |w: f32| Color::from_rgba8(r, g, b, self.color.a * w / total);
        let mut out = Vec::with_capacity(1 + 8 * rings);
        out.push((self.offset, at_weight(1.0)));
        for (k, w) in tap_weights.iter().enumerate() {
            let rad = radius * (k + 1) as f32 / rings as f32;
            let tap = at_weight(*w);
            out.push((self.offset + Vector::new(rad, 0.0), tap));
            out.push((self.offset + Vector::new(-rad, 0.0), tap));
            out.push((self.offset + Vector::new(0.0, rad), tap));
            out.push((self.offset + Vector::new(0.0, -rad), tap));
            out.push((self.offset + Vector::new(rad * DIAG, rad * DIAG), tap));
            out.push((self.offset + Vector::new(-rad * DIAG, rad * DIAG), tap));
            out.push((self.offset + Vector::new(rad * DIAG, -rad * DIAG), tap));
            out.push((
                self.offset + Vector::new(-rad * DIAG, -rad * DIAG),
                tap,
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style_with_shadow() -> EntryStyle {
        EntryStyle {
            shadow_enabled: true,
            shadow_color: [10, 20, 30, 255],
            shadow_angle: 180.0,
            shadow_distance: 5.0,
            shadow_blur: 0.0,
            ..EntryStyle::default()
        }
    }

    #[test]
    fn disabled_or_zero_distance_or_transparent_is_none() {
        let mut style = style_with_shadow();
        assert!(ShadowPaint::for_style(&style, 1.0).is_some());
        style.shadow_enabled = false;
        assert!(ShadowPaint::for_style(&style, 1.0).is_none());
        style.shadow_enabled = true;
        style.shadow_distance = 0.0;
        // No offset and no feather: nothing to draw.
        assert!(ShadowPaint::for_style(&style, 1.0).is_none());
        // ...but a feathered zero-distance shadow glows around the glyph.
        style.shadow_blur = 2.0;
        let paint = ShadowPaint::for_style(&style, 1.0).expect("glow on");
        assert!((paint.offset.x).abs() < 1e-6 && (paint.offset.y).abs() < 1e-6);
        assert_eq!(paint.passes().len(), 9);
        style.shadow_blur = 0.0;
        style.shadow_color[3] = 0;
        assert!(ShadowPaint::for_style(&style, 1.0).is_none());
    }

    #[test]
    fn angle_180_points_down_and_scales() {
        let style = style_with_shadow();
        let paint = ShadowPaint::for_style(&style, 2.0).expect("shadow on");
        // angle 180, same convention as gradients: straight down.
        assert!((paint.offset.x).abs() < 1e-4, "x {}", paint.offset.x);
        assert!((paint.offset.y - 10.0).abs() < 1e-4, "y {}", paint.offset.y);
        assert_eq!(paint.passes().len(), 1);
    }

    #[test]
    fn blur_adds_feather_rings() {
        let mut style = style_with_shadow();
        style.shadow_blur = 3.0;
        let paint = ShadowPaint::for_style(&style, 1.0).expect("shadow on");
        // radius 3 → 2 rings × 8 taps + base.
        assert_eq!(paint.passes().len(), 17);
    }

    #[test]
    fn sub_pixel_blur_still_feathers() {
        let mut style = style_with_shadow();
        style.shadow_blur = 0.25;
        let paint = ShadowPaint::for_style(&style, 1.0).expect("shadow on");
        // Floored to a 1px radius: 1 ring × 8 taps + base.
        assert_eq!(paint.passes().len(), 9);
    }

    #[test]
    fn feather_softens_the_core() {
        let mut style = style_with_shadow();
        style.shadow_blur = 3.0;
        let paint = ShadowPaint::for_style(&style, 1.0).expect("shadow on");
        let passes = paint.passes();
        // Normalized weights: the core drops below full alpha once blurred.
        assert!(passes[0].1.a < 1.0, "core {}", passes[0].1.a);
        assert!(passes[0].1.a > passes[1].1.a, "core must stay strongest");
    }
}
