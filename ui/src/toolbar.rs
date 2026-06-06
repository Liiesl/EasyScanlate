//! The vertical toolbar pinned to the left edge of the window: a fixed-width
//! column of tool buttons (only the inpainting toggle for now). Unlike the
//! side panel, it lives outside the pane grid and is never resizable.

use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, text};
use iced::{Element, Length};

use crate::event::UiEvent;
use crate::state::UiState;
use crate::panel::PANEL_BG;

/// Fixed width of the toolbar, in pixels.
pub const TOOLBAR_WIDTH: f32 = 76.0;

pub fn view<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    let inpaint_active = state.inpaint_mode().is_some();
    let inpaint = button(
        text(if inpaint_active {
            "Cancel Inpaint"
        } else {
            "Inpaint"
        })
        .size(13)
        .wrapping(Wrapping::Word),
    )
    .width(Length::Fill)
    .padding(4)
    .on_press_maybe(
        (!state.images().is_empty() && !state.running() && !state.translating())
            .then_some(UiEvent::Inpaint),
    );

    container(column![inpaint].spacing(6).padding(8))
        .width(Length::Fixed(TOOLBAR_WIDTH))
        .height(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(PANEL_BG.into()),
            ..container::Style::default()
        })
        .into()
}