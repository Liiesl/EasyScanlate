use std::collections::HashMap;

use super::{EntryId, EntryStyle, Extras, NewEntry, OcrEntry, OcrResult, Profiles};

/// The whole document model for one image: immutable OCR results, freely
/// editable profiles, cross-profile entry styles, and extras.
#[derive(Debug)]
pub struct Project {
    pub ocr: OcrResult,
    pub profiles: Profiles,
    /// Per-OCR-result styles shared by every profile. An entry without an
    /// entry falls back to `EntryStyle::default()`.
    styles: HashMap<EntryId, EntryStyle>,
    /// Reserved for upcoming features (notes, inpainting, geometries).
    #[allow(dead_code)]
    pub extras: Extras,
}

impl Project {
    pub fn new() -> Self {
        Self {
            ocr: OcrResult::new(),
            profiles: Profiles::default(),
            styles: HashMap::new(),
            extras: Extras::default(),
        }
    }

    /// Append one OCR run to the source-of-truth store.
    pub fn append_ocr(&mut self, entries: Vec<NewEntry>) -> usize {
        self.ocr.append_many(entries)
    }

    /// The text the UI should show for an entry: the selected profile's
    /// translation when present, otherwise the raw OCR text.
    pub fn display_text<'a>(&'a self, entry: &'a OcrEntry) -> &'a str {
        self.profiles.selected().translation_of(entry.id).unwrap_or(&entry.text)
    }

    /// The overlay/export style for an entry, identical across all profiles.
    pub fn entry_style(&self, entry_id: EntryId) -> EntryStyle {
        self.styles.get(&entry_id).copied().unwrap_or_default()
    }

    /// Set the overlay/export style for an entry. Setting the default style
    /// is equivalent to clearing the override.
    pub fn set_entry_style(&mut self, entry_id: EntryId, style: EntryStyle) {
        if style == EntryStyle::default() {
            self.styles.remove(&entry_id);
        } else {
            self.styles.insert(entry_id, style);
        }
    }
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Quad;

    #[test]
    fn display_text_prefers_selected_profile_translation() {
        let mut project = Project::new();
        let id = project.ocr.append(NewEntry {
            source: crate::model::EntrySource::AutoOcr,
            text: "안녕".to_string(),
            score: 0.9,
            quad: Quad {
                points: [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            },
        });
        let entry = project.ocr.get(id).unwrap();

        assert_eq!(project.display_text(entry), "안녕");
        project.profiles.selected_mut().set_translation(id, Some("Hello".to_string()));
        assert_eq!(project.display_text(entry), "Hello");
    }

    #[test]
    fn entry_style_is_shared_across_profiles() {
        let mut project = Project::new();
        let style = EntryStyle { font_size: 30.0, ..EntryStyle::default() };

        project.set_entry_style(EntryId(7), style);
        assert_eq!(project.entry_style(EntryId(7)), style);
        assert_eq!(project.entry_style(EntryId(8)), EntryStyle::default());

        let jp = project.profiles.add("JP");
        project.profiles.select(jp);
        assert_eq!(project.entry_style(EntryId(7)), style, "style must survive profile switch");

        project.set_entry_style(EntryId(7), EntryStyle::default());
        assert_eq!(project.entry_style(EntryId(7)), EntryStyle::default());
    }
}