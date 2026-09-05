//! Shared button wrapper giving disabled buttons a `NotAllowed` cursor.
//!
//! `iced::widget::Button` reports `Pointer` when enabled + hovered and
//! `None` (plain arrow) otherwise, so a disabled button looks identical to
//! dead background space. Wrapping in a `mouse_area` with
//! `Interaction::NotAllowed` fixes the hover affordance: the wrapper only
//! applies when the content reports `None` (see `mouse_area`
//! `mouse_interaction`), so enabled buttons still show `Pointer` while
//! disabled ones show the 🚫 cursor — mirroring `toggler`'s disabled
//! behavior. Always wrap (never branch on `enabled`) to keep the widget
//! tree shape stable across enabled/disabled flips.

use iced::{Element, mouse};
use iced::widget::mouse_area;

use crate::event::UiEvent;

/// Wrap a button element so hovering it while disabled shows `NotAllowed`.
pub fn with_disabled_cursor<'a>(content: Element<'a, UiEvent>) -> Element<'a, UiEvent> {
    mouse_area(content)
        .interaction(mouse::Interaction::NotAllowed)
        .into()
}
