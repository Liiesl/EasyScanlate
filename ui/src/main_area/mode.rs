use iced::widget::space::Space;
use iced::widget::{button, column, container, row, text};
use iced::{Element, Font, Length};

use crate::event::{MainAreaMode, ManualMode, UiEvent};
use crate::scale;
use crate::segmented::{segment, segmented_group};
use crate::state::UiState;

/// The floating "View | Compare" mode switcher, pinned to the center-top of the main area.
/// When a manual mode is active it is replaced by the mode banner with Start/Reset/Cancel.
pub fn mode_switcher<'a, S: UiState + ?Sized>(state: &'a S) -> Element<'a, UiEvent> {
    match state.manual_mode() {
        ManualMode::Inpaint | ManualMode::Ocr => manual_banner(state),
        ManualMode::None => view_compare_pill(state),
    }
}

fn view_compare_pill<'a, S: UiState + ?Sized>(state: &'a S) -> Element<'a, UiEvent> {
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

fn manual_banner<'a, S: UiState + ?Sized>(state: &'a S) -> Element<'a, UiEvent> {
    let is_inpaint = state.manual_mode() == ManualMode::Inpaint;
    let title = if is_inpaint { "Manual Inpaint Mode" } else { "Manual OCR Mode" };
    let desc = if is_inpaint {
        "Drag on the image to select areas to inpaint • Multiple selections"
    } else {
        "Drag on the image to select text areas to OCR • Multiple selections"
    };
    let count = state.manual_selections().len();
    let count_label = if count == 0 {
        "No selections".to_string()
    } else if count == 1 {
        "1 selection".to_string()
    } else {
        format!("{count} selections")
    };
    let busy = state.is_bulk_busy();
    let can_start = count > 0 && !busy;
    let start_label = if is_inpaint { format!("Start Inpaint ({count})") } else { format!("Start OCR ({count})") };
    let start_btn: Element<'_, UiEvent> = crate::button::with_disabled_cursor(
        button(text(start_label).size(scale::s(11.0)))
            .padding([scale::s(6.0), scale::s(12.0)])
            .style(crate::panel::button_style)
            .on_press_maybe(can_start.then_some(UiEvent::ManualModeStart))
            .into(),
    );
    let reset_btn: Element<'_, UiEvent> = crate::button::with_disabled_cursor(
        button(text("Reset").size(scale::s(11.0)))
            .padding([scale::s(6.0), scale::s(12.0)])
            .style(crate::panel::button_style)
            .on_press_maybe((count > 0).then_some(UiEvent::ManualModeReset))
            .into(),
    );
    let cancel_btn = button(text("Cancel").size(scale::s(11.0)))
        .padding([scale::s(6.0), scale::s(12.0)])
        .style(crate::panel::button_style)
        .on_press(UiEvent::ManualModeCancel);

    let card = container(
        column![
            text(title).size(scale::s(13.0)),
            text(desc).size(scale::s(10.0)).color(iced::Color::from_rgba8(160, 165, 180, 1.0)),
            row![text(count_label).size(scale::s(10.0)).width(Length::Fill), start_btn, reset_btn, cancel_btn]
                .spacing(scale::s(6.0))
                .align_y(iced::Alignment::Center)
        ]
        .spacing(scale::s(6.0)),
    )
    .padding(scale::s(10.0))
    .width(Length::Fixed(scale::s(420.0)))
    .style(|_theme| container::Style {
        background: Some(crate::panel::PANEL_BG.into()),
        border: iced::Border::default().rounded(scale::s(10.0)).color(iced::Color::from_rgba8(80, 85, 110, 0.6)).width(1.0),
        ..Default::default()
    });

    container(
        column![
            row![
                Space::new().width(Length::Fill),
                card,
                Space::new().width(Length::Fill)
            ],
            Space::new().height(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(scale::s(8.0))
    .into()
}
