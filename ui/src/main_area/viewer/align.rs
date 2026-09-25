//! Horizontal-only canvas auto-align for dragged entries.
//!
//! The long vertical strip is treated as one canvas whose width is
//! `state.width` (already `content_width`). While drag-moving, the entry's
//! left / center / right edge (in view px) snaps to the canvas left /
//! center / right, and a Figma-style vertical guide is shown.

use easyscanlate_model::Quad;

use super::constants::ALIGN_SNAP_THRESHOLD;
use super::state::TileViewState;
use super::TileSpec;

/// Left / center / right of an image-space box converted to view px.
pub fn image_box_view_xs(min_x: f32, width: f32, scale: f32) -> [f32; 3] {
    let left = min_x * scale;
    let right = (min_x + width) * scale;
    [left, (left + right) * 0.5, right]
}

/// Canvas snap lines in view px: left edge, horizontal center, right edge.
pub fn canvas_xs(content_w: f32) -> [f32; 3] {
    [0.0, content_w * 0.5, content_w]
}

/// DPI-aware snap threshold in view px.
pub fn snap_threshold() -> f32 {
    crate::scale::s(ALIGN_SNAP_THRESHOLD)
}

/// Best canvas snap for already view-space dragged lines.
/// Returns `(delta_view, guide_x_view)` where
/// `delta_view = dragged_line - guide_x` (subtract to snap).
pub fn best_canvas_snap(
    dragged: [f32; 3],
    content_w: f32,
    threshold: f32,
) -> Option<(f32, f32)> {
    if !content_w.is_finite() || content_w <= 0.0 {
        return None;
    }
    if !threshold.is_finite() || threshold < 0.0 {
        return None;
    }
    let targets = canvas_xs(content_w);
    let mut best: Option<(f32, f32)> = None;
    for d in dragged {
        if !d.is_finite() {
            continue;
        }
        for t in targets {
            if !t.is_finite() {
                continue;
            }
            let delta = d - t;
            if delta.abs() <= threshold {
                match best {
                    None => best = Some((delta, t)),
                    Some((bd, _)) if delta.abs() < bd.abs() => best = Some((delta, t)),
                    _ => {}
                }
            }
        }
    }
    best
}

/// Snap an image-space `min_x` (box width `width`) to the canvas.
/// Returns `(new_min_x, guide_x_view)`. X-only; Y is never touched.
pub fn snap_image_min_x(
    min_x: f32,
    width: f32,
    scale: f32,
    content_w: f32,
    snap_enabled: bool,
) -> (f32, Option<f32>) {
    if !snap_enabled {
        return (min_x, None);
    }
    if !min_x.is_finite() || !width.is_finite() || width <= 0.0 {
        return (min_x, None);
    }
    if !scale.is_finite() || scale <= 0.0 {
        return (min_x, None);
    }
    if !content_w.is_finite() || content_w <= 0.0 {
        return (min_x, None);
    }
    let dragged = image_box_view_xs(min_x, width, scale);
    match best_canvas_snap(dragged, content_w, snap_threshold()) {
        Some((delta, guide)) => (min_x - delta / scale, Some(guide)),
        None => (min_x, None),
    }
}

/// Guide line (view-space X) for an already-snapped image box.
/// Used by the update handler to store `state.align_guide` for drawing.
pub fn guide_for_image_box(
    min_x: f32,
    width: f32,
    scale: f32,
    content_w: f32,
    snap_enabled: bool,
) -> Option<f32> {
    snap_image_min_x(min_x, width, scale, content_w, snap_enabled).1
}

/// Guide for a snapped `Quad` on tile `index`.
pub fn guide_for_snapped_quad(
    quad: &Quad,
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    index: usize,
) -> Option<f32> {
    if state.keyboard_modifiers.alt() {
        return None;
    }
    let tile = tiles.get(index)?;
    if tile.source_width <= 0 {
        return None;
    }
    let scale = state.width / tile.source_width as f32;
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    if !state.width.is_finite() || state.width <= 0.0 {
        return None;
    }
    let b = quad.bounds();
    let width = b[2] - b[0];
    guide_for_image_box(b[0], width, scale, state.width, true)
}

/// Figma-style axis-lock guide for a `Shift`-dragged `Quad` on tile `index`.
/// `vertical` is the dominant-axis decision (`true` = X frozen, moving
/// vertically). Returns the locked axis in content coords, drawn with the
/// same pink style as the canvas auto-align guide.
pub fn axis_lock_guide_for_quad(
    quad: &Quad,
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    index: usize,
    vertical: bool,
) -> Option<super::state::AxisLockGuide> {
    use super::layout::tile_layout;
    let tile = tiles.get(index)?;
    if tile.source_width <= 0 {
        return None;
    }
    let scale = state.width / tile.source_width as f32;
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let (layout, _) = tile_layout(tiles, state.width);
    let (tile_y, _) = *layout.get(index)?;
    if !tile_y.is_finite() {
        return None;
    }
    let b = quad.bounds();
    if !b.iter().all(|v| v.is_finite()) {
        return None;
    }
    let cx_img = (b[0] + b[2]) * 0.5;
    let cy_img = (b[1] + b[3]) * 0.5;
    if vertical {
        let x = cx_img * scale;
        if !x.is_finite() {
            return None;
        }
        Some(super::state::AxisLockGuide::Vertical(x))
    } else {
        let y = tile_y + cy_img * scale;
        if !y.is_finite() {
            return None;
        }
        Some(super::state::AxisLockGuide::Horizontal(y))
    }
}
