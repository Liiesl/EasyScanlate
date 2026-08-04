use iced::widget::image::Handle;

use scanlateit_model::ImageId;

use crate::main_area::decode::PageDecode;

/// One in-memory inpainting result: the reconstructed pixels of a drawn
/// range, layered over the original image while the app runs. Never written
/// to disk.
pub struct InpaintLayer {
    /// The covered range `[x, y, w, h]` in image pixels.
    pub bounds: [f32; 4],
    /// The reconstructed patch, cached as a GPU handle.
    pub handle: Handle,
    pub width: u32,
    pub height: u32,
}

/// One loaded image plus everything the viewer needs that the model doesn't
/// know about (per-image canvas cache). Geometry (`width/height/path`) is
/// derived from the chapter-wide `Project` via `image_id`.
pub struct LoadedImage {
    pub image_id: ImageId,
    pub decode: PageDecode,
    /// Inpaint layers drawn over the original image, oldest first.
    pub inpaint: Vec<InpaintLayer>,
}