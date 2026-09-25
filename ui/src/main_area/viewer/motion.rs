use iced::{Point, Rectangle, Size};

use lucide_icons::Icon;
use easyscanlate_model::{EntryId, Quad};

use crate::main_area::geometry::order_quad;
use crate::scale;

use super::constants::{
    HANDLE_SIZE, MIN_BOX_EDGE, ROTATE_STEM, TOOLBAR_GAP, TOOLBAR_HEIGHT,
};
use super::interaction::{ResizeHandle, TopDecor};
use super::layout::tile_layout;
use super::state::TileViewState;
use super::TileSpec;

pub fn drag_grab(tiles: &[TileSpec<'_>], state: &TileViewState, index: usize, id: EntryId, local: Point) -> Option<[f32; 2]> {
    let tile = tiles.get(index)?;
    let (layout, _) = tile_layout(tiles, state.width);
    let (y, _) = layout.get(index)?;
    let scale = if tile.source_width > 0 {
        state.width / tile.source_width as f32
    } else {
        0.0
    };
    if scale <= 0.0 {
        return None;
    }
    let entry = tile.overlays.iter().find(|e| e.id == id)?;
    let [min_x, min_y, _, _] = entry.bounds;
    let img_x = local.x / scale;
    let img_y = (local.y + state.offset - y) / scale;
    Some([img_x - min_x, img_y - min_y])
}

pub fn drag_quad(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    index: usize,
    local: Point,
    offset: [f32; 2],
    quad: Quad,
    press: Point,
) -> Option<Quad> {
    let tile = tiles.get(index)?;
    let (layout, _) = tile_layout(tiles, state.width);
    let (y, _) = layout.get(index)?;
    let scale = if tile.source_width > 0 {
        state.width / tile.source_width as f32
    } else {
        0.0
    };
    if scale <= 0.0 {
        return None;
    }
    let img_x = local.x / scale;
    let img_y = (local.y + state.offset - y) / scale;
    let size = quad.bounds();
    let mut min_x = img_x - offset[0];
    let mut min_y = img_y - offset[1];
    // Figma-style Shift: lock to the dominant view-space axis measured from
    // the press point. Horizontal lock keeps Y at its press value, vertical
    // lock keeps X at its press value.
    let shift = state.keyboard_modifiers.shift();
    let mut vertical_locked = false;
    if shift {
        let img_px = press.x / scale;
        let img_py = (press.y + state.offset - y) / scale;
        let start_min_x = img_px - offset[0];
        let start_min_y = img_py - offset[1];
        let dx = local.x - press.x;
        let dy = local.y - press.y;
        if dx.abs() >= dy.abs() {
            min_y = start_min_y;
        } else {
            min_x = start_min_x;
            vertical_locked = true;
        }
    }
    // Horizontal-only canvas auto-align: snap left/center/right to the
    // canvas left/center/right. Y is never touched. Alt disables it, as does
    // a Shift vertical lock (X is frozen, a guide would mislead).
    let width = size[2] - size[0];
    let (snapped_min_x, _) = super::align::snap_image_min_x(
        min_x,
        width,
        scale,
        state.width,
        !state.keyboard_modifiers.alt() && !vertical_locked,
    );
    Some(quad.translate(snapped_min_x - size[0], min_y - size[1]))
}

/// Figma-style rubber-band adjustment: `Shift` forces a square, `Alt` draws
/// from the center (`start` is the center). Order-independent: callers keep
/// using `min/max` on the returned pair.
pub fn adjust_selection_points(
    start: Point,
    current: Point,
    square: bool,
    centered: bool,
) -> (Point, Point) {
    let dx = current.x - start.x;
    let dy = current.y - start.y;
    if centered {
        if square {
            let h = dx.abs().max(dy.abs());
            return (
                Point::new(start.x - h, start.y - h),
                Point::new(start.x + h, start.y + h),
            );
        }
        let ax = dx.abs();
        let ay = dy.abs();
        return (
            Point::new(start.x - ax, start.y - ay),
            Point::new(start.x + ax, start.y + ay),
        );
    }
    if square {
        let side = dx.abs().max(dy.abs());
        let sx = if dx < 0.0 { -1.0 } else { 1.0 };
        let sy = if dy < 0.0 { -1.0 } else { 1.0 };
        return (start, Point::new(start.x + side * sx, start.y + side * sy));
    }
    (start, current)
}

pub fn handle_anchors(quad: [[f32; 2]; 4]) -> [(ResizeHandle, Point); 8] {
    let ordered = order_quad(quad);
    let point = |i: usize| Point::new(ordered[i][0], ordered[i][1]);
    let midpoint = |a: usize, b: usize| {
        Point::new((ordered[a][0] + ordered[b][0]) / 2.0, (ordered[a][1] + ordered[b][1]) / 2.0)
    };
    [
        (ResizeHandle::NW, point(0)),
        (ResizeHandle::N, midpoint(0, 1)),
        (ResizeHandle::NE, point(1)),
        (ResizeHandle::E, midpoint(1, 2)),
        (ResizeHandle::SE, point(2)),
        (ResizeHandle::S, midpoint(2, 3)),
        (ResizeHandle::SW, point(3)),
        (ResizeHandle::W, midpoint(3, 0)),
    ]
}

pub fn handle_rect(anchor: Point) -> Rectangle {
    let hs = scale::s(HANDLE_SIZE);
    let half = hs / 2.0;
    Rectangle::new(
        Point::new(anchor.x - half, anchor.y - half),
        Size::new(hs, hs),
    )
}

pub fn quad_centroid(quad: [[f32; 2]; 4]) -> Point {
    Point::new(
        (quad[0][0] + quad[1][0] + quad[2][0] + quad[3][0]) / 4.0,
        (quad[0][1] + quad[1][1] + quad[2][1] + quad[3][1]) / 4.0,
    )
}

/// Side of a gradient stop square, in tile pixels.
pub const GRADIENT_SQUARE: f32 = 14.0;
/// Extra hit slop around a gradient square, in tile pixels.
pub const GRADIENT_HIT_PAD: f32 = 4.0;

/// Tile-local box rect of an entry's gradient (axis-aligned bounds scaled).
pub fn gradient_handle_box(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    index: usize,
    id: EntryId,
) -> Option<Rectangle> {
    let tile = tiles.get(index)?;
    let scale = if tile.source_width > 0 {
        state.width / tile.source_width as f32
    } else {
        0.0
    };
    if scale <= 0.0 {
        return None;
    }
    let entry = tile.overlays.iter().find(|e| e.id == id)?;
    let [min_x, min_y, max_x, max_y] = entry.bounds;
    Some(Rectangle::new(
        Point::new(min_x * scale, min_y * scale),
        Size::new((max_x - min_x) * scale, (max_y - min_y) * scale),
    ))
}

/// Gradient endpoints in tile-local coords, matching the overlay paint math.
pub fn gradient_handle_points(box_rect: Rectangle, angle: f32) -> (Point, Point) {
    crate::main_area::overlay::gradient::gradient_start_end_angle(angle, box_rect)
}

/// Center of the gradient box.
pub fn gradient_box_center(box_rect: Rectangle) -> Point {
    Point::new(
        box_rect.x + box_rect.width / 2.0,
        box_rect.y + box_rect.height / 2.0,
    )
}

/// Edge distance: center to either clamped endpoint (`factor 1.0`).
pub fn gradient_edge_distance(box_rect: Rectangle, angle: f32) -> f32 {
    let (start, end) = gradient_handle_points(box_rect, angle);
    ((end.x - start.x).hypot(end.y - start.y) / 2.0).max(f32::EPSILON)
}

/// Free handle endpoints: center ± direction × edge distance × `factor`.
/// `1.0` rests the squares on the entry border; larger values float them
/// outside like Figma.
pub fn gradient_free_points(box_rect: Rectangle, angle: f32, factor: f32) -> (Point, Point) {
    let (start, end) = gradient_handle_points(box_rect, angle);
    let mut dx = end.x - start.x;
    let mut dy = end.y - start.y;
    let len = dx.hypot(dy).max(f32::EPSILON);
    dx /= len;
    dy /= len;
    let center = gradient_box_center(box_rect);
    let radius = gradient_edge_distance(box_rect, angle) * factor.max(0.2);
    (
        Point::new(center.x - dx * radius, center.y - dy * radius),
        Point::new(center.x + dx * radius, center.y + dy * radius),
    )
}

/// Resting radius factor cached in the widget state for this handle, or
/// `1.0` (squares on the border) when nothing was dropped yet.
pub fn gradient_rest_factor(
    state: &TileViewState,
    index: usize,
    id: EntryId,
    field: crate::event::StyleField,
) -> f32 {
    match state.gradient_rest {
        Some(rest)
            if rest.index == index && rest.id == id && rest.field == field =>
        {
            rest.factor
        }
        _ => 1.0,
    }
}

/// Radius factor for a drop at distance `r` from the center, clamped so the
/// squares stay reachable: at least `0.2` (no center collapse) and at most
/// the tile frame (minus a square margin) so a dropped handle can be
/// grabbed again afterwards.
pub fn gradient_release_factor(
    box_rect: Rectangle,
    angle: f32,
    r: f32,
    frame: Size,
) -> f32 {
    let edge = gradient_edge_distance(box_rect, angle);
    let (start, end) = gradient_handle_points(box_rect, angle);
    let mut dx = end.x - start.x;
    let mut dy = end.y - start.y;
    let len = dx.hypot(dy).max(f32::EPSILON);
    dx /= len;
    dy /= len;
    let center = gradient_box_center(box_rect);
    let margin = scale::s(GRADIENT_SQUARE) / 2.0 + scale::s(2.0);
    let mut reach = f32::INFINITY;
    let eps = 1e-6;
    if dx > eps {
        reach = reach.min((frame.width - margin - center.x) / dx);
    } else if dx < -eps {
        reach = reach.min((margin - center.x) / dx);
    }
    if dy > eps {
        reach = reach.min((frame.height - margin - center.y) / dy);
    } else if dy < -eps {
        reach = reach.min((margin - center.y) / dy);
    }
    let max_factor = (reach / edge).max(0.2);
    (r / edge).clamp(0.2, max_factor)
}

/// Square centered at a handle point.
pub fn gradient_square_rect(center: Point) -> Rectangle {
    let side = scale::s(GRADIENT_SQUARE);
    let half = side / 2.0;
    Rectangle::new(
        Point::new(center.x - half, center.y - half),
        Size::new(side, side),
    )
}

/// Squares centered at the free handle endpoints.
pub fn gradient_handle_squares(
    box_rect: Rectangle,
    angle: f32,
    factor: f32,
) -> (Rectangle, Rectangle) {
    let (start, end) = gradient_free_points(box_rect, angle, factor);
    (gradient_square_rect(start), gradient_square_rect(end))
}

/// Hit-test squares with extra padding; returns endpoint `0`/`1`.
pub fn gradient_handle_hit(
    box_rect: Rectangle,
    angle: f32,
    factor: f32,
    p: Point,
) -> Option<usize> {
    let (a, b) = gradient_handle_squares(box_rect, angle, factor);
    let pad = scale::s(GRADIENT_HIT_PAD);
    let expand = |r: Rectangle| {
        Rectangle::new(
            Point::new(r.x - pad, r.y - pad),
            Size::new(r.width + pad * 2.0, r.height + pad * 2.0),
        )
    };
    if expand(a).contains(p) {
        return Some(0);
    }
    if expand(b).contains(p) {
        return Some(1);
    }
    None
}

/// Raw pointer angle in degrees around the box center (`-180..180`).
/// Unlike [`gradient_angle_from_point`], this has no gradient convention
/// folded in, so differences between two calls are pure cursor deltas —
/// independent of which stop square was grabbed.
pub fn gradient_pointer_angle(box_rect: Rectangle, p: Point) -> f32 {
    let center = Point::new(
        box_rect.x + box_rect.width / 2.0,
        box_rect.y + box_rect.height / 2.0,
    );
    f32::atan2(p.y - center.y, p.x - center.x).to_degrees()
}

/// Wraps an angle delta in degrees to `-180..180`.
pub fn wrap_angle_delta(delta: f32) -> f32 {
    (delta + 540.0).rem_euclid(360.0) - 180.0
}

/// Angle in degrees (`0..360`) of point `p` around the box center, inverting
/// `gradient_flow` (`angle - 90°` gives the stop-0→stop-1 vector).
pub fn gradient_angle_from_point(box_rect: Rectangle, p: Point) -> f32 {
    let center = Point::new(
        box_rect.x + box_rect.width / 2.0,
        box_rect.y + box_rect.height / 2.0,
    );
    let internal = f32::atan2(p.y - center.y, p.x - center.x);
    (internal + std::f32::consts::FRAC_PI_2)
        .to_degrees()
        .rem_euclid(360.0)
}

pub fn top_decor_geometry(rect: Rectangle, quad: [[f32; 2]; 4], width: f32, viewport_top: f32, viewport_bottom: f32) -> TopDecor {
    let center = quad_centroid(quad);
    let hs = scale::s(HANDLE_SIZE);
    let rot_stem = scale::s(ROTATE_STEM);
    let toolbar_gap = scale::s(TOOLBAR_GAP);
    let toolbar_height = scale::s(TOOLBAR_HEIGHT);
    let outward = |a: [f32; 2], b: [f32; 2]| -> Point {
        let mid = Point::new((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0);
        let edge = [b[0] - a[0], b[1] - a[1]];
        let mut normal = [-edge[1], edge[0]];
        let toward = [mid.x - center.x, mid.y - center.y];
        if normal[0] * toward[0] + normal[1] * toward[1] < 0.0 {
            normal = [edge[1], -edge[0]];
        }
        let len = (normal[0] * normal[0] + normal[1] * normal[1]).sqrt().max(f32::EPSILON);
        Point::new(
            mid.x + normal[0] / len * rot_stem,
            mid.y + normal[1] / len * rot_stem,
        )
    };
    let ordered = order_quad(quad);
    let top_mid = Point::new(
        (ordered[0][0] + ordered[1][0]) / 2.0,
        (ordered[0][1] + ordered[1][1]) / 2.0,
    );
    let bottom_mid = Point::new(
        (ordered[2][0] + ordered[3][0]) / 2.0,
        (ordered[2][1] + ordered[3][1]) / 2.0,
    );
    let stem_up = outward(ordered[0], ordered[1]);
    let stem_down = outward(ordered[3], ordered[2]);
    let flip = stem_up.y - hs / 2.0 < viewport_top && rect.y > viewport_top;
    let (stem_from, mut anchor) = if flip {
        (bottom_mid, stem_down)
    } else {
        (top_mid, stem_up)
    };
    anchor.y = anchor
        .y
        .clamp(viewport_top + hs / 2.0, viewport_bottom - hs / 2.0);
    let revert_width = button_width(Icon::Undo2);
    let revert = Rectangle::new(
        Point::new(
            (anchor.x + hs / 2.0 + toolbar_gap).clamp(0.0, (width - revert_width).max(0.0)),
            anchor.y - toolbar_height / 2.0,
        ),
        Size::new(revert_width, toolbar_height),
    );
    TopDecor { anchor, stem_from, revert }
}

pub fn delta_angle(center: Point, from: Point, to: Point, snap: bool) -> f32 {
    let from_angle = f32::atan2(from.y - center.y, from.x - center.x);
    let to_angle = f32::atan2(to.y - center.y, to.x - center.x);
    let mut delta = to_angle - from_angle;
    while delta > std::f32::consts::PI {
        delta -= std::f32::consts::TAU;
    }
    while delta < -std::f32::consts::PI {
        delta += std::f32::consts::TAU;
    }
    if snap {
        const ROTATE_SNAP_DEGREES: f32 = 15.0;
        let step = ROTATE_SNAP_DEGREES.to_radians();
        delta = (delta / step).round() * step;
    }
    delta
}

pub fn rotate_quad(quad: Quad, center_img: [f32; 2], center_view: Point, press: Point, local: Point, snap: bool) -> Quad {
    quad.rotate(center_img, delta_angle(center_view, press, local, snap))
}

pub fn toolbar_buttons() -> [(crate::event::ToolbarAction, Icon); 2] {
    [
        (crate::event::ToolbarAction::Rename, Icon::Pencil),
        (crate::event::ToolbarAction::Delete, Icon::Trash2),
    ]
}

pub fn inpaint_toolbar_buttons() -> [(crate::event::InpaintToolbarAction, Icon); 2] {
    [
        (crate::event::InpaintToolbarAction::Delete, Icon::Trash2),
        (crate::event::InpaintToolbarAction::Repaint, Icon::RefreshCw),
    ]
}

pub fn button_width(_icon: Icon) -> f32 {
    // Icon-only toolbar: fixed square button sized to toolbar height plus padding
    scale::s(TOOLBAR_HEIGHT + 6.0)
}

pub fn toolbar_width() -> f32 {
    toolbar_buttons().len() as f32 * button_width(Icon::Pencil)
}

pub fn inpaint_toolbar_width() -> f32 {
    inpaint_toolbar_buttons().len() as f32 * button_width(Icon::Trash2)
}

pub fn toolbar_rect(rect: Rectangle, width: f32, flip_at: f32) -> Rectangle {
    let tw = toolbar_width();
    let toolbar_gap = scale::s(TOOLBAR_GAP);
    let toolbar_height = scale::s(TOOLBAR_HEIGHT);
    let x = (rect.x + rect.width / 2.0 - tw / 2.0).clamp(0.0, (width - tw).max(0.0));
    let below = rect.y + rect.height + toolbar_gap;
    let y = if below + toolbar_height <= flip_at {
        below
    } else {
        (rect.y - toolbar_height - toolbar_gap).max(0.0)
    };
    Rectangle::new(Point::new(x, y), Size::new(tw, toolbar_height))
}

pub fn inpaint_toolbar_rect(rect: Rectangle, width: f32, flip_at: f32) -> Rectangle {
    let tw = inpaint_toolbar_width();
    let toolbar_gap = scale::s(TOOLBAR_GAP);
    let toolbar_height = scale::s(TOOLBAR_HEIGHT);
    let x = (rect.x + rect.width / 2.0 - tw / 2.0).clamp(0.0, (width - tw).max(0.0));
    let below = rect.y + rect.height + toolbar_gap;
    let y = if below + toolbar_height <= flip_at {
        below
    } else {
        (rect.y - toolbar_height - toolbar_gap).max(0.0)
    };
    Rectangle::new(Point::new(x, y), Size::new(tw, toolbar_height))
}

pub fn toolbar_button_rect(toolbar: Rectangle, action: crate::event::ToolbarAction) -> Rectangle {
    let mut x = toolbar.x;
    for (candidate, icon) in toolbar_buttons() {
        let width = button_width(icon);
        if candidate == action {
            return Rectangle::new(Point::new(x, toolbar.y), Size::new(width, toolbar.height));
        }
        x += width;
    }
    Rectangle::new(toolbar.position(), Size::new(0.0, toolbar.height))
}

pub fn inpaint_toolbar_button_rect(
    toolbar: Rectangle,
    action: crate::event::InpaintToolbarAction,
) -> Rectangle {
    let mut x = toolbar.x;
    for (candidate, icon) in inpaint_toolbar_buttons() {
        let width = button_width(icon);
        if candidate == action {
            return Rectangle::new(Point::new(x, toolbar.y), Size::new(width, toolbar.height));
        }
        x += width;
    }
    Rectangle::new(toolbar.position(), Size::new(0.0, toolbar.height))
}

pub fn resize_quad(tiles: &[TileSpec<'_>], state: &TileViewState, index: usize, handle: ResizeHandle, quad: Quad, local: Point) -> Option<Quad> {
    let tile = tiles.get(index)?;
    let (layout, _) = tile_layout(tiles, state.width);
    let (y, _) = layout.get(index)?;
    let scale = if tile.source_width > 0 {
        state.width / tile.source_width as f32
    } else {
        0.0
    };
    if scale <= 0.0 {
        return None;
    }
    let img_x = local.x / scale;
    let img_y = (local.y + state.offset - y) / scale;
    let min_edge = MIN_BOX_EDGE / scale;

    // Order to TL/TR/BR/BL so geometry is deterministic even if stored order drifted.
    let ordered = order_quad(quad.points);

    // Pick a rotation angle that maps the quad to an axis-aligned local space.
    // For a true rotated rectangle we get the exact angle, for a skewed / free-transformed
    // quad we fall back to the average direction of top+bottom edges (keeps resize stable).
    let angle = if let Some((_, _, _, a)) = crate::main_area::geometry::rotated_rect_geometry(ordered) {
        a
    } else {
        let top_dx = ordered[1][0] - ordered[0][0];
        let top_dy = ordered[1][1] - ordered[0][1];
        let bot_dx = ordered[2][0] - ordered[3][0];
        let bot_dy = ordered[2][1] - ordered[3][1];
        let avg_dx = (top_dx + bot_dx) * 0.5;
        let avg_dy = (top_dy + bot_dy) * 0.5;
        if avg_dx.abs() < f32::EPSILON && avg_dy.abs() < f32::EPSILON {
            top_dy.atan2(top_dx)
        } else {
            avg_dy.atan2(avg_dx)
        }
    };

    let center = quad_centroid(ordered);
    let (sin, cos) = angle.sin_cos();

    // Rotate point `p` around `center` by `angle` (positive = CCW).
    let rotate = |p: Point, s: f32, c: f32| -> Point {
        let dx = p.x - center.x;
        let dy = p.y - center.y;
        Point::new(center.x + dx * c - dy * s, center.y + dx * s + dy * c)
    };

    // World -> local (rotate by -angle)
    let local_points: [[f32; 2]; 4] = std::array::from_fn(|i| {
        let p = Point::new(ordered[i][0], ordered[i][1]);
        let lp = rotate(p, -sin, cos);
        [lp.x, lp.y]
    });

    // Local AABB (tight in local space, not the world AABB that was causing the bug)
    let mut min_lx = f32::INFINITY;
    let mut min_ly = f32::INFINITY;
    let mut max_lx = f32::NEG_INFINITY;
    let mut max_ly = f32::NEG_INFINITY;
    for p in local_points {
        min_lx = min_lx.min(p[0]);
        min_ly = min_ly.min(p[1]);
        max_lx = max_lx.max(p[0]);
        max_ly = max_ly.max(p[1]);
    }
    let old_local = [min_lx, min_ly, max_lx, max_ly];
    // Degenerate quad (should not happen) - fall back to old world refit
    if (max_lx - min_lx).abs() < f32::EPSILON || (max_ly - min_ly).abs() < f32::EPSILON {
        let start = quad.bounds();
        let [mut min_x, mut min_y, mut max_x, mut max_y] = start;
        if handle.left { min_x = img_x.min(max_x - min_edge); }
        if handle.right { max_x = img_x.max(min_x + min_edge); }
        if handle.top { min_y = img_y.min(max_y - min_edge); }
        if handle.bottom { max_y = img_y.max(min_y + min_edge); }
        return Some(quad.refit(start, [min_x, min_y, max_x, max_y]));
    }

    let cursor_local = rotate(Point::new(img_x, img_y), -sin, cos);
    let mut new_local = old_local;
    if handle.left {
        new_local[0] = cursor_local.x.min(new_local[2] - min_edge);
    }
    if handle.right {
        new_local[2] = cursor_local.x.max(new_local[0] + min_edge);
    }
    if handle.top {
        new_local[1] = cursor_local.y.min(new_local[3] - min_edge);
    }
    if handle.bottom {
        new_local[3] = cursor_local.y.max(new_local[1] + min_edge);
    }

    // Figma-style modifiers: Alt resizes from the center (mirrors the
    // opposite side), Shift preserves the press aspect ratio.
    let centered = state.keyboard_modifiers.alt();
    let preserve = state.keyboard_modifiers.shift();
    if centered {
        let cx = (old_local[0] + old_local[2]) * 0.5;
        let cy = (old_local[1] + old_local[3]) * 0.5;
        if handle.left || handle.right {
            let hw = (cursor_local.x - cx).abs().max(min_edge * 0.5);
            new_local[0] = cx - hw;
            new_local[2] = cx + hw;
        }
        if handle.top || handle.bottom {
            let hh = (cursor_local.y - cy).abs().max(min_edge * 0.5);
            new_local[1] = cy - hh;
            new_local[3] = cy + hh;
        }
    }
    if preserve {
        let old_w = (old_local[2] - old_local[0]).max(f32::EPSILON);
        let old_h = (old_local[3] - old_local[1]).max(f32::EPSILON);
        let new_w = (new_local[2] - new_local[0]).max(f32::EPSILON);
        let new_h = (new_local[3] - new_local[1]).max(f32::EPSILON);
        let is_corner = (handle.left || handle.right) && (handle.top || handle.bottom);
        let is_h = (handle.left || handle.right) && !(handle.top || handle.bottom);
        let is_v = !(handle.left || handle.right) && (handle.top || handle.bottom);
        // Dominant-axis scale so the box follows the cursor while keeping ratio.
        let mut s = if is_corner {
            let sx0 = new_w / old_w;
            let sy0 = new_h / old_h;
            if (sx0 - 1.0).abs() >= (sy0 - 1.0).abs() { sx0 } else { sy0 }
        } else if is_h {
            new_w / old_w
        } else if is_v {
            new_h / old_h
        } else {
            1.0
        };
        s = s.max(min_edge / old_w).max(min_edge / old_h).max(0.01);
        let nw = old_w * s;
        let nh = old_h * s;
        if is_corner {
            if centered {
                let cx = (old_local[0] + old_local[2]) * 0.5;
                let cy = (old_local[1] + old_local[3]) * 0.5;
                new_local = [cx - nw * 0.5, cy - nh * 0.5, cx + nw * 0.5, cy + nh * 0.5];
            } else {
                // Keep the anchored (non-dragged) side fixed.
                if handle.right {
                    new_local[0] = old_local[0];
                    new_local[2] = old_local[0] + nw;
                } else {
                    new_local[2] = old_local[2];
                    new_local[0] = old_local[2] - nw;
                }
                if handle.bottom {
                    new_local[1] = old_local[1];
                    new_local[3] = old_local[1] + nh;
                } else {
                    new_local[3] = old_local[3];
                    new_local[1] = old_local[3] - nh;
                }
            }
        } else if is_h {
            if centered {
                let cx = (old_local[0] + old_local[2]) * 0.5;
                let cy = (old_local[1] + old_local[3]) * 0.5;
                new_local = [cx - nw * 0.5, cy - nh * 0.5, cx + nw * 0.5, cy + nh * 0.5];
            } else {
                if handle.right {
                    new_local[0] = old_local[0];
                    new_local[2] = old_local[0] + nw;
                } else {
                    new_local[2] = old_local[2];
                    new_local[0] = old_local[2] - nw;
                }
                let cy = (old_local[1] + old_local[3]) * 0.5;
                new_local[1] = cy - nh * 0.5;
                new_local[3] = cy + nh * 0.5;
            }
        } else if is_v {
            if centered {
                let cx = (old_local[0] + old_local[2]) * 0.5;
                let cy = (old_local[1] + old_local[3]) * 0.5;
                new_local = [cx - nw * 0.5, cy - nh * 0.5, cx + nw * 0.5, cy + nh * 0.5];
            } else {
                if handle.bottom {
                    new_local[1] = old_local[1];
                    new_local[3] = old_local[1] + nh;
                } else {
                    new_local[3] = old_local[3];
                    new_local[1] = old_local[3] - nh;
                }
                let cx = (old_local[0] + old_local[2]) * 0.5;
                new_local[0] = cx - nw * 0.5;
                new_local[2] = cx + nw * 0.5;
            }
        }
    }

    let sx = (new_local[2] - new_local[0]) / (old_local[2] - old_local[0]).max(f32::EPSILON);
    let sy = (new_local[3] - new_local[1]) / (old_local[3] - old_local[1]).max(f32::EPSILON);

    let new_local_points: [[f32; 2]; 4] = std::array::from_fn(|i| {
        let [x, y] = local_points[i];
        let nx = new_local[0] + (x - old_local[0]) * sx;
        let ny = new_local[1] + (y - old_local[1]) * sy;
        let wp = rotate(Point::new(nx, ny), sin, cos);
        [wp.x, wp.y]
    });

    Some(Quad { points: new_local_points })
}

/// Ctrl+Shift corner distort: lock the dragged corner's displacement
/// (measured from its press position) to horizontal, vertical, or 45°
/// diagonal. Zone boundaries at 22.5°/67.5° of `atan2(|dy|, |dx|)`;
/// diagonal keeps both signs with magnitude `max(|dx|, |dy|)`.
pub fn constrain_corner_delta(dx: f32, dy: f32) -> (f32, f32) {
    if !dx.is_finite() || !dy.is_finite() {
        return (dx, dy);
    }
    let ax = dx.abs();
    let ay = dy.abs();
    if ax < f32::EPSILON && ay < f32::EPSILON {
        return (dx, dy);
    }
    let angle = ay.atan2(ax).to_degrees();
    if angle < 22.5 {
        (dx, 0.0)
    } else if angle > 67.5 {
        (0.0, dy)
    } else {
        let m = ax.max(ay);
        let sx = if dx < 0.0 { -1.0 } else { 1.0 };
        let sy = if dy < 0.0 { -1.0 } else { 1.0 };
        (sx * m, sy * m)
    }
}

/// Ctrl+Shift edge skew: keep only the dominant quad-local axis so shear
/// follows quad rotation. Ties fall through to horizontal.
pub fn constrain_skew_local(ldx: f32, ldy: f32) -> (f32, f32) {
    if !ldx.is_finite() || !ldy.is_finite() {
        return (ldx, ldy);
    }
    if ldx.abs() >= ldy.abs() {
        (ldx, 0.0)
    } else {
        (0.0, ldy)
    }
}

pub fn distort_quad(tiles: &[TileSpec<'_>], state: &TileViewState, index: usize, corner: usize, quad: Quad, local: Point) -> Option<Quad> {
    let tile = tiles.get(index)?;
    let (layout, _) = tile_layout(tiles, state.width);
    let (y, _) = layout.get(index)?;
    let scale = if tile.source_width > 0 {
        state.width / tile.source_width as f32
    } else {
        0.0
    };
    if scale <= 0.0 {
        return None;
    }
    if corner >= 4 {
        return None;
    }
    let img_x = local.x / scale;
    let img_y = (local.y + state.offset - y) / scale;
    if !img_x.is_finite() || !img_y.is_finite() {
        return None;
    }
    // Free-transform cap: the dragged corner must never break planarity
    // (bow-tie / concave flip / collapsed line). No image-bounds or stretch
    // cap by design — only the topology guard below.
    let reference = quad.points;
    let reference_sign = quad_winding(reference);
    let (min_len, min_area) = distort_floors(reference, MIN_BOX_EDGE / scale);
    let start = reference[corner];
    let mut target = [img_x, img_y];
    // Ctrl+Shift: axis-constrain the corner displacement from its press
    // position (horizontal / vertical / 45° diagonal).
    if state.keyboard_modifiers.shift() {
        let (dx, dy) = constrain_corner_delta(target[0] - start[0], target[1] - start[1]);
        target = [start[0] + dx, start[1] + dy];
    }
    let mut candidate = reference;
    candidate[corner] = target;
    if distort_valid(candidate, reference_sign, min_len, min_area) {
        return Some(Quad { points: candidate });
    }
    // Press position is the known-good anchor. If even that fails (legacy
    // degenerate save), only allow drags that heal back to a valid quad.
    if !distort_valid(reference, reference_sign, min_len, min_area) {
        return None;
    }
    // Clamp to the nearest valid point along the press -> cursor segment:
    // largest `t` that keeps the quad convex, correctly wound, and non-flat.
    let mut lo = 0.0f32;
    let mut hi = 1.0f32;
    for _ in 0..16 {
        let mid = (lo + hi) * 0.5;
        let mut probe = reference;
        probe[corner] = [start[0] + (target[0] - start[0]) * mid, start[1] + (target[1] - start[1]) * mid];
        if distort_valid(probe, reference_sign, min_len, min_area) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    if lo <= 0.0 {
        return None;
    }
    let mut clamped = reference;
    clamped[corner] = [
        start[0] + (target[0] - start[0]) * lo,
        start[1] + (target[1] - start[1]) * lo,
    ];
    if distort_valid(clamped, reference_sign, min_len, min_area) {
        Some(Quad { points: clamped })
    } else {
        None
    }
}

/// Ctrl-drag on a side handle: free-move the whole edge (skew). Both edge
/// corners translate by the press -> cursor delta so the opposite edge stays
/// fixed, mirroring corner `distort_quad` but with two corners. Rotation-aware
/// via the same local frame as `resize_quad`; the delta round-trips through
/// that frame so shear follows quad rotation.
pub fn skew_quad(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    index: usize,
    handle: ResizeHandle,
    quad: Quad,
    press: Point,
    local: Point,
) -> Option<Quad> {
    let edge = handle.edge()?;
    let tile = tiles.get(index)?;
    let (layout, _) = tile_layout(tiles, state.width);
    let (y, _) = layout.get(index)?;
    let scale = if tile.source_width > 0 {
        state.width / tile.source_width as f32
    } else {
        0.0
    };
    if scale <= 0.0 {
        return None;
    }
    let press_img_x = press.x / scale;
    let press_img_y = (press.y + state.offset - y) / scale;
    let img_x = local.x / scale;
    let img_y = (local.y + state.offset - y) / scale;
    if !press_img_x.is_finite()
        || !press_img_y.is_finite()
        || !img_x.is_finite()
        || !img_y.is_finite()
    {
        return None;
    }
    let reference = quad.points;
    let (a, b) = match edge {
        0 => (0, 1),
        1 => (1, 2),
        2 => (2, 3),
        3 => (3, 0),
        _ => return None,
    };
    // Same local frame as `resize_quad` so shear follows quad rotation.
    let ordered = order_quad(reference);
    let angle = if let Some((_, _, _, a)) = crate::main_area::geometry::rotated_rect_geometry(ordered) {
        a
    } else {
        let top_dx = ordered[1][0] - ordered[0][0];
        let top_dy = ordered[1][1] - ordered[0][1];
        let bot_dx = ordered[2][0] - ordered[3][0];
        let bot_dy = ordered[2][1] - ordered[3][1];
        let avg_dx = (top_dx + bot_dx) * 0.5;
        let avg_dy = (top_dy + bot_dy) * 0.5;
        if avg_dx.abs() < f32::EPSILON && avg_dy.abs() < f32::EPSILON {
            top_dy.atan2(top_dx)
        } else {
            avg_dy.atan2(avg_dx)
        }
    };
    let center = quad_centroid(ordered);
    let (sin, cos) = angle.sin_cos();
    // Image-space drag delta, rotated into the quad-local frame.
    let dx = img_x - press_img_x;
    let dy = img_y - press_img_y;
    let (local_dx, local_dy) = {
        let ldx = dx * cos + dy * sin;
        let ldy = -dx * sin + dy * cos;
        // Ctrl+Shift: axis-lock the shear to the dominant quad-local axis.
        if state.keyboard_modifiers.shift() {
            constrain_skew_local(ldx, ldy)
        } else {
            (ldx, ldy)
        }
    };
    // Back to image space (round-trip is identity for a rigid translation,
    // but keeps the math explicitly in the quad-local frame).
    let eff_dx = local_dx * cos - local_dy * sin;
    let eff_dy = local_dx * sin + local_dy * cos;
    let _ = (center, sin, cos);
    // Free-transform cap, same topology guard as `distort_quad`.
    let reference_sign = quad_winding(reference);
    let (min_len, min_area) = distort_floors(reference, MIN_BOX_EDGE / scale);
    let apply = |points: [[f32; 2]; 4], t: f32| -> [[f32; 2]; 4] {
        let mut out = points;
        out[a] = [reference[a][0] + eff_dx * t, reference[a][1] + eff_dy * t];
        out[b] = [reference[b][0] + eff_dx * t, reference[b][1] + eff_dy * t];
        out
    };
    let candidate = apply(reference, 1.0);
    if distort_valid(candidate, reference_sign, min_len, min_area) {
        return Some(Quad { points: candidate });
    }
    if !distort_valid(reference, reference_sign, min_len, min_area) {
        return None;
    }
    // Clamp to the largest valid `t` along the press -> cursor segment.
    let mut lo = 0.0f32;
    let mut hi = 1.0f32;
    for _ in 0..16 {
        let mid = (lo + hi) * 0.5;
        if distort_valid(apply(reference, mid), reference_sign, min_len, min_area) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    if lo <= 0.0 {
        return None;
    }
    let clamped = apply(reference, lo);
    if distort_valid(clamped, reference_sign, min_len, min_area) {
        Some(Quad { points: clamped })
    } else {
        None
    }
}

/// Signed double area (>0 for TL/TR/BR/BL order). Zero means flat.
fn quad_area2(points: [[f32; 2]; 4]) -> f32 {
    let mut sum = 0.0f32;
    for i in 0..4 {
        let [x0, y0] = points[i];
        let [x1, y1] = points[(i + 1) % 4];
        sum += x0 * y1 - x1 * y0;
    }
    sum
}

/// Winding of the press quad; defaults to +1 for a degenerate press so the
/// convexity test still has a reference direction.
fn quad_winding(points: [[f32; 2]; 4]) -> f32 {
    let area2 = quad_area2(points);
    if area2 > 1e-6 {
        1.0
    } else if area2 < -1e-6 {
        -1.0
    } else {
        1.0
    }
}

/// Collapse floors in image px. Adaptive so tiny OCR boxes (already smaller
/// than `MIN_BOX_EDGE / scale`) stay distortable down to half their press
/// size instead of freezing on grab.
fn distort_floors(press: [[f32; 2]; 4], min_edge: f32) -> (f32, f32) {
    let mut smallest = f32::INFINITY;
    for i in 0..4 {
        let [x0, y0] = press[i];
        let [x1, y1] = press[(i + 1) % 4];
        if x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite() {
            smallest = smallest.min((x1 - x0).hypot(y1 - y0));
        }
    }
    let min_len = if smallest.is_finite() {
        min_edge.min(smallest * 0.5).max(1.0)
    } else {
        min_edge.max(1.0)
    };
    (min_len, min_len * min_len)
}

/// Strictly convex, consistently wound, non-collapsed. One test rejects
/// bow-ties (mixed cross signs), concave darts, flipped winding, and lines.
fn distort_valid(points: [[f32; 2]; 4], reference_sign: f32, min_len: f32, min_area: f32) -> bool {
    for p in points {
        if !p[0].is_finite() || !p[1].is_finite() {
            return false;
        }
    }
    for i in 0..4 {
        let [x0, y0] = points[i];
        let [x1, y1] = points[(i + 1) % 4];
        if ((x1 - x0).hypot(y1 - y0)) < min_len {
            return false;
        }
    }
    if quad_area2(points) * reference_sign <= min_area {
        return false;
    }
    for i in 0..4 {
        let [x0, y0] = points[i];
        let [x1, y1] = points[(i + 1) % 4];
        let [x2, y2] = points[(i + 2) % 4];
        let cross = (x1 - x0) * (y2 - y1) - (y1 - y0) * (x2 - x1);
        if cross * reference_sign <= 1e-6 {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod constrain_tests {
    use super::{constrain_corner_delta, constrain_skew_local};

    #[test]
    fn corner_horizontal_zone_zeroes_dy() {
        assert_eq!(constrain_corner_delta(10.0, 1.0), (10.0, 0.0));
        assert_eq!(constrain_corner_delta(-8.0, 2.0), (-8.0, 0.0));
    }

    #[test]
    fn corner_vertical_zone_zeroes_dx() {
        assert_eq!(constrain_corner_delta(1.0, 10.0), (0.0, 10.0));
        assert_eq!(constrain_corner_delta(2.0, -8.0), (0.0, -8.0));
    }

    #[test]
    fn corner_diagonal_zone_snaps_to_45_degrees() {
        assert_eq!(constrain_corner_delta(10.0, 9.0), (10.0, 10.0));
        assert_eq!(constrain_corner_delta(-6.0, 8.0), (-8.0, 8.0));
        assert_eq!(constrain_corner_delta(5.0, -3.0), (5.0, -5.0));
    }

    #[test]
    fn corner_zero_delta_passes_through() {
        assert_eq!(constrain_corner_delta(0.0, 0.0), (0.0, 0.0));
    }

    #[test]
    fn skew_keeps_dominant_axis_only() {
        assert_eq!(constrain_skew_local(5.0, 2.0), (5.0, 0.0));
        assert_eq!(constrain_skew_local(2.0, 5.0), (0.0, 5.0));
        assert_eq!(constrain_skew_local(3.0, -7.0), (0.0, -7.0));
    }

    #[test]
    fn skew_tie_falls_through_to_horizontal() {
        assert_eq!(constrain_skew_local(4.0, 4.0), (4.0, 0.0));
    }
}
