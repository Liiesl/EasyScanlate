//! Complimentary project data that survives across profiles: translation
//! notes, inpainting patches, custom geometries, etc. — live DB, wired to
//! `Project` and `ModelEvent`.

use std::collections::HashMap;

use super::{EntryId, ImageId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct InpaintId(pub u64);

#[derive(Debug, Default, Clone)]
pub struct Extras {
    /// Free-form notes attached to entries (e.g. translation notes).
    pub notes: HashMap<EntryId, String>,
    /// Regions of the image that have been inpainted. Each patch has a stable
    /// `InpaintId` (first-class, survives reordering).
    /// Live DB: `Project::extras.inpaint_patches` is the single source of
    /// `bounds`+`image_id`; `ui::LoadedImage::inpaint` is a derived GPU-cache
    /// (`Handle`) keyed by `InpaintId` that mirrors this list. Do not treat the
    /// `LoadedImage` vec length as canonical — use `Project::inpaint_for()`.
    pub inpaint_patches: Vec<InpaintPatch>,
    /// User-drawn geometries independent of OCR output (future).
    /// Currently no `ModelEvent` is emitted (no UI yet); add one when wired.
    pub shapes: Vec<Shape>,
}

impl Extras {
    pub fn note(&self, entry_id: EntryId) -> Option<&str> {
        self.notes.get(&entry_id).map(String::as_str)
    }

    /// Set a note; an empty/whitespace note removes it.
    pub fn set_note(&mut self, entry_id: EntryId, note: String) {
        if note.trim().is_empty() {
            self.notes.remove(&entry_id);
        } else {
            self.notes.insert(entry_id, note);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InpaintPatch {
    pub id: InpaintId,
    /// Which image the patch belongs to (chapter-wide `Project`).
    pub image_id: ImageId,
    /// Pixel bounds `[x, y, w, h]` of the patched region.
    pub bounds: [f32; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    Rect,
    Polygon,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub kind: ShapeKind,
    pub points: Vec<[f32; 2]>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_keyed_by_entry_and_removed_when_empty() {
        let mut extras = Extras::default();
        extras.set_note(EntryId(1), "check this".into());
        assert_eq!(extras.note(EntryId(1)), Some("check this"));
        extras.set_note(EntryId(1), "  ".into());
        assert_eq!(extras.note(EntryId(1)), None);
    }
}