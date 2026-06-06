//! Top section: opening images, the OCR action row (start/stop) and profile
//! cycling.

use iced::widget::{button, column, row, text};
use iced::Element;

use crate::event::UiEvent;
use crate::state::UiState;

pub fn view<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    let profile_name = state
        .images()
        .first()
        .map(|i| i.project.profiles.selected().name.clone())
        .unwrap_or_else(|| "Default".to_string());
    column![
        row![
            text("Scanlateit").size(24),
            button("Open Images...")
                .on_press(UiEvent::OpenImages)
                .padding(4),
        ]
        .spacing(8),
        row![
            button("Start OCR").on_press_maybe(
                (!state.images().is_empty() && !state.running() && !state.translating())
                    .then_some(UiEvent::StartOcr)
            ),
            button("Stop").on_press_maybe(state.running().then_some(UiEvent::StopOcr)),
        ]
        .spacing(6),
        row![
            text(format!("Profile: {profile_name}")).size(12),
            button("Next").on_press_maybe(
                (state.images().first().is_some_and(|i| i.project.profiles.len() > 1))
                    .then_some(UiEvent::CycleProfile)
            ),
        ]
        .spacing(6),
    ]
    .spacing(6)
    .into()
}