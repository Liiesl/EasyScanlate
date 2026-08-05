use crate::event::EditOrigin;
use crate::main_area::overlay::OverlayEntry;
use crate::main_area::viewer::TileSpec;
use crate::state::UiState;

/// Builds the tile specs of one viewer pane. `original` strips everything the compare pane must not show.
/// While an inpaint patch is selected OCR overlays are hidden (temporarily) per user request.
pub fn tiles<'a, S: UiState + ?Sized>(state: &'a S, original: bool) -> Vec<TileSpec<'a>> {
    let hide_ocr = !original && state.selected_inpaint().is_some();
    let project = state.project();
    state
        .images()
        .iter()
        .enumerate()
        .map(|(index, image)| {
            let overlays: Vec<OverlayEntry<'a>> = if original || hide_ocr {
                Vec::new()
            } else {
                project
                    .visible_for(image.image_id)
                    .map(|entry| OverlayEntry {
                        id: entry.id,
                        text: project.display_text(entry),
                        quad: project.view_quad(entry),
                        bounds: project.view_quad(entry).bounds(),
                        style: project.entry_style(entry.id),
                        selected: state.selected() == Some((index, entry.id)),
                        quad_overridden: project.has_view_quad(entry.id),
                        hide_text: state.editing() == Some((index, entry.id))
                            && state.editing_origin() == EditOrigin::Overlay,
                    })
                    .collect()
            };
            let (source_width, source_height) = project
                .image(image.image_id)
                .map(|m| (m.width as u32, m.height as u32))
                .unwrap_or((0, 0));
            TileSpec {
                source_width,
                source_height,
                decode: &image.decode,
                inpaint: if original { &[] } else { &image.inpaint },
                overlays,
            }
        })
        .collect()
}
