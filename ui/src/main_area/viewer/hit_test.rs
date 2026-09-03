use iced::{Point, Rectangle, Size};

use easyscanlate_model::EntryId;

use crate::main_area::geometry::order_quad;
use crate::scale;

use super::interaction::{OverlayButton, SaveMenuButton, TopDecorHit};
use super::layout::tile_layout;
use super::motion::{
    button_width, handle_anchors, handle_rect, inpaint_toolbar_buttons, inpaint_toolbar_rect,
    toolbar_buttons, toolbar_rect, top_decor_geometry,
};
use super::state::TileViewState;
use super::TileSpec;

pub fn local_point(position: Point, bounds: Rectangle) -> Point {
    Point::new(position.x - bounds.position().x, position.y - bounds.position().y)
}

pub fn hit_tile(tiles: &[TileSpec<'_>], state: &TileViewState, local: Point) -> Option<usize> {
    let (layout, _) = tile_layout(tiles, state.width);
    let content_y = local.y + state.offset;
    layout
        .iter()
        .enumerate()
        .find(|(_, (y, height))| content_y >= *y && content_y < y + height)
        .map(|(index, _)| index)
}

pub fn tile_local_point(layout: &[(f32, f32)], index: usize, local: Point, offset: f32) -> Point {
    Point::new(local.x, local.y + offset - layout[index].0)
}

pub fn point_in_quad(point: Point, quad: [[f32; 2]; 4]) -> bool {
    let p = [point.x, point.y];
    let mut sign = 0.0;
    for i in 0..4 {
        let a = quad[i];
        let b = quad[(i + 1) % 4];
        let cross = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        let side = cross.signum();
        if side == 0.0 {
            continue;
        }
        if sign == 0.0 {
            sign = side;
        } else if side != sign {
            return false;
        }
    }
    true
}

pub fn hit_entry(tiles: &[TileSpec<'_>], state: &TileViewState, local: Point) -> Option<(usize, EntryId)> {
    let (layout, _) = tile_layout(tiles, state.width);
    let mut hit = None;
    for (index, tile) in tiles.iter().enumerate() {
        let (y, _) = layout[index];
        let scale = if tile.source_width > 0 {
            state.width / tile.source_width as f32
        } else {
            0.0
        };
        if scale <= 0.0 {
            continue;
        }
        for entry in &tile.overlays {
            let [min_x, min_y, max_x, max_y] = entry.bounds;
            let rect = Rectangle::new(
                Point::new(min_x * scale, y + min_y * scale - state.offset),
                Size::new((max_x - min_x) * scale, (max_y - min_y) * scale),
            );
            if !rect.contains(local) {
                continue;
            }
            let quad = order_quad(entry.quad.points.map(|p| {
                [p[0] * scale, y + p[1] * scale - state.offset]
            }));
            if point_in_quad(local, quad) {
                hit = Some((index, entry.id));
            }
        }
    }
    hit
}

pub fn entry_rect(tiles: &[TileSpec<'_>], state: &TileViewState, index: usize, id: EntryId) -> Option<Rectangle> {
    let tile = tiles.get(index)?;
    let (layout, _) = tile_layout(tiles, state.width);
    let (y, _) = layout.get(index)?;
    let scale = if tile.source_width > 0 {
        state.width / tile.source_width as f32
    } else {
        0.0
    };
    let (min_x, min_y, max_x, max_y) = {
        let [min_x, min_y, max_x, max_y] = tile.overlays.iter().find(|e| e.id == id)?.bounds;
        (min_x, min_y, max_x, max_y)
    };
    Some(Rectangle::new(
        Point::new(min_x * scale, y + min_y * scale - state.offset),
        Size::new((max_x - min_x) * scale, (max_y - min_y) * scale),
    ))
}

pub fn reveal_offset(tiles: &[TileSpec<'_>], state: &TileViewState, index: usize, id: EntryId) -> Option<f32> {
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
    let [_, min_y, _, max_y] = entry.bounds;
    let top = y + min_y * scale;
    let bottom = y + max_y * scale;
    let viewport = state.viewport_height;
    if viewport <= 0.0 {
        return None;
    }
    let max_offset = (state.content_height - viewport).max(0.0);
    if top >= state.offset && bottom <= state.offset + viewport {
        return None;
    }
    let target = (top - (viewport - (bottom - top)) / 2.0).clamp(0.0, max_offset);
    (target != state.offset).then_some(target)
}

pub fn editing_rect(tiles: &[TileSpec<'_>], state: &TileViewState, editing: (usize, EntryId)) -> Option<Rectangle> {
    let (index, id) = editing;
    entry_rect(tiles, state, index, id)
}

pub fn selected_rect(tiles: &[TileSpec<'_>], state: &TileViewState) -> Option<(usize, Rectangle)> {
    let (index, entry) = tiles
        .iter()
        .enumerate()
        .find_map(|(index, tile)| tile.overlays.iter().find(|e| e.selected).map(|e| (index, e)))?;
    Some((index, entry_rect(tiles, state, index, entry.id)?))
}

pub fn selected_quad_view(tiles: &[TileSpec<'_>], state: &TileViewState, index: usize) -> Option<[[f32; 2]; 4]> {
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
    let entry = tile.overlays.iter().find(|e| e.selected)?;
    Some(order_quad(entry.quad.points.map(|p| {
        [p[0] * scale, y + p[1] * scale - state.offset]
    })))
}

pub fn entry_quad(tiles: &[TileSpec<'_>], index: usize, id: EntryId) -> Option<easyscanlate_model::Quad> {
    let tile = tiles.get(index)?;
    tile.overlays.iter().find(|e| e.id == id).map(|e| e.quad)
}

pub fn hit_handle(tiles: &[TileSpec<'_>], state: &TileViewState, local: Point) -> Option<(usize, EntryId, super::interaction::ResizeHandle)> {
    let (index, entry) = tiles.iter().enumerate().find_map(|(index, tile)| {
        tile.overlays.iter().find(|e| e.selected).map(|e| (index, e))
    })?;
    let id = entry.id;
    let quad = selected_quad_view(tiles, state, index)?;
    for (handle, anchor) in handle_anchors(quad) {
        if handle_rect(anchor).contains(local) {
            return Some((index, id, handle));
        }
    }
    None
}

pub fn hit_top_decor(tiles: &[TileSpec<'_>], state: &TileViewState, local: Point) -> Option<(usize, EntryId, TopDecorHit)> {
    let (index, entry) = tiles.iter().enumerate().find_map(|(index, tile)| {
        tile.overlays.iter().find(|e| e.selected).map(|e| (index, e))
    })?;
    let id = entry.id;
    if state.inpaint_mode() || state.ocr_mode() {
        return None;
    }
    let (_, rect) = selected_rect(tiles, state)?;
    let quad = selected_quad_view(tiles, state, index)?;
    // rect/quad are viewport-relative (global - offset) and `local` is
    // viewport-local (0..viewport_height), so the decor must be built in
    // the same viewport-local space (0..vh), not global offset..offset+vh.
    let decor = top_decor_geometry(rect, quad, state.width, 0.0, state.viewport_height);
    if handle_rect(decor.anchor).contains(local) {
        return Some((index, id, TopDecorHit::Rotate));
    }
    if entry.quad_overridden && decor.revert.contains(local) {
        return Some((index, id, TopDecorHit::Revert));
    }
    None
}

pub fn hit_toolbar_button(toolbar: Rectangle, local: Point) -> Option<crate::event::ToolbarAction> {
    if !toolbar.contains(local) {
        return None;
    }
    let mut x = toolbar.x;
    for (action, label) in toolbar_buttons() {
        let width = button_width(label);
        if local.x < x + width {
            return Some(action);
        }
        x += width;
    }
    None
}

pub fn hit_toolbar(tiles: &[TileSpec<'_>], state: &TileViewState, local: Point) -> Option<(usize, EntryId, crate::event::ToolbarAction)> {
    let (index, rect) = selected_rect(tiles, state)?;
    let toolbar = toolbar_rect(rect, state.width, state.viewport_height);
    let id = tiles[index].overlays.iter().find(|e| e.selected)?.id;
    hit_toolbar_button(toolbar, local).map(|action| (index, id, action))
}

pub fn hit_inpaint_toolbar_button(
    toolbar: Rectangle,
    local: Point,
) -> Option<crate::event::InpaintToolbarAction> {
    if !toolbar.contains(local) {
        return None;
    }
    let mut x = toolbar.x;
    for (action, label) in inpaint_toolbar_buttons() {
        let width = button_width(label);
        if local.x < x + width {
            return Some(action);
        }
        x += width;
    }
    None
}

pub fn hit_inpaint_toolbar(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    selected_inpaint: Option<(usize, usize)>,
    local: Point,
) -> Option<(usize, usize, crate::event::InpaintToolbarAction)> {
    let (img_idx, patch_idx) = selected_inpaint?;
    let tile = tiles.get(img_idx)?;
    let layer = tile.inpaint.get(patch_idx)?;
    let (layout, _) = tile_layout(tiles, state.width);
    let (y, _) = layout.get(img_idx)?;
    let scale = if tile.source_width > 0 {
        state.width / tile.source_width as f32
    } else {
        0.0
    };
    if scale <= 0.0 {
        return None;
    }
    let [x, yy, w, h] = layer.bounds;
    let rect = Rectangle::new(
        Point::new(x * scale, y + yy * scale - state.offset),
        Size::new(w * scale, h * scale),
    );
    let toolbar = inpaint_toolbar_rect(rect, state.width, state.viewport_height);
    hit_inpaint_toolbar_button(toolbar, local).map(|action| (img_idx, patch_idx, action))
}

pub fn inpaint_reveal_offset(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    index: usize,
    bounds: [f32; 4],
) -> Option<f32> {
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
    let [_, min_y, _, h] = bounds;
    let top = y + min_y * scale;
    let bottom = top + h * scale;
    let viewport = state.viewport_height;
    if viewport <= 0.0 {
        return None;
    }
    let max_offset = (state.content_height - viewport).max(0.0);
    if top >= state.offset && bottom <= state.offset + viewport {
        return None;
    }
    let target = (top - (viewport - (bottom - top)) / 2.0).clamp(0.0, max_offset);
    (target != state.offset).then_some(target)
}

pub fn overlay_button_rects(bounds: Rectangle) -> [Rectangle; 3] {
    use super::constants::{
        OVERLAY_BTN_GAP, OVERLAY_BTN_MARGIN, OVERLAY_CIRCLE_DIAMETER, OVERLAY_SAVE_HEIGHT,
        OVERLAY_SAVE_WIDTH,
    };
    let circle = scale::s(OVERLAY_CIRCLE_DIAMETER);
    let save_w = scale::s(OVERLAY_SAVE_WIDTH);
    let save_h = scale::s(OVERLAY_SAVE_HEIGHT);
    let gap = scale::s(OVERLAY_BTN_GAP);
    let margin = scale::s(OVERLAY_BTN_MARGIN);
    let total = circle * 2.0 + save_h + gap * 2.0;
    let top = (bounds.height - margin - total).max(0.0);
    let mut rects = [Rectangle::new(Point::ORIGIN, Size::ZERO); 3];
    // GoTop circle
    rects[0] = Rectangle::new(Point::new(margin, top), Size::new(circle, circle));
    // Save rectangle (centered relative to circle if wider)
    rects[1] = Rectangle::new(
        Point::new(margin, top + circle + gap),
        Size::new(save_w, save_h),
    );
    // GoBottom circle
    rects[2] = Rectangle::new(
        Point::new(margin, top + circle + gap + save_h + gap),
        Size::new(circle, circle),
    );
    rects
}

pub fn hit_overlay_button(bounds: Rectangle, local: Point) -> Option<OverlayButton> {
    overlay_button_rects(bounds)
        .into_iter()
        .zip(OverlayButton::column())
        .find_map(|(rect, (button, _))| {
            let inside = match button {
                OverlayButton::GoTop | OverlayButton::GoBottom => {
                    let center = Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
                    let r = rect.width.min(rect.height) / 2.0;
                    let dx = local.x - center.x;
                    let dy = local.y - center.y;
                    dx * dx + dy * dy <= r * r
                }
                OverlayButton::Save => rect.contains(local),
            };
            inside.then_some(button)
        })
}

pub fn save_menu_button_rects(bounds: Rectangle) -> [Rectangle; 2] {
    use super::constants::{
        OVERLAY_BTN_GAP, OVERLAY_BTN_MARGIN, OVERLAY_CIRCLE_DIAMETER, OVERLAY_SAVE_HEIGHT,
        OVERLAY_SAVE_WIDTH, SAVE_MENU_GAP, SAVE_MENU_HEIGHT, SAVE_MENU_VGAP, SAVE_MENU_WIDTH,
    };
    let circle = scale::s(OVERLAY_CIRCLE_DIAMETER);
    let save_w = scale::s(OVERLAY_SAVE_WIDTH);
    let save_h = scale::s(OVERLAY_SAVE_HEIGHT);
    let gap = scale::s(OVERLAY_BTN_GAP);
    let margin = scale::s(OVERLAY_BTN_MARGIN);
    let menu_w = scale::s(SAVE_MENU_WIDTH);
    let menu_h = scale::s(SAVE_MENU_HEIGHT);
    let menu_gap = scale::s(SAVE_MENU_GAP);
    let menu_vgap = scale::s(SAVE_MENU_VGAP);
    let total = circle * 2.0 + save_h + gap * 2.0;
    let top = (bounds.height - margin - total).max(0.0);
    let save_y = top + circle + gap;
    let save_x = margin;
    let menu_x = save_x + save_w + menu_gap;
    let mut rects = [Rectangle::new(Point::ORIGIN, Size::ZERO); 2];
    // First menu button aligned to the right of Save, top-aligned with Save
    rects[0] = Rectangle::new(Point::new(menu_x, save_y), Size::new(menu_w, menu_h));
    // Second menu button vertically below first
    rects[1] = Rectangle::new(
        Point::new(menu_x, save_y + menu_h + menu_vgap),
        Size::new(menu_w, menu_h),
    );
    rects
}

pub fn hit_save_menu_button(bounds: Rectangle, local: Point) -> Option<SaveMenuButton> {
    save_menu_button_rects(bounds)
        .into_iter()
        .zip(SaveMenuButton::all())
        .find_map(|(rect, (button, _))| rect.contains(local).then_some(button))
}
