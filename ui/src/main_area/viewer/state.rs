use std::ops::Range;
use std::time::Instant;

use iced::keyboard;
use iced::Rectangle;

use easyscanlate_model::EntryId;

use crate::event::ManualMode;

use super::interaction::Interaction;

#[derive(Debug, Clone)]
pub struct TileViewState {
    pub offset: f32,
    pub width: f32,
    pub content_height: f32,
    pub viewport_height: f32,
    pub interaction: Interaction,
    pub last_visible: Option<Range<usize>>,
    /// The previous left-press hit plus when it happened, for double-click detection.
    pub last_click: Option<(Instant, Option<(usize, EntryId)>)>,
    /// The last published viewport rect of the edited entry.
    pub last_edit_rect: Option<Rectangle>,
    /// Current keyboard modifiers, cached from `ModifiersChanged`.
    pub keyboard_modifiers: keyboard::Modifiers,
    /// Persistent manual mode (when Some, draws multi-selection chrome).
    pub manual_mode: ManualMode,
    /// Snapshot of pending selections for drawing (mirrors App.manual_selections).
    pub manual_selections_snapshot: Vec<(usize, iced::Rectangle)>,
    /// The last `reveal` request consumed in `layout()`.
    pub last_revealed: Option<(usize, EntryId)>,
    /// The last inpaint reveal consumed in `layout()`.
    pub last_inpaint_revealed: Option<(usize, usize)>,
    /// The last scroll offset published through `on_scroll` (legacy, kept for
    /// exact-equality fallback; new code uses `last_published_anchor`).
    pub last_published_offset: Option<f32>,
    /// The last normalized center anchor published through `on_scroll`:
    /// `(offset + viewport/2)/content_height` clamped 0..1. Mirrored by the app
    /// as `viewer_scroll` so a resize or `View↔Compare` width change restores
    /// the same centered row instead of the same absolute pixel offset.
    pub last_published_anchor: Option<f32>,
    /// Whether the vertical save-menu (save / image) is expanded to the right of the Save button.
    pub save_menu_open: bool,
}

impl TileViewState {
    pub fn inpaint_mode(&self) -> bool {
        self.manual_mode == ManualMode::Inpaint
    }
    pub fn ocr_mode(&self) -> bool {
        self.manual_mode == ManualMode::Ocr
    }
    pub fn manual_active(&self) -> bool {
        self.manual_mode != ManualMode::None
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
            manual_mode: ManualMode::None,
            manual_selections_snapshot: Vec::new(),
            last_revealed: None,
            last_inpaint_revealed: None,
            last_published_offset: None,
            last_published_anchor: None,
            save_menu_open: false,
        }
    }
}
