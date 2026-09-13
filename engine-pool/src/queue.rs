//! Weighted backfill queue for the engine pool (UI-agnostic, no `iced`).
//!
//! Potato-PC guarantee: total concurrent engine weight ≤ 5.
//! Spec weights (hard-coded, not settings):
//!   OCR            = 4
//!   SEGMENT        = 4
//!   STYLE          = 2
//!   INPAINT telea  = 1
//!   INPAINT lama   = 4
//!   INPAINT aot    = 3
//!
//! Spec priorities (lower = run sooner, based on expected time; time-efficient):
//!   INPAINT telea (0) < STYLE auto-detect (1) < SEGMENT (2) < OCR (3) < INPAINT aot (4) < INPAINT lama (5)
//! Weight caps concurrency, priority decides dispatch order.
//!
//! Queue is FIFO insertion + priority backfill scan: insertion order is FIFO
//! (ties broken by FIFO), but dispatch scans pending in priority order and
//! picks the first job whose weight fits `remaining`. Head does NOT block
//! lighter higher-priority jobs behind it (backfill enabled). Full/partial
//! pipeline strict ordering is orchestrated outside the queue (segment ->
//! style -> inpaint chain pushed one-by-one) so queue can reorder freely.

use std::collections::VecDeque;

use crate::job::{AcquireResult, JobKind, OwnerId, QueuedJob};

pub const POOL_CAPACITY: u8 = 5;

/// Global queue + running accounting. Lives inside [`crate::pool::EnginePool`].
#[derive(Debug)]
pub struct EngineQueue {
    pending: VecDeque<QueuedJob>,
    running: Vec<QueuedJob>,
    used: u8,
    next_id: u64,
}

impl Default for EngineQueue {
    fn default() -> Self {
        Self {
            pending: VecDeque::new(),
            running: Vec::new(),
            used: 0,
            next_id: 1,
        }
    }
}

impl EngineQueue {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueue a job for `owner` with `kind`. Returns `QueuedJob` with new id.
    /// Weight is derived from `kind` and counted against `POOL_CAPACITY`.
    pub fn enqueue(&mut self, owner: OwnerId, kind: JobKind) -> QueuedJob {
        let job = QueuedJob {
            id: self.next_id,
            owner,
            kind,
        };
        self.next_id += 1;
        self.pending.push_back(job.clone());
        job
    }

    /// Snapshot helpers for UI / tests.
    pub fn used_weight(&self) -> u8 {
        self.used
    }
    pub fn remaining(&self) -> u8 {
        POOL_CAPACITY.saturating_sub(self.used)
    }
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
    pub fn pending_jobs(&self) -> Vec<QueuedJob> {
        self.pending.iter().cloned().collect()
    }

    /// 1-indexed position of `id` in pending, or None if not pending (maybe running).
    pub fn position(&self, id: u64) -> Option<usize> {
        self.pending
            .iter()
            .position(|j| j.id == id)
            .map(|p| p + 1)
    }

    /// Backfill pop: scan pending in priority order (lower priority value first,
    /// FIFO tie-break via id) and pop the first job whose weight fits.
    /// Marks it running and reserves weight. Returns the dispatched job.
    pub fn try_pop_dispatchable(&mut self) -> Option<QueuedJob> {
        let rem = self.remaining();
        // Find best candidate index by priority then FIFO (id)
        let mut best_idx: Option<usize> = None;
        let mut best_key: Option<(u8, u64)> = None;
        for (idx, job) in self.pending.iter().enumerate() {
            if job.weight() <= rem {
                let key = (job.priority(), job.id);
                if best_key.is_none() || key < best_key.unwrap() {
                    best_key = Some(key);
                    best_idx = Some(idx);
                }
            }
        }
        if let Some(idx) = best_idx {
            let job = self.pending.remove(idx).unwrap();
            self.used = self.used.saturating_add(job.weight());
            self.running.push(job.clone());
            Some(job)
        } else {
            None
        }
    }

    /// Peek the best dispatchable job (priority scan) without mutating.
    pub fn peek_dispatchable(&self) -> Option<QueuedJob> {
        let rem = self.remaining();
        let mut best: Option<&QueuedJob> = None;
        let mut best_key: Option<(u8, u64)> = None;
        for job in self.pending.iter() {
            if job.weight() <= rem {
                let key = (job.priority(), job.id);
                if best_key.is_none() || key < best_key.unwrap() {
                    best_key = Some(key);
                    best = Some(job);
                }
            }
        }
        best.cloned()
    }

    /// Release a running job by `id`. Returns the job if found.
    pub fn complete_by_id(&mut self, id: u64) -> Option<QueuedJob> {
        if let Some(pos) = self.running.iter().position(|j| j.id == id) {
            let job = self.running.remove(pos);
            self.used = self.used.saturating_sub(job.weight());
            Some(job)
        } else {
            None
        }
    }

    /// Release a running job by `owner` + `kind` (first match). Convenience
    /// for completion handlers that don't carry `id`.
    pub fn complete(&mut self, owner: OwnerId, kind: JobKind) -> Option<QueuedJob> {
        if let Some(pos) = self
            .running
            .iter()
            .position(|j| j.owner == owner && j.kind == kind)
        {
            let job = self.running.remove(pos);
            self.used = self.used.saturating_sub(job.weight());
            Some(job)
        } else {
            None
        }
    }

    /// Complete any running OCR job for `owner` regardless of mode
    /// (unified manual + auto share the OCR slot).
    pub fn complete_ocr(&mut self, owner: OwnerId) -> Option<QueuedJob> {
        if let Some(pos) = self
            .running
            .iter()
            .position(|j| j.owner == owner && j.kind.is_ocr())
        {
            let job = self.running.remove(pos);
            self.used = self.used.saturating_sub(job.weight());
            Some(job)
        } else {
            None
        }
    }

    /// Running job for `owner` + `kind`, if any.
    pub fn running_for(&self, owner: OwnerId, kind: JobKind) -> Option<&QueuedJob> {
        self.running.iter().find(|j| j.owner == owner && j.kind == kind)
    }

    /// Any running OCR job (either mode) for `owner`?
    pub fn running_ocr_for(&self, owner: OwnerId) -> Option<&QueuedJob> {
        self.running
            .iter()
            .find(|j| j.owner == owner && j.kind.is_ocr())
    }

    /// Any running job for `owner`?
    pub fn is_owner_running(&self, owner: OwnerId) -> bool {
        self.running.iter().any(|j| j.owner == owner)
    }

    /// Is `owner` queued (pending) for any kind?
    pub fn is_owner_queued(&self, owner: OwnerId) -> bool {
        self.pending.iter().any(|j| j.owner == owner)
    }

    /// Remove all pending jobs for `owner` (e.g. on tab close). Running jobs
    /// stay to finish; caller should cancel them separately if desired.
    pub fn cancel_pending_for_owner(&mut self, owner: OwnerId) -> Vec<QueuedJob> {
        let mut removed = Vec::new();
        let mut kept = VecDeque::with_capacity(self.pending.len());
        while let Some(job) = self.pending.pop_front() {
            if job.owner == owner {
                removed.push(job);
            } else {
                kept.push_back(job);
            }
        }
        self.pending = kept;
        removed
    }

    /// Force-remove a running job due to tab close / explicit cancel. Frees weight.
    pub fn cancel_running_for_owner(&mut self, owner: OwnerId) -> Vec<QueuedJob> {
        let mut removed = Vec::new();
        let mut kept = Vec::new();
        for job in self.running.drain(..) {
            if job.owner == owner {
                self.used = self.used.saturating_sub(job.weight());
                removed.push(job);
            } else {
                kept.push(job);
            }
        }
        self.running = kept;
        removed
    }

    pub fn pending_for_owner(&self, owner: OwnerId) -> Vec<QueuedJob> {
        self.pending
            .iter()
            .filter(|j| j.owner == owner)
            .cloned()
            .collect()
    }

    /// Backfill acquire: FIFO insertion + priority scan.
    /// If `kind` fits in `remaining` it is dispatched immediately via backfill
    /// (even if pending not empty), otherwise it is enqueued FIFO and returns
    /// Queued with FIFO position. Weight is reserved on Acquired.
    pub fn try_acquire_or_enqueue(&mut self, owner: OwnerId, kind: JobKind) -> AcquireResult {
        let w = kind.weight();
        if w <= self.remaining() {
            let mut best_pending_key: Option<(u8, u64)> = None;
            let rem = self.remaining();
            for job in self.pending.iter() {
                if job.weight() <= rem {
                    let key = (job.priority(), job.id);
                    if best_pending_key.is_none() || key < best_pending_key.unwrap() {
                        best_pending_key = Some(key);
                    }
                }
            }
            let new_key = (kind.priority(), self.next_id);
            if best_pending_key.is_none() || new_key < best_pending_key.unwrap() {
                let job = QueuedJob {
                    id: self.next_id,
                    owner,
                    kind,
                };
                self.next_id += 1;
                self.used = self.used.saturating_add(w);
                self.running.push(job.clone());
                return AcquireResult::Acquired(job);
            }
        }
        let job = self.enqueue(owner, kind);
        let pos = self.position(job.id).unwrap_or(self.pending_len());
        AcquireResult::Queued(job, pos)
    }

    // ---- Back-compat aliases using tab-style naming (owner == tab) ----
    #[allow(dead_code)]
    pub fn is_tab_running(&self, owner: OwnerId) -> bool {
        self.is_owner_running(owner)
    }
    #[allow(dead_code)]
    pub fn is_tab_queued(&self, owner: OwnerId) -> bool {
        self.is_owner_queued(owner)
    }
    #[allow(dead_code)]
    pub fn pending_for_tab(&self, owner: OwnerId) -> Vec<QueuedJob> {
        self.pending_for_owner(owner)
    }
    #[allow(dead_code)]
    pub fn cancel_pending_for_tab(&mut self, owner: OwnerId) -> Vec<QueuedJob> {
        self.cancel_pending_for_owner(owner)
    }
    #[allow(dead_code)]
    pub fn cancel_running_for_tab(&mut self, owner: OwnerId) -> Vec<QueuedJob> {
        self.cancel_running_for_owner(owner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::OcrMode;
    use easyscanlate_settings::InpaintBackend;

    fn oid(n: u64) -> OwnerId {
        OwnerId(n)
    }

    fn ocr() -> JobKind {
        JobKind::Ocr(OcrMode::Auto)
    }

    #[test]
    fn weights_match_spec() {
        assert_eq!(ocr().weight(), 4);
        assert_eq!(JobKind::Segment.weight(), 4);
        assert_eq!(JobKind::Styling.weight(), 2);
        assert_eq!(JobKind::Inpaint(InpaintBackend::Telea).weight(), 1);
        assert_eq!(JobKind::Inpaint(InpaintBackend::Lama).weight(), 4);
        assert_eq!(JobKind::Inpaint(InpaintBackend::Aot).weight(), 3);
        // Manual OCR shares the auto weight.
        assert_eq!(JobKind::Ocr(OcrMode::Manual).weight(), 4);
    }

    #[test]
    fn priorities_match_spec() {
        assert_eq!(JobKind::Inpaint(InpaintBackend::Telea).priority(), 0);
        assert_eq!(JobKind::Styling.priority(), 1);
        assert_eq!(JobKind::Segment.priority(), 2);
        assert_eq!(ocr().priority(), 3);
        assert_eq!(JobKind::Ocr(OcrMode::Manual).priority(), 3);
        assert_eq!(JobKind::Inpaint(InpaintBackend::Aot).priority(), 4);
        assert_eq!(JobKind::Inpaint(InpaintBackend::Lama).priority(), 5);
    }

    #[test]
    fn capacity_is_5() {
        assert_eq!(POOL_CAPACITY, 5);
    }

    #[test]
    fn backfill_telea_behind_lama_when_capacity_allows() {
        let mut q = EngineQueue::new();
        let s = q.enqueue(oid(10), JobKind::Styling); // w2
        let d = q.try_pop_dispatchable().unwrap();
        assert_eq!(d.id, s.id);
        assert_eq!(q.used_weight(), 2);
        let lama = q.enqueue(oid(1), JobKind::Inpaint(InpaintBackend::Lama));
        let telea = q.enqueue(oid(2), JobKind::Inpaint(InpaintBackend::Telea));
        assert_eq!(q.pending_len(), 2);
        let d2 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d2.id, telea.id, "backfill must prioritize telea over lama");
        assert_eq!(q.used_weight(), 3);
        assert!(q.try_pop_dispatchable().is_none());
        assert_eq!(q.position(lama.id), Some(1));
        assert_eq!(q.position(telea.id), None);
        q.complete_by_id(telea.id);
        assert_eq!(q.used_weight(), 2);
        assert!(q.try_pop_dispatchable().is_none());
        q.complete_by_id(s.id);
        assert_eq!(q.used_weight(), 0);
        let d3 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d3.id, lama.id);
    }

    #[test]
    fn priority_scan_telea_before_style_before_segment() {
        let mut q = EngineQueue::new();
        let lama = q.enqueue(oid(1), JobKind::Inpaint(InpaintBackend::Lama)); // 5
        let ocr_job = q.enqueue(oid(3), ocr()); // 3
        let seg = q.enqueue(oid(4), JobKind::Segment); // 2
        let style = q.enqueue(oid(5), JobKind::Styling); // 1
        let telea = q.enqueue(oid(6), JobKind::Inpaint(InpaintBackend::Telea)); // 0
        assert_eq!(q.pending_len(), 5);
        let d1 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d1.id, telea.id);
        assert_eq!(q.used_weight(), 1);
        let d2 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d2.id, style.id);
        assert_eq!(q.used_weight(), 3);
        assert!(q.try_pop_dispatchable().is_none());
        q.complete_by_id(telea.id);
        assert_eq!(q.used_weight(), 2);
        assert!(q.try_pop_dispatchable().is_none());
        q.complete_by_id(style.id);
        assert_eq!(q.used_weight(), 0);
        let d3 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d3.id, seg.id);
        assert_eq!(d3.kind, JobKind::Segment);
        let _ = (lama, ocr_job);
    }

    #[test]
    fn fifo_tie_break_within_same_priority() {
        let mut q = EngineQueue::new();
        let t1 = q.enqueue(oid(1), JobKind::Inpaint(InpaintBackend::Telea));
        let t2 = q.enqueue(oid(2), JobKind::Inpaint(InpaintBackend::Telea));
        let t3 = q.enqueue(oid(3), JobKind::Inpaint(InpaintBackend::Telea));
        let d1 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d1.id, t1.id);
        let d2 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d2.id, t2.id);
        let d3 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d3.id, t3.id);
    }

    #[test]
    fn ocr_manual_and_auto_share_slot() {
        let mut q = EngineQueue::new();
        let a = q.try_acquire_or_enqueue(oid(1), JobKind::Ocr(OcrMode::Auto));
        assert!(matches!(a, AcquireResult::Acquired(_)));
        // Manual for another owner cannot fit (4+4>5) -> queued.
        let m = q.try_acquire_or_enqueue(oid(2), JobKind::Ocr(OcrMode::Manual));
        assert!(matches!(m, AcquireResult::Queued(_, _)));
        // complete_ocr frees regardless of mode.
        assert!(q.complete_ocr(oid(1)).is_some());
        assert_eq!(q.used_weight(), 0);
    }

    #[test]
    fn weight_packing_with_priority() {
        let mut q = EngineQueue::new();
        // Pending [Style, Style, Telea] -> priority scan picks Telea first.
        // With capacity 5, all three fit: 1+2+2=5.
        let s1 = q.enqueue(oid(1), JobKind::Styling); // w2 prio1
        let s2 = q.enqueue(oid(2), JobKind::Styling); // w2 prio1
        let t1 = q.enqueue(oid(3), JobKind::Inpaint(InpaintBackend::Telea)); // w1 prio0
        let d1 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d1.id, t1.id, "telea prio0 should go first");
        assert_eq!(q.used_weight(), 1);
        let d2 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d2.id, s1.id, "style FIFO tie break s1 before s2");
        assert_eq!(q.used_weight(), 3);
        // remaining 2, s2 w2 fits 3+2=5
        let d3 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d3.id, s2.id);
        assert_eq!(q.used_weight(), 5);
        assert!(q.try_pop_dispatchable().is_none());
        q.complete_by_id(t1.id);
        assert_eq!(q.used_weight(), 4);
        q.complete_by_id(s1.id);
        assert_eq!(q.used_weight(), 2);
        let _ = s2;
    }

    #[test]
    fn four_telea_fill_capacity() {
        let mut q = EngineQueue::new();
        let mut ids = vec![];
        for i in 0..6 {
            ids.push(q.enqueue(oid(i), JobKind::Inpaint(InpaintBackend::Telea)));
        }
        for _ in 0..5 {
            assert!(q.try_pop_dispatchable().is_some());
        }
        assert_eq!(q.used_weight(), 5);
        // 6th blocked (remaining 0)
        assert!(q.try_pop_dispatchable().is_none());
        assert_eq!(q.pending_len(), 1);
        q.complete_by_id(ids[0].id);
        assert!(q.try_pop_dispatchable().is_some());
        assert_eq!(q.used_weight(), 5);
    }

    #[test]
    fn lama_vs_style_priority() {
        let mut q = EngineQueue::new();
        let lama = q.enqueue(oid(1), JobKind::Inpaint(InpaintBackend::Lama)); // prio5
        let style = q.enqueue(oid(2), JobKind::Styling); // prio1
        // Used 0, style should be picked before lama despite FIFO.
        let d1 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d1.id, style.id);
        assert_eq!(q.used_weight(), 2);
        // lama w4 doesn't fit 2+4=6>5: blocked.
        assert!(q.try_pop_dispatchable().is_none());
        q.complete_by_id(style.id);
        let d2 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d2.id, lama.id);
        assert!(
            q.running_for(oid(1), JobKind::Inpaint(InpaintBackend::Lama)).is_some()
        );
        let _ = lama;
    }

    #[test]
    fn try_acquire_backfill_immediate() {
        let mut q = EngineQueue::new();
        // Acquire a Style w2 directly.
        let a = q.try_acquire_or_enqueue(oid(1), JobKind::Styling);
        assert!(matches!(a, AcquireResult::Acquired(_)));
        assert_eq!(q.used_weight(), 2);
        // Enqueue Lama w4 (doesn't fit 2+4=6>5) -> Queued.
        let lama_res =
            q.try_acquire_or_enqueue(oid(2), JobKind::Inpaint(InpaintBackend::Lama));
        assert!(matches!(lama_res, AcquireResult::Queued(_, _)));
        assert_eq!(q.pending_len(), 1);
        // Telea w1 fits 2+1<=5 -> backfill Acquired even though pending non-empty.
        let telea_res =
            q.try_acquire_or_enqueue(oid(3), JobKind::Inpaint(InpaintBackend::Telea));
        assert!(
            matches!(telea_res, AcquireResult::Acquired(_)),
            "telea should backfill when lama head blocked"
        );
        assert_eq!(q.used_weight(), 3);
        assert_eq!(q.pending_len(), 1); // lama still pending
        let telea2 =
            q.try_acquire_or_enqueue(oid(4), JobKind::Inpaint(InpaintBackend::Telea));
        assert!(matches!(telea2, AcquireResult::Acquired(_)));
        assert_eq!(q.used_weight(), 4);
        let telea3 =
            q.try_acquire_or_enqueue(oid(5), JobKind::Inpaint(InpaintBackend::Telea));
        assert!(matches!(telea3, AcquireResult::Acquired(_)));
        assert_eq!(q.used_weight(), 5);
        // Next Style w2 would not fit 5+2>5.
        let s = q.try_acquire_or_enqueue(oid(6), JobKind::Styling);
        assert!(matches!(s, AcquireResult::Queued(_, _)));
    }

    #[test]
    fn try_acquire_respects_higher_priority_pending() {
        let mut q = EngineQueue::new();
        // Fill used 2 with one style running.
        let _ = q.try_acquire_or_enqueue(oid(1), JobKind::Styling);
        // Pending fitting Style w2 prio1.
        let style_pending = q.enqueue(oid(2), JobKind::Styling);
        assert_eq!(q.pending_len(), 1);
        // Pending Telea prio0 fitting vs new Style prio1: new Style is lower
        // priority than pending Telea... mirror of the original scenario:
        let mut q3 = EngineQueue::new();
        let _ = q3.enqueue(oid(20), JobKind::Styling); // pending style prio1 w2
        // used 0, remaining 5, style fits. Lama prio5 also fits but is lower
        // priority than pending style, so it must queue, not acquire.
        let lama_q =
            q3.try_acquire_or_enqueue(oid(21), JobKind::Inpaint(InpaintBackend::Lama));
        assert!(
            matches!(lama_q, AcquireResult::Queued(_, _)),
            "lama lower priority than pending style should be queued"
        );
        // Telea prio0 outranks pending style prio1, so it backfills.
        let telea_a =
            q3.try_acquire_or_enqueue(oid(22), JobKind::Inpaint(InpaintBackend::Telea));
        assert!(
            matches!(telea_a, AcquireResult::Acquired(_)),
            "higher priority telea should backfill ahead of style"
        );
        let _ = style_pending;
    }

    #[test]
    fn cancel_pending_for_owner() {
        let mut q = EngineQueue::new();
        q.enqueue(oid(1), ocr());
        q.enqueue(oid(2), JobKind::Styling);
        q.enqueue(oid(1), JobKind::Styling);
        assert_eq!(q.pending_len(), 3);
        let removed = q.cancel_pending_for_owner(oid(1));
        assert_eq!(removed.len(), 2);
        assert_eq!(q.pending_len(), 1);
        assert_eq!(q.pending.front().unwrap().owner, oid(2));
    }

    #[test]
    fn complete_by_owner_kind() {
        let mut q = EngineQueue::new();
        let _j = q.enqueue(oid(1), JobKind::Styling);
        q.try_pop_dispatchable().unwrap();
        assert_eq!(q.used_weight(), 2);
        let c = q.complete(oid(1), JobKind::Styling).unwrap();
        assert_eq!(c.kind, JobKind::Styling);
        assert_eq!(q.used_weight(), 0);
    }
}
