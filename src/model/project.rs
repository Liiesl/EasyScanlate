use super::{EntryId, EntryStyle, Extras, NewEntry, OcrEntry, OcrResult, Profiles};

/// The whole document model for one image: immutable OCR results, freely
/// editable profiles, and cross-profile extras.
#[derive(Debug)]
pub struct Project {
    pub ocr: OcrResult,
    pub profiles: Profiles,
    /// Reserved for upcoming features (notes, inpainting, geometries).
    #[allow(dead_code)]
    pub extras: Extras,
}

impl Project {
    pub fn new() -> Self {
        Self {
            ocr: OcrResult::new(),
            profiles: Profiles::default(),
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

    /// The overlay/export style for an entry under the selected profile.
    pub fn entry_style(&self, entry_id: EntryId) -> EntryStyle {
        self.profiles.selected().style_of(entry_id)
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
    fn entry_style_comes_from_selected_profile() {
        let mut project = Project::new();
        let style = EntryStyle { font_size: 30.0, ..EntryStyle::default() };
        project.profiles.selected_mut().set_style(EntryId(7), style);
        assert_eq!(project.entry_style(EntryId(7)), style);
        assert_eq!(project.entry_style(EntryId(8)), EntryStyle::default());
    }
}