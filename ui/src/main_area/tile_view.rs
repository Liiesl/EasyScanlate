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
use iced::keyboard;
use iced::touch::Event as TouchEvent;
use iced::{Color, Element, Event, Font, Length, Pixels, Point, Rectangle, Size, Vector};

use crate::event::ToolbarAction;
use super::decode::PageDecode;
use super::overlay::{self, OverlayEntry};
use scanlateit_model::{EntryId, Quad};

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

/// Smallest inpainting range edge, in image pixels; smaller drags are
/// treated as accidental presses.
const MIN_INPAINT_EDGE: f32 = 4.0;

/// Selection toolbar geometry, in viewport/tile pixels.
const TOOLBAR_HEIGHT: f32 = 22.0;
const TOOLBAR_GAP: f32 = 5.0;
const TOOLBAR_BTN_PAD: f32 = 10.0;
const TOOLBAR_BG: Color = Color::from_rgba8(28, 30, 38, 0.96);
const TOOLBAR_HOVER_BG: Color = Color::from_rgba8(58, 62, 76, 1.0);
const TOOLBAR_FG: Color = Color::from_rgba8(215, 220, 235, 1.0);
const HANDLE_FILL: Color = Color::WHITE;
const HANDLE_BORDER: Color = Color::from_rgba8(92, 190, 255, 1.0);

/// Length of the stem connecting the rotation knob to the box, in viewport
/// pixels.
const ROTATE_STEM: f32 = 16.0;

const PLACEHOLDER_BG: Color = Color::from_rgba8(45, 47, 60, 1.0);
const PLACEHOLDER_FG: Color = Color::from_rgba8(140, 145, 160, 1.0);
const FAILED_BG: Color = Color::from_rgba8(70, 40, 45, 1.0);
const FAILED_FG: Color = Color::from_rgba8(200, 120, 120, 1.0);
const SCROLLBAR_TRACK: Color = Color::from_rgba8(255, 255, 255, 0.07);
const SCROLLBAR_THUMB: Color = Color::from_rgba8(255, 255, 255, 0.35);

/// Inpainting range marquee colors.
const INPAINT_FILL: Color = Color::from_rgba8(92, 190, 255, 0.16);
const INPAINT_STROKE: Color = Color::from_rgba8(92, 190, 255, 1.0);

/// One stacked page in the viewer.
pub struct TileSpec<'a> {
    pub source_width: u32,
    pub source_height: u32,
    pub decode: &'a PageDecode,
    pub overlays: Vec<OverlayEntry<'a>>,
    /// Inpaint layers drawn over the page raster, below the entry overlays.
    pub inpaint: &'a [crate::loaded::InpaintLayer],
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
    L = fn((usize, EntryId, Quad)) -> Message,
    M = fn((usize, EntryId, ToolbarAction)) -> Message,
    P = fn() -> Message,
    Q = fn((usize, Rectangle)) -> Message,
> where
    F: Fn(Range<usize>) -> Message,
    G: Fn(Option<(usize, EntryId)>) -> Message,
    H: Fn((usize, EntryId)) -> Message,
    K: Fn(Rectangle) -> Message,
    L: Fn((usize, EntryId, Quad)) -> Message,
    M: Fn((usize, EntryId, ToolbarAction)) -> Message,
    P: Fn() -> Message,
    Q: Fn((usize, Rectangle)) -> Message,
{
    tiles: Vec<TileSpec<'a>>,
    font: Font,
    on_visible_range: Option<F>,
    on_entry_clicked: Option<G>,
    on_entry_double_clicked: Option<H>,
    on_edit_rect: Option<K>,
    on_entry_moved: Option<L>,
    /// Called when a button of the selection decorations (the toolbar under
    /// the selected entry's box or the revert button above it) is clicked.
    on_toolbar_action: Option<M>,
    /// Called when the user finishes dragging an inpainting range on the
    /// tile whose index matches [`TileView::inpaint_mode`]; the rectangle is
    /// in image pixels.
    on_inpaint_selection: Option<Q>,
    /// Called when a scrollbar drag or touch pan ends, after the final
    /// `on_visible_range` update.
    on_scroll_ended: Option<P>,
    /// The overlay entry currently being edited with a floating text input;
    /// its drawn overlay is hidden and its viewport rect is published.
    editing: Option<(usize, EntryId)>,
    /// The image index whose tile accepts inpainting range drags; `None`
    /// disables the mode. Set by the app from the panel's Inpaint button.
    inpaint_mode: Option<usize>,
    /// Whether applied inpainting patches are drawn over the page rasters.
    show_inpaint: bool,
    /// Whether the overlay text is drawn over the pages; `false` hides only
    /// the text, keeping the boxes and selection decorations interactive.
    show_overlay_text: bool,
    /// A request (from a selection change elsewhere in the UI) to scroll the
    /// entry `(index, id)` into view, centered if out of view; `None` when
    /// there is nothing to reveal. Consumed once in `layout()`.
    reveal: Option<(usize, EntryId)>,
}

impl<'a, Message, F, G, H, K, L, M, P, Q> TileView<'a, Message, F, G, H, K, L, M, P, Q>
where
    F: Fn(Range<usize>) -> Message,
    G: Fn(Option<(usize, EntryId)>) -> Message,
    H: Fn((usize, EntryId)) -> Message,
    K: Fn(Rectangle) -> Message,
    L: Fn((usize, EntryId, Quad)) -> Message,
    M: Fn((usize, EntryId, ToolbarAction)) -> Message,
    P: Fn() -> Message,
    Q: Fn((usize, Rectangle)) -> Message,
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
            on_inpaint_selection: None,
            on_scroll_ended: None,
            editing: None,
            inpaint_mode: None,
            show_inpaint: true,
            show_overlay_text: true,
            reveal: None,
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

    /// Called while an overlay entry is dragged, resized, or free-transformed
    /// (Ctrl+drag a corner), once per cursor move, with the entry's new view
    /// quad in image pixels.
    pub fn on_entry_moved(mut self, f: L) -> Self {
        self.on_entry_moved = Some(f);
        self
    }

    /// Called when a button of the selection decorations (the toolbar drawn
    /// under the selected entry's box, or the revert button beside the
    /// rotation knob above it) is clicked.
    pub fn on_toolbar_action(mut self, f: M) -> Self {
        self.on_toolbar_action = Some(f);
        self
    }

    /// Called when a scrollbar drag or touch pan ends. The viewport stops
    /// moving here, so a full-res settle can start immediately instead of
    /// waiting out the debounce.
    pub fn on_scroll_ended(mut self, f: P) -> Self {
        self.on_scroll_ended = Some(f);
        self
    }

    /// Called when the user finishes dragging an inpainting range on the
    /// tile whose index matches [`Self::inpaint_mode`]; the rectangle is in
    /// image pixels.
    pub fn on_inpaint_selection(mut self, f: Q) -> Self {
        self.on_inpaint_selection = Some(f);
        self
    }

    /// Marks the tile that accepts inpainting range drags; `None` disables
    /// the mode.
    pub fn inpaint_mode(mut self, inpaint_mode: Option<usize>) -> Self {
        self.inpaint_mode = inpaint_mode;
        self
    }

    /// Controls whether applied inpainting patches are drawn over the page
    /// rasters; `false` hides them.
    pub fn show_inpaint(mut self, show_inpaint: bool) -> Self {
        self.show_inpaint = show_inpaint;
        self
    }

    /// Controls whether the overlay text is drawn; `false` hides only the
    /// text, keeping the boxes and selection decorations interactive.
    pub fn show_overlay_text(mut self, show_overlay_text: bool) -> Self {
        self.show_overlay_text = show_overlay_text;
        self
    }

    /// Marks the overlay entry being edited; its painted overlay is hidden
    /// and its live viewport rect is reported through `on_edit_rect`.
    pub fn editing(mut self, editing: Option<(usize, EntryId)>) -> Self {
        self.editing = editing;
        self
    }

    /// Requests that the entry `(index, id)` be scrolled into view (centered
    /// if out of view) on the next layout; `None` clears the request.
    pub fn reveal(mut self, reveal: Option<(usize, EntryId)>) -> Self {
        self.reveal = reveal;
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
    /// [`DRAG_THRESHOLD`]. `offset` is in image pixels: the grab point
    /// relative to the entry's box top-left, fixed at press so the box
    /// tracks the cursor exactly even as it moves. `quad` is the entry's
    /// view quad at press.
    DragPending {
        index: usize,
        id: EntryId,
        offset: [f32; 2],
        quad: Quad,
        press: Point,
    },
    /// A press past the drag threshold: publishes the entry's new view quad
    /// on every cursor move.
    Dragging {
        index: usize,
        id: EntryId,
        offset: [f32; 2],
        quad: Quad,
    },
    /// A press on a resize handle of the selected entry that could still
    /// resolve into a click of nothing; turns into [`Interaction::Resizing`]
    /// past the drag threshold. `quad` is the entry's view quad at press.
    ResizePending {
        index: usize,
        id: EntryId,
        handle: ResizeHandle,
        quad: Quad,
        press: Point,
    },
    /// A press past the drag threshold on a resize handle: publishes the
    /// entry's new view quad on every cursor move.
    Resizing {
        index: usize,
        id: EntryId,
        handle: ResizeHandle,
        quad: Quad,
    },
    /// A Ctrl+press on a corner handle of the selected entry that could
    /// still resolve into a click of nothing; turns into
    /// [`Interaction::Distorting`] past the drag threshold. `quad` is the
    /// entry's view quad at press, ordered TL/TR/BR/BL; dragging replaces
    /// the single point at `corner`.
    DistortPending {
        index: usize,
        id: EntryId,
        corner: usize,
        quad: Quad,
        press: Point,
    },
    /// A press past the drag threshold on a corner handle with Ctrl held:
    /// publishes the entry's new view quad (one corner moved) on every
    /// cursor move.
    Distorting {
        index: usize,
        id: EntryId,
        corner: usize,
        quad: Quad,
    },
    /// A press on the rotation knob above the selected entry that could
    /// still resolve into a click of nothing; turns into
    /// [`Interaction::Rotating`] past the drag threshold. `quad` is the
    /// entry's view quad at press (image pixels), `center_img` its centroid
    /// in image pixels and `center_view` the centroid in viewport pixels;
    /// dragging spins the quad around that center by the cursor's angle
    /// delta around `center_view`.
    RotatePending {
        index: usize,
        id: EntryId,
        quad: Quad,
        center_img: [f32; 2],
        center_view: Point,
        press: Point,
    },
    /// A press past the drag threshold on the rotation knob: publishes the
    /// entry's new view quad (rotated around its center) on every cursor
    /// move.
    Rotating {
        index: usize,
        id: EntryId,
        quad: Quad,
        center_img: [f32; 2],
        center_view: Point,
        press: Point,
    },
    /// A press on a button of the selection toolbar; resolves (publishes the
    /// action) on release iff the cursor is still over the same button.
    ToolbarPressed {
        index: usize,
        id: EntryId,
        action: ToolbarAction,
    },
    /// A drag on the tile targeted by `inpaint_mode`: selects the range to
    /// clean. `start`/`current` are in tile-local (content) coordinates.
    InpaintSelecting {
        index: usize,
        start: Point,
        current: Point,
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

    /// The quad corner this handle anchors, when it is a corner handle. The
    /// index is into the quad ordered TL/TR/BR/BL (see [`overlay::order_quad`]).
    fn corner(self) -> Option<usize> {
        match self {
            Self::NW => Some(0),
            Self::NE => Some(1),
            Self::SE => Some(2),
            Self::SW => Some(3),
            _ => None,
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
    /// Current keyboard modifiers, cached from `ModifiersChanged` so a press
    /// can tell a plain handle drag from a Ctrl free-transform drag.
    keyboard_modifiers: keyboard::Modifiers,
    /// The image index whose tile accepts inpainting range drags (`None`
    /// disables the mode). Mirrors the widget field every frame.
    inpaint_mode: Option<usize>,
    /// The last `reveal` request consumed in `layout()`; requests fire once
    /// per selection change.
    last_revealed: Option<(usize, EntryId)>,
}

impl TileViewState {
    /// The mirror of the widget's inpainting mode when the last frame was
    /// drawn.
    fn inpaint_mode(&self) -> Option<usize> {
        self.inpaint_mode
    }
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
            keyboard_modifiers: keyboard::Modifiers::default(),
            inpaint_mode: None,
            last_revealed: None,
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

/// The tile index whose content contains `local` (viewport-relative), if any.
fn hit_tile(tiles: &[TileSpec<'_>], state: &TileViewState, local: Point) -> Option<usize> {
    let (layout, _) = tile_layout(tiles, state.width);
    let content_y = local.y + state.offset;
    layout
        .iter()
        .enumerate()
        .find(|(_, (y, height))| content_y >= *y && content_y < y + height)
        .map(|(index, _)| index)
}

/// Converts a viewport-relative point into tile-local (content, unscrolled)
/// coordinates for tile `index`.
fn tile_local_point(
    layout: &[(f32, f32)],
    index: usize,
    local: Point,
    offset: f32,
) -> Point {
    Point::new(local.x, local.y + offset - layout[index].0)
}

/// True when `point` is inside the (possibly convex) quad. Cheap for convex
/// quads: every edge must put the point on the same side. Non-convex quads
/// simply report the bounding-box-ish result the edge sweep gives.
fn point_in_quad(point: Point, quad: [[f32; 2]; 4]) -> bool {
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

/// The topmost overlay entry whose box contains `local` (viewport-relative).
///
/// Tiles below the fold are skipped via the scroll offset; within a tile the
/// scale is the frame's width over the image width. Entries are not bounded
/// by their page image: an entry sticking out past the page's edges stays
/// hit-testable there. Entries in later tiles and later positions are drawn
/// on top, so the last hit wins.
fn hit_entry(tiles: &[TileSpec<'_>], state: &TileViewState, local: Point) -> Option<(usize, EntryId)> {
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
            // The box may be free-transformed: confirm the hit against the
            // quad, brought into viewport space.
            let quad = overlay::order_quad(entry.quad.points.map(|p| {
                [p[0] * scale, y + p[1] * scale - state.offset]
            }));
            if point_in_quad(local, quad) {
                hit = Some((index, entry.id));
            }
        }
    }
    hit
}

/// The grab geometry for starting a drag on `(index, id)` at `local`
/// (viewport-relative): the cursor's offset from the entry's box top-left,
/// in image pixels. `None` when the tile or entry is not present.
fn drag_grab(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    index: usize,
    id: EntryId,
    local: Point,
) -> Option<[f32; 2]> {
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

/// The image-pixel view quad when the entry whose grab geometry was
/// captured at press is dragged so its box top-left sits under the cursor
/// minus the grab offset. The box follows the cursor freely and may leave
/// the image.
fn drag_quad(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    index: usize,
    local: Point,
    offset: [f32; 2],
    quad: Quad,
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
    let min_x = img_x - offset[0];
    let min_y = img_y - offset[1];
    Some(quad.translate(min_x - size[0], min_y - size[1]))
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

/// The scroll offset that centers the entry `(index, id)` in the viewport,
/// or `None` when it is already fully visible (or not measurable).
fn reveal_offset(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    index: usize,
    id: EntryId,
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
        return None; // already fully visible
    }
    let target = (top - (viewport - (bottom - top)) / 2.0).clamp(0.0, max_offset);
    (target != state.offset).then_some(target)
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

/// The centers of the eight transform handles around the quad: the four
/// corners and the four edge midpoints. Handle centers sit on the quad's
/// outline (straddling it like Figma/Photoshop); for boxes smaller than a
/// handle the anchors collapse toward the box center.
fn handle_anchors(quad: [[f32; 2]; 4]) -> [(ResizeHandle, Point); 8] {
    let ordered = overlay::order_quad(quad);
    let point = |i: usize| Point::new(ordered[i][0], ordered[i][1]);
    let midpoint = |a: usize, b: usize| {
        Point::new((ordered[a][0] + ordered[b][0]) / 2.0, (ordered[a][1] + ordered[b][1]) / 2.0)
    };
    // The quad is ordered TL/TR/BR/BL; keep the anchors exactly in the old
    // order so interactions (NW/NE/SE/SW corners, N/E/S/W edges) map 1:1.
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

fn handle_rect(anchor: Point) -> Rectangle {
    let half = HANDLE_SIZE / 2.0;
    Rectangle::new(
        Point::new(anchor.x - half, anchor.y - half),
        Size::new(HANDLE_SIZE, HANDLE_SIZE),
    )
}

/// The transform handle of the selected entry under `local`
/// (viewport-relative), if any.
fn hit_handle(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    local: Point,
) -> Option<(usize, EntryId, ResizeHandle)> {
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

/// The quad's centroid (average of its four points).
fn quad_centroid(quad: [[f32; 2]; 4]) -> Point {
    Point::new(
        (quad[0][0] + quad[1][0] + quad[2][0] + quad[3][0]) / 4.0,
        (quad[0][1] + quad[1][1] + quad[2][1] + quad[3][1]) / 4.0,
    )
}

/// The geometry of the top selection decorations (rotation knob + revert
/// button), in the caller's coordinate space: `rect` is the box's AABB,
/// `quad` its polygon, `width` the content width and `viewport_top` /
/// `viewport_bottom` the visible band the decorations must stay inside. The
/// decorations hang off the box's top edge by a stem, and flip below the box
/// when they would cross the viewport's top edge (mirroring the bottom
/// toolbar's flip).
struct TopDecor {
    /// The rotation knob's center, connected to the box by a stem.
    anchor: Point,
    /// The box-edge midpoint the stem is drawn from.
    stem_from: Point,
    /// The revert button's rect.
    revert: Rectangle,
}

fn top_decor_geometry(
    rect: Rectangle,
    quad: [[f32; 2]; 4],
    width: f32,
    viewport_top: f32,
    viewport_bottom: f32,
) -> TopDecor {
    let center = quad_centroid(quad);
    // The point `ROTATE_STEM` outward from the midpoint of the edge `a`-`b`,
    // on the side pointing away from the box's centroid.
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
            mid.x + normal[0] / len * ROTATE_STEM,
            mid.y + normal[1] / len * ROTATE_STEM,
        )
    };
    let ordered = overlay::order_quad(quad);
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
    // Flip below the box only when the knob would cross the viewport's top
    // while the box itself still sits inside it; with the box fully above
    // the viewport the clamp below pins the knob to the visible band.
    let flip = stem_up.y - HANDLE_SIZE / 2.0 < viewport_top && rect.y > viewport_top;
    let (stem_from, mut anchor) = if flip {
        (bottom_mid, stem_down)
    } else {
        (top_mid, stem_up)
    };
    // Keep the knob inside the visible band; with the box itself above the
    // viewport the flipped-down knob would otherwise sit off-screen.
    anchor.y = anchor
        .y
        .clamp(viewport_top + HANDLE_SIZE / 2.0, viewport_bottom - HANDLE_SIZE / 2.0);
    let revert_width = button_width("Revert");
    let revert = Rectangle::new(
        Point::new(
            (anchor.x + HANDLE_SIZE / 2.0 + TOOLBAR_GAP).clamp(0.0, (width - revert_width).max(0.0)),
            anchor.y - TOOLBAR_HEIGHT / 2.0,
        ),
        Size::new(revert_width, TOOLBAR_HEIGHT),
    );
    TopDecor { anchor, stem_from, revert }
}

/// What part of the top selection decorations is under `local`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TopDecorHit {
    /// The rotation knob: press-drag rotates the box.
    Rotate,
    /// The revert button: resets the box's transform.
    Revert,
}

/// The top selection decorations of the selected entry under `local`
/// (viewport-relative), if any: its tile index, entry id and which part was
/// hit. The revert button is only offered when the box is actually a
/// user-adjusted override.
fn hit_top_decor(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    local: Point,
) -> Option<(usize, EntryId, TopDecorHit)> {
    let (index, entry) = tiles.iter().enumerate().find_map(|(index, tile)| {
        tile.overlays.iter().find(|e| e.selected).map(|e| (index, e))
    })?;
    let id = entry.id;
    if state.inpaint_mode() == Some(index) {
        return None;
    }
    let (_, rect) = selected_rect(tiles, state)?;
    let quad = selected_quad_view(tiles, state, index)?;
    let decor = top_decor_geometry(
        rect,
        quad,
        state.width,
        state.offset,
        state.offset + state.viewport_height,
    );
    if handle_rect(decor.anchor).contains(local) {
        return Some((index, id, TopDecorHit::Rotate));
    }
    if entry.quad_overridden && decor.revert.contains(local) {
        return Some((index, id, TopDecorHit::Revert));
    }
    None
}

/// The angle the cursor swept around `center` between `from` and `to`, in
/// radians; snapped to [`ROTATE_SNAP_DEGREES`] steps when `snap` is true.
fn delta_angle(center: Point, from: Point, to: Point, snap: bool) -> f32 {
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

/// The image-pixel view quad when the rotation knob captured at press spins
/// the entry by the cursor's angle delta around the box center.
fn rotate_quad(
    quad: Quad,
    center_img: [f32; 2],
    center_view: Point,
    press: Point,
    local: Point,
    snap: bool,
) -> Quad {
    quad.rotate(center_img, delta_angle(center_view, press, local, snap))
}

/// The toolbar's buttons in drawing order, with their labels.
fn toolbar_buttons() -> [(ToolbarAction, &'static str); 2] {
    [
        (ToolbarAction::Rename, "Rename"),
        (ToolbarAction::Delete, "Delete"),
    ]
}

/// Width of one labeled toolbar button, in viewport pixels.
fn button_width(label: &str) -> f32 {
    label.chars().count() as f32 * 6.5 + TOOLBAR_BTN_PAD * 2.0
}

/// Width of the toolbar: two side-by-side buttons ("Rename", "Delete").
fn toolbar_width() -> f32 {
    toolbar_buttons()
        .into_iter()
        .map(|(_, label)| button_width(label))
        .sum()
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

/// The toolbar of the selected entry under `local` (viewport-relative), if
/// any: its tile index, entry id and the hovered button.
fn hit_toolbar(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    local: Point,
) -> Option<(usize, EntryId, ToolbarAction)> {
    let (index, rect) = selected_rect(tiles, state)?;
    // The toolbar flips above the box at the viewport's bottom, matching
    // how it is drawn: entries may sit far below their page image.
    let toolbar = toolbar_rect(rect, state.width, state.viewport_height);
    let id = tiles[index].overlays.iter().find(|e| e.selected)?.id;
    hit_toolbar_button(toolbar, local).map(|action| (index, id, action))
}

/// The entry quad's full viewport-relative polygon (ordered TL/TR/BR/BL),
/// used to place the resize/free-transform handles.
fn selected_quad_view(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    index: usize,
) -> Option<[[f32; 2]; 4]> {
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
    Some(overlay::order_quad(entry.quad.points.map(|p| {
        [p[0] * scale, y + p[1] * scale - state.offset]
    })))
}

/// The entry's current view quad, in image pixels, used as the fixed start
/// geometry of a move/resize/free-transform gesture.
fn entry_quad(tiles: &[TileSpec<'_>], index: usize, id: EntryId) -> Option<Quad> {
    let tile = tiles.get(index)?;
    tile.overlays.iter().find(|e| e.id == id).map(|e| e.quad)
}

/// The image-pixel view quad when the resize handle captured at press moves
/// the corresponding edges toward `local` (viewport-relative). Only the
/// edges owned by `handle` move; the opposite edges keep their press-time
/// position, the box may leave the image and never gets smaller than
/// [`MIN_BOX_EDGE`] viewport pixels, and the quad's shape is refit into the
/// new bounds proportionally.
fn resize_quad(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    index: usize,
    handle: ResizeHandle,
    quad: Quad,
    local: Point,
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

/// The image-pixel view quad when the corner captured with Ctrl held moves
/// to `local` (viewport-relative): the quad with that single corner dragged
/// to the cursor, possibly outside the image. The other corners stay put.
fn distort_quad(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    index: usize,
    corner: usize,
    quad: Quad,
    local: Point,
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
    let mut points = quad.points;
    points[corner] = [img_x, img_y];
    Some(Quad { points })
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

fn draw_placeholder<F>(frame: &mut F, failed: bool, _font: Font)
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

/// The inpainting range marquee: a translucent rect between `start` and
/// `current` (tile-local coordinates), clipped to the tile.
fn draw_inpaint_marquee<F>(frame: &mut F, start: Point, current: Point, tile: Size)
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

/// The rect of one toolbar button inside the toolbar.
fn toolbar_button_rect(toolbar: Rectangle, action: ToolbarAction) -> Rectangle {
    let mut x = toolbar.x;
    for (candidate, label) in toolbar_buttons() {
        let width = button_width(label);
        if candidate == action {
            return Rectangle::new(Point::new(x, toolbar.y), Size::new(width, toolbar.height));
        }
        x += width;
    }
    Rectangle::new(toolbar.position(), Size::new(0.0, toolbar.height))
}

fn draw_toolbar_button<F>(frame: &mut F, toolbar: Rectangle, action: ToolbarAction, hovered: bool)
where
    F: geometry::frame::Backend,
{
    let rect = toolbar_button_rect(toolbar, action);
    let label = toolbar_buttons()
        .into_iter()
        .find(|(candidate, _)| *candidate == action)
        .map(|(_, label)| label)
        .unwrap_or("");
    draw_action_button(frame, rect, label, hovered);
}

/// Fills one action button (the bottom toolbar's buttons and the top revert
/// button).
fn draw_action_button<F>(frame: &mut F, rect: Rectangle, label: &str, hovered: bool)
where
    F: geometry::frame::Backend,
{
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

/// Draws the rotation knob with its stem, and the revert button beside it
/// when the box is a user-adjusted override.
fn draw_top_decor<F>(frame: &mut F, decor: TopDecor, show_revert: bool, hover: bool)
where
    F: geometry::frame::Backend,
{
    frame.stroke(
        &Path::line(decor.stem_from, decor.anchor),
        Stroke::default().with_color(HANDLE_BORDER).with_width(1.5),
    );
    let knob = Path::circle(decor.anchor, HANDLE_SIZE / 2.0);
    frame.fill(&knob, Fill::from(HANDLE_FILL));
    frame.stroke(&knob, Stroke::default().with_color(HANDLE_BORDER).with_width(1.5));
    if show_revert {
        draw_action_button(frame, decor.revert, "Revert", hover);
    }
}

/// Draws the resize handles, the rotation knob with the revert button, and
/// the Rename/Delete toolbar around the selected entry, in the tile-local
/// coordinates of its overlay frame.
///
/// The decorations are skipped while the entry is being edited inline, while
/// the user is already moving/resizing/rotating it, or while the overlay
/// layer is hidden entirely (`show_overlay_text` is `false`). `cursor_local`
/// is the cursor in the frame's coordinates (`None` outside the widget),
/// used for the buttons' hover highlight. `flip_from`/`flip_at` are the
/// viewer viewport's top/bottom in the frame's coordinates: the toolbar
/// hangs below the box and flips above when it would cross the viewport's
/// bottom edge, and the rotation knob hangs above the box and flips below
/// when it would cross the viewport's top edge.
fn draw_selection_decorations<'a, F>(
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
    // Hide the decorations while this entry is actually being moved,
    // resized, rotated or free-transformed; pending presses (not yet past
    // the drag threshold) and interactions on other entries keep them
    // visible.
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
    let quad = overlay::order_quad(entry.quad.points.map(|p| [p[0] * scale, p[1] * scale]));
    let rect = Rectangle::new(
        Point::new(entry.bounds[0] * scale, entry.bounds[1] * scale),
        Size::new(
            (entry.bounds[2] - entry.bounds[0]) * scale,
            (entry.bounds[3] - entry.bounds[1]) * scale,
        ),
    );
    // While inpainting mode is on for this tile, the transform handles and
    // the rotation knob are hidden so the range drag has an uncluttered
    // canvas; the panel's Inpaint button toggles the mode back off.
    if state.inpaint_mode() != Some(tile_index) {
        let anchors = handle_anchors(quad);
        for (_, anchor) in anchors {
            let handle = handle_rect(anchor);
            frame.fill_rectangle(handle.position(), handle.size(), Fill::from(HANDLE_FILL));
            frame.stroke(
                &Path::rectangle(handle.position(), handle.size()),
                Stroke::default().with_color(HANDLE_BORDER).with_width(1.0),
            );
        }
        // The rotation knob is always offered; the revert button beside it
        // only when the box is a user-adjusted override.
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

impl<'a, Message, F, G, H, K, L, M, P, Q, Theme, Renderer> Widget<Message, Theme, Renderer>
    for TileView<'a, Message, F, G, H, K, L, M, P, Q>
where
    F: Fn(Range<usize>) -> Message,
    G: Fn(Option<(usize, EntryId)>) -> Message,
    H: Fn((usize, EntryId)) -> Message,
    K: Fn(Rectangle) -> Message,
    L: Fn((usize, EntryId, Quad)) -> Message,
    M: Fn((usize, EntryId, ToolbarAction)) -> Message,
    P: Fn() -> Message,
    Q: Fn((usize, Rectangle)) -> Message,
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
        state.inpaint_mode = self.inpaint_mode;
        let (_, content_height) = tile_layout(&self.tiles, state.width);
        state.content_height = content_height;
        if self.reveal != state.last_revealed {
            state.last_revealed = self.reveal;
            if let Some((index, id)) = self.reveal {
                if let Some(new_offset) = reveal_offset(&self.tiles, state, index, id) {
                    state.offset = new_offset;
                }
            }
        }
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

        let local_bounds = Rectangle::new(
            Point::new(visible_bounds.x - bounds.x, visible_bounds.y - bounds.y),
            visible_bounds.size(),
        );
        renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
            renderer.with_layer(local_bounds, |renderer| {
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
                    match tile.decode.image() {
                        Some(decoded) => {
                            frame.draw_image(
                                Rectangle::with_size(frame.size()),
                                geometry::Image::new(decoded.handle.clone()),
                            );
                            // Inpaint layers sit right above the page raster
                            // and below the entry overlays.
                            if self.show_inpaint {
                                let scale =
                                    frame.width() / tile.source_width.max(1) as f32;
                                for layer in tile.inpaint {
                                    let bounds = layer.bounds;
                                    frame.draw_image(
                                        Rectangle::new(
                                            Point::new(bounds[0] * scale, bounds[1] * scale),
                                            Size::new(bounds[2] * scale, bounds[3] * scale),
                                        ),
                                        geometry::Image::new(layer.handle.clone()),
                                    );
                                }
                            }
                        }
                        None => draw_placeholder(&mut frame, tile.decode.thumb_failed(), self.font),
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
            //
            // The pass is independent of the image pass: entries are not
            // bounded by their page image, so a tile scrolled out of view
            // (or not yet decoded) can still have entries sticking into the
            // viewport. Tiles are culled by their entries' content-space
            // bounding boxes against the visible content region, so
            // off-screen entries never reach the text shaper.
            let content_w = content_width(bounds.width);
            let (layout, _) = tile_layout(&self.tiles, content_w);
            let visible_top = state.offset;
            let visible_bottom = state.offset + visible_bounds.height;
            let visible_tiles: Vec<usize> = self
                .tiles
                .iter()
                .enumerate()
                .filter_map(|(index, tile)| {
                    let (y, _) = layout[index];
                    let scale = state.width / tile.source_width.max(1) as f32;
                    let has_visible_entry = tile.overlays.iter().any(|entry| {
                        let [min_x, min_y, max_x, max_y] = entry.bounds;
                        let left = min_x * scale;
                        let right = max_x * scale;
                        let top = y + min_y * scale;
                        let bottom = y + max_y * scale;
                        right >= 0.0
                            && left <= content_w
                            && bottom >= visible_top
                            && top <= visible_bottom
                    });
                    has_visible_entry.then_some(index)
                })
                .collect();
            if !visible_tiles.is_empty() {
                renderer.with_layer(local_bounds, |renderer| {
                    renderer.with_translation(Vector::new(0.0, -state.offset), |renderer| {
                        // One frame over the visible content region instead
                        // of one per tile: overlays are clipped only by the
                        // viewer viewport, not by their page image, so
                        // entries may extend past the page edges. The frame
                        // width still equals every tile's display width, so
                        // the scale derived from it stays correct.
                        let overlay_clip = Rectangle::new(
                            Point::new(0.0, state.offset),
                            Size::new(content_w, visible_bounds.height),
                        );
                        let mut overlay_frame = renderer.new_frame(overlay_clip);
                        for index in visible_tiles {
                            let (y, height) = layout[index];
                            let tile_bounds = Rectangle::new(
                                Point::new(0.0, y),
                                Size::new(content_w, height),
                            );
                            overlay_frame.push_transform();
                            overlay_frame
                                .translate(Vector::new(tile_bounds.x, tile_bounds.y));
                            overlay::draw_entries(
                                &mut overlay_frame,
                                &self.tiles[index].overlays,
                                self.font,
                                self.tiles[index].source_width as f32,
                                !self.show_overlay_text,
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
                            // The toolbar flips above the box only when it
                            // would cross the viewer viewport's bottom, not
                            // the page's bottom: entries may sit far below
                            // their page and the actions should still hang
                            // below the box.
                            let flip_at =
                                state.offset + visible_bounds.height - tile_bounds.y;
                            // The rotation knob flips below the box only when
                            // it would cross the viewer viewport's top.
                            let flip_from = state.offset - tile_bounds.y;
                            draw_selection_decorations(
                                &mut overlay_frame,
                                state,
                                &self.tiles,
                                index,
                                cursor_local,
                                flip_from,
                                flip_at,
                                self.show_overlay_text,
                            );
                            // The inpainting range marquee, drawn last so it
                            // sits on top of the tile's content.
                            if state.inpaint_mode() == Some(index) {
                                if let Interaction::InpaintSelecting {
                                    index: selecting,
                                    start,
                                    current,
                                } = state.interaction
                                {
                                    if selecting == index {
                                        draw_inpaint_marquee(
                                            &mut overlay_frame,
                                            start,
                                            current,
                                            tile_bounds.size(),
                                        );
                                    }
                                }
                            }
                            overlay_frame.pop_transform();
                        }
                        renderer.draw_geometry(overlay_frame.into_geometry());
                    });
                });
            }
        });

        if state.content_height > visible_bounds.height + 1.0 {
            renderer.with_layer(local_bounds, |renderer| {
                let mut frame = renderer.new_frame(local_bounds);
                draw_scrollbar(&mut frame, state, local_bounds);
                renderer.draw_geometry(frame.into_geometry());
            });
        }
        });
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
                    if let Some(callback) = self.on_scroll_ended.as_ref() {
                        shell.publish(callback());
                    }
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
                        // Inpainting mode: a drag anywhere on the targeted
                        // tile selects the range to clean (the scrollbar
                        // keeps working below).
                        if let (Some(hover_index), Some(mode_index)) =
                            (hit_tile(&self.tiles, state, local), self.inpaint_mode)
                        {
                            if hover_index == mode_index && !track_rect(bounds).contains(local) {
                                let (layout, _) = tile_layout(&self.tiles, state.width);
                                let content = tile_local_point(&layout, mode_index, local, state.offset);
                                state.interaction = Interaction::InpaintSelecting {
                                    index: mode_index,
                                    start: content,
                                    current: content,
                                };
                                shell.capture_event();
                                return;
                            }
                        }
                        if let Some((index, id, hit)) = hit_top_decor(&self.tiles, state, local) {
                            match hit {
                                TopDecorHit::Revert => {
                                    state.interaction = Interaction::ToolbarPressed {
                                        index,
                                        id,
                                        action: ToolbarAction::RevertTransform,
                                    };
                                    shell.capture_event();
                                    return;
                                }
                                TopDecorHit::Rotate => {
                                    if let (Some(quad), Some(view)) = (
                                        entry_quad(&self.tiles, index, id),
                                        selected_quad_view(&self.tiles, state, index),
                                    ) {
                                        let center_img = [
                                            quad.points.iter().map(|p| p[0]).sum::<f32>() / 4.0,
                                            quad.points.iter().map(|p| p[1]).sum::<f32>() / 4.0,
                                        ];
                                        state.interaction = Interaction::RotatePending {
                                            index,
                                            id,
                                            quad,
                                            center_img,
                                            center_view: quad_centroid(view),
                                            press: local,
                                        };
                                        shell.capture_event();
                                        return;
                                    }
                                }
                            }
                        }
                        if let Some((index, id, handle)) = hit_handle(&self.tiles, state, local) {
                            if let Some(quad) = entry_quad(&self.tiles, index, id) {
                                // Ctrl turns a corner handle into a free
                                // transform: that corner follows the cursor.
                                if let Some(corner) = handle.corner() {
                                    if state.keyboard_modifiers.command() {
                                        state.interaction = Interaction::DistortPending {
                                            index,
                                            id,
                                            corner,
                                            quad: Quad {
                                                points: overlay::order_quad(quad.points),
                                            },
                                            press: local,
                                        };
                                        shell.capture_event();
                                        return;
                                    }
                                }
                                state.interaction = Interaction::ResizePending {
                                    index,
                                    id,
                                    handle,
                                    quad,
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
                            if let Some(offset) =
                                drag_grab(&self.tiles, state, index, id, local)
                            {
                                if let Some(quad) = entry_quad(&self.tiles, index, id) {
                                    state.interaction = Interaction::DragPending {
                                        index,
                                        id,
                                        offset,
                                        quad,
                                        press: local,
                                    };
                                }
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
                        let still_hovered = match action {
                            ToolbarAction::RevertTransform => {
                                hit_top_decor(&self.tiles, state, local)
                                    == Some((index, id, TopDecorHit::Revert))
                            }
                            _ => hit_toolbar(&self.tiles, state, local) == Some((index, id, action)),
                        };
                        if still_hovered {
                            if let Some(callback) = self.on_toolbar_action.as_ref() {
                                shell.publish(callback((index, id, action)));
                                shell.request_redraw();
                            }
                        }
                    }
                }
                if let Interaction::InpaintSelecting { index, start, current } = state.interaction {
                    let tile = &self.tiles[index];
                    let (layout, _) = tile_layout(&self.tiles, state.width);
                    let scale = if tile.source_width > 0 {
                        state.width / tile.source_width as f32
                    } else {
                        0.0
                    };
                    if scale > 0.0 {
                        let tile_height = layout.get(index).map(|(_, h)| *h).unwrap_or(0.0);
                        let x0 = start.x.min(current.x).clamp(0.0, state.width);
                        let y0 = start.y.min(current.y).clamp(0.0, tile_height);
                        let x1 = start.x.max(current.x).clamp(0.0, state.width);
                        let y1 = start.y.max(current.y).clamp(0.0, tile_height);
                        // The selected range in image pixels; ranges smaller
                        // than a few pixels are accidental presses.
                        let rect = Rectangle::new(
                            Point::new(x0 / scale, y0 / scale),
                            Size::new((x1 - x0) / scale, (y1 - y0) / scale),
                        );
                        if rect.width >= MIN_INPAINT_EDGE && rect.height >= MIN_INPAINT_EDGE {
                            if let Some(callback) = self.on_inpaint_selection.as_ref() {
                                shell.publish(callback((index, rect)));
                                shell.request_redraw();
                            }
                        }
                    }
                }
                let ended_scroll = matches!(state.interaction, Interaction::ScrollerGrabbed { .. });
                if matches!(
                    state.interaction,
                    Interaction::ScrollerGrabbed { .. }
                        | Interaction::DragPending { .. }
                        | Interaction::Dragging { .. }
                        | Interaction::ResizePending { .. }
                        | Interaction::Resizing { .. }
                        | Interaction::DistortPending { .. }
                        | Interaction::Distorting { .. }
                        | Interaction::RotatePending { .. }
                        | Interaction::Rotating { .. }
                        | Interaction::ToolbarPressed { .. }
                        | Interaction::InpaintSelecting { .. }
                ) {
                    state.interaction = Interaction::None;
                    shell.capture_event();
                    // The release ends any press gesture: redraw right away so
                    // the selection decorations (handles, toolbar) become
                    // visible immediately after a click instead of waiting
                    // for the next cursor move or redraw.
                    shell.request_redraw();
                }
                if ended_scroll {
                    if let Some(callback) = self.on_scroll_ended.as_ref() {
                        shell.publish(callback());
                    }
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
                        quad,
                        press,
                    } => {
                        let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                        let dx = local.x - press.x;
                        let dy = local.y - press.y;
                        if dx * dx + dy * dy >= DRAG_THRESHOLD * DRAG_THRESHOLD {
                            state.interaction =
                                Interaction::Dragging { index, id, offset, quad };
                            if let (Some(callback), Some(quad)) = (
                                self.on_entry_moved.as_ref(),
                                drag_quad(&self.tiles, state, index, local, offset, quad),
                            ) {
                                shell.publish(callback((index, id, quad)));
                                shell.request_redraw();
                            }
                        }
                        shell.capture_event();
                    }
                    Interaction::Dragging { index, id, offset, quad } => {
                        let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                        if let (Some(callback), Some(quad)) = (
                            self.on_entry_moved.as_ref(),
                            drag_quad(&self.tiles, state, index, local, offset, quad),
                        ) {
                            shell.publish(callback((index, id, quad)));
                            shell.request_redraw();
                        }
                        shell.capture_event();
                    }
                    Interaction::ResizePending {
                        index,
                        id,
                        handle,
                        quad,
                        press,
                    } => {
                        let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                        let dx = local.x - press.x;
                        let dy = local.y - press.y;
                        if dx * dx + dy * dy >= DRAG_THRESHOLD * DRAG_THRESHOLD {
                            state.interaction =
                                Interaction::Resizing { index, id, handle, quad };
                            if let (Some(callback), Some(quad)) = (
                                self.on_entry_moved.as_ref(),
                                resize_quad(&self.tiles, state, index, handle, quad, local),
                            ) {
                                shell.publish(callback((index, id, quad)));
                                shell.request_redraw();
                            }
                        }
                        shell.capture_event();
                    }
                    Interaction::Resizing { index, id, handle, quad } => {
                        let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                        if let (Some(callback), Some(quad)) = (
                            self.on_entry_moved.as_ref(),
                            resize_quad(&self.tiles, state, index, handle, quad, local),
                        ) {
                            shell.publish(callback((index, id, quad)));
                            shell.request_redraw();
                        }
                        shell.capture_event();
                    }
                    Interaction::DistortPending {
                        index,
                        id,
                        corner,
                        quad,
                        press,
                    } => {
                        let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                        let dx = local.x - press.x;
                        let dy = local.y - press.y;
                        if dx * dx + dy * dy >= DRAG_THRESHOLD * DRAG_THRESHOLD {
                            state.interaction =
                                Interaction::Distorting { index, id, corner, quad };
                            if let (Some(callback), Some(quad)) = (
                                self.on_entry_moved.as_ref(),
                                distort_quad(&self.tiles, state, index, corner, quad, local),
                            ) {
                                shell.publish(callback((index, id, quad)));
                                shell.request_redraw();
                            }
                        }
                        shell.capture_event();
                    }
                    Interaction::Distorting { index, id, corner, quad } => {
                        let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                        if let (Some(callback), Some(quad)) = (
                            self.on_entry_moved.as_ref(),
                            distort_quad(&self.tiles, state, index, corner, quad, local),
                        ) {
                            shell.publish(callback((index, id, quad)));
                            shell.request_redraw();
                        }
                        shell.capture_event();
                    }
                    Interaction::RotatePending {
                        index,
                        id,
                        quad,
                        center_img,
                        center_view,
                        press,
                    } => {
                        let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                        let dx = local.x - press.x;
                        let dy = local.y - press.y;
                        if dx * dx + dy * dy >= DRAG_THRESHOLD * DRAG_THRESHOLD {
                            state.interaction = Interaction::Rotating {
                                index,
                                id,
                                quad,
                                center_img,
                                center_view,
                                press,
                            };
                            if let Some(callback) = self.on_entry_moved.as_ref() {
                                let snap = state.keyboard_modifiers.shift();
                                let rotated = rotate_quad(
                                    quad, center_img, center_view, press, local, snap,
                                );
                                shell.publish(callback((index, id, rotated)));
                                shell.request_redraw();
                            }
                        }
                        shell.capture_event();
                    }
                    Interaction::Rotating {
                        index,
                        id,
                        quad,
                        center_img,
                        center_view,
                        press,
                    } => {
                        let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                        if let Some(callback) = self.on_entry_moved.as_ref() {
                            let snap = state.keyboard_modifiers.shift();
                            let rotated =
                                rotate_quad(quad, center_img, center_view, press, local, snap);
                            shell.publish(callback((index, id, rotated)));
                            shell.request_redraw();
                        }
                        shell.capture_event();
                    }
                    Interaction::InpaintSelecting { index, start, .. } => {
                        let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                        let (layout, _) = tile_layout(&self.tiles, state.width);
                        let current =
                            tile_local_point(&layout, index, local, state.offset);
                        state.interaction = Interaction::InpaintSelecting {
                            index,
                            start,
                            current,
                        };
                        shell.request_redraw();
                        shell.capture_event();
                    }
                    Interaction::ToolbarPressed { .. } => {
                        shell.capture_event();
                    }
                    Interaction::None | Interaction::TouchScrolling { .. } => {}
                }
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                // Ctrl (Cmd on macOS) turns a corner-handle press into the
                // free-transform drag; cache the modifiers here so the press
                // handler can tell the two apart.
                state.keyboard_modifiers = *modifiers;
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
            | Interaction::DistortPending { .. }
            | Interaction::Distorting { .. }
            | Interaction::RotatePending { .. }
            | Interaction::Rotating { .. }
            | Interaction::ToolbarPressed { .. }
            | Interaction::InpaintSelecting { .. } => {
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
                        if let Some((_, _, hit)) = hit_top_decor(&self.tiles, state, local) {
                            return match hit {
                                TopDecorHit::Rotate => mouse::Interaction::Grabbing,
                                TopDecorHit::Revert => mouse::Interaction::Pointer,
                            };
                        }
                        if let Some((_, _, handle)) = hit_handle(&self.tiles, state, local) {
                            return handle.cursor();
                        }
                        if hit_tile(&self.tiles, state, local) == self.inpaint_mode {
                            return mouse::Interaction::Crosshair;
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

impl<'a, Message: 'a, F: 'a, G: 'a, H: 'a, K: 'a, L: 'a, M: 'a, P: 'a, Q: 'a, Theme, Renderer>
    From<TileView<'a, Message, F, G, H, K, L, M, P, Q>> for Element<'a, Message, Theme, Renderer>
where
    F: Fn(Range<usize>) -> Message,
    G: Fn(Option<(usize, EntryId)>) -> Message,
    H: Fn((usize, EntryId)) -> Message,
    K: Fn(Rectangle) -> Message,
    L: Fn((usize, EntryId, Quad)) -> Message,
    M: Fn((usize, EntryId, ToolbarAction)) -> Message,
    P: Fn() -> Message,
    Q: Fn((usize, Rectangle)) -> Message,
    Renderer: renderer::Renderer + geometry::Renderer,
{
    fn from(view: TileView<'a, Message, F, G, H, K, L, M, P, Q>) -> Self {
        Self::new(view)
    }
}
