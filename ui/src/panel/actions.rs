//! Top row: the action buttons and live state. Opening images, starting and
//! stopping OCR, the running status and the settings button all live in one
//! compact row across the panel's full width.

use iced::widget::{button, container, row, text, tooltip};
use iced::{Element, Length};
use lucide_icons::Icon;

use crate::event::UiEvent;
use crate::scale;
use crate::state::UiState;

fn tip_label(label: &str) -> container::Container<'_, UiEvent> {
    container(text(label).size(scale::s(11.0)))
        .padding(scale::s(6.0))
        .style(container::rounded_box)
}

pub fn view<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    let is_running = state.running();
    let is_bulk_busy = state.is_bulk_busy();
    // While any bulk job (pipeline / translate / inpaint / manual OCR) is active,
    // Start OCR must stay disabled; Stop OCR stays enabled only for raw OCR.
    let is_stop = is_running;
    let can_start = !state.images().is_empty() && !is_bulk_busy && !is_running;
    let ocr_label = if is_stop { "Stop OCR" } else { "Start OCR" };
    let ocr_btn = button(
        row![
            crate::icon::lucide(if is_stop { Icon::Square } else { Icon::Play })
                .size(scale::s(14.0))
                .center(),
            text(ocr_label).size(scale::s(12.0))
        ]
        .spacing(scale::s(4.0))
        .align_y(iced::Alignment::Center),
    )
    .style(crate::panel::button_style)
    .on_press_maybe(
        if is_stop {
            Some(UiEvent::StopOcr)
        } else if can_start {
            Some(UiEvent::StartOcr)
        } else {
            None
        },
    )
    .padding(scale::s(6.0));
    let ocr: Element<'_, UiEvent> = tooltip(ocr_btn, tip_label(ocr_label), tooltip::Position::Bottom)
        .gap(scale::s(4.0))
        .into();

    let settings_btn = button(crate::icon::lucide(Icon::Settings).size(scale::s(16.0)).center())
        .style(crate::panel::button_style)
        .on_press(UiEvent::SettingsOpen)
        .padding(scale::s(6.0));
    let settings: Element<'_, UiEvent> =
        tooltip(settings_btn, tip_label("Settings"), tooltip::Position::Bottom)
            .gap(scale::s(4.0))
            .into();

    container(
        row![ocr, text(state.status()).size(scale::s(12.0)).width(Length::Fill), settings]
        .spacing(scale::s(6.0))
        .width(Length::Fill),
    )
    .padding(scale::s(8.0))
    .width(Length::Fill)
    .style(|_theme| container::Style {
        background: None,
        ..container::Style::default()
    })
    .into()
}