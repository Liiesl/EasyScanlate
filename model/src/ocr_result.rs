//! Append-only store for all text detected in the image: the single source of
//! truth of the document model.
//!
//! Some methods are reserved for upcoming features (delete UI, manual OCR,
//! diagnostics) and are not yet reachable from the UI.
#![allow(dead_code)]

use super::{EntryId, NewEntry, OcrEntry};

#[derive(Debug, Default)]
pub struct OcrResult {
    entries: Vec<OcrEntry>,
    next_id: u64,
}

impl OcrResult {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one entry, returning its freshly assigned id.
    pub fn append(&mut self, new: NewEntry) -> EntryId {
        let id = EntryId(self.next_id);
        self.next_id += 1;
        self.entries.push(OcrEntry {
            id,
            source: new.source,
            text: new.text,
            score: new.score,
            quad: new.quad,
            deleted: false,
        });
        id
    }

    /// Append many entries (one OCR run). Returns the number appended.
    pub fn append_many(&mut self, new: Vec<NewEntry>) -> usize {
        let count = new.len();
        for entry in new {
            self.append(entry);
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

    /// Non-deleted entries in insertion order.
    pub fn visible(&self) -> impl Iterator<Item = &OcrEntry> {
        self.entries.iter().filter(|e| !e.deleted)
    }

    /// Every entry in insertion order, including soft-deleted ones. Used by
    /// inpainting so text removed from the view still contributes to the
    /// cleanup mask.
    pub fn all(&self) -> impl Iterator<Item = &OcrEntry> {
        self.entries.iter()
    }

    pub fn visible_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.deleted).count()
    }

    pub fn total_count(&self) -> usize {
        self.entries.len()
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
        let a = store.append(new_entry("a"));
        let b = store.append(new_entry("b"));
        assert_ne!(a, b);
        assert_eq!(store.visible_count(), 2);
        assert_eq!(store.total_count(), 2);
    }

    #[test]
    fn soft_delete_hides_but_keeps_entry() {
        let mut store = OcrResult::new();
        let id = store.append(new_entry("a"));
        assert!(store.soft_delete(id));
        assert_eq!(store.visible_count(), 0);
        assert_eq!(store.total_count(), 1);
        assert!(store.get(id).unwrap().deleted);
        assert!(!store.soft_delete(EntryId(999)));
    }

    #[test]
    fn ids_are_never_reused() {
        let mut store = OcrResult::new();
        let deleted = store.append(new_entry("a"));
        store.soft_delete(deleted);
        let next = store.append(new_entry("b"));
        assert_ne!(deleted, next);
        assert!(store.get(next).is_some());
    }

    #[test]
    fn append_many_counts_and_preserves_order() {
        let mut store = OcrResult::new();
        let n = store.append_many(vec![new_entry("a"), new_entry("b"), new_entry("c")]);
        assert_eq!(n, 3);
        let texts: Vec<&str> = store.visible().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, vec!["a", "b", "c"]);
    }
}