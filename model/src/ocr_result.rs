//! Append-only store for all text detected in the image: the single source of
//! truth of the document model.
//!
//! Some methods are reserved for upcoming features (delete UI, manual OCR,
//! diagnostics) and are not yet reachable from the UI.
#![allow(dead_code)]

use super::{EntryId, ImageId, NewEntry, OcrEntry};

#[derive(Debug, Default)]
pub struct OcrResult {
    entries: Vec<OcrEntry>,
    next_id: u64,
}

impl OcrResult {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one entry for `image_id`, returning its freshly assigned id.
    /// Global `EntryId` is unique across the whole chapter `Project`.
    pub fn append_for_image(&mut self, image_id: ImageId, new: NewEntry) -> EntryId {
        let id = EntryId(self.next_id);
        self.next_id += 1;
        self.entries.push(OcrEntry {
            id,
            image_id,
            source: new.source,
            text: new.text,
            score: new.score,
            quad: new.quad,
            deleted: false,
        });
        id
    }

    /// Append many entries for `image_id`.
    pub fn append_many_for_image(&mut self, image_id: ImageId, new: Vec<NewEntry>) -> usize {
        let count = new.len();
        for entry in new {
            self.append_for_image(image_id, entry);
        }
        count
    }

    /// Mark an entry as deleted. The entry stays in the store and keeps its id.
    pub fn soft_delete(&mut self, id: EntryId) -> bool {
        match self.entries.iter_mut().find(|e| e.id == id) {
            Some(entry) => {
                entry.deleted = true;
                true
            }
            None => false,
        }
    }

    pub fn get(&self, id: EntryId) -> Option<&OcrEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Non-deleted entries for `image_id` in insertion order.
    pub fn visible_for(&self, image_id: ImageId) -> impl Iterator<Item = &OcrEntry> {
        self.entries.iter().filter(move |e| !e.deleted && e.image_id == image_id)
    }

    /// Every entry for `image_id` in insertion order, including soft-deleted.
    pub fn all_for(&self, image_id: ImageId) -> impl Iterator<Item = &OcrEntry> {
        self.entries.iter().filter(move |e| e.image_id == image_id)
    }

    pub fn visible_count_for(&self, image_id: ImageId) -> usize {
        self.entries
            .iter()
            .filter(|e| !e.deleted && e.image_id == image_id)
            .count()
    }

    pub fn total_count_for(&self, image_id: ImageId) -> usize {
        self.entries.iter().filter(|e| e.image_id == image_id).count()
    }

    /// Reorder entries in place by a custom comparator. Stable sort; not
    /// used directly by the UI — `Project::reorder_entries_for_image`
    /// provides the view-quad-aware ordering.
    pub fn sort_by<F>(&mut self, compare: F)
    where
        F: FnMut(&OcrEntry, &OcrEntry) -> std::cmp::Ordering,
    {
        self.entries.sort_by(compare);
    }

    /// Reorder only entries of `image_id` by position, keeping other images
    /// untouched. Stable sort; touches every entry of that image including
    /// soft-deleted ones, so `all_for()` and `visible_for()` both reflect the
    /// new order — important for translation which iterates `visible_for()` per image.
    pub fn reorder_by_position_for_image(&mut self, image_id: ImageId) {
        // Indices of entries belonging to this image, in current order.
        let indices: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.image_id == image_id)
            .map(|(i, _)| i)
            .collect();
        if indices.len() <= 1 {
            return;
        }
        let mut subset: Vec<OcrEntry> = indices.iter().map(|&i| self.entries[i].clone()).collect();
        subset.sort_by(|a, b| {
            let ba = a.quad.bounds();
            let bb = b.quad.bounds();
            ba[1]
                .partial_cmp(&bb[1])
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    ba[0]
                        .partial_cmp(&bb[0])
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| a.id.cmp(&b.id))
        });
        for (pos, entry) in indices.into_iter().zip(subset) {
            self.entries[pos] = entry;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Quad;

    fn new_entry(text: &str) -> NewEntry {
        NewEntry {
            source: crate::EntrySource::AutoOcr,
            text: text.to_string(),
            score: 0.9,
            quad: Quad {
                points: [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            },
        }
    }

    #[test]
    fn append_assigns_unique_ids() {
        let mut store = OcrResult::new();
        let a = store.append_for_image(ImageId(0), new_entry("a"));
        let b = store.append_for_image(ImageId(0), new_entry("b"));
        assert_ne!(a, b);
        assert_eq!(store.visible_count_for(ImageId(0)), 2);
        assert_eq!(store.total_count_for(ImageId(0)), 2);
    }

    #[test]
    fn soft_delete_hides_but_keeps_entry() {
        let mut store = OcrResult::new();
        let id = store.append_for_image(ImageId(0), new_entry("a"));
        assert!(store.soft_delete(id));
        assert_eq!(store.visible_count_for(ImageId(0)), 0);
        assert_eq!(store.total_count_for(ImageId(0)), 1);
        assert!(store.get(id).unwrap().deleted);
        assert!(!store.soft_delete(EntryId(999)));
    }

    #[test]
    fn ids_are_never_reused() {
        let mut store = OcrResult::new();
        let deleted = store.append_for_image(ImageId(0), new_entry("a"));
        store.soft_delete(deleted);
        let next = store.append_for_image(ImageId(0), new_entry("b"));
        assert_ne!(deleted, next);
        assert!(store.get(next).is_some());
    }

    #[test]
    fn append_many_counts_and_preserves_order() {
        let mut store = OcrResult::new();
        let n = store.append_many_for_image(ImageId(0), vec![new_entry("a"), new_entry("b"), new_entry("c")]);
        assert_eq!(n, 3);
        let texts: Vec<&str> = store.visible_for(ImageId(0)).map(|e| e.text.as_str()).collect();
        assert_eq!(texts, vec!["a", "b", "c"]);
    }

    fn entry_with_quad(text: &str, min_x: f32, min_y: f32) -> NewEntry {
        NewEntry {
            source: crate::EntrySource::AutoOcr,
            text: text.to_string(),
            score: 0.9,
            quad: Quad {
                points: [
                    [min_x, min_y],
                    [min_x + 10.0, min_y],
                    [min_x + 10.0, min_y + 10.0],
                    [min_x, min_y + 10.0],
                ],
            },
        }
    }

    #[test]
    fn reorder_by_position_orders_top_first() {
        let mut store = OcrResult::new();
        store.append_for_image(ImageId(0), entry_with_quad("bottom", 0.0, 100.0));
        store.append_for_image(ImageId(0), entry_with_quad("top", 0.0, 10.0));
        store.append_for_image(ImageId(0), entry_with_quad("middle", 0.0, 50.0));
        store.reorder_by_position_for_image(ImageId(0));
        let texts: Vec<&str> = store.visible_for(ImageId(0)).map(|e| e.text.as_str()).collect();
        assert_eq!(texts, vec!["top", "middle", "bottom"]);
    }

    #[test]
    fn reorder_by_position_tie_breaks_by_x_left_to_right() {
        let mut store = OcrResult::new();
        store.append_for_image(ImageId(0), entry_with_quad("right", 100.0, 10.0));
        store.append_for_image(ImageId(0), entry_with_quad("left", 10.0, 10.0));
        store.append_for_image(ImageId(0), entry_with_quad("center", 50.0, 10.0));
        store.reorder_by_position_for_image(ImageId(0));
        let texts: Vec<&str> = store.visible_for(ImageId(0)).map(|e| e.text.as_str()).collect();
        assert_eq!(texts, vec!["left", "center", "right"]);
    }

    #[test]
    fn reorder_by_position_is_stable_on_equal_coords() {
        let mut store = OcrResult::new();
        store.append_for_image(ImageId(0), entry_with_quad("a", 10.0, 10.0));
        store.append_for_image(ImageId(0), entry_with_quad("b", 10.0, 10.0));
        store.append_for_image(ImageId(0), entry_with_quad("c", 10.0, 10.0));
        store.reorder_by_position_for_image(ImageId(0));
        let texts: Vec<&str> = store.visible_for(ImageId(0)).map(|e| e.text.as_str()).collect();
        // stable + id tie-breaker keeps insertion order
        assert_eq!(texts, vec!["a", "b", "c"]);
    }

    #[test]
    fn reorder_by_position_sorts_all_including_deleted() {
        let mut store = OcrResult::new();
        let bottom = store.append_for_image(ImageId(0), entry_with_quad("bottom", 0.0, 100.0));
        let top = store.append_for_image(ImageId(0), entry_with_quad("top", 0.0, 10.0));
        store.soft_delete(top);
        store.reorder_by_position_for_image(ImageId(0));
        // all_for() includes deleted, now sorted top first even though deleted
        let texts: Vec<&str> = store.all_for(ImageId(0)).map(|e| e.text.as_str()).collect();
        assert_eq!(texts, vec!["top", "bottom"]);
        assert!(store.get(top).unwrap().deleted);
        assert!(store.get(bottom).unwrap().deleted == false);
        // visible skips deleted
        let visible: Vec<&str> = store.visible_for(ImageId(0)).map(|e| e.text.as_str()).collect();
        assert_eq!(visible, vec!["bottom"]);
    }

    #[test]
    fn reorder_by_position_handles_empty_and_single() {
        let mut empty = OcrResult::new();
        empty.reorder_by_position_for_image(ImageId(0));
        assert_eq!(empty.visible_count_for(ImageId(0)), 0);
        let mut single = OcrResult::new();
        single.append_for_image(ImageId(0), entry_with_quad("only", 5.0, 5.0));
        single.reorder_by_position_for_image(ImageId(0));
        assert_eq!(single.visible_for(ImageId(0)).map(|e| e.text.as_str()).collect::<Vec<_>>(), vec!["only"]);
    }
}