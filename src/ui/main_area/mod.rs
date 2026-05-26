//! The left column: a scrollable canvas of the loaded pages with OCR
//! overlays, or an empty-state placeholder before any images are loaded.

pub mod decode;
pub mod overlay;
pub mod tile_view;

use iced::widget::{container, text};
use iced::{Element, Font, Length};

use crate::app::{App, Message};
use crate::ui::main_area::overlay::OverlayEntry;
use crate::ui::main_area::tile_view::{TileSpec, TileView};

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
            .map(|image| TileSpec {
                source_width: image.width as u32,
                source_height: image.height as u32,
                decode: &image.decode,
                overlays: image
                    .project
                    .ocr
                    .visible()
                    .map(|entry| OverlayEntry {
                        text: image.project.display_text(entry),
                        bounds: entry.quad.bounds(),
                        style: app.style,
                    })
                    .collect(),
            })
            .collect();
        TileView::new(tiles, app.font.unwrap_or(Font::DEFAULT))
            .on_visible_range(Message::TilesVisible)
            .into()
    }
}
