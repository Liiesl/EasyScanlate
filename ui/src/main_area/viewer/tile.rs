use crate::main_area::decode::PageDecode;
use crate::main_area::overlay::OverlayEntry;

/// One stacked page in the viewer.
pub struct TileSpec<'a> {
    pub source_width: u32,
    pub source_height: u32,
    pub decode: &'a PageDecode,
    pub overlays: Vec<OverlayEntry<'a>>,
    /// Inpaint layers drawn over the page raster, below the entry overlays.
    pub inpaint: &'a [crate::loaded::InpaintLayer],
}
