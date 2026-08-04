//! The vertical toolbar pinned to the left edge of the window: a fixed-width
//! column of tool buttons (inpainting toggle, settings). Unlike the
//! side panel, it lives outside the pane grid and is never resizable.

use iced::widget::{button, column, container, tooltip, text};
use iced::{Element, Length};
use lucide_icons::Icon;

use crate::event::UiEvent;
use crate::scale;
use crate::state::UiState;

/// Fixed width of the toolbar, in pixels.
pub const TOOLBAR_WIDTH: f32 = 52.0;

fn tip<'a>(label: &'a str) -> container::Container<'a, UiEvent> {
    container(text(label).size(scale::s(11.0)))
        .padding(scale::s(6.0))
        .style(container::rounded_box)
}

pub fn view<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    let can_toggle = !state.images().is_empty();
    let btn_size = scale::s(36.0);

    let toggle_text_btn = button(
        crate::icon::lucide(if state.show_overlay_text() {
            Icon::EyeOff
        } else {
            Icon::Eye
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
            Icon::EyeOff
        } else {
            Icon::Eye
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

    let inpaint_active = state.inpaint_mode();
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
    .on_press_maybe(
        (!state.images().is_empty() && !state.running() && !state.translating())
            .then_some(UiEvent::Inpaint),
    );
    let inpaint: Element<'_, UiEvent> =
        tooltip(inpaint_btn, tip(if inpaint_active { "Exit inpaint" } else { "Inpaint" }), tooltip::Position::Right)
            .gap(scale::s(4.0))
            .into();

    let ocr_active = state.ocr_mode();
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
    .on_press_maybe(
        (!state.images().is_empty() && !state.running() && !state.translating())
            .then_some(UiEvent::ManualOcr),
    );
    let manual_ocr: Element<'_, UiEvent> =
        tooltip(ocr_btn, tip(if ocr_active { "Exit manual OCR" } else { "Manual OCR" }), tooltip::Position::Right)
            .gap(scale::s(4.0))
            .into();

    let open_btn = button(crate::icon::lucide(Icon::FolderOpen).size(scale::s(16.0)).center())
        .width(Length::Fixed(btn_size))
        .height(Length::Fixed(btn_size))
        .padding(scale::s(0.0))
        .style(crate::panel::button_style)
        .on_press(UiEvent::OpenProject);
    let open: Element<'_, UiEvent> =
        tooltip(open_btn, tip("Open project (.mmtl)"), tooltip::Position::Right)
            .gap(scale::s(4.0))
            .into();

    let save_btn = button(crate::icon::lucide(Icon::Download).size(scale::s(16.0)).center())
        .width(Length::Fixed(btn_size))
        .height(Length::Fixed(btn_size))
        .padding(scale::s(0.0))
        .style(crate::panel::button_style)
        .on_press(UiEvent::SaveProject);
    let save: Element<'_, UiEvent> =
        tooltip(save_btn, tip("Save project (.mmtl)  Ctrl+S"), tooltip::Position::Right)
            .gap(scale::s(4.0))
            .into();

    let save_as_btn = button(crate::icon::lucide(Icon::Copy).size(scale::s(16.0)).center())
        .width(Length::Fixed(btn_size))
        .height(Length::Fixed(btn_size))
        .padding(scale::s(0.0))
        .style(crate::panel::button_style)
        .on_press(UiEvent::SaveProjectAs);
    let save_as: Element<'_, UiEvent> =
        tooltip(save_as_btn, tip("Save As...  Ctrl+Shift+S"), tooltip::Position::Right)
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

    container(column![toggle_text, toggle_inpaint, inpaint, manual_ocr, open, save, save_as, settings]
        .spacing(scale::s(6.0))
        .padding(scale::s(4.0))
        .align_x(iced::Alignment::Center))
        .width(Length::Fixed(scale::s(TOOLBAR_WIDTH)))
        .height(Length::Fill)
        .style(|_theme| container::Style {
            background: None,
            ..container::Style::default()
        })
        .into()
}