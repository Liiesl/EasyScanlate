use iced::advanced::graphics::geometry::{self, Fill, Path, Stroke, Text};
use iced::border::Radius;
use iced::{Color, Font, Pixels, Point, Rectangle, Size};

use crate::main_area::geometry::order_quad;
use crate::scale;

use super::constants::{
    FAILED_BG, FAILED_FG, HANDLE_BORDER, HANDLE_FILL, HANDLE_SIZE, INPAINT_FILL, INPAINT_STROKE,
    PLACEHOLDER_BG, PLACEHOLDER_FG, SCROLLBAR_THUMB, SCROLLBAR_TRACK, TOOLBAR_BG, TOOLBAR_FG,
    TOOLBAR_HOVER_BG,
};
use super::hit_test::{hit_inpaint_toolbar_button, hit_toolbar_button};
use super::interaction::Interaction;
use super::motion::{
    button_width, handle_anchors, handle_rect, inpaint_toolbar_button_rect, inpaint_toolbar_buttons,
    inpaint_toolbar_rect, toolbar_buttons, toolbar_rect, top_decor_geometry,
};
use super::scroll::{thumb_rect, track_rect};
use super::state::TileViewState;
use super::TileSpec;

pub fn draw_scrollbar<F>(frame: &mut F, state: &TileViewState, bounds: Rectangle)
where
    F: geometry::frame::Backend,
{
    let track = track_rect(bounds);
    let thumb = thumb_rect(bounds, state);
    frame.fill_rectangle(track.position(), track.size(), Fill::from(SCROLLBAR_TRACK));
    frame.fill_rectangle(thumb.position(), thumb.size(), Fill::from(SCROLLBAR_THUMB));
}

pub fn draw_placeholder<F>(frame: &mut F, failed: bool, _font: Font)
where
    F: geometry::frame::Backend,
{
    let (bg, fg, label) = if failed {
        (FAILED_BG, FAILED_FG, "Failed to load")
    } else {
        (PLACEHOLDER_BG, PLACEHOLDER_FG, "Loading...")
    };
    frame.fill_rectangle(Point::ORIGIN, frame.size(), Fill::from(bg));
    frame.fill_text(Text {
        content: label.to_string(),
        position: frame.center(),
        max_width: frame.width(),
        size: Pixels(16.0),
        color: fg,
        ..Text::default()
    });
}

pub fn draw_inpaint_marquee<F>(frame: &mut F, start: Point, current: Point, tile: Size)
where
    F: geometry::frame::Backend,
{
    let x0 = start.x.min(current.x).clamp(0.0, tile.width);
    let y0 = start.y.min(current.y).clamp(0.0, tile.height);
    let x1 = start.x.max(current.x).clamp(0.0, tile.width);
    let y1 = start.y.max(current.y).clamp(0.0, tile.height);
    let rect = Rectangle::new(Point::new(x0, y0), Size::new(x1 - x0, y1 - y0));
    frame.fill_rectangle(rect.position(), rect.size(), Fill::from(INPAINT_FILL));
    frame.stroke(
        &Path::rectangle(rect.position(), rect.size()),
        Stroke::default().with_color(INPAINT_STROKE).with_width(1.0),
    );
}

pub fn draw_action_button<F>(frame: &mut F, rect: Rectangle, label: &str, hovered: bool)
where
    F: geometry::frame::Backend,
{
    if hovered {
        frame.fill(
            &Path::rounded_rectangle(rect.position(), rect.size(), Radius::from(scale::s(4.0))),
            Fill::from(TOOLBAR_HOVER_BG),
        );
    } else {
        frame.fill(
            &Path::rounded_rectangle(rect.position(), rect.size(), Radius::from(scale::s(4.0))),
            Fill::from(TOOLBAR_BG),
        );
    }
    frame.fill_text(Text {
        content: label.to_string(),
        position: Point::new(rect.x, rect.y + (rect.height - scale::s(13.0)).max(0.0) / 2.0),
        max_width: rect.width,
        size: Pixels(scale::s(11.0)),
        color: TOOLBAR_FG,
        ..Text::default()
    });
}

fn draw_toolbar_button<F>(frame: &mut F, toolbar: Rectangle, action: crate::event::ToolbarAction, hovered: bool)
where
    F: geometry::frame::Backend,
{
    let rect = super::motion::toolbar_button_rect(toolbar, action);
    let label = toolbar_buttons()
        .into_iter()
        .find(|(candidate, _)| *candidate == action)
        .map(|(_, label)| label)
        .unwrap_or("");
    draw_action_button(frame, rect, label, hovered);
}

pub fn draw_overlay_buttons<F>(frame: &mut F, bounds: Rectangle, hovered: Option<super::interaction::OverlayButton>)
where
    F: geometry::frame::Backend,
{
    for (rect, (button, label)) in super::hit_test::overlay_button_rects(bounds)
        .into_iter()
        .zip(super::interaction::OverlayButton::column())
    {
        draw_action_button(frame, rect, label, hovered == Some(button));
    }
}

pub fn draw_top_decor<F>(frame: &mut F, decor: super::interaction::TopDecor, show_revert: bool, hover: bool)
where
    F: geometry::frame::Backend,
{
    frame.stroke(
        &Path::line(decor.stem_from, decor.anchor),
        Stroke::default().with_color(HANDLE_BORDER).with_width(scale::s(1.5)),
    );
    let knob = Path::circle(decor.anchor, scale::s(HANDLE_SIZE) / 2.0);
    frame.fill(&knob, Fill::from(HANDLE_FILL));
    frame.stroke(&knob, Stroke::default().with_color(HANDLE_BORDER).with_width(scale::s(1.5)));
    if show_revert {
        draw_action_button(frame, decor.revert, "Revert", hover);
    }
}

pub fn draw_selection_decorations<'a, F>(
    frame: &mut F,
    state: &TileViewState,
    tiles: &[TileSpec<'a>],
    tile_index: usize,
    cursor_local: Option<Point>,
    flip_from: f32,
    flip_at: f32,
    show_overlay_text: bool,
) where
    F: geometry::frame::Backend,
{
    let Some(entry) = tiles[tile_index].overlays.iter().find(|e| e.selected) else {
        return;
    };
    if entry.hide_text || !show_overlay_text {
        return;
    }
    let interacting_with_selected = match state.interaction {
        Interaction::Dragging { index, id, .. }
        | Interaction::Resizing { index, id, .. }
        | Interaction::Distorting { index, id, .. }
        | Interaction::Rotating { index, id, .. } => index == tile_index && id == entry.id,
        _ => false,
    };
    if interacting_with_selected {
        return;
    }
    let scale = frame.width() / tiles[tile_index].source_width.max(1) as f32;
    let quad = order_quad(entry.quad.points.map(|p| [p[0] * scale, p[1] * scale]));
    let rect = Rectangle::new(
        Point::new(entry.bounds[0] * scale, entry.bounds[1] * scale),
        Size::new(
            (entry.bounds[2] - entry.bounds[0]) * scale,
            (entry.bounds[3] - entry.bounds[1]) * scale,
        ),
    );
    if !state.inpaint_mode() {
        let anchors = handle_anchors(quad);
        for (_, anchor) in anchors {
            let handle = handle_rect(anchor);
            frame.fill_rectangle(handle.position(), handle.size(), Fill::from(HANDLE_FILL));
            frame.stroke(
                &Path::rectangle(handle.position(), handle.size()),
                Stroke::default().with_color(HANDLE_BORDER).with_width(scale::s(1.0)),
            );
        }
        let decor = top_decor_geometry(rect, quad, frame.width(), flip_from, flip_at);
        let hover = cursor_local.is_some_and(|local| decor.revert.contains(local));
        draw_top_decor(frame, decor, entry.quad_overridden, hover);
    }
    let toolbar = toolbar_rect(rect, frame.width(), flip_at);
    let hover = cursor_local.and_then(|local| hit_toolbar_button(toolbar, local));
    for (action, _) in toolbar_buttons() {
        draw_toolbar_button(frame, toolbar, action, hover == Some(action));
    }
}

/// Draws the static highlight around the selected inpaint patch on `tile_index`.
/// No handles, no top decor, no drag. Only a border (like result selection)
/// plus a floating Delete / Repaint toolbar below the bbox.
pub fn draw_inpaint_decorations<'a, F>(
    frame: &mut F,
    tiles: &[TileSpec<'a>],
    tile_index: usize,
    selected_inpaint: Option<(usize, usize)>,
    cursor_local: Option<Point>,
    flip_at: f32,
) where
    F: geometry::frame::Backend,
{
    let Some((img_idx, patch_idx)) = selected_inpaint else {
        return;
    };
    if img_idx != tile_index {
        return;
    }
    let tile = &tiles[tile_index];
    let Some(layer) = tile.inpaint.get(patch_idx) else {
        return;
    };
    let scale = frame.width() / tile.source_width.max(1) as f32;
    let [x, y, w, h] = layer.bounds;
    let rect = Rectangle::new(
        Point::new(x * scale, y * scale),
        Size::new(w * scale, h * scale),
    );
    // Border highlight – same color as overlay selection.
    frame.stroke(
        &Path::rectangle(rect.position(), rect.size()),
        Stroke::default()
            .with_color(Color::from_rgba8(92, 190, 255, 1.0))
            .with_width(scale::s(2.0)),
    );
    // Floating toolbar below (like OCR). No move/resize.
    let toolbar = inpaint_toolbar_rect(rect, frame.width(), flip_at);
    let hover = cursor_local.and_then(|local| hit_inpaint_toolbar_button(toolbar, local));
    for (action, label) in inpaint_toolbar_buttons() {
        let r = inpaint_toolbar_button_rect(toolbar, action);
        draw_action_button(frame, r, label, hover == Some(action));
    }
}
