use super::constants::{SCROLLBAR_MARGIN, SCROLLBAR_WIDTH};
use super::TileSpec;

/// Width available to tile content: scrollbar gutter reserved.
pub fn content_width(width: f32) -> f32 {
    (width - SCROLLBAR_WIDTH - SCROLLBAR_MARGIN).max(0.0)
}

/// Returns `(tile_y, tile_height)` per tile and total content height.
pub fn tile_layout(tiles: &[TileSpec<'_>], width: f32) -> (Vec<(f32, f32)>, f32) {
    let mut layout = Vec::with_capacity(tiles.len());
    let mut y = 0.0;
    for tile in tiles {
        let height = if width > 0.0 && tile.source_width > 0 {
            width * tile.source_height as f32 / tile.source_width as f32
        } else {
            0.0
        };
        layout.push((y, height));
        y += height;
    }
    (layout, y)
}
