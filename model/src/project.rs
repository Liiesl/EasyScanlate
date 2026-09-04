use std::collections::HashMap;

use super::{
    EntryId, EntryStyle, Extras, ImageId, ImageMeta, InpaintId, InpaintPatch, ModelEvent,
    NewEntry, OcrEntry, OcrResult, ProfileId, Profiles, Quad,
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
#[derive(Debug, Clone)]
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
    /// Extras (notes, inpaint patches, shapes) — survives across profiles.
    pub extras: Extras,
    next_inpaint_id: u64,
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
            next_inpaint_id: 0,
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

    /// Monotonic next image id.
    pub fn next_image_id(&self) -> u64 {
        self.next_image_id
    }

    pub fn next_inpaint_id(&self) -> u64 {
        self.next_inpaint_id
    }

    // -----------------------------------------------------------------------
    // Entry-centric queries: filtered by default, escape hatch for deleted
    // -----------------------------------------------------------------------
    /// Visible (non-deleted) entries globally, in storage order (grouped by
    /// image insertion order and Y→X within each image after reorder).
    pub fn visible_entries(&self) -> impl Iterator<Item = &OcrEntry> {
        self.ocr.entries().iter().filter(|e| !e.deleted)
    }

    /// All entries including deleted, in storage order (for persistence/debug).
    pub fn all_entries(&self) -> &[OcrEntry] {
        self.ocr.entries()
    }

    /// Filtered read: returns `None` if the entry is deleted or missing.
    /// This is the primary read path — callers don't manually check `deleted`.
    pub fn entry(&self, id: EntryId) -> Option<&OcrEntry> {
        self.ocr.get(id).filter(|e| !e.deleted)
    }

    /// Escape hatch: returns the entry even if soft-deleted.
    pub fn entry_including_deleted(&self, id: EntryId) -> Option<&OcrEntry> {
        self.ocr.get(id)
    }

    /// Visible entries for `image_id` (deleted hidden). Primary per-image query.
    pub fn visible_for(&self, image_id: ImageId) -> impl Iterator<Item = &OcrEntry> {
        self.ocr.visible_for(image_id)
    }

    /// All entries for `image_id` including deleted (escape hatch for dedup,
    /// inpaint intersection and future "show deleted" feature).
    pub fn all_for(&self, image_id: ImageId) -> impl Iterator<Item = &OcrEntry> {
        self.ocr.all_for(image_id)
    }

    pub fn visible_count_for(&self, image_id: ImageId) -> usize {
        self.ocr.visible_count_for(image_id)
    }

    pub fn total_count_for(&self, image_id: ImageId) -> usize {
        self.ocr.total_count_for(image_id)
    }

    /// Profile-resolved text for the entry (falls back to OCR text).
    pub fn display_text_for(&self, id: EntryId) -> Option<&str> {
        self.ocr.get(id).map(|e| self.display_text(e))
    }

    /// Text for `entry_id` as seen through `profile_id` (falls back to OCR text).
    /// Centralized profile resolution — callers in `app`/`ui` should use this
    /// instead of iterating `profiles` manually.
    pub fn resolved_text_for(&self, profile_id: ProfileId, entry_id: EntryId) -> Option<&str> {
        let entry = self.ocr.get(entry_id)?;
        if let Some(p) = self.profiles.iter().find(|p| p.id == profile_id) {
            p.translation_of(entry_id).or(Some(entry.text.as_str()))
        } else {
            Some(entry.text.as_str())
        }
    }

    /// Like `display_text_for` but for an explicit profile (not selected).
    pub fn display_text_for_profile(&self, profile_id: ProfileId, entry_id: EntryId) -> Option<String> {
        self.resolved_text_for(profile_id, entry_id).map(|s| s.to_string())
    }

    /// Inpaint patches for `image_id`.
    pub fn inpaint_for(&self, image_id: ImageId) -> impl Iterator<Item = &InpaintPatch> {
        self.extras.inpaint_patches.iter().filter(move |p| p.image_id == image_id)
    }

    pub fn inpaint_patches(&self) -> &[InpaintPatch] {
        &self.extras.inpaint_patches
    }

    /// All per-entry style overrides (for persistence).
    pub fn styles(&self) -> &HashMap<EntryId, EntryStyle> {
        &self.styles
    }

    /// All view-quad overrides (for persistence).
    pub fn view_quads(&self) -> &HashMap<EntryId, Quad> {
        &self.view_quads
    }

    /// Reconstruct from raw parts (for persistence).
    pub fn from_raw(
        images: Vec<ImageMeta>,
        next_image_id: u64,
        ocr: OcrResult,
        profiles: Profiles,
        styles: HashMap<EntryId, EntryStyle>,
        view_quads: HashMap<EntryId, Quad>,
        extras: Extras,
    ) -> Self {
        let next_inpaint_id = extras
            .inpaint_patches
            .iter()
            .map(|p| p.id.0 + 1)
            .max()
            .unwrap_or(0);
        Self { images, next_image_id, ocr, profiles, styles, view_quads, extras, next_inpaint_id }
    }

    /// Append entries for `image_id`. `EntryId` remains globally unique.
    pub fn append_ocr_for_image(&mut self, image_id: ImageId, entries: Vec<NewEntry>) -> usize {
        self.ocr.append_many_for_image(image_id, entries)
    }

    pub fn append_ocr_for_image_with_event(&mut self, image_id: ImageId, entries: Vec<NewEntry>) -> Option<ModelEvent> {
        if entries.is_empty() {
            return None;
        }
        let start = self.ocr.next_id();
        let count = self.ocr.append_many_for_image(image_id, entries);
        if count == 0 {
            return None;
        }
        let ids: Vec<EntryId> = (start..start + count as u64).map(EntryId).collect();
        Some(ModelEvent::EntriesAdded { image_id, ids })
    }

    pub fn add_image_with_event(&mut self, path: impl Into<String>, width: f32, height: f32) -> (ImageId, ModelEvent) {
        let id = self.add_image(path, width, height);
        (id, ModelEvent::ImageAdded { image_id: id })
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

    pub fn set_entry_style_with_event(&mut self, entry_id: EntryId, style: EntryStyle) -> ModelEvent {
        self.set_entry_style(entry_id, style);
        ModelEvent::EntryStyleUpdated { id: entry_id }
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

    pub fn set_view_quad_with_event(&mut self, entry_id: EntryId, quad: Quad) -> ModelEvent {
        self.set_view_quad(entry_id, quad);
        ModelEvent::EntryMoved { id: entry_id, quad }
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

    pub fn revert_transform_with_event(&mut self, entry_id: EntryId) -> Option<ModelEvent> {
        let ocr_quad = self.ocr.get(entry_id)?.quad;
        if !self.has_view_quad(entry_id) {
            return None;
        }
        self.revert_transform(entry_id);
        let quad = self.view_quads.get(&entry_id).copied().unwrap_or(ocr_quad);
        Some(ModelEvent::EntryMoved { id: entry_id, quad })
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

    pub fn delete_entry_with_event(&mut self, entry_id: EntryId) -> Option<ModelEvent> {
        if self.ocr.soft_delete(entry_id) {
            Some(ModelEvent::EntryDeleted { id: entry_id })
        } else {
            None
        }
    }

    pub fn restore_entry_with_event(&mut self, entry_id: EntryId) -> Option<ModelEvent> {
        if self.ocr.restore(entry_id) {
            Some(ModelEvent::EntryRestored { id: entry_id })
        } else {
            None
        }
    }

    /// Reorder all images chapter-wide by position (per-image Y→X).
    pub fn reorder_entries_by_position(&mut self) {
        let ids: Vec<ImageId> = self.images.iter().map(|m| m.id).collect();
        if ids.is_empty() {
            // Legacy fallback: no images registered but entries have ImageId(0)
            self.reorder_entries_for_image(ImageId(0));
            return;
        }
        for id in ids {
            self.reorder_entries_for_image(id);
        }
    }

    pub fn reorder_entries_for_image_with_event(&mut self, image_id: ImageId) -> ModelEvent {
        self.reorder_entries_for_image(image_id);
        ModelEvent::EntriesReordered { image_id }
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

    pub fn store_translation_with_event(
        &mut self,
        profile_name: &str,
        entry_id: EntryId,
        text: Option<String>,
    ) -> (ProfileId, Vec<ModelEvent>) {
        let mut events = Vec::new();
        let existed = self.profiles.find_by_name(profile_name);
        let id = existed.unwrap_or_else(|| {
            let new_id = self.profiles.add(profile_name);
            events.push(ModelEvent::ProfileCreated { id: new_id, name: profile_name.to_string() });
            new_id
        });
        if self.profiles.selected_id() != id {
            self.profiles.select(id);
            events.push(ModelEvent::ProfileSelected { id });
        }
        self.profiles.selected_mut().set_translation(entry_id, text);
        events.push(ModelEvent::EntryTextUpdated { id: entry_id, profile: id });
        (id, events)
    }

    // -----------------------------------------------------------------------
    // Profile helpers with events (wrappers around `self.profiles`)
    // -----------------------------------------------------------------------
    pub fn create_profile_with_event(&mut self, name: impl Into<String>) -> (ProfileId, ModelEvent) {
        let name = name.into();
        let id = self.profiles.add(name.clone());
        (id, ModelEvent::ProfileCreated { id, name })
    }

    pub fn select_profile_with_event(&mut self, id: ProfileId) -> Option<ModelEvent> {
        if self.profiles.select(id) {
            Some(ModelEvent::ProfileSelected { id })
        } else {
            None
        }
    }

    pub fn set_translation_with_event(&mut self, entry_id: EntryId, text: Option<String>) -> ModelEvent {
        let pid = self.profiles.selected_id();
        self.profiles.selected_mut().set_translation(entry_id, text);
        ModelEvent::EntryTextUpdated { id: entry_id, profile: pid }
    }

    /// Remove a profile (cannot be selected or last remaining). Emits `ProfileRemoved`.
    pub fn remove_profile_with_event(&mut self, id: ProfileId) -> Option<ModelEvent> {
        if self.profiles.remove(id) {
            Some(ModelEvent::ProfileRemoved { id })
        } else {
            None
        }
    }

    /// Rename a profile. Emits `ProfileRenamed`.
    pub fn rename_profile_with_event(&mut self, id: ProfileId, new_name: impl Into<String>) -> Option<ModelEvent> {
        let new_name = new_name.into();
        if self.profiles.rename(id, new_name.clone()) {
            Some(ModelEvent::ProfileRenamed { id, name: new_name })
        } else {
            None
        }
    }

    /// Fork a fresh profile off the original ("Default") when it is selected.
    /// Returns `(name, events)` where events are `ProfileCreated` + `ProfileSelected`.
    /// Prefer this over `profiles.fork_for_edit()` so callers get granular events.
    pub fn fork_for_edit_with_event(&mut self) -> Option<(String, Vec<ModelEvent>)> {
        if self.profiles.selected_id() != self.profiles.original_id() {
            return None;
        }
        let name = self.profiles.next_available_name();
        let id = self.profiles.add(name.clone());
        let ok = self.profiles.select(id);
        debug_assert!(ok);
        Some((
            name.clone(),
            vec![
                ModelEvent::ProfileCreated { id, name: name.clone() },
                ModelEvent::ProfileSelected { id },
            ],
        ))
    }

    // -----------------------------------------------------------------------
    // Inpaint patches — first-class with stable InpaintId
    // -----------------------------------------------------------------------
    pub fn add_inpaint_patch(&mut self, image_id: ImageId, bounds: [f32; 4]) -> ModelEvent {
        // Legacy path: no quad, derive quad None. Prefer add_inpaint_patch_with_quad.
        let id = InpaintId(self.next_inpaint_id);
        self.next_inpaint_id += 1;
        self.extras
            .inpaint_patches
            .push(InpaintPatch { id, image_id, bounds, quad: None });
        ModelEvent::InpaintAdded { id, image_id, bounds, quad: None }
    }

    /// Add a patch with its actual quad (rotated/skewed). `bounds` must be
    /// `quad.bounds()` as `[x,y,w,h]`; helper computes it if you pass only quad.
    pub fn add_inpaint_patch_with_quad(&mut self, image_id: ImageId, quad: Quad) -> ModelEvent {
        let [min_x, min_y, max_x, max_y] = quad.bounds();
        let bounds = [min_x, min_y, max_x - min_x, max_y - min_y];
        let id = InpaintId(self.next_inpaint_id);
        self.next_inpaint_id += 1;
        self.extras.inpaint_patches.push(InpaintPatch {
            id,
            image_id,
            bounds,
            quad: Some(quad),
        });
        ModelEvent::InpaintAdded { id, image_id, bounds, quad: Some(quad) }
    }

    /// Add a patch where both bounds and quad are known (e.g. from engine that
    /// already computed a clipped quad). If `quad` is `None`, falls back to
    /// legacy behavior.
    pub fn add_inpaint_patch_with_bounds_and_quad(
        &mut self,
        image_id: ImageId,
        bounds: [f32; 4],
        quad: Option<Quad>,
    ) -> ModelEvent {
        let id = InpaintId(self.next_inpaint_id);
        self.next_inpaint_id += 1;
        self.extras.inpaint_patches.push(InpaintPatch { id, image_id, bounds, quad });
        ModelEvent::InpaintAdded { id, image_id, bounds, quad }
    }

    pub fn remove_inpaint_patch(&mut self, id: InpaintId) -> Option<ModelEvent> {
        let pos = self.extras.inpaint_patches.iter().position(|p| p.id == id)?;
        self.extras.inpaint_patches.remove(pos);
        Some(ModelEvent::InpaintRemoved { id })
    }

    /// Remove by image-relative index (legacy helper for UI that tracks per-image index).
    /// Prefer `remove_inpaint_patch(InpaintId)` with a stable id. This index
    /// helper exists only because `ui::LoadedImage::inpaint` is a per-image `Vec`
    /// cache; the live DB order is `extras.inpaint_patches`.
    #[deprecated(note = "use remove_inpaint_patch(InpaintId) — stable id, single source in model")]
    pub fn remove_inpaint_patch_by_image_index(&mut self, image_id: ImageId, per_image_idx: usize) -> Option<ModelEvent> {
        let mut count = 0;
        let pos = self.extras.inpaint_patches.iter().position(|p| {
            if p.image_id == image_id {
                let cur = count;
                count += 1;
                cur == per_image_idx
            } else {
                false
            }
        })?;
        let id = self.extras.inpaint_patches[pos].id;
        self.extras.inpaint_patches.remove(pos);
        Some(ModelEvent::InpaintRemoved { id })
    }

    pub fn set_note_with_event(&mut self, entry_id: EntryId, note: String) -> ModelEvent {
        self.extras.set_note(entry_id, note);
        ModelEvent::NoteUpdated { entry: entry_id }
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