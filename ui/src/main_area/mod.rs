//! The left column: a scrollable canvas of the loaded pages with OCR
//! overlays, or an empty-state placeholder before any images are loaded.

pub mod decode;
pub mod overlay;
pub mod tile_view;

use iced::keyboard::{key, Key};
use iced::widget::{container, space, text, text_editor};
use iced::{Background, Border, Color, Element, Font, Length, Size};

use scanlateit_model::EntryStyle;

use crate::event::UiEvent;
use crate::main_area::overlay::OverlayEntry;
use crate::main_area::tile_view::{TileSpec, TileView};
use crate::state::UiState;

/// Widget id of the floating inline editor; must match the app's focus id.
const EDIT_INPUT_ID: &'static str = "overlay-editor";

/// The floating multi-line `TextEditor` used to edit a double-clicked overlay
/// entry, positioned (and clipped) exactly over the entry's box.
///
/// The tile viewer publishes the entry's current viewport rect
/// (`UiEvent::EditRect`), which the app stores in `editing_rect`. The editor
/// is wrapped in a [`iced::widget::Pin`] at the rect's coordinates; its size
/// and line height match the overlay's fitted text block, and its top-left is
/// pinned to the top-left of the overlay's vertically-centered wrapped text
/// block, so the editable text sits exactly where the static overlay draws
/// it. `Pin` gives the editor real layout coordinates (unlike `Float`, whose
/// overlay translation leaves event positions in window space and breaks
/// click-drag hit-testing). The editor is styled from the entry's style (text
/// color, background, radius) so it looks like the static overlay.
///
/// Enter inserts a newline (the editor is multi-line); Escape or Ctrl+Enter
/// commit and exit.
fn edit_overlay<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    let (Some((index, id)), Some(rect)) = (state.editing(), state.editing_rect()) else {
        return space().into();
    };
    let Some(content) = state.edit_content() else {
        return space().into();
    };
    let (text, style) = match state
        .images()
        .get(index)
        .and_then(|image| image.project.ocr.get(id))
    {
        Some(entry) => (
            state.images()[index].project.display_text(entry).to_string(),
            state.images()[index].project.entry_style(entry.id),
        ),
        None => (String::new(), EntryStyle::default()),
    };
    let font = overlay::styled_font(state.font().unwrap_or(Font::DEFAULT), &style);
    let wrap_width = rect.width.max(8.0);
    let (size, fitted_height) = overlay::fit_font_metrics(&text, font, Size::new(wrap_width, rect.height));
    let size = size.max(8.0);
    let text_color = to_color(style.text_color);
    let editor = text_editor::TextEditor::new(content)
        .id(EDIT_INPUT_ID)
        .font(font)
        .size(size)
        .line_height(1.2)
        .width(rect.width)
        .height(Length::Fixed(fitted_height))
        .padding(0)
        .on_action(UiEvent::EditAction)
        .key_binding(|press| match press.modified_key.as_ref() {
            Key::Named(key::Named::Escape) => {
                Some(text_editor::Binding::Custom(UiEvent::EditSubmit))
            }
            Key::Named(key::Named::Enter) if press.modifiers.command() => {
                Some(text_editor::Binding::Custom(UiEvent::EditSubmit))
            }
            _ => text_editor::Binding::from_key_press(press),
        })
        .style(move |_theme, _status| text_editor::Style {
            background: Background::Color(Color::TRANSPARENT),
            border: Border::default().rounded(0.0),
            placeholder: text_color,
            value: text_color,
            selection: Color::from_rgba8(92, 190, 255, 0.35),
        });
    // The editor is sized to the wrapped text block and centered in the box;
    // anchor its top-left to the wrapped block's top-left so the first line
    // (and the selection highlight) sits exactly where the static overlay
    // draws it.
    let block_top = rect.y + (rect.height - fitted_height).max(0.0) / 2.0;
    iced::widget::Pin::new(editor)
        .x(rect.x)
        .y(block_top)
        .into()
}

fn to_color(rgba: [u8; 4]) -> Color {
    Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3] as f32 / 255.0)
}

pub fn view<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    if state.images().is_empty() {
        container(text("No images loaded. Click \"Open Images\" to pick some."))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        let tiles: Vec<TileSpec<'_>> = state
            .images()
            .iter()
            .enumerate()
            .map(|(index, image)| TileSpec {
                source_width: image.width as u32,
                source_height: image.height as u32,
                decode: &image.decode,
                overlays: image
                    .project
                    .ocr
                    .visible()
                    .map(|entry| OverlayEntry {
                        id: entry.id,
                        text: image.project.display_text(entry),
                        quad: image.project.view_quad(entry),
                        bounds: image.project.view_quad(entry).bounds(),
                        style: image.project.entry_style(entry.id),
                        selected: state.selected() == Some((index, entry.id)),
                        hide_text: state.editing() == Some((index, entry.id)),
                    })
                    .collect(),
            })
            .collect();
        let viewer = TileView::new(tiles, state.font().unwrap_or(Font::DEFAULT))
            .on_visible_range(UiEvent::TilesVisible)
            .on_scroll_ended(|| UiEvent::TileScrollEnded)
            .on_entry_clicked(UiEvent::EntryClicked)
            .on_entry_double_clicked(|(index, id)| UiEvent::EntryDoubleClicked((index, id)))
            .on_edit_rect(UiEvent::EditRect)
            .on_entry_moved(UiEvent::EntryMoved)
            .on_toolbar_action(UiEvent::EntryToolbar)
            .editing(state.editing());
        iced::widget::stack![viewer, edit_overlay(state)].into()
    }
}