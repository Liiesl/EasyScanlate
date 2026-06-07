//! Top row: the action buttons and live state. Opening images, starting and
//! stopping OCR, the running status, profile cycling and the settings button
//! all live in one compact row across the panel's full width.

use iced::widget::{button, row, text};
use iced::{Element, Length};

use crate::event::UiEvent;
use crate::state::UiState;

pub fn view<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    let profile_name = state
        .images()
        .first()
        .map(|i| i.project.profiles.selected().name.clone())
        .unwrap_or_else(|| "Default".to_string());
    row![
        button("Open Images...")
            .on_press(UiEvent::OpenImages)
            .padding(4),
        button("Start OCR").on_press_maybe(
            (!state.images().is_empty() && !state.running() && !state.translating())
                .then_some(UiEvent::StartOcr)
        ),
        button("Stop").on_press_maybe(state.running().then_some(UiEvent::StopOcr)),
        text(state.status()).size(12).width(Length::Fill),
        text(format!("Profile: {profile_name}")).size(12),
        button("Next").on_press_maybe(
            (state.images().first().is_some_and(|i| i.project.profiles.len() > 1))
                .then_some(UiEvent::CycleProfile)
        ),
        button("Settings")
            .on_press(UiEvent::SettingsOpen)
            .padding(4),
    ]
    .spacing(6)
    .width(Length::Fill)
    .into()
}