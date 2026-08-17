use std::ops::Range;

use iced::advanced::Shell;
use iced::{Point, Rectangle, Size};

use super::constants::{MIN_THUMB_HEIGHT, SCROLLBAR_MARGIN, SCROLLBAR_WIDTH};
use super::hit_test::editing_rect;
use super::layout::tile_layout;
use super::state::TileViewState;
use super::TileSpec;

pub fn scroll_by(state: &mut TileViewState, delta: f32) -> bool {
    let max_offset = (state.content_height - state.viewport_height).max(0.0);
    let new = (state.offset + delta).clamp(0.0, max_offset);
    if (new - state.offset).abs() > f32::EPSILON {
        state.offset = new;
        true
    } else {
        false
    }
}

pub fn visible_range(tiles: &[TileSpec<'_>], state: &TileViewState) -> Option<Range<usize>> {
    if tiles.is_empty() {
        return None;
    }
    let (layout, _) = tile_layout(tiles, state.width);
    let top = state.offset;
    let bottom = state.offset + state.viewport_height;
    let mut first = None;
    let mut last = None;
    for (index, (y, height)) in layout.iter().enumerate() {
        if y + height > top && *y < bottom {
            first.get_or_insert(index);
            last = Some(index);
        }
    }
    match (first, last) {
        (Some(first), Some(last)) => Some(first..last + 1),
        _ => None,
    }
}

pub fn publish_visible<'a, Message, F>(
    shell: &mut Shell<'_, Message>,
    tiles: &[TileSpec<'a>],
    state: &mut TileViewState,
    on_visible_range: &Option<F>,
) where
    F: Fn(Range<usize>) -> Message,
{
    let range = visible_range(tiles, state);
    if state.last_visible != range {
        state.last_visible = range.clone();
        if let (Some(range), Some(callback)) = (range, on_visible_range.as_ref()) {
            shell.publish(callback(range));
        }
    }
}

pub fn publish_offset<'a, Message, R>(
    shell: &mut Shell<'_, Message>,
    state: &mut TileViewState,
    on_scroll: &Option<R>,
) where
    R: Fn(f32) -> Message,
{
    if state.last_published_offset != Some(state.offset) {
        state.last_published_offset = Some(state.offset);
        if let Some(callback) = on_scroll.as_ref() {
            shell.publish(callback(state.offset));
        }
    }
}

/// Normalized center anchor: fraction of `content_height` that sits at the
/// viewport's vertical center. `0.0` = top, `1.0` = bottom, stable across a
/// `content_width` change (resize / `View↔Compare` halves the width).
pub fn anchor_from_state(state: &TileViewState) -> f32 {
    if state.content_height <= f32::EPSILON || state.content_height <= state.viewport_height {
        return 0.0;
    }
    ((state.offset + state.viewport_height * 0.5) / state.content_height).clamp(0.0, 1.0)
}

/// Inverse of [`anchor_from_state`]: content-pixel `offset` that puts `anchor`
/// at the viewport center for the given geometry.
pub fn offset_from_anchor(anchor: f32, content_height: f32, viewport_height: f32) -> f32 {
    if content_height <= viewport_height || content_height <= f32::EPSILON {
        return 0.0;
    }
    let clamped = anchor.clamp(0.0, 1.0);
    (clamped * content_height - viewport_height * 0.5)
        .clamp(0.0, (content_height - viewport_height).max(0.0))
}

/// Publishes the normalized center anchor through `on_scroll` whenever it
/// changes (epsilon `1e-4` to hide float quantization). Also mirrors the
/// absolute offset into `last_published_offset` for legacy reads. The app
/// stores this anchor as `viewer_scroll` so a geometry change can restore the
/// same *centered* row instead of the same absolute offset.
pub fn publish_anchor<'a, Message, R>(
    shell: &mut Shell<'_, Message>,
    state: &mut TileViewState,
    on_scroll: &Option<R>,
) where
    R: Fn(f32) -> Message,
{
    let anchor = anchor_from_state(state);
    let should_publish = match state.last_published_anchor {
        None => true,
        Some(prev) => (prev - anchor).abs() > 1e-4,
    };
    if should_publish {
        state.last_published_anchor = Some(anchor);
        state.last_published_offset = Some(state.offset);
        if let Some(callback) = on_scroll.as_ref() {
            shell.publish(callback(anchor));
        }
    }
}

pub fn publish_edit_rect<'a, Message, K>(
    shell: &mut Shell<'_, Message>,
    tiles: &[TileSpec<'a>],
    state: &mut TileViewState,
    editing: Option<(usize, scanlateit_model::EntryId)>,
    on_edit_rect: &Option<K>,
) where
    K: Fn(Rectangle) -> Message,
{
    let rect = editing.and_then(|e| editing_rect(tiles, state, e));
    if state.last_edit_rect != rect {
        state.last_edit_rect = rect;
        if let (Some(rect), Some(callback)) = (rect, on_edit_rect.as_ref()) {
            shell.publish(callback(rect));
        }
    }
}

pub fn track_rect(bounds: Rectangle) -> Rectangle {
    Rectangle::new(
        Point::new(
            bounds.width - SCROLLBAR_WIDTH - SCROLLBAR_MARGIN,
            SCROLLBAR_MARGIN,
        ),
        Size::new(
            SCROLLBAR_WIDTH,
            (bounds.height - SCROLLBAR_MARGIN * 2.0).max(0.0),
        ),
    )
}

pub fn thumb_rect(bounds: Rectangle, state: &TileViewState) -> Rectangle {
    let track = track_rect(bounds);
    let max_offset = (state.content_height - bounds.height).max(0.0);
    let thumb_height = (track.height * track.height / state.content_height.max(1.0))
        .max(MIN_THUMB_HEIGHT)
        .min(track.height);
    let distance = (track.height - thumb_height).max(1.0);
    let ratio = if max_offset > 0.0 {
        state.offset / max_offset
    } else {
        0.0
    };
    Rectangle::new(
        Point::new(track.x, track.y + ratio * distance),
        Size::new(track.width, thumb_height),
    )
}
