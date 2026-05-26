//! A self-contained, canvas-based vertical tile viewer.
//!
//! Owns its own scrolling (wheel, touch pan, scrollbar drag), tiles page
//! images vertically, paints only the tiles that are actually visible, and
//! paints OCR overlays on top of each tile. Visible-range changes are
//! reported through [`TileView::on_visible_range`] so the app can decode
//! exactly the pages that are needed.

use std::ops::Range;

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

const SCROLL_LINE_HEIGHT: f32 = 180.0;
const SCROLLBAR_WIDTH: f32 = 8.0;
const SCROLLBAR_MARGIN: f32 = 2.0;
const MIN_THUMB_HEIGHT: f32 = 20.0;

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
pub struct TileView<'a, Message, F = fn(Range<usize>) -> Message>
where
    F: Fn(Range<usize>) -> Message,
{
    tiles: Vec<TileSpec<'a>>,
    font: Font,
    on_visible_range: Option<F>,
}

impl<'a, Message, F> TileView<'a, Message, F>
where
    F: Fn(Range<usize>) -> Message,
{
    pub fn new(tiles: Vec<TileSpec<'a>>, font: Font) -> Self {
        Self {
            tiles,
            font,
            on_visible_range: None,
        }
    }

    /// Called whenever the set of visible tiles changes, including on the
    /// first frame and on window resizes.
    pub fn on_visible_range(mut self, f: F) -> Self {
        self.on_visible_range = Some(f);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Interaction {
    None,
    TouchScrolling { origin: Point },
    ScrollerGrabbed { grab_offset: f32 },
}

#[derive(Debug, Clone)]
struct TileViewState {
    offset: f32,
    width: f32,
    content_height: f32,
    viewport_height: f32,
    interaction: Interaction,
    last_visible: Option<Range<usize>>,
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

fn draw_tile<F>(frame: &mut F, tile: &TileSpec<'_>, font: Font)
where
    F: geometry::frame::Backend,
{
    match tile.decode {
        PageDecode::Ready(decoded) => {
            frame.draw_image(
                Rectangle::with_size(frame.size()),
                geometry::Image::new(decoded.handle.clone()),
            );
            overlay::draw_entries(frame, &tile.overlays, font, tile.source_width as f32);
        }
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
    }
}

impl<'a, Message, F, Theme, Renderer> Widget<Message, Theme, Renderer> for TileView<'a, Message, F>
where
    F: Fn(Range<usize>) -> Message,
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
                    draw_tile(&mut frame, tile, self.font);
                    renderer.draw_geometry(frame.into_geometry());
                }
            });
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
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if matches!(state.interaction, Interaction::ScrollerGrabbed { .. }) {
                    state.interaction = Interaction::None;
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let Interaction::ScrollerGrabbed { grab_offset } = state.interaction {
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
            }
            Event::Window(_) => {
                publish_visible(shell, &self.tiles, state, &self.on_visible_range);
            }
            _ => {}
        }
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
            Interaction::TouchScrolling { .. } | Interaction::ScrollerGrabbed { .. } => {
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

impl<'a, Message: 'a, F: 'a, Theme, Renderer> From<TileView<'a, Message, F>>
    for Element<'a, Message, Theme, Renderer>
where
    F: Fn(Range<usize>) -> Message,
    Renderer: renderer::Renderer + geometry::Renderer,
{
    fn from(view: TileView<'a, Message, F>) -> Self {
        Self::new(view)
    }
}
