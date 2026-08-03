pub mod tile;
pub use tile::TileSpec;
pub mod constants;
pub mod draw;
pub mod hit_test;
pub mod interaction;
pub mod layout;
pub mod motion;
pub mod scroll;
pub mod state;

use std::ops::Range;
use std::time::Instant;

use iced::advanced::graphics::geometry::frame::Backend as _;
use iced::advanced::graphics::geometry::{self, Fill, Path, Stroke, Text};
use iced::advanced::layout::{self as iced_layout, Layout};
use iced::advanced::mouse;
use iced::advanced::renderer;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::widget::Widget;
use iced::advanced::{Clipboard, Shell};
use iced::border::Radius;
use iced::keyboard;
use iced::touch::Event as TouchEvent;
use iced::{Color, Element, Event, Font, Length, Pixels, Point, Rectangle, Size, Vector};

use crate::event::{InpaintToolbarAction, ToolbarAction};
use crate::main_area::decode::PageDecode;
use crate::main_area::overlay::OverlayEntry;
use scanlateit_model::{EntryId, Quad};

use self::constants::{
    DOUBLE_CLICK_DELAY, DRAG_THRESHOLD, HANDLE_SIZE, MIN_BOX_EDGE, MIN_INPAINT_EDGE,
    MIN_OCR_EDGE, SCROLL_LINE_HEIGHT,
};
use self::draw::{
    draw_inpaint_decorations, draw_inpaint_marquee, draw_ocr_marquee, draw_overlay_buttons,
    draw_placeholder, draw_scrollbar, draw_selection_decorations,
};
use self::hit_test::{
    editing_rect, entry_quad, entry_rect, hit_entry, hit_handle, hit_inpaint_toolbar,
    hit_overlay_button, hit_tile, hit_toolbar, hit_top_decor, inpaint_reveal_offset, local_point,
    overlay_button_rects, point_in_quad, reveal_offset, selected_quad_view, selected_rect,
    tile_local_point,
};
use self::interaction::{Interaction, OverlayButton, ResizeHandle, TopDecorHit};
use self::layout::{content_width, tile_layout};
use self::motion::{
    distort_quad, drag_grab, drag_quad, handle_anchors, handle_rect, quad_centroid, resize_quad,
    rotate_quad, toolbar_buttons,
};
use self::scroll::{
    anchor_from_state, offset_from_anchor, publish_anchor, publish_edit_rect, publish_visible,
    scroll_by, thumb_rect, track_rect,
};
use self::state::TileViewState;

/// The tile viewer widget. Scroll state lives in the widget tree and survives rebuilds.
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
    R = fn(f32) -> Message,
    S = fn((usize, usize, InpaintToolbarAction)) -> Message,
    T = fn((usize, Rectangle)) -> Message,
> where
    F: Fn(Range<usize>) -> Message,
    G: Fn(Option<(usize, EntryId)>) -> Message,
    H: Fn((usize, EntryId)) -> Message,
    K: Fn(Rectangle) -> Message,
    L: Fn((usize, EntryId, Quad)) -> Message,
    M: Fn((usize, EntryId, ToolbarAction)) -> Message,
    P: Fn() -> Message,
    Q: Fn((usize, Rectangle)) -> Message,
    R: Fn(f32) -> Message,
    S: Fn((usize, usize, InpaintToolbarAction)) -> Message,
    T: Fn((usize, Rectangle)) -> Message,
{
    tiles: Vec<TileSpec<'a>>,
    font: Font,
    on_visible_range: Option<F>,
    on_entry_clicked: Option<G>,
    on_entry_double_clicked: Option<H>,
    on_edit_rect: Option<K>,
    on_entry_moved: Option<L>,
    on_toolbar_action: Option<M>,
    on_scroll_ended: Option<P>,
    on_inpaint_selection: Option<Q>,
    editing: Option<(usize, EntryId)>,
    inpaint_mode: bool,
    ocr_mode: bool,
    show_inpaint: bool,
    show_overlay_text: bool,
    show_overlay_buttons: bool,
    show_scrollbar: bool,
    reveal: Option<(usize, EntryId)>,
    on_scroll: Option<R>,
    /// Normalized center anchor `0..1` (`(offset+viewport/2)/content_height`);
    /// the app's `viewer_scroll`. Stable across width/viewport changes
    /// (resize, `View↔Compare`).
    scroll_to: Option<f32>,
    selected_inpaint: Option<(usize, usize)>,
    on_inpaint_toolbar: Option<S>,
    inpaint_reveal: Option<(usize, usize)>,
    on_ocr_selection: Option<T>,
}

impl<'a, Message, F, G, H, K, L, M, P, Q, R, S, T> TileView<'a, Message, F, G, H, K, L, M, P, Q, R, S, T>
where
    F: Fn(Range<usize>) -> Message,
    G: Fn(Option<(usize, EntryId)>) -> Message,
    H: Fn((usize, EntryId)) -> Message,
    K: Fn(Rectangle) -> Message,
    L: Fn((usize, EntryId, Quad)) -> Message,
    M: Fn((usize, EntryId, ToolbarAction)) -> Message,
    P: Fn() -> Message,
    Q: Fn((usize, Rectangle)) -> Message,
    R: Fn(f32) -> Message,
    S: Fn((usize, usize, InpaintToolbarAction)) -> Message,
    T: Fn((usize, Rectangle)) -> Message,
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
            inpaint_mode: false,
            ocr_mode: false,
            show_inpaint: true,
            show_overlay_text: true,
            show_overlay_buttons: true,
            show_scrollbar: true,
            reveal: None,
            on_scroll: None,
            scroll_to: None,
            selected_inpaint: None,
            on_inpaint_toolbar: None,
            inpaint_reveal: None,
            on_ocr_selection: None,
        }
    }

    pub fn on_visible_range(mut self, f: F) -> Self {
        self.on_visible_range = Some(f);
        self
    }

    pub fn on_entry_clicked(mut self, f: G) -> Self {
        self.on_entry_clicked = Some(f);
        self
    }

    pub fn on_entry_double_clicked(mut self, f: H) -> Self {
        self.on_entry_double_clicked = Some(f);
        self
    }

    pub fn on_edit_rect(mut self, f: K) -> Self {
        self.on_edit_rect = Some(f);
        self
    }

    pub fn on_entry_moved(mut self, f: L) -> Self {
        self.on_entry_moved = Some(f);
        self
    }

    pub fn on_toolbar_action(mut self, f: M) -> Self {
        self.on_toolbar_action = Some(f);
        self
    }

    pub fn on_scroll_ended(mut self, f: P) -> Self {
        self.on_scroll_ended = Some(f);
        self
    }

    pub fn on_scroll(mut self, f: R) -> Self {
        self.on_scroll = Some(f);
        self
    }

    /// Requests that the viewport be scrolled so that this normalized center
    /// anchor (`0..1`) sits at the viewport's vertical center on next
    /// `layout`. Idempotent; the app feeds `viewer_scroll` (center anchor)
    /// here to keep `View↔Compare` and resizes visually stable.
    pub fn scroll_to(mut self, anchor: f32) -> Self {
        self.scroll_to = Some(anchor);
        self
    }

    pub fn on_inpaint_selection(mut self, f: Q) -> Self {
        self.on_inpaint_selection = Some(f);
        self
    }

    pub fn on_ocr_selection(mut self, f: T) -> Self {
        self.on_ocr_selection = Some(f);
        self
    }

    pub fn inpaint_mode(mut self, inpaint_mode: bool) -> Self {
        self.inpaint_mode = inpaint_mode;
        self
    }

    pub fn ocr_mode(mut self, ocr_mode: bool) -> Self {
        self.ocr_mode = ocr_mode;
        self
    }

    pub fn show_inpaint(mut self, show_inpaint: bool) -> Self {
        self.show_inpaint = show_inpaint;
        self
    }

    pub fn show_overlay_text(mut self, show_overlay_text: bool) -> Self {
        self.show_overlay_text = show_overlay_text;
        self
    }

    pub fn show_overlay_buttons(mut self, show_overlay_buttons: bool) -> Self {
        self.show_overlay_buttons = show_overlay_buttons;
        self
    }

    pub fn show_scrollbar(mut self, show_scrollbar: bool) -> Self {
        self.show_scrollbar = show_scrollbar;
        self
    }

    pub fn editing(mut self, editing: Option<(usize, EntryId)>) -> Self {
        self.editing = editing;
        self
    }

    pub fn reveal(mut self, reveal: Option<(usize, EntryId)>) -> Self {
        self.reveal = reveal;
        self
    }

    pub fn selected_inpaint(mut self, selected: Option<(usize, usize)>) -> Self {
        self.selected_inpaint = selected;
        self
    }

    pub fn inpaint_reveal(mut self, reveal: Option<(usize, usize)>) -> Self {
        self.inpaint_reveal = reveal;
        self
    }

    pub fn on_inpaint_toolbar(mut self, f: S) -> Self {
        self.on_inpaint_toolbar = Some(f);
        self
    }
}

// ---------------------------------------------------------------------------
// Widget impl - delegates geometry/hit-testing/drawing to submodules
// ---------------------------------------------------------------------------

impl<'a, Message, F, G, H, K, L, M, P, Q, R, S, T, Theme, Renderer> Widget<Message, Theme, Renderer>
    for TileView<'a, Message, F, G, H, K, L, M, P, Q, R, S, T>
where
    F: Fn(Range<usize>) -> Message,
    G: Fn(Option<(usize, EntryId)>) -> Message,
    H: Fn((usize, EntryId)) -> Message,
    K: Fn(Rectangle) -> Message,
    L: Fn((usize, EntryId, Quad)) -> Message,
    M: Fn((usize, EntryId, ToolbarAction)) -> Message,
    P: Fn() -> Message,
    Q: Fn((usize, Rectangle)) -> Message,
    R: Fn(f32) -> Message,
    S: Fn((usize, usize, InpaintToolbarAction)) -> Message,
    T: Fn((usize, Rectangle)) -> Message,
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

    fn layout(&mut self, tree: &mut Tree, _renderer: &Renderer, limits: &iced_layout::Limits) -> iced_layout::Node {
        let width = limits.max().width;
        let new_viewport = limits.max().height;
        let state = tree.state.downcast_mut::<TileViewState>();
        // Visual anchor before geometry is overwritten: center fraction, so a
        // resize or View↔Compare width change keeps the same row centered
        // instead of the same absolute offset.
        let old_anchor = if state.content_height > f32::EPSILON && state.viewport_height > 0.0 {
            anchor_from_state(state)
        } else {
            state.last_published_anchor.unwrap_or(0.0)
        };
        let new_width = content_width(width);
        let (_, new_content_height) = tile_layout(&self.tiles, new_width);
        let size_changed = (new_width - state.width).abs() > f32::EPSILON
            || (new_viewport - state.viewport_height).abs() > f32::EPSILON
            || (new_content_height - state.content_height).abs() > f32::EPSILON;
        state.width = new_width;
        state.inpaint_mode = self.inpaint_mode;
        state.ocr_mode = self.ocr_mode;
        state.content_height = new_content_height;
        state.viewport_height = new_viewport;
        if let Some(anchor) = self.scroll_to {
            let new_offset = offset_from_anchor(anchor, new_content_height, new_viewport);
            if (new_offset - state.offset).abs() > f32::EPSILON {
                state.offset = new_offset;
            }
        } else if size_changed {
            let new_offset = offset_from_anchor(old_anchor, new_content_height, new_viewport);
            if (new_offset - state.offset).abs() > f32::EPSILON {
                state.offset = new_offset;
            }
        }
        if self.reveal != state.last_revealed {
            state.last_revealed = self.reveal;
            if let Some((index, id)) = self.reveal {
                if let Some(new_offset) = hit_test::reveal_offset(&self.tiles, state, index, id) {
                    state.offset = new_offset;
                }
            }
        }
        // Inpaint selection reveal (panel -> main area): bring its bbox into view.
        if self.inpaint_reveal != state.last_inpaint_revealed {
            state.last_inpaint_revealed = self.inpaint_reveal;
            if let Some((index, patch_idx)) = self.inpaint_reveal {
                if let Some(tile) = self.tiles.get(index) {
                    if let Some(layer) = tile.inpaint.get(patch_idx) {
                        if let Some(new_offset) = inpaint_reveal_offset(&self.tiles, state, index, layer.bounds) {
                            state.offset = new_offset;
                        }
                    }
                }
            }
        }
        if new_viewport > 0.0 {
            state.offset = state
                .offset
                .min((new_content_height - new_viewport).max(0.0))
                .max(0.0);
        }
        iced_layout::Node::new(Size::new(width, limits.max().height))
    }

    fn draw(&self, tree: &Tree, renderer: &mut Renderer, _theme: &Theme, _style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle) {
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
                    let tile_bounds = Rectangle::new(Point::new(0.0, y), Size::new(content_w, height));
                    let mut frame = renderer.new_frame(tile_bounds);
                    frame.translate(Vector::new(tile_bounds.x, tile_bounds.y));
                    match tile.decode.image() {
                        Some(decoded) => {
                            frame.draw_image(
                                Rectangle::with_size(frame.size()),
                                geometry::Image::new(decoded.handle.clone()),
                            );
                            if self.show_inpaint {
                                let scale = frame.width() / tile.source_width.max(1) as f32;
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
            let content_w = content_width(bounds.width);
            let (layout, _) = tile_layout(&self.tiles, content_w);
            let visible_top = state.offset;
            let visible_bottom = state.offset + visible_bounds.height;
            let inpaint_selecting = if state.inpaint_mode() {
                match state.interaction {
                    Interaction::InpaintSelecting { index, .. } => Some(index),
                    _ => None,
                }
            } else {
                None
            };
            let ocr_selecting = if state.ocr_mode() {
                match state.interaction {
                    Interaction::OcrSelecting { index, .. } => Some(index),
                    _ => None,
                }
            } else {
                None
            };
            let mut visible_tiles: Vec<usize> = self
                .tiles
                .iter()
                .enumerate()
                .filter_map(|(index, tile)| {
                    let (y, height) = layout[index];
                    let is_selecting = inpaint_selecting == Some(index) || ocr_selecting == Some(index);
                    // Keep the selecting tile visible even when it has no overlays so the
                    // inpaint marquee (rubber band) always has a frame to draw into.
                    let tile_visible = y + height > visible_top && y < visible_bottom;
                    if is_selecting && tile_visible {
                        return Some(index);
                    }
                    let scale = state.width / tile.source_width.max(1) as f32;
                    let has_visible_entry = tile.overlays.iter().any(|entry| {
                        let [min_x, min_y, max_x, max_y] = entry.bounds;
                        let left = min_x * scale;
                        let right = max_x * scale;
                        let top = y + min_y * scale;
                        let bottom = y + max_y * scale;
                        right >= 0.0 && left <= content_w && bottom >= visible_top && top <= visible_bottom
                    });
                    has_visible_entry.then_some(index)
                })
                .collect();
            // Inpaint selection must also drive a frame even while OCR overlays are hidden.
            if let Some((img_idx, _)) = self.selected_inpaint {
                if !visible_tiles.contains(&img_idx) {
                    if let Some((y, height)) = layout.get(img_idx).copied() {
                        if y + height > visible_top && y < visible_bottom {
                            visible_tiles.push(img_idx);
                        }
                    }
                }
            }
            // Ensure selecting marquee tile is visible even if overlays hidden (original does, but guard again)
            if visible_tiles.is_empty() && inpaint_selecting.is_some() {
                if let Some(idx) = inpaint_selecting {
                    if let Some((y, height)) = layout.get(idx).copied() {
                        if y + height > visible_top && y < visible_bottom {
                            visible_tiles.push(idx);
                        }
                    }
                }
            }
            if visible_tiles.is_empty() && ocr_selecting.is_some() {
                if let Some(idx) = ocr_selecting {
                    if let Some((y, height)) = layout.get(idx).copied() {
                        if y + height > visible_top && y < visible_bottom {
                            visible_tiles.push(idx);
                        }
                    }
                }
            }
            if !visible_tiles.is_empty() {
                renderer.with_layer(local_bounds, |renderer| {
                    renderer.with_translation(Vector::new(0.0, -state.offset), |renderer| {
                        let overlay_clip = Rectangle::new(
                            Point::new(0.0, state.offset),
                            Size::new(content_w, visible_bounds.height),
                        );
                        let mut overlay_frame = renderer.new_frame(overlay_clip);
                        for index in visible_tiles {
                            let (y, height) = layout[index];
                            let tile_bounds = Rectangle::new(Point::new(0.0, y), Size::new(content_w, height));
                            overlay_frame.push_transform();
                            overlay_frame.translate(Vector::new(tile_bounds.x, tile_bounds.y));
                            crate::main_area::overlay::draw_entries(
                                &mut overlay_frame,
                                &self.tiles[index].overlays,
                                self.font,
                                self.tiles[index].source_width as f32,
                                !self.show_overlay_text,
                            );
                            let cursor_local = cursor.position_over(bounds).map(|position| {
                                Point::new(
                                    position.x - bounds.x,
                                    position.y - bounds.y + state.offset - tile_bounds.y,
                                )
                            });
                            let flip_at = state.offset + visible_bounds.height - tile_bounds.y;
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
                            // Inpaint static highlight + floating toolbar (no handles), like result border.
                            if self.show_inpaint {
                                draw_inpaint_decorations(
                                    &mut overlay_frame,
                                    &self.tiles,
                                    index,
                                    self.selected_inpaint,
                                    cursor_local,
                                    flip_at,
                                );
                            }
                            if state.inpaint_mode() {
                                if let Interaction::InpaintSelecting { index: selecting, start, current } = state.interaction {
                                    if selecting == index {
                                        draw_inpaint_marquee(&mut overlay_frame, start, current, tile_bounds.size());
                                    }
                                }
                            }
                            if state.ocr_mode() {
                                if let Interaction::OcrSelecting { index: selecting, start, current } = state.interaction {
                                    if selecting == index {
                                        draw_ocr_marquee(&mut overlay_frame, start, current, tile_bounds.size());
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
        if self.show_scrollbar && state.content_height > visible_bounds.height + 1.0 {
            renderer.with_layer(local_bounds, |renderer| {
                let mut frame = renderer.new_frame(local_bounds);
                draw_scrollbar(&mut frame, state, local_bounds);
                renderer.draw_geometry(frame.into_geometry());
            });
        }
        if self.show_overlay_buttons {
            let hovered_button = cursor
                .position_over(bounds)
                .map(|position| {
                    let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                    hit_overlay_button(local_bounds, local)
                })
                .unwrap_or(None);
            renderer.with_layer(local_bounds, |renderer| {
                let mut frame = renderer.new_frame(local_bounds);
                draw_overlay_buttons(&mut frame, local_bounds, hovered_button);
                renderer.draw_geometry(frame.into_geometry());
            });
        }
        });
    }

    fn update(&mut self, tree: &mut Tree, event: &Event, layout: Layout<'_>, cursor: mouse::Cursor, _renderer: &Renderer, _clipboard: &mut dyn Clipboard, shell: &mut Shell<'_, Message>, _viewport: &Rectangle) {
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
                    if self.show_overlay_buttons {
                        if let Some(button) = hit_overlay_button(bounds, local) {
                            state.interaction = Interaction::OverlayButtonPressed { button };
                            shell.capture_event();
                            return;
                        }
                    }
                    // Inpaint selection has its own floating toolbar (no handles). It is drawn
                    // like the result border but must not be movable/resizable.
                    if self.selected_inpaint.is_some() {
                        if self.show_inpaint {
                            if let Some((idx, patch, action)) = hit_inpaint_toolbar(
                                &self.tiles,
                                state,
                                self.selected_inpaint,
                                local,
                            ) {
                                state.interaction = Interaction::InpaintToolbarPressed {
                                    index: idx,
                                    patch,
                                    action,
                                };
                                shell.capture_event();
                                return;
                            }
                        }
                        // While an inpaint is selected OCR overlays are hidden and must not
                        // be interactive (no drag/resize/rotate/toolbar). Fall through to
                        // scrollbar / empty click handling only.
                    } else if self.editing.is_none() {
                        if let Some((index, id, action)) = hit_toolbar(&self.tiles, state, local) {
                            state.interaction = Interaction::ToolbarPressed { index, id, action };
                            shell.capture_event();
                            return;
                        }
                        if self.inpaint_mode {
                            if let Some(hover_index) = hit_tile(&self.tiles, state, local) {
                                if !track_rect(bounds).contains(local) {
                                    let (layout, _) = tile_layout(&self.tiles, state.width);
                                    let content = tile_local_point(&layout, hover_index, local, state.offset);
                                    state.interaction = Interaction::InpaintSelecting {
                                        index: hover_index,
                                        start: content,
                                        current: content,
                                    };
                                    shell.capture_event();
                                    return;
                                }
                            }
                        }
                        if self.ocr_mode {
                            if let Some(hover_index) = hit_tile(&self.tiles, state, local) {
                                if !track_rect(bounds).contains(local) {
                                    let (layout, _) = tile_layout(&self.tiles, state.width);
                                    let content = tile_local_point(&layout, hover_index, local, state.offset);
                                    state.interaction = Interaction::OcrSelecting {
                                        index: hover_index,
                                        start: content,
                                        current: content,
                                    };
                                    shell.capture_event();
                                    return;
                                }
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
                                        hit_test::entry_quad(&self.tiles, index, id),
                                        hit_test::selected_quad_view(&self.tiles, state, index),
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
                                            center_view: motion::quad_centroid(view),
                                            press: local,
                                        };
                                        shell.capture_event();
                                        return;
                                    }
                                }
                            }
                        }
                        if let Some((index, id, handle)) = hit_handle(&self.tiles, state, local) {
                            if let Some(quad) = hit_test::entry_quad(&self.tiles, index, id) {
                                if let Some(corner) = handle.corner() {
                                    if state.keyboard_modifiers.command() {
                                        state.interaction = Interaction::DistortPending {
                                            index,
                                            id,
                                            corner,
                                            quad: Quad {
                                                points: crate::main_area::geometry::order_quad(quad.points),
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
                    if self.show_scrollbar && track_rect(bounds).contains(local) {
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
                            if let (Some(hit), Some(callback)) = (hit, self.on_entry_double_clicked.as_ref()) {
                                shell.publish(callback(hit));
                            }
                            if let (Some(hit), Some(rect_callback)) = (hit, self.on_edit_rect.as_ref()) {
                                if let Some(rect) = editing_rect(&self.tiles, state, hit) {
                                    state.last_edit_rect = Some(rect);
                                    shell.publish(rect_callback(rect));
                                }
                            }
                        } else if let Some(callback) = self.on_entry_clicked.as_ref() {
                            shell.publish(callback(hit));
                        }
                        if let Some((index, id)) = hit {
                            if let Some(offset) = drag_grab(&self.tiles, state, index, id, local) {
                                if let Some(quad) = hit_test::entry_quad(&self.tiles, index, id) {
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
                if let Interaction::InpaintToolbarPressed { index, patch, action } = state.interaction {
                    if let Some(position) = cursor.position_over(bounds) {
                        let local = local_point(position, bounds);
                        let still_hovered = hit_inpaint_toolbar(
                            &self.tiles,
                            state,
                            self.selected_inpaint,
                            local,
                        ) == Some((index, patch, action));
                        if still_hovered {
                            if let Some(callback) = self.on_inpaint_toolbar.as_ref() {
                                shell.publish(callback((index, patch, action)));
                                shell.request_redraw();
                            }
                        }
                    }
                }
                if let Interaction::ToolbarPressed { index, id, action } = state.interaction {
                    if let Some(position) = cursor.position_over(bounds) {
                        let local = local_point(position, bounds);
                        let still_hovered = match action {
                            ToolbarAction::RevertTransform => {
                                hit_top_decor(&self.tiles, state, local) == Some((index, id, TopDecorHit::Revert))
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
                if let Interaction::OcrSelecting { index, start, current } = state.interaction {
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
                        let rect = Rectangle::new(
                            Point::new(x0 / scale, y0 / scale),
                            Size::new((x1 - x0) / scale, (y1 - y0) / scale),
                        );
                        if rect.width >= MIN_OCR_EDGE && rect.height >= MIN_OCR_EDGE {
                            if let Some(callback) = self.on_ocr_selection.as_ref() {
                                shell.publish(callback((index, rect)));
                                shell.request_redraw();
                            }
                        }
                    }
                }
                if let Interaction::OverlayButtonPressed { button } = state.interaction {
                    if let Some(position) = cursor.position_over(bounds) {
                        let local = local_point(position, bounds);
                        if hit_overlay_button(bounds, local) == Some(button) {
                            match button {
                                OverlayButton::GoTop | OverlayButton::GoBottom => {
                                    let max_offset = (state.content_height - state.viewport_height).max(0.0);
                                    let target = match button {
                                        OverlayButton::GoTop => 0.0,
                                        _ => max_offset,
                                    };
                                    if (target - state.offset).abs() > f32::EPSILON {
                                        state.offset = target;
                                        shell.request_redraw();
                                        publish_visible(shell, &self.tiles, state, &self.on_visible_range);
                                        if let Some(callback) = self.on_scroll_ended.as_ref() {
                                            shell.publish(callback());
                                        }
                                    }
                                }
                                OverlayButton::Save => {}
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
                        | Interaction::InpaintToolbarPressed { .. }
                        | Interaction::OverlayButtonPressed { .. }
                        | Interaction::InpaintSelecting { .. }
                        | Interaction::OcrSelecting { .. }
                ) {
                    state.interaction = Interaction::None;
                    shell.capture_event();
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
                    Interaction::DragPending { index, id, offset, quad, press } => {
                        let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                        let dx = local.x - press.x;
                        let dy = local.y - press.y;
                        if dx * dx + dy * dy >= DRAG_THRESHOLD * DRAG_THRESHOLD {
                            state.interaction = Interaction::Dragging { index, id, offset, quad };
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
                    Interaction::ResizePending { index, id, handle, quad, press } => {
                        let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                        let dx = local.x - press.x;
                        let dy = local.y - press.y;
                        if dx * dx + dy * dy >= DRAG_THRESHOLD * DRAG_THRESHOLD {
                            state.interaction = Interaction::Resizing { index, id, handle, quad };
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
                    Interaction::DistortPending { index, id, corner, quad, press } => {
                        let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                        let dx = local.x - press.x;
                        let dy = local.y - press.y;
                        if dx * dx + dy * dy >= DRAG_THRESHOLD * DRAG_THRESHOLD {
                            state.interaction = Interaction::Distorting { index, id, corner, quad };
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
                    Interaction::RotatePending { index, id, quad, center_img, center_view, press } => {
                        let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                        let dx = local.x - press.x;
                        let dy = local.y - press.y;
                        if dx * dx + dy * dy >= DRAG_THRESHOLD * DRAG_THRESHOLD {
                            state.interaction = Interaction::Rotating { index, id, quad, center_img, center_view, press };
                            if let Some(callback) = self.on_entry_moved.as_ref() {
                                let snap = state.keyboard_modifiers.shift();
                                let rotated = rotate_quad(quad, center_img, center_view, press, local, snap);
                                shell.publish(callback((index, id, rotated)));
                                shell.request_redraw();
                            }
                        }
                        shell.capture_event();
                    }
                    Interaction::Rotating { index, id, quad, center_img, center_view, press } => {
                        let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                        if let Some(callback) = self.on_entry_moved.as_ref() {
                            let snap = state.keyboard_modifiers.shift();
                            let rotated = rotate_quad(quad, center_img, center_view, press, local, snap);
                            shell.publish(callback((index, id, rotated)));
                            shell.request_redraw();
                        }
                        shell.capture_event();
                    }
                    Interaction::InpaintSelecting { index, start, .. } => {
                        let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                        let (layout, _) = tile_layout(&self.tiles, state.width);
                        let current = tile_local_point(&layout, index, local, state.offset);
                        state.interaction = Interaction::InpaintSelecting { index, start, current };
                        shell.request_redraw();
                        shell.capture_event();
                    }
                    Interaction::OcrSelecting { index, start, .. } => {
                        let local = Point::new(position.x - bounds.x, position.y - bounds.y);
                        let (layout, _) = tile_layout(&self.tiles, state.width);
                        let current = tile_local_point(&layout, index, local, state.offset);
                        state.interaction = Interaction::OcrSelecting { index, start, current };
                        shell.request_redraw();
                        shell.capture_event();
                    }
                    Interaction::ToolbarPressed { .. }
                    | Interaction::InpaintToolbarPressed { .. } => {
                        shell.capture_event();
                    }
                    Interaction::OverlayButtonPressed { .. } => {
                        shell.capture_event();
                    }
                    Interaction::None | Interaction::TouchScrolling { .. } => {}
                }
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.keyboard_modifiers = *modifiers;
            }
            Event::Window(_) => {
                publish_visible(shell, &self.tiles, state, &self.on_visible_range);
            }
            _ => {}
        }
        publish_edit_rect(shell, &self.tiles, state, self.editing, &self.on_edit_rect);
        publish_anchor(shell, state, &self.on_scroll);
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, _viewport: &Rectangle, _renderer: &Renderer) -> mouse::Interaction {
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
            | Interaction::InpaintToolbarPressed { .. }
            | Interaction::OverlayButtonPressed { .. }
            | Interaction::InpaintSelecting { .. }
            | Interaction::OcrSelecting { .. } => mouse::Interaction::Grabbing,
            Interaction::None => {
                let bounds = layout.bounds();
                if let Some(position) = cursor.position_over(bounds) {
                    let local = local_point(position, bounds);
                    if self.show_overlay_buttons {
                        if hit_overlay_button(bounds, local).is_some() {
                            return mouse::Interaction::Pointer;
                        }
                    }
                    // Inpaint toolbar takes precedence and OCR handles are hidden while inpaint selected.
                    if self.selected_inpaint.is_some() {
                        if self.show_inpaint {
                            if hit_inpaint_toolbar(&self.tiles, state, self.selected_inpaint, local).is_some() {
                                return mouse::Interaction::Pointer;
                            }
                        }
                        if self.show_scrollbar && (track_rect(bounds).contains(local) || thumb_rect(bounds, state).contains(local)) {
                            return mouse::Interaction::Pointer;
                        }
                        return mouse::Interaction::None;
                    }
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
                        if (self.inpaint_mode || self.ocr_mode) && hit_tile(&self.tiles, state, local).is_some() {
                            return mouse::Interaction::Crosshair;
                        }
                    }
                    if self.show_scrollbar && (track_rect(bounds).contains(local) || thumb_rect(bounds, state).contains(local)) {
                        return mouse::Interaction::Pointer;
                    }
                }
                mouse::Interaction::None
            }
        }
    }
}

impl<'a, Message: 'a, F: 'a, G: 'a, H: 'a, K: 'a, L: 'a, M: 'a, P: 'a, Q: 'a, R: 'a, S: 'a, T: 'a, Theme, Renderer>
    From<TileView<'a, Message, F, G, H, K, L, M, P, Q, R, S, T>> for Element<'a, Message, Theme, Renderer>
where
    F: Fn(Range<usize>) -> Message,
    G: Fn(Option<(usize, EntryId)>) -> Message,
    H: Fn((usize, EntryId)) -> Message,
    K: Fn(Rectangle) -> Message,
    L: Fn((usize, EntryId, Quad)) -> Message,
    M: Fn((usize, EntryId, ToolbarAction)) -> Message,
    P: Fn() -> Message,
    Q: Fn((usize, Rectangle)) -> Message,
    R: Fn(f32) -> Message,
    S: Fn((usize, usize, InpaintToolbarAction)) -> Message,
    T: Fn((usize, Rectangle)) -> Message,
    Renderer: renderer::Renderer + geometry::Renderer,
{
    fn from(view: TileView<'a, Message, F, G, H, K, L, M, P, Q, R, S, T>) -> Self {
        Self::new(view)
    }
}
