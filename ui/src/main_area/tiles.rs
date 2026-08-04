use crate::event::EditOrigin;
use crate::main_area::overlay::OverlayEntry;
use crate::main_area::viewer::TileSpec;
use crate::state::UiState;

/// Builds the tile specs of one viewer pane. `original` strips everything the compare pane must not show.
/// While an inpaint patch is selected OCR overlays are hidden (temporarily) per user request.
pub fn tiles<'a, S: UiState + ?Sized>(state: &'a S, original: bool) -> Vec<TileSpec<'a>> {
    let hide_ocr = !original && state.selected_inpaint().is_some();
    state
        .images()
        .iter()
        .enumerate()
        .map(|(index, image)| {
            let overlays: Vec<OverlayEntry<'a>> = if original || hide_ocr {
                Vec::new()
            } else {
                image
                    .project
                    .ocr
                    .visible_for(image.image_id)
                    .map(|entry| OverlayEntry {
                        id: entry.id,
                        text: image.project.display_text(entry),
                        quad: image.project.view_quad(entry),
                        bounds: image.project.view_quad(entry).bounds(),
                        style: image.project.entry_style(entry.id),
                        selected: state.selected() == Some((index, entry.id)),
                        quad_overridden: image.project.has_view_quad(entry.id),
                        hide_text: state.editing() == Some((index, entry.id))
                            && state.editing_origin() == EditOrigin::Overlay,
                    })
                    .collect()
            };
            TileSpec {
                source_width: image.width as u32,
                source_height: image.height as u32,
                decode: &image.decode,
                inpaint: if original { &[] } else { &image.inpaint },
                overlays,
            }
        })
        .collect()
}
