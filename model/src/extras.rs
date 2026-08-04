//! Complimentary project data that survives across profiles: translation
//! notes, inpainting patches, custom geometries, etc.
//!
//! Reserved for upcoming features; nothing here is wired to the UI yet.
#![allow(dead_code)]

use std::collections::HashMap;

use super::{EntryId, ImageId};

#[derive(Debug, Default, Clone)]
pub struct Extras {
    /// Free-form notes attached to entries (e.g. translation notes).
    pub notes: HashMap<EntryId, String>,
    /// Regions of the image that have been inpainted (future).
    pub inpaint_patches: Vec<InpaintPatch>,
    /// User-drawn geometries independent of OCR output (future).
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