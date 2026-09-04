//! The vertical toolbar pinned to the left edge of the window: a fixed-width
//! column of tool buttons (inpainting toggle, settings). Unlike the
//! side panel, it lives outside the pane grid and is never resizable.

use iced::widget::{button, column, container, tooltip, text};
use iced::{Element, Length};
use lucide_icons::Icon;

use crate::event::UiEvent;
use crate::scale;
use crate::state::UiState;

/// Width of the toolbar, in pixels — always equal to the button size.
pub const TOOLBAR_WIDTH: f32 = 36.0;

fn tip<'a>(label: &'a str) -> container::Container<'a, UiEvent> {
    container(text(label).size(scale::s(11.0)))
        .padding(scale::s(6.0))
        .style(container::rounded_box)
}

pub fn view<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    let can_toggle = !state.images().is_empty();
    let btn_size = scale::s(TOOLBAR_WIDTH);

    let toggle_text_btn = button(
        crate::icon::lucide(if state.show_overlay_text() {
            Icon::MessageCircle
        } else {
            Icon::MessageCircleOff
        })
        .size(scale::s(16.0))
        .center(),
    )
    .width(Length::Fixed(btn_size))
    .height(Length::Fixed(btn_size))
    .padding(scale::s(0.0))
    .style(crate::panel::button_style)
    .on_press_maybe(can_toggle.then_some(UiEvent::ToggleOverlayText));
    let toggle_text: Element<'_, UiEvent> =
        tooltip(toggle_text_btn, tip("Toggle text overlay"), tooltip::Position::Right)
            .gap(scale::s(4.0))
            .into();

    let toggle_inpaint_btn = button(
        crate::icon::lucide(if state.show_inpaint() {
            Icon::Image
        } else {
            Icon::ImageOff
        })
        .size(scale::s(16.0))
        .center(),
    )
    .width(Length::Fixed(btn_size))
    .height(Length::Fixed(btn_size))
    .padding(scale::s(0.0))
    .style(crate::panel::button_style)
    .on_press_maybe(can_toggle.then_some(UiEvent::ToggleInpaintLayer));
    let toggle_inpaint: Element<'_, UiEvent> =
        tooltip(toggle_inpaint_btn, tip("Toggle inpaint layer"), tooltip::Position::Right)
            .gap(scale::s(4.0))
            .into();

    let inpaint_active = state.manual_mode() == crate::event::ManualMode::Inpaint;
    let busy = state.is_bulk_busy();
    // legacy inpaint_mode is now only used when not in manual mode; toolbar reflects manual
    let inpaint_btn = button(
        crate::icon::lucide(if inpaint_active {
            Icon::X
        } else {
            Icon::Brush
        })
        .size(scale::s(16.0))
        .center(),
    )
    .width(Length::Fixed(btn_size))
    .height(Length::Fixed(btn_size))
    .padding(scale::s(0.0))
    .style(crate::panel::button_style)
    .on_press_maybe(if inpaint_active {
        Some(UiEvent::ManualModeCancel)
    } else if !state.images().is_empty() && !busy {
        Some(UiEvent::ManualModeEnter(crate::event::ManualMode::Inpaint))
    } else {
        None
    });
    let inpaint: Element<'_, UiEvent> =
        tooltip(inpaint_btn, tip(if inpaint_active { "Exit inpaint" } else { "Inpaint (multi-select)" }), tooltip::Position::Right)
            .gap(scale::s(4.0))
            .into();

    let ocr_active = state.manual_mode() == crate::event::ManualMode::Ocr;
    let ocr_btn = button(
        crate::icon::lucide(if ocr_active {
            Icon::X
        } else {
            Icon::ScanSearch
        })
        .size(scale::s(16.0))
        .center(),
    )
    .width(Length::Fixed(btn_size))
    .height(Length::Fixed(btn_size))
    .padding(scale::s(0.0))
    .style(crate::panel::button_style)
    .on_press_maybe(if ocr_active {
        Some(UiEvent::ManualModeCancel)
    } else if !state.images().is_empty() && !busy {
        Some(UiEvent::ManualModeEnter(crate::event::ManualMode::Ocr))
    } else {
        None
    });
    let manual_ocr: Element<'_, UiEvent> =
        tooltip(ocr_btn, tip(if ocr_active { "Exit manual OCR" } else { "Manual OCR (multi-select)" }), tooltip::Position::Right)
            .gap(scale::s(4.0))
            .into();

    let settings_btn = button(crate::icon::lucide(Icon::Settings).size(scale::s(16.0)).center())
        .width(Length::Fixed(btn_size))
        .height(Length::Fixed(btn_size))
        .padding(scale::s(0.0))
        .style(crate::panel::button_style)
        .on_press(UiEvent::SettingsOpen);
    let settings: Element<'_, UiEvent> =
        tooltip(settings_btn, tip("Settings"), tooltip::Position::Right)
            .gap(scale::s(4.0))
            .into();

    container(column![toggle_text, toggle_inpaint, inpaint, manual_ocr, settings]
        .spacing(scale::s(6.0))
        .align_x(iced::Alignment::Center))
        .width(Length::Fixed(btn_size))
        .height(Length::Fill)
        .style(|_theme| container::Style {
            background: None,
            ..container::Style::default()
        })
        .into()
}