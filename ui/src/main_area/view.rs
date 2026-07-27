use iced::widget::{container, row, text};
use iced::{Element, Font, Length};

use crate::event::UiEvent;
use crate::main_area::overlay::OverlayEntry;
use crate::main_area::viewer::{TileSpec, TileView};
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
        match state.view_mode() {
            crate::event::MainAreaMode::View => {
                let viewer = build_viewer(state, tiles(state, false), false).show_overlay_buttons(true);
                iced::widget::stack![viewer, edit_overlay(state), mode_switcher(state)].into()
            }
            crate::event::MainAreaMode::Compare => {
                let left = build_viewer(state, tiles(state, true), true);
                let right = build_viewer(state, tiles(state, false), false);
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
            .show_inpaint(false)
            .show_overlay_text(false)
            .show_scrollbar(false);
    } else {
        viewer = viewer
            .on_entry_clicked(UiEvent::EntryClicked)
            .on_entry_double_clicked(|(index, id)| UiEvent::EntryDoubleClicked((index, id)))
            .on_edit_rect(UiEvent::EditRect)
            .on_entry_moved(UiEvent::EntryMoved)
            .on_toolbar_action(UiEvent::EntryToolbar)
            .on_inpaint_selection(UiEvent::InpaintSelection)
            .on_inpaint_toolbar(UiEvent::InpaintToolbar)
            .inpaint_mode(state.inpaint_mode())
            .show_inpaint(state.show_inpaint())
            .show_overlay_text(state.show_overlay_text())
            .show_overlay_buttons(false)
            .editing(state.editing())
            .reveal(state.selected())
            .selected_inpaint(state.selected_inpaint())
            .inpaint_reveal(state.selected_inpaint());
    }
    viewer
}
