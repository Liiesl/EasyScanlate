//! Top row: the action buttons and live state. Opening images, starting and
//! stopping OCR, the running status and the settings button all live in one
//! compact row across the panel's full width.

use iced::widget::{button, container, row, text};
use iced::{Element, Length};

use crate::event::UiEvent;
use crate::state::UiState;

pub fn view<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    container(
        row![
            button("Open Images...")
                .on_press(UiEvent::OpenImages)
                .padding(4),
            button(if state.running() { "Stop" } else { "Start OCR" }).on_press_maybe(
                if state.running() {
                    Some(UiEvent::StopOcr)
                } else if !state.images().is_empty() && !state.translating() {
                    Some(UiEvent::StartOcr)
                } else {
                    None
                }
            ),
            text(state.status()).size(12).width(Length::Fill),
            button("Settings")
                .on_press(UiEvent::SettingsOpen)
                .padding(4),
        ]
        .spacing(6)
        .width(Length::Fill),
    )
    .padding(8)
    .width(Length::Fill)
    .style(|_theme| container::Style {
        background: None,
        ..container::Style::default()
    })
    .into()
}