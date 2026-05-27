use scanlateit_model::Project;

use crate::main_area::decode::PageDecode;

/// One loaded image plus everything the viewer needs that the model doesn't
/// know about (per-image canvas cache).
pub struct LoadedImage {
    pub width: f32,
    pub height: f32,
    pub path: String,
    pub project: Project,
    pub decode: PageDecode,
}