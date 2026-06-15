use std::collections::HashMap;

use super::{EntryId, EntryStyle, Extras, NewEntry, OcrEntry, OcrResult, ProfileId, Profiles, Quad};

/// The whole document model for one image: immutable OCR results, freely
/// editable profiles, cross-profile entry styles, view bounds, and extras.
#[derive(Debug)]
pub struct Project {
    pub ocr: OcrResult,
    pub profiles: Profiles,
    /// Per-OCR-result styles shared by every profile. An entry without an
    /// entry falls back to `EntryStyle::default()`.
    styles: HashMap<EntryId, EntryStyle>,
    /// Where each entry's overlay box is shown in the view, as a freely
    /// editable quadrilateral in image pixels. Unlike the immutable OCR
    /// `quad`, this supports free transform (move, resize, corner distort);
    /// an entry without an override falls back to its OCR quad.
    view_quads: HashMap<EntryId, Quad>,
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
            view_quads: HashMap::new(),
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

    /// The entry's overlay box in the view: the user-adjusted view quad when
    /// present, otherwise the OCR quad itself (which may be rotated or
    /// skewed).
    pub fn view_quad<'a>(&'a self, entry: &'a OcrEntry) -> Quad {
        self.view_quads.get(&entry.id).copied().unwrap_or(entry.quad)
    }

    /// Set the overlay box shown in the view for an entry, in image pixels.
    /// The OCR quad stays untouched.
    pub fn set_view_quad(&mut self, entry_id: EntryId, quad: Quad) {
        self.view_quads.insert(entry_id, quad);
    }

    /// Drop the view-quad override, falling back to the OCR quad.
    #[allow(dead_code)]
    pub fn reset_view_quad(&mut self, entry_id: EntryId) {
        self.view_quads.remove(&entry_id);
    }

    /// Soft-delete an entry. Its style and view-bounds overrides are kept in
    /// place, exactly like the entry itself: everything is hidden, nothing
    /// is dropped, so a future restore keeps every adjustment.
    pub fn delete_entry(&mut self, entry_id: EntryId) -> bool {
        self.ocr.soft_delete(entry_id)
    }

    /// Ensures a profile named `profile_name` exists, selects it and sets the
    /// entry's translated text in it. Returns the profile id.
    pub fn store_translation(
        &mut self,
        profile_name: &str,
        entry_id: EntryId,
        text: Option<String>,
    ) -> ProfileId {
        let id = self
            .profiles
            .find_by_name(profile_name)
            .unwrap_or_else(|| self.profiles.add(profile_name));
        self.profiles.select(id);
        self.profiles.selected_mut().set_translation(entry_id, text);
        id
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
    use crate::Quad;

    #[test]
    fn display_text_prefers_selected_profile_translation() {
        let mut project = Project::new();
        let id = project.ocr.append(NewEntry {
            source: crate::EntrySource::AutoOcr,
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

    #[test]
    fn view_quad_falls_back_to_quad_then_override_then_reset() {
        let mut project = Project::new();
        let id = project.ocr.append(NewEntry {
            source: crate::EntrySource::AutoOcr,
            text: "hi".to_string(),
            score: 0.9,
            quad: Quad {
                points: [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            },
        });
        let entry = project.ocr.get(id).unwrap();

        assert_eq!(project.view_quad(entry), entry.quad);

        let distorted = Quad {
            points: [[0.0, 0.0], [12.0, 1.0], [10.0, 10.0], [2.0, 9.0]],
        };
        project.set_view_quad(id, distorted);
        let entry = project.ocr.get(id).unwrap();
        assert_eq!(project.view_quad(entry), distorted);
        assert_eq!(
            entry.quad,
            Quad {
                points: [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            },
            "view quad must never touch the OCR quad"
        );

        project.reset_view_quad(id);
        let entry = project.ocr.get(id).unwrap();
        assert_eq!(project.view_quad(entry), entry.quad);
    }

    #[test]
    fn view_quads_are_shared_across_profiles() {
        let mut project = Project::new();
        let jp = project.profiles.add("JP");

        let id = project.ocr.append(NewEntry {
            source: crate::EntrySource::AutoOcr,
            text: "hi".to_string(),
            score: 0.9,
            quad: Quad {
                points: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            },
        });
        let distorted = Quad {
            points: [[5.0, 5.0], [25.0, 6.0], [24.0, 20.0], [6.0, 19.0]],
        };
        project.set_view_quad(id, distorted);

        let entry = project.ocr.get(id).unwrap();
        assert_eq!(project.view_quad(entry), distorted);

        project.profiles.select(jp);
        let entry = project.ocr.get(id).unwrap();
        assert_eq!(
            project.view_quad(entry),
            distorted,
            "geometry must survive profile switch"
        );
    }

    #[test]
    fn delete_entry_hides_it_but_keeps_its_overrides() {
        let mut project = Project::new();
        let style = EntryStyle { bold: true, ..EntryStyle::default() };
        let id = project.ocr.append(NewEntry {
            source: crate::EntrySource::AutoOcr,
            text: "bye".to_string(),
            score: 0.9,
            quad: Quad {
                points: [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            },
        });
        project.set_entry_style(id, style);
        let distorted = Quad {
            points: [[1.0, 2.0], [11.0, 3.0], [10.0, 12.0], [2.0, 11.0]],
        };
        project.set_view_quad(id, distorted);

        assert!(project.delete_entry(id));
        assert_eq!(project.ocr.visible_count(), 0);
        assert!(project.ocr.get(id).unwrap().deleted);
        assert_eq!(
            project.entry_style(id),
            style,
            "style override must survive delete"
        );
        assert_eq!(
            project.view_quad(project.ocr.get(id).unwrap()),
            distorted,
            "view quad override must survive delete"
        );
        assert!(!project.delete_entry(EntryId(999)));
    }

    #[test]
    fn store_translation_creates_and_selects_a_profile_by_name() {
        let mut project = Project::new();
        let id = project.ocr.append(NewEntry {
            source: crate::EntrySource::AutoOcr,
            text: "raw".to_string(),
            score: 0.9,
            quad: Quad {
                points: [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            },
        });

        let profile_id = project.store_translation("english(auto)", id, Some("Hello".into()));

        assert_eq!(project.profiles.len(), 2, "one new profile");
        assert_eq!(project.profiles.find_by_name("english(auto)"), Some(profile_id));
        assert_eq!(project.profiles.selected_id(), profile_id, "new profile selected");
        assert_eq!(project.profiles.selected().translation_of(id), Some("Hello"));
        assert_eq!(project.ocr.get(id).unwrap().text, "raw", "OCR text untouched");
    }

    #[test]
    fn store_translation_reuses_an_existing_profile() {
        let mut project = Project::new();
        let first = project.store_translation("english(auto)", EntryId(1), Some("Hello".into()));
        let again = project.store_translation("english(auto)", EntryId(2), Some("Hi".into()));

        assert_eq!(again, first, "existing profile reused");
        assert_eq!(project.profiles.len(), 2, "no duplicate profile");
        assert_eq!(project.profiles.selected().translation_of(EntryId(1)), Some("Hello"));
        assert_eq!(project.profiles.selected().translation_of(EntryId(2)), Some("Hi"));
    }
}