//! The left column: a scrollable canvas of the loaded pages with OCR
//! overlays, or an empty-state placeholder before any images are loaded.

pub mod decode;
pub mod edit;
pub mod geometry;
pub mod mode;
pub mod overlay;
pub mod tiles;
pub mod view;
pub mod viewer;

// Compatibility shim: old `tile_view` path now re-exports `viewer`.
#[allow(dead_code)]
pub mod tile_view {
    pub use crate::main_area::viewer::{TileSpec, TileView};
}

pub use view::view;
pub use tiles::tiles;
pub use edit::{edit_overlay, EDIT_INPUT_ID};
pub use mode::mode_switcher;
