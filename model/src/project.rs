use std::collections::HashMap;

use super::{
    EntryId, EntryStyle, Extras, ImageId, ImageMeta, NewEntry, OcrEntry, OcrResult, ProfileId,
    Profiles, Quad,
};

/// The whole document model for one chapter (session): immutable OCR results
/// for every image, freely editable profiles (chapter-wide), cross-profile
/// entry styles, view bounds, and extras. Images are immutable after being
/// added (from `Start OCR` till close); `reorder` is for manual-OCR order
/// fixing so translation sees reading order.
///
/// Every `OcrEntry` carries `image_id: ImageId` — `EntryId` is globally
/// unique within the chapter `Project` (single `OcrResult.next_id`), and
/// `Profiles` is shared across all images.
#[derive(Debug)]
pub struct Project {
    /// Images in this chapter, insertion order. Immutable after add.
    images: Vec<ImageMeta>,
    next_image_id: u64,
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
            images: Vec::new(),
            next_image_id: 0,
            ocr: OcrResult::new(),
            profiles: Profiles::default(),
            styles: HashMap::new(),
            view_quads: HashMap::new(),
            extras: Extras::default(),
        }
    }

    /// Add an image to the chapter. Returns its stable `ImageId`.
    /// Images are append-only and immutable thereafter (until `Project` is
    /// dropped at close), matching the current UI lifecycle.
    pub fn add_image(&mut self, path: impl Into<String>, width: f32, height: f32) -> ImageId {
        let id = ImageId(self.next_image_id);
        self.next_image_id += 1;
        self.images.push(ImageMeta {
            id,
            path: path.into(),
            width,
            height,
        });
        id
    }

    pub fn images(&self) -> &[ImageMeta] {
        &self.images
    }

    pub fn image(&self, id: ImageId) -> Option<&ImageMeta> {
        self.images.iter().find(|m| m.id == id)
    }

    pub fn image_count(&self) -> usize {
        self.images.len()
    }

    /// Append entries for `image_id`. `EntryId` remains globally unique.
    pub fn append_ocr_for_image(&mut self, image_id: ImageId, entries: Vec<NewEntry>) -> usize {
        self.ocr.append_many_for_image(image_id, entries)
    }

    /// The text the UI should show for an entry: the selected profile's
    /// translation when present, otherwise the raw OCR text.
    pub fn display_text<'a>(&'a self, entry: &'a OcrEntry) -> &'a str {
        self.profiles.selected().translation_of(entry.id).unwrap_or(&entry.text)
    }

    /// The overlay/export style for an entry, identical across all profiles.
    pub fn entry_style(&self, entry_id: EntryId) -> EntryStyle {
        self.styles.get(&entry_id).cloned().unwrap_or_default()
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

    /// Whether the entry has a user-adjusted view quad (vs. falling back to
    /// its OCR quad). The UI shows the revert-transform action only then.
    pub fn has_view_quad(&self, entry_id: EntryId) -> bool {
        self.view_quads.contains_key(&entry_id)
    }

    /// Revert the box's transform (rotation, skew and size) back to the OCR
    /// quad while keeping its current position: the override is rebuilt as
    /// the OCR shape placed at the view quad's TL corner.
    pub fn revert_transform(&mut self, entry_id: EntryId) {
        let Some(entry) = self.ocr.get(entry_id) else {
            return;
        };
        let ocr = entry.quad;
        let view = self.view_quads.get(&entry_id).copied().unwrap_or(ocr);
        let dx = view.points[0][0] - ocr.points[0][0];
        let dy = view.points[0][1] - ocr.points[0][1];
        self.view_quads.insert(entry_id, ocr.translate(dx, dy));
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

    /// Reorder only the entries of `image_id` by position. Uses
    /// `view_quad` fallback, stable sort, touches deleted entries too so both
    /// `ocr.visible_for(image_id)` and `ocr.all_for(image_id)` reflect the new
    /// order — needed for translation which iterates per image.
    pub fn reorder_entries_for_image(&mut self, image_id: ImageId) {
        let view_quads = self.view_quads.clone();
        // Collect entries of this image in current global order, sort that
        // subset, then put back in the same slots to keep images grouped by
        // insertion but sorted within the image.
        // We need positions of this image's entries in the global vec.
        // Since `OcrResult.entries` is private, we operate via `sort_by` with
        // a comparator that only reorders within the image and keeps cross-image
        // order stable by image insertion order.
        let image_order: std::collections::HashMap<ImageId, usize> = self
            .images
            .iter()
            .enumerate()
            .map(|(idx, m)| (m.id, idx))
            .collect();
        self.ocr.sort_by(|a, b| {
            let a_is_target = a.image_id == image_id;
            let b_is_target = b.image_id == image_id;
            match (a_is_target, b_is_target) {
                (true, true) => {
                    let qa = view_quads
                        .get(&a.id)
                        .copied()
                        .unwrap_or(a.quad)
                        .bounds();
                    let qb = view_quads
                        .get(&b.id)
                        .copied()
                        .unwrap_or(b.quad)
                        .bounds();
                    qa[1]
                        .partial_cmp(&qb[1])
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| {
                            qa[0]
                                .partial_cmp(&qb[0])
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                        .then_with(|| a.id.cmp(&b.id))
                }
                (true, false) | (false, true) => {
                    // Keep chapter image order stable; do not intermix.
                    let ai = image_order.get(&a.image_id).copied().unwrap_or(usize::MAX);
                    let bi = image_order.get(&b.image_id).copied().unwrap_or(usize::MAX);
                    ai.cmp(&bi).then_with(|| a.image_id.cmp(&b.image_id)).then_with(|| a.id.cmp(&b.id))
                }
                (false, false) => std::cmp::Ordering::Equal, // keep original
            }
        });
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
        let id = project.ocr.append_for_image(ImageId(0), NewEntry {
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

        project.set_entry_style(EntryId(7), style.clone());
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
        let id = project.ocr.append_for_image(ImageId(0), NewEntry {
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

        let id = project.ocr.append_for_image(ImageId(0), NewEntry {
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
        let id = project.ocr.append_for_image(ImageId(0), NewEntry {
            source: crate::EntrySource::AutoOcr,
            text: "bye".to_string(),
            score: 0.9,
            quad: Quad {
                points: [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            },
        });
        project.set_entry_style(id, style.clone());
        let distorted = Quad {
            points: [[1.0, 2.0], [11.0, 3.0], [10.0, 12.0], [2.0, 11.0]],
        };
        project.set_view_quad(id, distorted);

        assert!(project.delete_entry(id));
        assert_eq!(project.ocr.visible_count_for(ImageId(0)), 0);
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
        let id = project.ocr.append_for_image(ImageId(0), NewEntry {
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

    #[test]
    fn revert_transform_keeps_position_and_restores_ocr_shape() {
        let mut project = Project::new();
        let id = project.ocr.append_for_image(ImageId(0), NewEntry {
            source: crate::EntrySource::AutoOcr,
            text: "hi".to_string(),
            score: 0.9,
            quad: Quad {
                points: [[0.0, 0.0], [100.0, 0.0], [100.0, 30.0], [0.0, 30.0]],
            },
        });
        let ocr = project.ocr.get(id).unwrap().quad;
        let view = ocr.translate(20.0, 10.0).rotate([50.0, 15.0], 0.6);
        project.set_view_quad(id, view);
        assert_ne!(project.view_quad(project.ocr.get(id).unwrap()), ocr);

        project.revert_transform(id);

        let reverted = project.view_quad(project.ocr.get(id).unwrap());
        let reverted_tl = reverted.ordered()[0];
        assert_eq!(reverted_tl, view.ordered()[0], "position must be kept");
        let shifted = reverted.translate(-reverted_tl[0], -reverted_tl[1]);
        for (point, expected) in shifted.points.iter().zip(ocr.points) {
            assert!((point[0] - expected[0]).abs() < 1e-3, "x: {point:?}");
            assert!((point[1] - expected[1]).abs() < 1e-3, "y: {point:?}");
        }
    }

    #[test]
    fn revert_transform_without_override_is_a_noop() {
        let mut project = Project::new();
        let id = project.ocr.append_for_image(ImageId(0), NewEntry {
            source: crate::EntrySource::AutoOcr,
            text: "hi".to_string(),
            score: 0.9,
            quad: Quad {
                points: [[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [0.0, 5.0]],
            },
        });
        project.revert_transform(id);
        let entry = project.ocr.get(id).unwrap();
        assert_eq!(project.view_quad(entry), entry.quad);
    }

    fn quad_at(min_x: f32, min_y: f32) -> Quad {
        Quad {
            points: [
                [min_x, min_y],
                [min_x + 10.0, min_y],
                [min_x + 10.0, min_y + 10.0],
                [min_x, min_y + 10.0],
            ],
        }
    }

    #[test]
    fn reorder_entries_by_position_sorts_by_y_then_x() {
        let mut project = Project::new();
        project.ocr.append_for_image(ImageId(0), NewEntry {
            source: crate::EntrySource::AutoOcr,
            text: "bottom".into(),
            score: 0.9,
            quad: quad_at(0.0, 100.0),
        });
        project.ocr.append_for_image(ImageId(0), NewEntry {
            source: crate::EntrySource::AutoOcr,
            text: "top".into(),
            score: 0.9,
            quad: quad_at(0.0, 10.0),
        });
        project.ocr.append_for_image(ImageId(0), NewEntry {
            source: crate::EntrySource::AutoOcr,
            text: "middle".into(),
            score: 0.9,
            quad: quad_at(0.0, 50.0),
        });
        project.reorder_entries_for_image(ImageId(0));
        let texts: Vec<&str> = project.ocr.visible_for(ImageId(0)).map(|e| e.text.as_str()).collect();
        assert_eq!(texts, vec!["top", "middle", "bottom"]);
    }

    #[test]
    fn reorder_entries_by_position_tie_breaks_by_x_left_to_right() {
        let mut project = Project::new();
        project.ocr.append_for_image(ImageId(0), NewEntry {
            source: crate::EntrySource::AutoOcr,
            text: "right".into(),
            score: 0.9,
            quad: quad_at(100.0, 10.0),
        });
        project.ocr.append_for_image(ImageId(0), NewEntry {
            source: crate::EntrySource::AutoOcr,
            text: "left".into(),
            score: 0.9,
            quad: quad_at(10.0, 10.0),
        });
        project.ocr.append_for_image(ImageId(0), NewEntry {
            source: crate::EntrySource::AutoOcr,
            text: "center".into(),
            score: 0.9,
            quad: quad_at(50.0, 10.0),
        });
        project.reorder_entries_for_image(ImageId(0));
        let texts: Vec<&str> = project.ocr.visible_for(ImageId(0)).map(|e| e.text.as_str()).collect();
        assert_eq!(texts, vec!["left", "center", "right"]);
    }

    #[test]
    fn reorder_entries_by_position_uses_view_quad_when_overridden() {
        let mut project = Project::new();
        let low_id = project.ocr.append_for_image(ImageId(0), NewEntry {
            source: crate::EntrySource::AutoOcr,
            text: "low".into(),
            score: 0.9,
            quad: quad_at(0.0, 100.0),
        });
        let high_id = project.ocr.append_for_image(ImageId(0), NewEntry {
            source: crate::EntrySource::AutoOcr,
            text: "high".into(),
            score: 0.9,
            quad: quad_at(0.0, 10.0),
        });
        // drag "low" to the top via view_quad
        project.set_view_quad(low_id, quad_at(0.0, 5.0));
        // visible before reorder is insertion order [low, high]
        assert_eq!(
            project.ocr.visible_for(ImageId(0)).map(|e| e.text.as_str()).collect::<Vec<_>>(),
            vec!["low", "high"]
        );
        project.reorder_entries_for_image(ImageId(0));
        // after reorder, "low" (now at y=5 via view_quad) should be first
        let texts: Vec<&str> = project.ocr.visible_for(ImageId(0)).map(|e| e.text.as_str()).collect();
        assert_eq!(texts, vec!["low", "high"]);
        // ensure view_quad is still respected
        assert!(project.has_view_quad(low_id));
        assert!(!project.has_view_quad(high_id));
    }

    #[test]
    fn reorder_entries_by_position_keeps_deleted_but_sorts_them() {
        let mut project = Project::new();
        let bottom = project.ocr.append_for_image(ImageId(0), NewEntry {
            source: crate::EntrySource::AutoOcr,
            text: "bottom".into(),
            score: 0.9,
            quad: quad_at(0.0, 100.0),
        });
        let top = project.ocr.append_for_image(ImageId(0), NewEntry {
            source: crate::EntrySource::AutoOcr,
            text: "top".into(),
            score: 0.9,
            quad: quad_at(0.0, 10.0),
        });
        project.delete_entry(top);
        project.reorder_entries_for_image(ImageId(0));
        let all: Vec<&str> = project.ocr.all_for(ImageId(0)).map(|e| e.text.as_str()).collect();
        assert_eq!(all, vec!["top", "bottom"]);
        let visible: Vec<&str> = project.ocr.visible_for(ImageId(0)).map(|e| e.text.as_str()).collect();
        assert_eq!(visible, vec!["bottom"]);
        assert!(project.ocr.get(top).unwrap().deleted);
        assert!(!project.ocr.get(bottom).unwrap().deleted);
    }
}