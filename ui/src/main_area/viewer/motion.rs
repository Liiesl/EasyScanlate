use iced::{Point, Rectangle, Size};

use lucide_icons::Icon;
use scanlateit_model::{EntryId, Quad};

use crate::main_area::geometry::order_quad;
use crate::scale;

use super::constants::{
    HANDLE_SIZE, MIN_BOX_EDGE, ROTATE_STEM, TOOLBAR_BTN_PAD, TOOLBAR_GAP, TOOLBAR_HEIGHT,
};
use super::hit_test::selected_quad_view;
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

pub fn drag_quad(tiles: &[TileSpec<'_>], state: &TileViewState, index: usize, local: Point, offset: [f32; 2], quad: Quad) -> Option<Quad> {
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
    let min_x = img_x - offset[0];
    let min_y = img_y - offset[1];
    Some(quad.translate(min_x - size[0], min_y - size[1]))
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
    let start = quad.bounds();
    let [mut min_x, mut min_y, mut max_x, mut max_y] = start;
    if handle.left {
        min_x = img_x.min(max_x - min_edge);
    }
    if handle.right {
        max_x = img_x.max(min_x + min_edge);
    }
    if handle.top {
        min_y = img_y.min(max_y - min_edge);
    }
    if handle.bottom {
        max_y = img_y.max(min_y + min_edge);
    }
    Some(quad.refit(start, [min_x, min_y, max_x, max_y]))
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
    let img_x = local.x / scale;
    let img_y = (local.y + state.offset - y) / scale;
    let mut points = quad.points;
    points[corner] = [img_x, img_y];
    Some(Quad { points })
}
