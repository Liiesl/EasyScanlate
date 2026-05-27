//! The left column: a scrollable canvas of the loaded pages with OCR
//! overlays, or an empty-state placeholder before any images are loaded.

pub mod decode;
pub mod overlay;
pub mod tile_view;

use iced::widget::{container, space, text, text_input};
use iced::{Background, Border, Color, Element, Font, Length, Size};

use crate::app::{App, Message};
use crate::model::EntryStyle;
use crate::ui::main_area::overlay::OverlayEntry;
use crate::ui::main_area::tile_view::{TileSpec, TileView};

/// Widget id of the floating inline editor; must match the app's focus id.
const EDIT_INPUT_ID: &'static str = "overlay-editor";

/// The floating `TextInput` used to edit a double-clicked overlay entry,
/// positioned (and clipped) exactly over the entry's box.
///
/// The tile viewer publishes the entry's current viewport rect
/// (`Message::EditRect`), which the app stores in `editing_rect`. The input
/// is wrapped in a [`iced::widget::Pin`] at the rect's coordinates; the
/// input's size and line height match the overlay's fitted text, and its
/// top-left is pinned to the top-left of the overlay's vertically-centered
/// wrapped text block, so the editable first line and its selection highlight
/// sit exactly where the static overlay draws them. `Pin` gives the input
/// real layout coordinates (unlike `Float`, whose overlay translation leaves
/// event positions in window space and breaks click-drag hit-testing). The
/// input is styled from the entry's style (text color, background, radius) so
/// it looks like the static overlay.
fn edit_overlay(app: &App) -> Element<'_, Message> {
    let (Some((index, id)), Some(rect)) = (app.editing, app.editing_rect) else {
        return space().into();
    };
    let (text, style) = match app
        .images
        .get(index)
        .and_then(|image| image.project.ocr.get(id))
    {
        Some(entry) => (
            app.images[index].project.display_text(entry).to_string(),
            app.images[index].project.entry_style(entry.id),
        ),
        None => (String::new(), EntryStyle::default()),
    };
    let font = overlay::styled_font(app.font.unwrap_or(Font::DEFAULT), &style);
    let wrap_width = rect.width.max(8.0);
    let (size, fitted_height) = overlay::fit_font_metrics(&text, font, Size::new(wrap_width, rect.height));
    let size = size.max(8.0);
    let text_color = to_color(style.text_color);
    let input = text_input::TextInput::new("", &text)
        .id(EDIT_INPUT_ID)
        .font(font)
        .size(size)
        .line_height(1.2)
        .width(rect.width)
        .padding(0)
        .on_input(Message::EditChanged)
        .on_submit(Message::EditSubmit)
        .style(move |_theme, _status| text_input::Style {
            background: Background::Color(Color::TRANSPARENT),
            border: Border::default().rounded(0.0),
            icon: Color::TRANSPARENT,
            placeholder: text_color,
            value: text_color,
            selection: Color::from_rgba8(92, 190, 255, 0.35),
        });
    // The input is one line tall while the overlay wraps the text block and
    // centers it in the box; anchor the input's top-left to the wrapped
    // block's top-left so the first line (and the selection highlight) sits
    // exactly where the static overlay draws it.
    let block_top = rect.y + (rect.height - fitted_height).max(0.0) / 2.0;
    iced::widget::Pin::new(input)
        .x(rect.x)
        .y(block_top)
        .into()
}

fn to_color(rgba: [u8; 4]) -> Color {
    Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3] as f32 / 255.0)
}

pub fn view(app: &App) -> Element<'_, Message> {
    if app.images.is_empty() {
        container(text("No images loaded. Click \"Open Images\" to pick some."))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        let tiles: Vec<TileSpec<'_>> = app
            .images
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
                        bounds: entry.quad.bounds(),
                        style: image.project.entry_style(entry.id),
                        selected: app.selected == Some((index, entry.id)),
                        hide_text: app.editing == Some((index, entry.id)),
                    })
                    .collect(),
            })
            .collect();
        let viewer = TileView::new(tiles, app.font.unwrap_or(Font::DEFAULT))
            .on_visible_range(Message::TilesVisible)
            .on_entry_clicked(Message::EntryClicked)
            .on_entry_double_clicked(|(index, id)| Message::EntryDoubleClicked((index, id)))
            .on_edit_rect(Message::EditRect)
            .editing(app.editing);
        if app.editing.is_some() {
            iced::widget::stack![viewer, edit_overlay(app)].into()
        } else {
            viewer.into()
        }
    }
}
