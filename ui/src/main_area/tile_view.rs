//! A self-contained, canvas-based vertical tile viewer.
//!
//! Owns its own scrolling (wheel, touch pan, scrollbar drag), tiles page
//! images vertically, paints only the tiles that are actually visible, and
//! paints OCR overlays on top of each tile. Visible-range changes are
//! reported through [`TileView::on_visible_range`] so the app can decode
//! exactly the pages that are needed.

use std::ops::Range;
use std::time::{Duration, Instant};

use iced::advanced::graphics::geometry::frame::Backend as _;
use iced::advanced::graphics::geometry::{self, Fill, Path, Stroke, Text};
use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse;
use iced::advanced::renderer;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::widget::Widget;
use iced::advanced::{Clipboard, Shell};
use iced::border::Radius;
use iced::touch::Event as TouchEvent;
use iced::{Color, Element, Event, Font, Length, Pixels, Point, Rectangle, Size, Vector};

use crate::event::ToolbarAction;
use super::decode::PageDecode;
use super::overlay::{self, OverlayEntry};
use scanlateit_model::EntryId;

const SCROLL_LINE_HEIGHT: f32 = 180.0;
const SCROLLBAR_WIDTH: f32 = 8.0;
const SCROLLBAR_MARGIN: f32 = 2.0;
const MIN_THUMB_HEIGHT: f32 = 20.0;

/// Maximum gap between two presses on the same entry to count as a
/// double-click.
const DOUBLE_CLICK_DELAY: Duration = Duration::from_millis(400);

/// Cursor movement (viewport pixels) needed before a press on an entry turns
/// into a drag. Presses that stay inside the threshold remain clicks.
const DRAG_THRESHOLD: f32 = 3.0;

/// Side of a resize handle square, in viewport pixels.
const HANDLE_SIZE: f32 = 8.0;
/// Smallest box edge allowed while resizing, in viewport pixels.
const MIN_BOX_EDGE: f32 = 6.0;

/// Selection toolbar geometry, in viewport/tile pixels.
const TOOLBAR_HEIGHT: f32 = 22.0;
const TOOLBAR_GAP: f32 = 5.0;
const TOOLBAR_BTN_PAD: f32 = 10.0;
const TOOLBAR_BG: Color = Color::from_rgba8(28, 30, 38, 0.96);
const TOOLBAR_HOVER_BG: Color = Color::from_rgba8(58, 62, 76, 1.0);
const TOOLBAR_FG: Color = Color::from_rgba8(215, 220, 235, 1.0);
const HANDLE_FILL: Color = Color::WHITE;
const HANDLE_BORDER: Color = Color::from_rgba8(92, 190, 255, 1.0);

const PLACEHOLDER_BG: Color = Color::from_rgba8(45, 47, 60, 1.0);
const PLACEHOLDER_FG: Color = Color::from_rgba8(140, 145, 160, 1.0);
const FAILED_BG: Color = Color::from_rgba8(70, 40, 45, 1.0);
const FAILED_FG: Color = Color::from_rgba8(200, 120, 120, 1.0);
const SCROLLBAR_TRACK: Color = Color::from_rgba8(255, 255, 255, 0.07);
const SCROLLBAR_THUMB: Color = Color::from_rgba8(255, 255, 255, 0.35);

/// One stacked page in the viewer.
pub struct TileSpec<'a> {
    pub source_width: u32,
    pub source_height: u32,
    pub decode: &'a PageDecode,
    pub overlays: Vec<OverlayEntry<'a>>,
}

/// The tile viewer widget. Scroll state lives in the widget tree and survives
/// rebuilds; decoded pages are owned by the app (see [`PageDecode`]).
pub struct TileView<
    'a,
    Message,
    F = fn(Range<usize>) -> Message,
    G = fn(Option<(usize, EntryId)>) -> Message,
    H = fn((usize, EntryId)) -> Message,
    K = fn(Rectangle) -> Message,
    L = fn((usize, EntryId, [f32; 4])) -> Message,
    M = fn((usize, EntryId, ToolbarAction)) -> Message,
> where
    F: Fn(Range<usize>) -> Message,
    G: Fn(Option<(usize, EntryId)>) -> Message,
    H: Fn((usize, EntryId)) -> Message,
    K: Fn(Rectangle) -> Message,
    L: Fn((usize, EntryId, [f32; 4])) -> Message,
    M: Fn((usize, EntryId, ToolbarAction)) -> Message,
{
    tiles: Vec<TileSpec<'a>>,
    font: Font,
    on_visible_range: Option<F>,
    on_entry_clicked: Option<G>,
    on_entry_double_clicked: Option<H>,
    on_edit_rect: Option<K>,
    on_entry_moved: Option<L>,
    /// Called when a button of the selection toolbar under the selected entry
    /// is clicked.
    on_toolbar_action: Option<M>,
    /// The overlay entry currently being edited with a floating text input;
    /// its drawn overlay is hidden and its viewport rect is published.
    editing: Option<(usize, EntryId)>,
}

impl<'a, Message, F, G, H, K, L, M> TileView<'a, Message, F, G, H, K, L, M>
where
    F: Fn(Range<usize>) -> Message,
    G: Fn(Option<(usize, EntryId)>) -> Message,
    H: Fn((usize, EntryId)) -> Message,
    K: Fn(Rectangle) -> Message,
    L: Fn((usize, EntryId, [f32; 4])) -> Message,
    M: Fn((usize, EntryId, ToolbarAction)) -> Message,
{
    pub fn new(tiles: Vec<TileSpec<'a>>, font: Font) -> Self {
        Self {
            tiles,
            font,
            on_visible_range: None,
            on_entry_clicked: None,
            on_entry_double_clicked: None,
            on_edit_rect: None,
            on_entry_moved: None,
            on_toolbar_action: None,
            editing: None,
        }
    }

    /// Called whenever the set of visible tiles changes, including on the
    /// first frame and on window resizes.
    pub fn on_visible_range(mut self, f: F) -> Self {
        self.on_visible_range = Some(f);
        self
    }

    /// Called whenever an overlay entry is clicked (`Some((tile, entry))`) or
    /// the page is clicked outside every entry (`None`).
    pub fn on_entry_clicked(mut self, f: G) -> Self {
        self.on_entry_clicked = Some(f);
        self
    }

    /// Called when an overlay entry is double-clicked; the app starts an
    /// inline text edit for it.
    pub fn on_entry_double_clicked(mut self, f: H) -> Self {
        self.on_entry_double_clicked = Some(f);
        self
    }

    /// Called whenever the viewport rect (in widget coordinates) of the
    /// edited overlay entry changes, so the app can reposition the floating
    /// text input. Only called while `editing` targets a present entry.
    pub fn on_edit_rect(mut self, f: K) -> Self {
        self.on_edit_rect = Some(f);
        self
    }

    /// Called while an overlay entry is dragged, once per cursor move, with
    /// the entry's new view bounds as `[min_x, min_y, max_x, max_y]` clamped
    /// to the image, in image pixels.
    pub fn on_entry_moved(mut self, f: L) -> Self {
        self.on_entry_moved = Some(f);
        self
    }

    /// Called when a button of the selection toolbar (drawn under the
    /// selected entry's box) is clicked.
    pub fn on_toolbar_action(mut self, f: M) -> Self {
        self.on_toolbar_action = Some(f);
        self
    }

    /// Marks the overlay entry being edited; its painted overlay is hidden
    /// and its live viewport rect is reported through `on_edit_rect`.
    pub fn editing(mut self, editing: Option<(usize, EntryId)>) -> Self {
        self.editing = editing;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Interaction {
    None,
    TouchScrolling { origin: Point },
    ScrollerGrabbed { grab_offset: f32 },
    /// A left press on an entry that could still resolve to a click; turns
    /// into [`Interaction::Dragging`] once the cursor leaves
    /// [`DRAG_THRESHOLD`]. `offset` and `size` are in image pixels: the grab
    /// point relative to the entry's top-left and the box's size, both fixed
    /// at press so the box tracks the cursor exactly even as it moves.
    DragPending {
        index: usize,
        id: EntryId,
        offset: [f32; 2],
        size: [f32; 2],
        press: Point,
    },
    /// A press past the drag threshold: publishes the entry's new view bounds
    /// on every cursor move.
    Dragging {
        index: usize,
        id: EntryId,
        offset: [f32; 2],
        size: [f32; 2],
    },
    /// A press on a resize handle of the selected entry that could still
    /// resolve into a click of nothing; turns into [`Interaction::Resizing`]
    /// past the drag threshold. `start` is the entry's view bounds at press.
    ResizePending {
        index: usize,
        id: EntryId,
        handle: ResizeHandle,
        start: [f32; 4],
        press: Point,
    },
    /// A press past the drag threshold on a resize handle: publishes the
    /// entry's new view bounds on every cursor move.
    Resizing {
        index: usize,
        id: EntryId,
        handle: ResizeHandle,
        start: [f32; 4],
    },
    /// A press on a button of the selection toolbar; resolves (publishes the
    /// action) on release iff the cursor is still over the same button.
    ToolbarPressed {
        index: usize,
        id: EntryId,
        action: ToolbarAction,
    },
}

/// One of the eight resize handles around the selected entry's box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResizeHandle {
    /// The handle moves the box's min-x edge when true.
    left: bool,
    /// The handle moves the box's max-x edge when true.
    right: bool,
    /// The handle moves the box's min-y edge when true.
    top: bool,
    /// The handle moves the box's max-y edge when true.
    bottom: bool,
}

impl ResizeHandle {
    const NW: Self = Self { left: true, right: false, top: true, bottom: false };
    const N: Self = Self { left: false, right: false, top: true, bottom: false };
    const NE: Self = Self { left: false, right: true, top: true, bottom: false };
    const E: Self = Self { left: false, right: true, top: false, bottom: false };
    const SE: Self = Self { left: false, right: true, top: false, bottom: true };
    const S: Self = Self { left: false, right: false, top: false, bottom: true };
    const SW: Self = Self { left: true, right: false, top: false, bottom: true };
    const W: Self = Self { left: true, right: false, top: false, bottom: false };

    /// The resize cursor for this handle.
    fn cursor(self) -> mouse::Interaction {
        match self {
            Self { left: true, right: false, top: true, bottom: false }
            | Self { left: false, right: true, top: false, bottom: true } => {
                mouse::Interaction::ResizingDiagonallyDown
            }
            Self { left: false, right: true, top: true, bottom: false }
            | Self { left: true, right: false, top: false, bottom: true } => {
                mouse::Interaction::ResizingDiagonallyUp
            }
            Self { top: true, .. } | Self { bottom: true, .. } => {
                mouse::Interaction::ResizingVertically
            }
            _ => mouse::Interaction::ResizingHorizontally,
        }
    }
}

#[derive(Debug, Clone)]
struct TileViewState {
    offset: f32,
    width: f32,
    content_height: f32,
    viewport_height: f32,
    interaction: Interaction,
    last_visible: Option<Range<usize>>,
    /// The previous left-press hit plus when it happened, for double-click
    /// detection.
    last_click: Option<(Instant, Option<(usize, EntryId)>)>,
    /// The last published viewport rect of the edited entry.
    last_edit_rect: Option<Rectangle>,
}

impl Default for TileViewState {
    fn default() -> Self {
        Self {
            offset: 0.0,
            width: 0.0,
            content_height: 0.0,
            viewport_height: 0.0,
            interaction: Interaction::None,
            last_visible: None,
            last_click: None,
            last_edit_rect: None,
        }
    }
}

/// Width available to tile content: the scrollbar gutter is reserved on the
/// right so the track never overlays the pages.
fn content_width(width: f32) -> f32 {
    (width - SCROLLBAR_WIDTH - SCROLLBAR_MARGIN).max(0.0)
}

/// Returns `(tile_y, tile_height)` per tile and the total content height.
fn tile_layout(tiles: &[TileSpec<'_>], width: f32) -> (Vec<(f32, f32)>, f32) {
    let mut layout = Vec::with_capacity(tiles.len());
    let mut y = 0.0;
    for tile in tiles {
        let height = if width > 0.0 && tile.source_width > 0 {
            width * tile.source_height as f32 / tile.source_width as f32
        } else {
            0.0
        };
        layout.push((y, height));
        y += height;
    }
    (layout, y)
}

fn scroll_by(state: &mut TileViewState, delta: f32) -> bool {
    let max_offset = (state.content_height - state.viewport_height).max(0.0);
    let new = (state.offset + delta).clamp(0.0, max_offset);
    if (new - state.offset).abs() > f32::EPSILON {
        state.offset = new;
        true
    } else {
        false
    }
}

fn visible_range(tiles: &[TileSpec<'_>], state: &TileViewState) -> Option<Range<usize>> {
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

fn publish_visible<'a, Message, F>(
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

fn local_point(position: Point, bounds: Rectangle) -> Point {
    Point::new(position.x - bounds.position().x, position.y - bounds.position().y)
}

/// The topmost overlay entry whose box contains `local` (viewport-relative).
///
/// Tiles below the fold are skipped via the scroll offset; within a tile the
/// scale is the frame's width over the image width. Entries in later tiles
/// and later positions are drawn on top, so the last hit wins.
fn hit_entry(tiles: &[TileSpec<'_>], state: &TileViewState, local: Point) -> Option<(usize, EntryId)> {
    let (layout, _) = tile_layout(tiles, state.width);
    let content_y = local.y + state.offset;
    let mut hit = None;
    for (index, tile) in tiles.iter().enumerate() {
        let (y, height) = layout[index];
        if content_y < y || content_y >= y + height {
            continue;
        }
        let scale = if tile.source_width > 0 {
            state.width / tile.source_width as f32
        } else {
            0.0
        };
        for entry in &tile.overlays {
            let [min_x, min_y, max_x, max_y] = entry.bounds;
            let rect = Rectangle::new(
                Point::new(min_x * scale, y + min_y * scale - state.offset),
                Size::new((max_x - min_x) * scale, (max_y - min_y) * scale),
            );
            if rect.contains(local) {
                hit = Some((index, entry.id));
            }
        }
    }
    hit
}

/// The grab geometry for starting a drag on `(index, id)` at `local`
/// (viewport-relative): the cursor's offset from the entry's top-left and the
/// entry's box size, both in image pixels. `None` when the tile or entry is
/// not present.
fn drag_grab(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    index: usize,
    id: EntryId,
    local: Point,
) -> Option<([f32; 2], [f32; 2])> {
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
    let [min_x, min_y, max_x, max_y] = tile.overlays.iter().find(|e| e.id == id)?.bounds;
    let img_x = local.x / scale;
    let img_y = (local.y + state.offset - y) / scale;
    Some((
        [img_x - min_x, img_y - min_y],
        [max_x - min_x, max_y - min_y],
    ))
}

/// The clamped image-pixel view bounds when the entry whose grab geometry was
/// captured at press is dragged so its top-left sits under the cursor minus
/// the grab offset. The box never leaves the image.
fn drag_bounds(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    index: usize,
    local: Point,
    offset: [f32; 2],
    size: [f32; 2],
) -> Option<[f32; 4]> {
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
    let min_x = (img_x - offset[0]).clamp(0.0, (tile.source_width as f32 - size[0]).max(0.0));
    let min_y = (img_y - offset[1]).clamp(0.0, (tile.source_height as f32 - size[1]).max(0.0));
    Some([min_x, min_y, min_x + size[0], min_y + size[1]])
}

/// Viewport-relative rect of an overlay entry (widget coordinates): the
/// displayed box as `[min_x, min_y, max_x, max_y] * scale`, shifted by the
/// tile's content position and the scroll offset. `None` when the tile or
/// entry is not present.
fn entry_rect(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    index: usize,
    id: EntryId,
) -> Option<Rectangle> {
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

/// Viewport-relative rect of the overlay entry `editing` (widget
/// coordinates), used to position the floating text input over it. `None`
/// when the tile or entry is not present.
fn editing_rect(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    editing: (usize, EntryId),
) -> Option<Rectangle> {
    let (index, id) = editing;
    entry_rect(tiles, state, index, id)
}

/// The viewport-relative rect of the selected overlay entry, plus its tile
/// index. `None` when nothing is selected.
fn selected_rect(tiles: &[TileSpec<'_>], state: &TileViewState) -> Option<(usize, Rectangle)> {
    let (index, entry) = tiles
        .iter()
        .enumerate()
        .find_map(|(index, tile)| tile.overlays.iter().find(|e| e.selected).map(|e| (index, e)))?;
    Some((index, entry_rect(tiles, state, index, entry.id)?))
}

/// The centers of the eight resize handles around `rect`. Handle centers sit
/// exactly on the box's edges (straddling the outline like Figma/Photoshop,
/// so edge handles read as dashes on the line); for boxes smaller than a
/// handle the anchors collapse toward the box center.
fn handle_anchors(rect: Rectangle) -> [(ResizeHandle, Point); 8] {
    let cx = rect.x + rect.width / 2.0;
    let cy = rect.y + rect.height / 2.0;
    let span_x = rect.width.max(HANDLE_SIZE);
    let span_y = rect.height.max(HANDLE_SIZE);
    let left = cx - span_x / 2.0;
    let right = cx + span_x / 2.0;
    let top = cy - span_y / 2.0;
    let bottom = cy + span_y / 2.0;
    [
        (ResizeHandle::NW, Point::new(left, top)),
        (ResizeHandle::N, Point::new(cx, top)),
        (ResizeHandle::NE, Point::new(right, top)),
        (ResizeHandle::E, Point::new(right, cy)),
        (ResizeHandle::SE, Point::new(right, bottom)),
        (ResizeHandle::S, Point::new(cx, bottom)),
        (ResizeHandle::SW, Point::new(left, bottom)),
        (ResizeHandle::W, Point::new(left, cy)),
    ]
}

fn handle_rect(anchor: Point) -> Rectangle {
    let half = HANDLE_SIZE / 2.0;
    Rectangle::new(
        Point::new(anchor.x - half, anchor.y - half),
        Size::new(HANDLE_SIZE, HANDLE_SIZE),
    )
}

/// The resize handle of the selected entry under `local` (viewport-relative),
/// if any.
fn hit_handle(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    local: Point,
) -> Option<(usize, EntryId, ResizeHandle)> {
    let (index, rect) = selected_rect(tiles, state)?;
    let id = tiles[index].overlays.iter().find(|e| e.selected)?.id;
    for (handle, anchor) in handle_anchors(rect) {
        if handle_rect(anchor).contains(local) {
            return Some((index, id, handle));
        }
    }
    None
}

/// Width of the toolbar: two side-by-side buttons ("Rename", "Delete").
fn toolbar_width() -> f32 {
    fn button_width(label: &str) -> f32 {
        label.chars().count() as f32 * 6.5 + TOOLBAR_BTN_PAD * 2.0
    }
    button_width("Rename") + button_width("Delete")
}

/// The toolbar rect under the selected box: horizontally centered and
/// clamped inside `width`, flipped above the box when it would cross
/// `flip_at` (the tile's bottom, in the same coordinate space as `rect`).
fn toolbar_rect(rect: Rectangle, width: f32, flip_at: f32) -> Rectangle {
    let tw = toolbar_width();
    let x = (rect.x + rect.width / 2.0 - tw / 2.0).clamp(0.0, (width - tw).max(0.0));
    let below = rect.y + rect.height + TOOLBAR_GAP;
    let y = if below + TOOLBAR_HEIGHT <= flip_at {
        below
    } else {
        (rect.y - TOOLBAR_HEIGHT - TOOLBAR_GAP).max(0.0)
    };
    Rectangle::new(Point::new(x, y), Size::new(tw, TOOLBAR_HEIGHT))
}

/// The toolbar button under `local` (same space as `toolbar`), if any.
fn hit_toolbar_button(toolbar: Rectangle, local: Point) -> Option<ToolbarAction> {
    if !toolbar.contains(local) {
        return None;
    }
    let rename_width = "Rename".chars().count() as f32 * 6.5 + TOOLBAR_BTN_PAD * 2.0;
    if local.x < toolbar.x + rename_width {
        Some(ToolbarAction::Rename)
    } else {
        Some(ToolbarAction::Delete)
    }
}

/// The toolbar of the selected entry under `local` (viewport-relative), if
/// any: its tile index, entry id and the hovered button.
fn hit_toolbar(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    local: Point,
) -> Option<(usize, EntryId, ToolbarAction)> {
    let (index, rect) = selected_rect(tiles, state)?;
    let (layout, _) = tile_layout(tiles, state.width);
    let (tile_y, tile_height) = layout.get(index)?;
    let tile_bottom = tile_y - state.offset + tile_height;
    let toolbar = toolbar_rect(rect, state.width, tile_bottom);
    let id = tiles[index].overlays.iter().find(|e| e.selected)?.id;
    hit_toolbar_button(toolbar, local).map(|action| (index, id, action))
}

/// The entry's current view bounds, in image pixels, used as the fixed start
/// geometry of a resize gesture.
fn entry_bounds(tiles: &[TileSpec<'_>], index: usize, id: EntryId) -> Option<[f32; 4]> {
    let tile = tiles.get(index)?;
    tile.overlays.iter().find(|e| e.id == id).map(|e| e.bounds)
}

/// The clamped image-pixel view bounds when the resize handle captured at
/// press moves the corresponding edges toward `local` (viewport-relative).
/// Only the edges owned by `handle` move; the opposite edges keep their
/// press-time position, and the box never leaves the image nor gets smaller
/// than [`MIN_BOX_EDGE`] viewport pixels.
fn resize_bounds(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    index: usize,
    handle: ResizeHandle,
    start: [f32; 4],
    local: Point,
) -> Option<[f32; 4]> {
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
    let img_width = tile.source_width as f32;
    let img_height = tile.source_height as f32;
    let min_edge = (MIN_BOX_EDGE / scale).min(img_width).min(img_height);
    let [mut min_x, mut min_y, mut max_x, mut max_y] = start;
    if handle.left {
        min_x = img_x.clamp(0.0, (max_x - min_edge).max(0.0));
    }
    if handle.right {
        max_x = img_x.clamp((min_x + min_edge).min(img_width), img_width);
    }
    if handle.top {
        min_y = img_y.clamp(0.0, (max_y - min_edge).max(0.0));
    }
    if handle.bottom {
        max_y = img_y.clamp((min_y + min_edge).min(img_height), img_height);
    }
    Some([min_x, min_y, max_x, max_y])
}

/// Reports the edited entry's current viewport rect through `on_edit_rect`
/// whenever it changes.
fn publish_edit_rect<'a, Message, K>(
    shell: &mut Shell<'_, Message>,
    tiles: &[TileSpec<'a>],
    state: &mut TileViewState,
    editing: Option<(usize, EntryId)>,
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

fn track_rect(bounds: Rectangle) -> Rectangle {
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

fn thumb_rect(bounds: Rectangle, state: &TileViewState) -> Rectangle {
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

fn draw_scrollbar<F>(frame: &mut F, state: &TileViewState, bounds: Rectangle)
where
    F: geometry::frame::Backend,
{
    let track = track_rect(bounds);
    let thumb = thumb_rect(bounds, state);
    frame.fill_rectangle(track.position(), track.size(), Fill::from(SCROLLBAR_TRACK));
    frame.fill_rectangle(thumb.position(), thumb.size(), Fill::from(SCROLLBAR_THUMB));
}

fn draw_tile<F>(frame: &mut F, tile: &TileSpec<'_>, _font: Font)
where
    F: geometry::frame::Backend,
{
    match tile.decode {
        PageDecode::Failed => {
            frame.fill_rectangle(Point::ORIGIN, frame.size(), Fill::from(FAILED_BG));
            frame.fill_text(Text {
                content: "Failed to load".to_string(),
                position: frame.center(),
                max_width: frame.width(),
                size: Pixels(16.0),
                color: FAILED_FG,
                ..Text::default()
            });
        }
        PageDecode::Pending | PageDecode::Decoding => {
            frame.fill_rectangle(Point::ORIGIN, frame.size(), Fill::from(PLACEHOLDER_BG));
            frame.fill_text(Text {
                content: "Loading...".to_string(),
                position: frame.center(),
                max_width: frame.width(),
                size: Pixels(16.0),
                color: PLACEHOLDER_FG,
                ..Text::default()
            });
        }
        // `PageDecode::Ready` images are drawn by the caller in the base
        // layer; overlays go into their own layer on top.
        PageDecode::Ready(_) => {}
    }
}

/// The rect of one toolbar button inside the toolbar.
fn toolbar_button_rect(toolbar: Rectangle, action: ToolbarAction) -> Rectangle {
    let rename_width = "Rename".chars().count() as f32 * 6.5 + TOOLBAR_BTN_PAD * 2.0;
    match action {
        ToolbarAction::Rename => Rectangle::new(
            toolbar.position(),
            Size::new(rename_width, toolbar.height),
        ),
        ToolbarAction::Delete => Rectangle::new(
            Point::new(toolbar.x + rename_width, toolbar.y),
            Size::new((toolbar.width - rename_width).max(0.0), toolbar.height),
        ),
    }
}

fn draw_toolbar_button<F>(frame: &mut F, toolbar: Rectangle, action: ToolbarAction, hovered: bool)
where
    F: geometry::frame::Backend,
{
    let rect = toolbar_button_rect(toolbar, action);
    let label = match action {
        ToolbarAction::Rename => "Rename",
        ToolbarAction::Delete => "Delete",
    };
    if hovered {
        frame.fill(
            &Path::rounded_rectangle(rect.position(), rect.size(), Radius::from(4.0)),
            Fill::from(TOOLBAR_HOVER_BG),
        );
    } else {
        frame.fill(
            &Path::rounded_rectangle(rect.position(), rect.size(), Radius::from(4.0)),
            Fill::from(TOOLBAR_BG),
        );
    }
    frame.fill_text(Text {
        content: label.to_string(),
        position: Point::new(rect.x, rect.y + (rect.height - 13.0).max(0.0) / 2.0),
        max_width: rect.width,
        size: Pixels(11.0),
        color: TOOLBAR_FG,
        ..Text::default()
    });
}

/// Draws the resize handles and the Rename/Delete toolbar around the
/// selected entry, in the tile-local coordinates of its overlay frame.
///
/// The decorations are skipped while the entry is being edited inline or
/// while the user is already moving/resizing it. `cursor_local` is the
/// cursor in the frame's coordinates (`None` outside the widget), used for
/// the toolbar's hover highlight.
fn draw_selection_decorations<'a, F>(
    frame: &mut F,
    state: &TileViewState,
    tiles: &[TileSpec<'a>],
    tile_index: usize,
    cursor_local: Option<Point>,
) where
    F: geometry::frame::Backend,
{
    let Some(entry) = tiles[tile_index].overlays.iter().find(|e| e.selected) else {
        return;
    };
    if entry.hide_text {
        return;
    }
    // Hide the decorations while this entry is actually being moved or
    // resized; pending presses (not yet past the drag threshold) and
    // interactions on other entries keep them visible.
    let interacting_with_selected = match state.interaction {
        Interaction::Dragging { index, id, .. } | Interaction::Resizing { index, id, .. } => {
            index == tile_index && id == entry.id
        }
        _ => false,
    };
    if interacting_with_selected {
        return;
    }
    let scale = frame.width() / tiles[tile_index].source_width.max(1) as f32;
    let rect = Rectangle::new(
        Point::new(entry.bounds[0] * scale, entry.bounds[1] * scale),
        Size::new(
            (entry.bounds[2] - entry.bounds[0]) * scale,
            (entry.bounds[3] - entry.bounds[1]) * scale,
        ),
    );
    for (_, anchor) in handle_anchors(rect) {
        let handle = handle_rect(anchor);
        frame.fill_rectangle(handle.position(), handle.size(), Fill::from(HANDLE_FILL));
        frame.stroke(
            &Path::rectangle(handle.position(), handle.size()),
            Stroke::default().with_color(HANDLE_BORDER).with_width(1.0),
        );
    }
    let toolbar = toolbar_rect(rect, frame.width(), frame.height());
    let hover = cursor_local.and_then(|local| hit_toolbar_button(toolbar, local));
    for action in [ToolbarAction::Rename, ToolbarAction::Delete] {
        draw_toolbar_button(frame, toolbar, action, hover == Some(action));
    }
}

impl<'a, Message, F, G, H, K, L, M, Theme, Renderer> Widget<Message, Theme, Renderer>
    for TileView<'a, Message, F, G, H, K, L, M>
where
    F: Fn(Range<usize>) -> Message,
    G: Fn(Option<(usize, EntryId)>) -> Message,
    H: Fn((usize, EntryId)) -> Message,
    K: Fn(Rectangle) -> Message,
    L: Fn((usize, EntryId, [f32; 4])) -> Message,
    M: Fn((usize, EntryId, ToolbarAction)) -> Message,
    Renderer: renderer::Renderer + geometry::Renderer,
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<TileViewState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(TileViewState::default())
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let width = limits.max().width;
        let state = tree.state.downcast_mut::<TileViewState>();
        state.width = content_width(width);
        let (_, content_height) = tile_layout(&self.tiles, state.width);
        state.content_height = content_height;
        if state.viewport_height > 0.0 {
            state.offset = state.offset.min((content_height - state.viewport_height).max(0.0));
        }
        layout::Node::new(Size::new(width, limits.max().height))
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<TileViewState>();
        let bounds = layout.bounds();
        let Some(visible_bounds) = bounds.intersection(viewport) else {
            return;
        };

        let mut overlay_frames: Vec<(usize, Rectangle)> = Vec::new();
        renderer.with_layer(visible_bounds, |renderer| {
            renderer.with_translation(Vector::new(0.0, -state.offset), |renderer| {
                let top = state.offset;
                let bottom = state.offset + visible_bounds.height;
                let content_w = content_width(bounds.width);
                let (layout, _) = tile_layout(&self.tiles, content_w);
                for (index, tile) in self.tiles.iter().enumerate() {
                    let (y, height) = layout[index];
                    if y + height <= top || y >= bottom {
                        continue;
                    }
                    let tile_bounds =
                        Rectangle::new(Point::new(0.0, y), Size::new(content_w, height));
                    let mut frame = renderer.new_frame(tile_bounds);
                    frame.translate(Vector::new(tile_bounds.x, tile_bounds.y));
                    match tile.decode {
                        PageDecode::Ready(decoded) => {
                            frame.draw_image(
                                Rectangle::with_size(frame.size()),
                                geometry::Image::new(decoded.handle.clone()),
                            );
                            overlay_frames.push((index, tile_bounds));
                        }
                        _ => draw_tile(&mut frame, tile, self.font),
                    }
                    renderer.draw_geometry(frame.into_geometry());
                }
            });
            // Overlays cannot share a layer with the images: every backend
            // paints meshes (backgrounds, strokes) below raster images and
            // only layers on top of them. A dedicated layer keeps the
            // entries above their page. The layer must be created outside
            // the scroll translation: layer clips are transformed by the
            // current translation at push time, so a layer created inside
            // it would only ever show the top viewport-height of content.
            if !overlay_frames.is_empty() {
                renderer.with_layer(visible_bounds, |renderer| {
                    renderer.with_translation(Vector::new(0.0, -state.offset), |renderer| {
                        for (index, tile_bounds) in overlay_frames {
                            let mut overlay_frame = renderer.new_frame(tile_bounds);
                            overlay_frame
                                .translate(Vector::new(tile_bounds.x, tile_bounds.y));
                            overlay::draw_entries(
                                &mut overlay_frame,
                                &self.tiles[index].overlays,
                                self.font,
                                self.tiles[index].source_width as f32,
                            );
                            // The frame is translated to content coordinates;
                            // bring the cursor into the same tile-local space
                            // for the selection decorations' hover state.
                            let cursor_local = cursor
                                .position_over(bounds)
                                .map(|position| {
                                    Point::new(
                                        position.x - bounds.x,
                                        position.y - bounds.y + state.offset - tile_bounds.y,
                                    )
                                });
                            draw_selection_decorations(
                                &mut overlay_frame,
                                state,
                                &self.tiles,
                                index,
                                cursor_local,
                            );
                            renderer.draw_geometry(overlay_frame.into_geometry());
                        }
                    });
                });
            }
        });

        if state.content_height > visible_bounds.height + 1.0 {
            renderer.with_layer(visible_bounds, |renderer| {
                let mut frame = renderer.new_frame(visible_bounds);
                draw_scrollbar(&mut frame, state, visible_bounds);
                renderer.draw_geometry(frame.into_geometry());
            });
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<TileViewState>();
        let bounds = layout.bounds();
        state.width = content_width(bounds.width);
        state.viewport_height = bounds.height;

        match event {
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if cursor.position_over(bounds).is_none() {
                    return;
                }
                let delta_y = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => -y * SCROLL_LINE_HEIGHT,
                    mouse::ScrollDelta::Pixels { y, .. } => -y,
                };
                if delta_y != 0.0 && scroll_by(state, delta_y) {
                    shell.request_redraw();
                    publish_visible(shell, &self.tiles, state, &self.on_visible_range);
                    shell.capture_event();
                }
            }
            Event::Touch(TouchEvent::FingerPressed { position, .. }) => {
                state.interaction = Interaction::TouchScrolling { origin: *position };
                shell.capture_event();
            }
            Event::Touch(TouchEvent::FingerMoved { position, .. }) => {
                if let Interaction::TouchScrolling { origin } = state.interaction {
                    if scroll_by(state, origin.y - position.y) {
                        shell.request_redraw();
                        publish_visible(shell, &self.tiles, state, &self.on_visible_range);
                    }
                    state.interaction = Interaction::TouchScrolling { origin: *position };
                    shell.capture_event();
                }
            }
            Event::Touch(TouchEvent::FingerLifted { .. }) | Event::Touch(TouchEvent::FingerLost { .. }) => {
                if matches!(state.interaction, Interaction::TouchScrolling { .. }) {
                    state.interaction = Interaction::None;
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(position) = cursor.position_over(bounds) {
                    let local = local_point(position, bounds);
                    if self.editing.is_none() {
                        if let Some((index, id, action)) = hit_toolbar(&self.tiles, state, local) {
                            state.interaction = Interaction::ToolbarPressed { index, id, action };
                            shell.capture_event();
                            return;
                        }
                        if let Some((index, id, handle)) = hit_handle(&self.tiles, state, local) {
                            if let Some(start) = entry_bounds(&self.tiles, index, id) {
                                state.interaction = Interaction::ResizePending {
                                    index,
                                    id,
                                    handle,
                                    start,
                                    press: local,
                                };
                                shell.capture_event();
                                return;
                            }
                        }
                    }
                    if track_rect(bounds).contains(local) {
                        state.interaction = Interaction::ScrollerGrabbed {
                            grab_offset: local.y - thumb_rect(bounds, state).y,
                        };
                        shell.capture_event();
                    } else {
                        let hit = hit_entry(&self.tiles, state, local);
                        let now = Instant::now();
                        let is_double = matches!(&state.last_click, Some((at, prev)) if *prev == hit && now.duration_since(*at) <= DOUBLE_CLICK_DELAY);
                        state.last_click = Some((now, hit));
                        if is_double {
                            if let (Some(hit), Some(callback)) = (hit, self.on_entry_double_clicked.as_ref())
                            {
                                shell.publish(callback(hit));
                            }
                            // Seed the editor rect in the same update pass so
                            // the floating input exists (and can be focused)
                            // on the very next frame.
                            if let (Some(hit), Some(rect_callback)) = (
                                hit,
                                self.on_edit_rect.as_ref(),
                            ) {
                                if let Some(rect) = editing_rect(&self.tiles, state, hit) {
                                    state.last_edit_rect = Some(rect);
                                    shell.publish(rect_callback(rect));
                                }
                            }
                        } else if let Some(callback) = self.on_entry_clicked.as_ref() {
                            shell.publish(callback(hit));
                        }
                        if let Some((index, id)) = hit {
                            if let Some((offset, size)) =
                                drag_grab(&self.tiles, state, index, id, local)
                            {
                                state.interaction = Interaction::DragPending {
                                    index,
                                    id,
                                    offset,
                                    size,
                                    press: local,
                                };
                            }
                        }
                        shell.capture_event();
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Interaction::ToolbarPressed { index, id, action } = state.interaction {
                    if let Some(position) = cursor.position_over(bounds) {
                        let local = local_point(position, bounds);
                        if hit_toolbar(&self.tiles, state, local) == Some((index, id, action)) {
                            if let Some(callback) = self.on_toolbar_action.as_ref() {
                                shell.publish(callback((index, id, action)));
                                shell.request_redraw();
                            }
                        }
                    }
                }
                if matches!(
                    state.interaction,
                    Interaction::ScrollerGrabbed { .. }
                        | Interaction::DragPending { .. }
                        | Interaction::Dragging { .. }
                        | Interaction::ResizePending { .. }
                        | Interaction::Resizing { .. }
                        | Interaction::ToolbarPressed { .. }
                ) {
                    state.interaction = Interaction::None;
                    shell.capture_event();
                    // The release ends any press gesture: redraw right away so
                    // the selection decorations (handles, toolbar) become
                    // visible immediately after a click instead of waiting
                    // for the next cursor move or redraw.
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                match state.interaction {
                    Interaction::ScrollerGrabbed { grab_offset } => {
                        let track = track_rect(bounds);
                        let thumb = thumb_rect(bounds, state);
                        let distance = (track.height - thumb.height).max(1.0);
                        let local_y = position.y - bounds.position().y;
                        let ratio = ((local_y - track.y - grab_offset) / distance).clamp(0.0, 1.0);
                        let max_offset = (state.content_height - state.viewport_height).max(0.0);
                        let new_offset = ratio * max_offset;
                        if (new_offset - state.offset).abs() > f32::EPSILON {
                            state.offset = new_offset;
                            shell.request_redraw();
                            publish_visible(shell, &self.tiles, state, &self.on_visible_range);
                        }
                        shell.capture_event();
                    }
                    Interaction::DragPending {
                        index,
                        id,
                        offset,
                        size,
                        press,
                    } => {
                        let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                        let dx = local.x - press.x;
                        let dy = local.y - press.y;
                        if dx * dx + dy * dy >= DRAG_THRESHOLD * DRAG_THRESHOLD {
                            state.interaction = Interaction::Dragging { index, id, offset, size };
                            if let (Some(callback), Some(bounds)) = (
                                self.on_entry_moved.as_ref(),
                                drag_bounds(&self.tiles, state, index, local, offset, size),
                            ) {
                                shell.publish(callback((index, id, bounds)));
                                shell.request_redraw();
                            }
                        }
                        shell.capture_event();
                    }
                    Interaction::Dragging { index, id, offset, size } => {
                        let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                        if let (Some(callback), Some(bounds)) = (
                            self.on_entry_moved.as_ref(),
                            drag_bounds(&self.tiles, state, index, local, offset, size),
                        ) {
                            shell.publish(callback((index, id, bounds)));
                            shell.request_redraw();
                        }
                        shell.capture_event();
                    }
                    Interaction::ResizePending {
                        index,
                        id,
                        handle,
                        start,
                        press,
                    } => {
                        let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                        let dx = local.x - press.x;
                        let dy = local.y - press.y;
                        if dx * dx + dy * dy >= DRAG_THRESHOLD * DRAG_THRESHOLD {
                            state.interaction = Interaction::Resizing { index, id, handle, start };
                            if let (Some(callback), Some(bounds)) = (
                                self.on_entry_moved.as_ref(),
                                resize_bounds(&self.tiles, state, index, handle, start, local),
                            ) {
                                shell.publish(callback((index, id, bounds)));
                                shell.request_redraw();
                            }
                        }
                        shell.capture_event();
                    }
                    Interaction::Resizing { index, id, handle, start } => {
                        let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                        if let (Some(callback), Some(bounds)) = (
                            self.on_entry_moved.as_ref(),
                            resize_bounds(&self.tiles, state, index, handle, start, local),
                        ) {
                            shell.publish(callback((index, id, bounds)));
                            shell.request_redraw();
                        }
                        shell.capture_event();
                    }
                    Interaction::ToolbarPressed { .. } => {
                        shell.capture_event();
                    }
                    Interaction::None | Interaction::TouchScrolling { .. } => {}
                }
            }
            Event::Window(_) => {
                publish_visible(shell, &self.tiles, state, &self.on_visible_range);
            }
            _ => {}
        }

        publish_edit_rect(shell, &self.tiles, state, self.editing, &self.on_edit_rect);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<TileViewState>();
        match state.interaction {
            Interaction::TouchScrolling { .. }
            | Interaction::ScrollerGrabbed { .. }
            | Interaction::DragPending { .. }
            | Interaction::Dragging { .. }
            | Interaction::ResizePending { .. }
            | Interaction::Resizing { .. }
            | Interaction::ToolbarPressed { .. } => {
                mouse::Interaction::Grabbing
            }
            Interaction::None => {
                let bounds = layout.bounds();
                if let Some(position) = cursor.position_over(bounds) {
                    let local = local_point(position, bounds);
                    if self.editing.is_none() {
                        if hit_toolbar(&self.tiles, state, local).is_some() {
                            return mouse::Interaction::Pointer;
                        }
                        if let Some((_, _, handle)) = hit_handle(&self.tiles, state, local) {
                            return handle.cursor();
                        }
                    }
                    if track_rect(bounds).contains(local) || thumb_rect(bounds, state).contains(local)
                    {
                        return mouse::Interaction::Pointer;
                    }
                }
                mouse::Interaction::None
            }
        }
    }
}

impl<'a, Message: 'a, F: 'a, G: 'a, H: 'a, K: 'a, L: 'a, M: 'a, Theme, Renderer>
    From<TileView<'a, Message, F, G, H, K, L, M>> for Element<'a, Message, Theme, Renderer>
where
    F: Fn(Range<usize>) -> Message,
    G: Fn(Option<(usize, EntryId)>) -> Message,
    H: Fn((usize, EntryId)) -> Message,
    K: Fn(Rectangle) -> Message,
    L: Fn((usize, EntryId, [f32; 4])) -> Message,
    M: Fn((usize, EntryId, ToolbarAction)) -> Message,
    Renderer: renderer::Renderer + geometry::Renderer,
{
    fn from(view: TileView<'a, Message, F, G, H, K, L, M>) -> Self {
        Self::new(view)
    }
}
