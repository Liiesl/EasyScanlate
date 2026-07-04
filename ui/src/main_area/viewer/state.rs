use std::ops::Range;
use std::time::Instant;

use iced::keyboard;
use iced::Rectangle;

use scanlateit_model::EntryId;

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
    /// Whether inpainting range drags are enabled.
    pub inpaint_mode: bool,
    /// The last `reveal` request consumed in `layout()`.
    pub last_revealed: Option<(usize, EntryId)>,
    /// The last scroll offset published through `on_scroll`.
    pub last_published_offset: Option<f32>,
}

impl TileViewState {
    pub fn inpaint_mode(&self) -> bool {
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
            inpaint_mode: false,
            last_revealed: None,
            last_published_offset: None,
        }
    }
}
