use iced::widget::space::Space;
use iced::widget::{column, container, row};
use iced::{Element, Font, Length};

use crate::event::{MainAreaMode, UiEvent};
use crate::scale;
use crate::segmented::{segment, segmented_group};
use crate::state::UiState;

/// The floating "View | Compare" mode switcher, pinned to the center-top of the main area.
pub fn mode_switcher<'a, S: UiState + ?Sized>(state: &'a S) -> Element<'a, UiEvent> {
    let mode = state.view_mode();
    let pill = container(segmented_group(vec![
        segment(
            mode == MainAreaMode::View,
            "View",
            Some(UiEvent::MainAreaMode(MainAreaMode::View)),
            Font::DEFAULT,
        ),
        segment(
            mode == MainAreaMode::Compare,
            "Compare",
            Some(UiEvent::MainAreaMode(MainAreaMode::Compare)),
            Font::DEFAULT,
        ),
    ]))
    .width(Length::Fixed(scale::s(180.0)))
    .padding(scale::s(6.0));
    container(
        column![
            row![
                Space::new().width(Length::Fill),
                pill,
                Space::new().width(Length::Fill)
            ],
            Space::new().height(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
