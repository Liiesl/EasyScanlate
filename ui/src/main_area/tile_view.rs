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
use iced::advanced::graphics::geometry::{self, Fill, Text};
use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse;
use iced::advanced::renderer;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::widget::Widget;
use iced::advanced::{Clipboard, Shell};
use iced::touch::Event as TouchEvent;
use iced::{Color, Element, Event, Font, Length, Pixels, Point, Rectangle, Size, Vector};

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
> where
    F: Fn(Range<usize>) -> Message,
    G: Fn(Option<(usize, EntryId)>) -> Message,
    H: Fn((usize, EntryId)) -> Message,
    K: Fn(Rectangle) -> Message,
    L: Fn((usize, EntryId, [f32; 4])) -> Message,
{
    tiles: Vec<TileSpec<'a>>,
    font: Font,
    on_visible_range: Option<F>,
    on_entry_clicked: Option<G>,
    on_entry_double_clicked: Option<H>,
    on_edit_rect: Option<K>,
    on_entry_moved: Option<L>,
    /// The overlay entry currently being edited with a floating text input;
    /// its drawn overlay is hidden and its viewport rect is published.
    editing: Option<(usize, EntryId)>,
}

impl<'a, Message, F, G, H, K, L> TileView<'a, Message, F, G, H, K, L>
where
    F: Fn(Range<usize>) -> Message,
    G: Fn(Option<(usize, EntryId)>) -> Message,
    H: Fn((usize, EntryId)) -> Message,
    K: Fn(Rectangle) -> Message,
    L: Fn((usize, EntryId, [f32; 4])) -> Message,
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

/// Viewport-relative rect of the overlay entry `editing` (widget
/// coordinates), used to position the floating text input over it. `None`
/// when the tile or entry is not present.
fn editing_rect(
    tiles: &[TileSpec<'_>],
    state: &TileViewState,
    editing: (usize, EntryId),
) -> Option<Rectangle> {
    let (index, id) = editing;
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

impl<'a, Message, F, G, H, K, L, Theme, Renderer> Widget<Message, Theme, Renderer>
    for TileView<'a, Message, F, G, H, K, L>
where
    F: Fn(Range<usize>) -> Message,
    G: Fn(Option<(usize, EntryId)>) -> Message,
    H: Fn((usize, EntryId)) -> Message,
    K: Fn(Rectangle) -> Message,
    L: Fn((usize, EntryId, [f32; 4])) -> Message,
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
        _cursor: mouse::Cursor,
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
                if matches!(
                    state.interaction,
                    Interaction::ScrollerGrabbed { .. }
                        | Interaction::DragPending { .. }
                        | Interaction::Dragging { .. }
                ) {
                    state.interaction = Interaction::None;
                    shell.capture_event();
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
            | Interaction::Dragging { .. } => {
                mouse::Interaction::Grabbing
            }
            Interaction::None => {
                let bounds = layout.bounds();
                if let Some(position) = cursor.position_over(bounds) {
                    let local = local_point(position, bounds);
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

impl<'a, Message: 'a, F: 'a, G: 'a, H: 'a, K: 'a, L: 'a, Theme, Renderer>
    From<TileView<'a, Message, F, G, H, K, L>> for Element<'a, Message, Theme, Renderer>
where
    F: Fn(Range<usize>) -> Message,
    G: Fn(Option<(usize, EntryId)>) -> Message,
    H: Fn((usize, EntryId)) -> Message,
    K: Fn(Rectangle) -> Message,
    L: Fn((usize, EntryId, [f32; 4])) -> Message,
    Renderer: renderer::Renderer + geometry::Renderer,
{
    fn from(view: TileView<'a, Message, F, G, H, K, L>) -> Self {
        Self::new(view)
    }
}
