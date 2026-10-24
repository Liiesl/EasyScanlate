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
    pub use crate::main_area::viewer::{GradientHandleSpec, TileSpec, TileView};
}

pub use edit::{EDIT_INPUT_ID, edit_overlay};
pub use mode::mode_switcher;
pub use tiles::tiles;
pub use view::view;
