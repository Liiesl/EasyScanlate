//! Top section: opening images plus the OCR action row (start/stop) and
//! profile cycling.

use iced::widget::{button, column, row, text};
use iced::Element;

use crate::app::{App, Message};

pub fn view(app: &App) -> Element<'_, Message> {
    let profile_name = app
        .images
        .first()
        .map(|i| i.project.profiles.selected().name.clone())
        .unwrap_or_else(|| "Default".to_string());
    column![
        row![
            text("Scanlateit").size(24),
            button("Open Images...")
                .on_press(Message::OpenImages)
                .padding(4),
        ]
        .spacing(8),
        row![
            button("Start OCR").on_press_maybe(
                (!app.images.is_empty() && !app.running && !app.translating)
                    .then_some(Message::StartOcr)
            ),
            button("Stop").on_press_maybe(app.running.then_some(Message::StopOcr)),
        ]
        .spacing(6),
        row![
            text(format!("Profile: {profile_name}")).size(12),
            button("Next").on_press_maybe(
                (app.images.first().is_some_and(|i| i.project.profiles.len() > 1))
                    .then_some(Message::CycleProfile)
            ),
        ]
        .spacing(6),
    ]
    .spacing(6)
    .into()
}
