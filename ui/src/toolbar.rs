//! The vertical toolbar pinned to the left edge of the window: a fixed-width
//! column of tool buttons (inpainting toggle, settings). Unlike the
//! side panel, it lives outside the pane grid and is never resizable.

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, text};
use iced::{Element, Length};

use crate::event::UiEvent;
use crate::scale;
use crate::state::UiState;

/// Fixed width of the toolbar, in pixels.
pub const TOOLBAR_WIDTH: f32 = 76.0;

pub fn view<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    let can_toggle = !state.images().is_empty();

    let toggle_text = button(
        text(if state.show_overlay_text() {
            "Hide Text"
        } else {
            "Show Text"
        })
        .size(scale::s(13.0))
        .wrapping(Wrapping::Word),
    )
    .width(Length::Fill)
    .padding(scale::s(4.0))
    .on_press_maybe(can_toggle.then_some(UiEvent::ToggleOverlayText));

    let toggle_inpaint = button(
        text(if state.show_inpaint() {
            "Hide Inpaint"
        } else {
            "Show Inpaint"
        })
        .size(scale::s(13.0))
        .wrapping(Wrapping::Word),
    )
    .width(Length::Fill)
    .padding(scale::s(4.0))
    .on_press_maybe(can_toggle.then_some(UiEvent::ToggleInpaintLayer));

    let inpaint_active = state.inpaint_mode();
    let inpaint = button(
        text(if inpaint_active {
            "Cancel Inpaint"
        } else {
            "Inpaint"
        })
        .size(scale::s(13.0))
        .wrapping(Wrapping::Word),
    )
    .width(Length::Fill)
    .padding(scale::s(4.0))
    .on_press_maybe(
        (!state.images().is_empty() && !state.running() && !state.translating())
            .then_some(UiEvent::Inpaint),
    );

    let settings = button(text("Settings").size(scale::s(13.0)).wrapping(Wrapping::Word))
        .width(Length::Fill)
        .padding(scale::s(4.0))
        .on_press(UiEvent::SettingsOpen);

    container(column![toggle_text, toggle_inpaint, inpaint, settings]
        .spacing(scale::s(6.0))
        .padding(scale::s(8.0)))
        .width(Length::Fixed(scale::s(TOOLBAR_WIDTH)))
        .height(Length::Fill)
        .style(|_theme| container::Style {
            background: None,
            ..container::Style::default()
        })
        .into()
}