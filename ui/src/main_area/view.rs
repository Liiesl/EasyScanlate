use iced::widget::{container, row, text};
use iced::{Element, Font, Length};

use crate::event::{StyleField, UiEvent};
use crate::main_area::viewer::{GradientHandleSpec, TileSpec, TileView};
use crate::state::UiState;

use super::edit::edit_overlay;
use super::mode::mode_switcher;
use super::tiles::tiles;

pub fn view<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    if state.images().is_empty() {
        container(text("No images loaded. Click \"Open Images\" to pick some."))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        // Persistent manual mode forces View and hides overlay save buttons
        let manual_active = state.manual_mode() != crate::event::ManualMode::None;
        let effective_mode = if manual_active {
            crate::event::MainAreaMode::View
        } else {
            state.view_mode()
        };
        let show_overlay = !manual_active;
        match effective_mode {
            crate::event::MainAreaMode::View => {
                let viewer = build_viewer(state, tiles(state, false), false)
                    .show_overlay_buttons(show_overlay)
                    .on_save(|| UiEvent::SaveProject)
                    .on_export(|| UiEvent::ExportAll);
                iced::widget::stack![viewer, edit_overlay(state), mode_switcher(state)].into()
            }
            crate::event::MainAreaMode::Compare => {
                let left = build_viewer(state, tiles(state, true), true).show_overlay_buttons(false);
                let right = build_viewer(state, tiles(state, false), false)
                    .show_overlay_buttons(show_overlay)
                    .on_save(|| UiEvent::SaveProject)
                    .on_export(|| UiEvent::ExportAll);
                iced::widget::stack![
                    row![left, iced::widget::stack![right, edit_overlay(state)]].spacing(2),
                    mode_switcher(state),
                ]
                .into()
            }
        }
    }
}

/// Builds one `TileView` pane. `original` renders a pure raster pane in Compare mode.
/// `viewer_scroll` is the normalized center anchor `0..1`, so both Compare
/// panes and a later `View` restore the same centered row after a width /
/// viewport change instead of the same absolute pixel offset.
fn build_viewer<'a, S: UiState + ?Sized>(state: &'a S, tiles: Vec<TileSpec<'a>>, original: bool) -> TileView<'a, UiEvent> {
    let mut viewer: TileView<'a, UiEvent> = TileView::new(tiles, state.font().unwrap_or(Font::DEFAULT));
    viewer = viewer
        .on_visible_range(UiEvent::TilesVisible)
        .on_scroll_ended(|| UiEvent::TileScrollEnded)
        .on_scroll(UiEvent::ViewerScroll)
        .scroll_to(state.viewer_scroll());
    if original {
        viewer = viewer
            .on_entry_clicked(UiEvent::EntryClicked)
            .inpaint_mode(false)
            .ocr_mode(false)
            .show_inpaint(false)
            .show_overlay_text(false)
            .show_scrollbar(false);
    } else {
        // Manual multi-select routes through unified ManualSelection events
        let manual_mode = state.manual_mode();
        let manual_sels = state.manual_selections();
        viewer = viewer
            .on_entry_clicked(UiEvent::EntryClicked)
            .on_entry_double_clicked(|(index, id)| UiEvent::EntryDoubleClicked((index, id)))
            .on_edit_rect(UiEvent::EditRect)
            .on_entry_moved(UiEvent::EntryMoved)
            .on_toolbar_action(UiEvent::EntryToolbar)
            .on_inpaint_toolbar(UiEvent::InpaintToolbar)
            .on_manual_selection(UiEvent::ManualSelectionAdded)
            .on_manual_span(UiEvent::ManualSelectionSpan)
            .manual_mode(manual_mode)
            .manual_selections(manual_sels.to_vec())
            .inpaint_mode(manual_mode == crate::event::ManualMode::Inpaint)
            .ocr_mode(manual_mode == crate::event::ManualMode::Ocr)
            .show_inpaint(state.show_inpaint())
            .show_overlay_text(state.show_overlay_text())
            .editing(state.editing())
            .reveal(state.selected())
            .selected_inpaint(state.selected_inpaint())
            .inpaint_reveal(state.selected_inpaint())
            .gradient_handle(gradient_handle_spec(state))
            .on_gradient_angle(UiEvent::StyleGradientAngle);
    }
    viewer
}

/// Figma-like angle handle for the selected entry, shown only while a
/// gradient picker is open for a gradient field. `None` otherwise.
fn gradient_handle_spec<S: UiState + ?Sized>(state: &S) -> Option<GradientHandleSpec> {
    if state.manual_mode() != crate::event::ManualMode::None {
        return None;
    }
    if state.selected_inpaint().is_some() {
        return None;
    }
    let (index, id) = state.selected()?;
    // Suppress while the inline overlay editor hides the entry text.
    if state.editing() == Some((index, id)) {
        return None;
    }
    let field = state.style_picker_open()?;
    // Reactive tab gating: hide on the Color tab even if the working style
    // is still gradient, show immediately on the Gradient tab even before
    // the app's tab-changed conversion lands (provisional solid→gradient).
    let picker_tab = state.style_picker_tab();
    if picker_tab == Some(neverliie_iced_widgets::color_picker::PickerTab::Color) {
        return None;
    }
    let on_gradient_tab =
        picker_tab == Some(neverliie_iced_widgets::color_picker::PickerTab::Gradient);
    let style = state.style_working();
    let (angle, a, b) = match field {
        StyleField::Fill if style.text_gradient => {
            (style.gradient_angle, style.gradient_a, style.gradient_b)
        }
        StyleField::Fill if on_gradient_tab => {
            (style.gradient_angle, style.text_color, style.text_color)
        }
        StyleField::Stroke if style.stroke_gradient => (
            style.stroke_gradient_angle,
            style.stroke_gradient_a,
            style.stroke_gradient_b,
        ),
        StyleField::Stroke if on_gradient_tab => (
            style.stroke_gradient_angle,
            style.stroke_color,
            style.stroke_color,
        ),
        StyleField::Background if style.bg_gradient => (
            style.bg_gradient_angle,
            style.bg_gradient_a,
            style.bg_gradient_b,
        ),
        StyleField::Background if on_gradient_tab => (
            style.bg_gradient_angle,
            style.bg_color,
            style.bg_color,
        ),
        _ => return None,
    };
    Some(GradientHandleSpec {
        index,
        id,
        field,
        angle,
        color_a: crate::color::rgba_to_color(a),
        color_b: crate::color::rgba_to_color(b),
    })
}
