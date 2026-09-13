//! Per-tab progress for style-detection jobs: which entries are already
//! classified (so an auto-run never classifies the same entry twice) plus the
//! pending manual single-entry job requested while the engine was building.
//!
//! The loaded engine handle + build flag are global in
//! `easyscanlate_engine_pool::EnginePool` (like OCR/inpaint/segment), not per-tab.

use std::collections::HashSet;

use easyscanlate_model::EntryId;

/// Pending manual single-entry auto-detect requested while the engine was
/// still building. Stored so `handle_styling_ready` can run the *original*
/// entry (not whatever is selected at ready time).
#[derive(Debug, Clone)]
pub struct PendingSingle {
    pub index: usize,
    pub id: EntryId,
    pub path: String,
    pub quad: easyscanlate_model::Quad,
}

/// Per-tab styling progress (no engine cache — see `EnginePool::styling`).
#[derive(Debug, Default)]
pub struct JobTracker {
    done: HashSet<(usize, EntryId)>,
    pending_single: Option<PendingSingle>,
}

impl JobTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_done(&self, index: usize, id: EntryId) -> bool {
        self.done.contains(&(index, id))
    }

    pub fn mark_done(&mut self, index: usize, id: EntryId) {
        self.done.insert((index, id));
    }

    /// Re-opens `(index, id)` so a manual StyleAutoDetect can rerun it.
    pub fn reopen(&mut self, index: usize, id: EntryId) {
        self.done.remove(&(index, id));
    }

    /// The number of classified entries (for tests).
    pub fn done_count(&self) -> usize {
        self.done.len()
    }

    /// Store a pending single-entry job (overwrites any prior pending).
    pub fn set_pending_single(&mut self, pending: PendingSingle) {
        self.pending_single = Some(pending);
    }

    /// Take the pending single-entry job if any.
    pub fn take_pending_single(&mut self) -> Option<PendingSingle> {
        self.pending_single.take()
    }

    /// Clear any pending single-entry job (e.g. on build failure).
    pub fn clear_pending_single(&mut self) {
        self.pending_single = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn done_reopen_semantics() {
        let mut tracker = JobTracker::new();
        let (index, id) = (3usize, EntryId(7));
        assert!(!tracker.is_done(index, id));
        tracker.mark_done(index, id);
        assert!(tracker.is_done(index, id));
        assert!(!tracker.is_done(index, EntryId(8)), "ids are distinct");
        assert!(!tracker.is_done(4, id), "image indexes are distinct");
        assert_eq!(tracker.done_count(), 1);
        tracker.mark_done(index, id);
        assert_eq!(tracker.done_count(), 1, "marking twice is idempotent");
        tracker.reopen(index, id);
        assert!(!tracker.is_done(index, id));
        assert_eq!(tracker.done_count(), 0);
    }

    #[test]
    fn done_is_a_set_of_pairs() {
        let mut tracker = JobTracker::new();
        tracker.mark_done(0, EntryId(1));
        tracker.mark_done(1, EntryId(1));
        tracker.mark_done(0, EntryId(2));
        assert_eq!(tracker.done_count(), 3);
    }

    #[test]
    fn pending_single_round_trips() {
        let mut tracker = JobTracker::new();
        assert!(tracker.take_pending_single().is_none());
        tracker.set_pending_single(PendingSingle {
            index: 1,
            id: EntryId(2),
            path: "p".to_string(),
            quad: easyscanlate_model::Quad { points: [[0.0,0.0],[1.0,0.0],[1.0,1.0],[0.0,1.0]] },
        });
        let pending = tracker.take_pending_single().unwrap();
        assert_eq!(pending.index, 1);
        assert_eq!(pending.id, EntryId(2));
        assert!(tracker.take_pending_single().is_none(), "taken is consumed");
        tracker.set_pending_single(PendingSingle {
            index: 0,
            id: EntryId(5),
            path: "q".to_string(),
            quad: easyscanlate_model::Quad { points: [[0.0,0.0],[1.0,0.0],[1.0,1.0],[0.0,1.0]] },
        });
        tracker.clear_pending_single();
        assert!(tracker.take_pending_single().is_none());
    }

    // NOTE: `Engine::classify_entry` needs the ONNX model file, so it is
    // smoke-tested manually; its mapping (`to_entry_style`) is covered by the
    // unit tests in lib.rs.
}
