use iced::widget::image::Handle;

use easyscanlate_model::{ImageId, Quad};

use crate::main_area::decode::PageDecode;

/// One in-memory inpainting result: the reconstructed pixels of a drawn
/// range, layered over the original image while the app runs. Never written
/// to disk.
#[derive(Debug, Clone)]
pub struct InpaintLayer {
    /// The covered range `[x, y, w, h]` in image pixels (AABB).
    pub bounds: [f32; 4],
    /// The actual quad in image pixels, if known (for rotated/skewed patches).
    /// `None` for legacy upright patches.
    pub quad: Option<Quad>,
    /// The reconstructed patch, cached as a GPU handle. Pixels outside `quad`
    /// are transparent (`alpha=0`) for rotated patches.
    pub handle: Handle,
    pub width: u32,
    pub height: u32,
}

/// One loaded image plus everything the viewer needs that the model doesn't
/// know about (per-image canvas cache). Geometry (`width/height/path`) is
/// derived from the chapter-wide `Project` via `image_id`.
///
/// `decode` is a pure view-cache (thumb/full tiers) that lives outside the
/// live DB. `inpaint` is a **derived GPU cache** that mirrors
/// `Project::extras.inpaint_patches` (single source of `bounds`+`InpaintId`);
/// its length must not be treated as canonical — use `Project::inpaint_for()`
/// for counts and prefer stable `InpaintId` over per-image `usize` indexes.
#[derive(Debug, Clone)]
pub struct LoadedImage {
    pub image_id: ImageId,
    pub decode: PageDecode,
    /// Inpaint layers drawn over the original image, oldest first.
    /// Derived from `Project`; `ModelEvent::InpaintAdded/Removed` is the
    /// reactivity source, not this vec's length.
    pub inpaint: Vec<InpaintLayer>,
}