//! Bookkeeping for auto style-detection jobs: the shared engine (built
//! lazily), the in-flight build flag, and the set of entries already
//! classified so an auto-run never classifies the same entry twice.

use std::collections::HashSet;

use scanlateit_model::EntryId;

use crate::Engine;

/// Pending manual single-entry auto-detect requested while the engine was
/// still building. Stored so `handle_styling_ready` can run the *original*
/// entry (not whatever is selected at ready time).
#[derive(Debug, Clone)]
pub struct PendingSingle {
    pub index: usize,
    pub id: EntryId,
    pub path: String,
    pub quad: scanlateit_model::Quad,
}

/// The auto-detect job state the app owns: the lazily-built engine, whether
/// an engine build task is in flight, and the `(image index, entry id)` pairs
/// already classified. Generic over the engine so the bookkeeping rules are
/// unit-testable without the ONNX model; the app uses the [`Engine`] default.
#[derive(Debug)]
pub struct JobTracker<E = Engine> {
    engine: Option<E>,
    building: bool,
    done: HashSet<(usize, EntryId)>,
    pending_single: Option<PendingSingle>,
}

impl<E> Default for JobTracker<E> {
    fn default() -> Self {
        Self {
            engine: None,
            building: false,
            done: HashSet::new(),
            pending_single: None,
        }
    }
}

impl<E> JobTracker<E> {
    pub fn new() -> Self {
        Self::default()
    }

    /// The engine, once it finished loading.
    pub fn engine(&self) -> Option<&E> {
        self.engine.as_ref()
    }

    /// True when an engine build task is in flight.
    pub fn is_building(&self) -> bool {
        self.building
    }

    /// Records that a build task was started.
    pub fn mark_building(&mut self) {
        self.building = true;
    }

    /// Stores the loaded engine. Returns true when a build was pending (the
    /// app starts the queued jobs).
    pub fn set_engine(&mut self, engine: E) -> bool {
        self.engine = Some(engine);
        let pending = self.building;
        self.building = false;
        pending
    }

    /// Clears the building flag after a failed build.
    pub fn fail_build(&mut self) {
        self.building = false;
        self.pending_single = None;
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
    fn engine_is_none_until_set() {
        let tracker = JobTracker::<()>::new();
        assert!(tracker.engine().is_none());
        assert!(!tracker.is_building());
    }

    #[test]
    fn set_engine_returns_true_only_when_building_was_marked() {
        let mut tracker = JobTracker::<()>::new();
        assert!(!tracker.set_engine(()), "no build was pending");
        assert!(!tracker.is_building(), "flag cleared even without pending");
        assert!(tracker.engine().is_some());

        tracker.mark_building();
        assert!(tracker.is_building());
        assert!(
            tracker.set_engine(()),
            "pending build reported so queued jobs start"
        );
        assert!(!tracker.is_building());
        assert!(tracker.engine().is_some());
    }

    #[test]
    fn fail_build_clears_the_flag() {
        let mut tracker = JobTracker::<()>::new();
        tracker.mark_building();
        tracker.fail_build();
        assert!(!tracker.is_building());
        assert!(tracker.engine().is_none());
    }

    #[test]
    fn done_reopen_semantics() {
        let mut tracker = JobTracker::<()>::new();
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
        let mut tracker = JobTracker::<()>::new();
        tracker.mark_done(0, EntryId(1));
        tracker.mark_done(1, EntryId(1));
        tracker.mark_done(0, EntryId(2));
        assert_eq!(tracker.done_count(), 3);
    }

    #[test]
    fn pending_single_round_trips_and_clears_on_fail() {
        let mut tracker = JobTracker::<()>::new();
        assert!(tracker.take_pending_single().is_none());
        tracker.set_pending_single(PendingSingle {
            index: 1,
            id: EntryId(2),
            path: "p".to_string(),
            quad: scanlateit_model::Quad { points: [[0.0,0.0],[1.0,0.0],[1.0,1.0],[0.0,1.0]] },
        });
        let pending = tracker.take_pending_single().unwrap();
        assert_eq!(pending.index, 1);
        assert_eq!(pending.id, EntryId(2));
        assert!(tracker.take_pending_single().is_none(), "taken is consumed");
        // set again then fail_build clears
        tracker.set_pending_single(PendingSingle {
            index: 0,
            id: EntryId(5),
            path: "q".to_string(),
            quad: scanlateit_model::Quad { points: [[0.0,0.0],[1.0,0.0],[1.0,1.0],[0.0,1.0]] },
        });
        tracker.mark_building();
        tracker.fail_build();
        assert!(tracker.take_pending_single().is_none());
        assert!(!tracker.is_building());
    }

    // NOTE: `Engine::classify_entry` needs the ONNX model file, so it is
    // smoke-tested manually; its mapping (`to_entry_style`) is covered by the
    // unit tests in lib.rs.
}
