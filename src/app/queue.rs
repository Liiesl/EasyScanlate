//! Weighted FIFO queue for the engine pool.
//!
//! Potato-PC guarantee: total concurrent engine weight ≤ 4.
//! Spec weights (hard-coded, not settings):
//!   OCR            = 4
//!   SEGMENT        = 4
//!   STYLE          = 2
//!   INPAINT telea  = 1
//!   INPAINT lama   = 4
//!   INPAINT aot    = 3
//!
//! Queue is strict FIFO: head blocks if its weight doesn't fit, even if a
//! smaller job behind would fit (backfill disabled by spec). Full pipeline
//! is decomposed into 4 sequential queued jobs, pushed one-by-one as each
//! stage finishes — preserving global FIFO interleaving across tabs.

use std::collections::VecDeque;

use super::tab::TabId;
use iced::Task;

pub const POOL_CAPACITY: u8 = 4;

/// Engine kind that occupies the pool. Translation is excluded (network).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EngineKind {
    Ocr,
    Segment,
    Style,
    InpaintTelea,
    InpaintLama,
    InpaintAot,
}

impl EngineKind {
    pub fn weight(self) -> u8 {
        match self {
            EngineKind::Ocr => 4,
            EngineKind::Segment => 4,
            EngineKind::Style => 2,
            EngineKind::InpaintTelea => 1,
            EngineKind::InpaintLama => 4,
            EngineKind::InpaintAot => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            EngineKind::Ocr => "OCR",
            EngineKind::Segment => "SEGMENT",
            EngineKind::Style => "STYLE",
            EngineKind::InpaintTelea => "INPAINT telea",
            EngineKind::InpaintLama => "INPAINT lama",
            EngineKind::InpaintAot => "INPAINT aot-gan",
        }
    }
}

/// One entry waiting or running in the pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedJob {
    pub id: u64,
    pub tab_id: TabId,
    pub kind: EngineKind,
}

impl QueuedJob {
    pub fn weight(&self) -> u8 {
        self.kind.weight()
    }
}

/// Global queue + running accounting. Lives inside `EnginePool`.
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
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueue a job for `tab_id` with `kind`. Returns `QueuedJob` with new id.
    /// Weight is derived from `kind` and counted against `POOL_CAPACITY`.
    pub fn enqueue(&mut self, tab_id: TabId, kind: EngineKind) -> QueuedJob {
        let job = QueuedJob {
            id: self.next_id,
            tab_id,
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
    pub fn running_len(&self) -> usize {
        self.running.len()
    }
    pub fn pending_jobs(&self) -> Vec<QueuedJob> {
        self.pending.iter().cloned().collect()
    }
    pub fn running_jobs(&self) -> Vec<QueuedJob> {
        self.running.clone()
    }

    /// 1-indexed position of `id` in pending, or None if not pending (maybe running).
    pub fn position(&self, id: u64) -> Option<usize> {
        self.pending
            .iter()
            .position(|j| j.id == id)
            .map(|p| p + 1)
    }

    /// 1-indexed position of the pending job for `tab_id` + `kind`, if any.
    pub fn position_for(&self, tab_id: TabId, kind: EngineKind) -> Option<usize> {
        self.pending
            .iter()
            .position(|j| j.tab_id == tab_id && j.kind == kind)
            .map(|p| p + 1)
    }

    /// Check strict-FIFO dispatchability: head weight fits in remaining capacity.
    pub fn can_dispatch(&self) -> bool {
        if let Some(head) = self.pending.front() {
            head.weight() <= self.remaining()
        } else {
            false
        }
    }

    /// Try to pop the head if it fits. Marks it running and reserves weight.
    /// Returns the dispatched job (now running).
    pub fn try_pop_dispatchable(&mut self) -> Option<QueuedJob> {
        let fits = if let Some(head) = self.pending.front() {
            head.weight() <= self.remaining()
        } else {
            false
        };
        if fits {
            let job = self.pending.pop_front().unwrap();
            self.used = self.used.saturating_add(job.weight());
            self.running.push(job.clone());
            Some(job)
        } else {
            None
        }
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

    /// Release a running job by `tab_id` + `kind` (first match). Convenience
    /// for completion handlers that don't carry `id`.
    pub fn complete(&mut self, tab_id: TabId, kind: EngineKind) -> Option<QueuedJob> {
        if let Some(pos) = self
            .running
            .iter()
            .position(|j| j.tab_id == tab_id && j.kind == kind)
        {
            let job = self.running.remove(pos);
            self.used = self.used.saturating_sub(job.weight());
            Some(job)
        } else {
            None
        }
    }

    /// Running job for `tab_id` + `kind`, if any.
    pub fn running_for(&self, tab_id: TabId, kind: EngineKind) -> Option<&QueuedJob> {
        self.running.iter().find(|j| j.tab_id == tab_id && j.kind == kind)
    }

    /// Any running job for `tab_id`?
    pub fn is_tab_running(&self, tab_id: TabId) -> bool {
        self.running.iter().any(|j| j.tab_id == tab_id)
    }

    /// Is `tab_id` queued (pending) for any kind?
    pub fn is_tab_queued(&self, tab_id: TabId) -> bool {
        self.pending.iter().any(|j| j.tab_id == tab_id)
    }

    /// Remove all pending jobs for `tab_id` (e.g. on tab close). Running jobs
    /// stay to finish; caller should cancel them separately if desired.
    pub fn cancel_pending_for_tab(&mut self, tab_id: TabId) -> Vec<QueuedJob> {
        let mut removed = Vec::new();
        let mut kept = VecDeque::with_capacity(self.pending.len());
        while let Some(job) = self.pending.pop_front() {
            if job.tab_id == tab_id {
                removed.push(job);
            } else {
                kept.push_back(job);
            }
        }
        self.pending = kept;
        removed
    }

    /// Force-remove a running job due to tab close / explicit cancel. Frees weight.
    pub fn cancel_running_for_tab(&mut self, tab_id: TabId) -> Vec<QueuedJob> {
        let mut removed = Vec::new();
        let mut kept = Vec::new();
        for job in self.running.drain(..) {
            if job.tab_id == tab_id {
                self.used = self.used.saturating_sub(job.weight());
                removed.push(job);
            } else {
                kept.push(job);
            }
        }
        self.running = kept;
        removed
    }

    pub fn pending_for_tab(&self, tab_id: TabId) -> Vec<QueuedJob> {
        self.pending
            .iter()
            .filter(|j| j.tab_id == tab_id)
            .cloned()
            .collect()
    }

    /// Strict-FIFO acquire: if pending empty and capacity fits, reserve immediately
    /// and return Acquired; otherwise enqueue and return Queued with position.
    pub fn try_acquire_or_enqueue(&mut self, tab_id: TabId, kind: EngineKind) -> AcquireResult {
        let w = kind.weight();
        if self.pending.is_empty() && self.used + w <= POOL_CAPACITY {
            let job = QueuedJob {
                id: self.next_id,
                tab_id,
                kind,
            };
            self.next_id += 1;
            self.used = self.used.saturating_add(w);
            self.running.push(job.clone());
            AcquireResult::Acquired(job)
        } else {
            let job = self.enqueue(tab_id, kind);
            let pos = self.position(job.id).unwrap_or(self.pending_len());
            AcquireResult::Queued(job, pos)
        }
    }

    /// Release and then report remaining capacity for UI.
    pub fn status_line(&self) -> String {
        format!(
            "pool {}/{} pending {} running {}",
            self.used,
            POOL_CAPACITY,
            self.pending_len(),
            self.running_len()
        )
    }
}

pub enum AcquireResult {
    Acquired(QueuedJob),
    Queued(QueuedJob, usize),
}

/// Refresh status text for all tabs that are queued (pending). Call after
/// any queue mutation (complete / dispatch) to keep UI in sync. This helper
/// avoids borrow-checker conflicts that arise from iterating `pending_jobs()`
/// while mutably borrowing `app.tabs`.
pub fn refresh_queued_statuses(app: &mut crate::app::App) {
    let pending = app.engines.queue.pending_jobs();
    let used = app.engines.queue.used_weight();
    for (pos, job) in pending.iter().enumerate() {
        // find tab index without holding borrow across queue
        let idx_opt = app.tabs.iter().position(|t| t.id == job.tab_id);
        if let Some(idx) = idx_opt {
            app.tabs[idx].status = format!(
                "Queued {} (pos {}, pool {}/{}) ...",
                job.kind.label(),
                pos + 1,
                used,
                POOL_CAPACITY
            );
        }
    }
}

/// Try to dispatch as many pending jobs as weight allows.
/// Called after enqueue or after a job completes. Spawns the actual
/// engine work for each dispatched job (build + run) while weight remains
/// reserved until completion handlers free it.
pub fn dispatch_pending(app: &mut crate::app::App) -> Task<crate::app::Message> {
    let mut tasks: Vec<Task<crate::app::Message>> = Vec::new();
    loop {
        let head = { app.engines.queue.pending.front().cloned() };
        let Some(head) = head else { break };
        if head.weight() > app.engines.queue.remaining() {
            break;
        }
        // reserve
        let job = app.engines.queue.try_pop_dispatchable().unwrap();
        let idx_opt = app.tabs.iter().position(|t| t.id == job.tab_id);
        if idx_opt.is_none() {
            // tab closed while queued
            app.engines.queue.complete_by_id(job.id);
            continue;
        }
        let idx = idx_opt.unwrap();
        let tab_id = job.tab_id;
        // update status to running
        app.tabs[idx].status = format!(
            "{} running (pool {}/{})",
            job.kind.label(),
            app.engines.queue.used_weight(),
            super::super::app::queue::POOL_CAPACITY
        );
        let task = match job.kind {
            EngineKind::Ocr => dispatch_ocr(app, tab_id),
            EngineKind::Segment => dispatch_segment(app, tab_id),
            EngineKind::Style => dispatch_style(app, tab_id),
            EngineKind::InpaintTelea => {
                dispatch_inpaint(app, tab_id, scanlateit_settings::InpaintBackend::Telea)
            }
            EngineKind::InpaintLama => {
                dispatch_inpaint(app, tab_id, scanlateit_settings::InpaintBackend::Lama)
            }
            EngineKind::InpaintAot => {
                dispatch_inpaint(app, tab_id, scanlateit_settings::InpaintBackend::Aot)
            }
        };
        tasks.push(task);
    }
    if tasks.is_empty() {
        Task::none()
    } else {
        Task::batch(tasks)
    }
}

#[cfg(feature = "ocr")]
fn dispatch_ocr(app: &mut crate::app::App, tab_id: super::tab::TabId) -> Task<crate::app::Message> {
    // if pipeline already cached, start stream; else build it (weight already reserved)
    if app.engines.pipeline.is_some() {
        return crate::app::ocr::maybe_start_ocr_for(app, tab_id);
    }
    // need to build pipeline
    let (workers, cfg) = scanlateit_settings::get(|s| {
        let workers = s.ocr_workers.parse::<usize>().unwrap_or(2).max(1);
        let cfg = scanlateit_ocr::config_from_strings(&s.ocr_text_score, &s.ocr_max_side_len);
        (workers, cfg)
    });
    let tid = tab_id;
    Task::perform(
        async move { scanlateit_ocr::ParallelEngine::build_with_config(cfg, workers) },
        move |res| crate::app::Message::Tab(tid, crate::app::TabMessage::ParallelEngineReady(res)),
    )
}

#[cfg(not(feature = "ocr"))]
fn dispatch_ocr(_app: &mut crate::app::App, _tab_id: super::tab::TabId) -> Task<crate::app::Message> {
    Task::none()
}

fn dispatch_segment(app: &mut crate::app::App, tab_id: super::tab::TabId) -> Task<crate::app::Message> {
    crate::app::segment::start_segment_filter_for(app, tab_id)
}

fn dispatch_style(app: &mut crate::app::App, tab_id: super::tab::TabId) -> Task<crate::app::Message> {
    #[cfg(feature = "styling")]
    {
        return crate::app::styling::classify_for(app, tab_id);
    }
    #[cfg(not(feature = "styling"))]
    {
        let _ = (app, tab_id);
        return Task::none();
    }
}

fn dispatch_inpaint(
    app: &mut crate::app::App,
    tab_id: super::tab::TabId,
    backend: scanlateit_settings::InpaintBackend,
) -> Task<crate::app::Message> {
    #[cfg(feature = "inpaint")]
    {
        // Pull pending jobs for this backend from the tab (stored at enqueue time).
        // For queue promotion we expect the tab already has pending_auto_*_jobs set.
        let idx_opt = app.tabs.iter().position(|t| t.id == tab_id);
        if idx_opt.is_none() {
            return Task::none();
        }
        let idx = idx_opt.unwrap();
        let jobs_opt = match backend {
            scanlateit_settings::InpaintBackend::Telea => app.tabs[idx].pending_auto_telea_jobs.clone(),
            scanlateit_settings::InpaintBackend::Lama => app.tabs[idx].pending_auto_lama_jobs.clone(),
            scanlateit_settings::InpaintBackend::Aot => app.tabs[idx].pending_auto_aot_jobs.clone(),
        };
        if let Some(jobs) = jobs_opt {
            // Clear tab pending so re-entrance doesn't duplicate; queue holds running
            match backend {
                scanlateit_settings::InpaintBackend::Telea => app.tabs[idx].pending_auto_telea_jobs = None,
                scanlateit_settings::InpaintBackend::Lama => app.tabs[idx].pending_auto_lama_jobs = None,
                scanlateit_settings::InpaintBackend::Aot => app.tabs[idx].pending_auto_aot_jobs = None,
            }
            return crate::app::inpaint::dispatch_auto_for(app, tab_id, jobs, backend);
        }
        // Fallback: if no bulk pending, maybe it's a manual inpaint (pending_manual_multi)
        if app.tabs[idx].pending_manual_multi.is_some() {
            // manual inpaint dispatch is handled via manual queue kind, but we map telea here
            // For now, no-op; manual jobs are enqueued as InpaintTelea etc via manual handler
            return Task::none();
        }
        return Task::none();
    }
    #[cfg(not(feature = "inpaint"))]
    {
        let _ = (app, tab_id, backend);
        return Task::none();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tid(n: u64) -> TabId {
        TabId(n)
    }

    #[test]
    fn weights_match_spec() {
        assert_eq!(EngineKind::Ocr.weight(), 4);
        assert_eq!(EngineKind::Segment.weight(), 4);
        assert_eq!(EngineKind::Style.weight(), 2);
        assert_eq!(EngineKind::InpaintTelea.weight(), 1);
        assert_eq!(EngineKind::InpaintLama.weight(), 4);
        assert_eq!(EngineKind::InpaintAot.weight(), 3);
    }

    #[test]
    fn capacity_is_hard_4() {
        assert_eq!(POOL_CAPACITY, 4);
    }

    #[test]
    fn fifo_strict_blocks_head_even_if_smaller_behind_fits() {
        let mut q = EngineQueue::new();
        // enqueue OCR w4 then telea w1
        let ocr = q.enqueue(tid(1), EngineKind::Ocr);
        let telea = q.enqueue(tid(2), EngineKind::InpaintTelea);
        assert_eq!(q.pending_len(), 2);
        // dispatch OCR fits (0+4<=4)
        let d1 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d1.id, ocr.id);
        assert_eq!(q.used_weight(), 4);
        // head now telea w1, but 4+1>4 so cannot dispatch -> strict FIFO blocks
        assert!(q.try_pop_dispatchable().is_none());
        assert_eq!(q.position(telea.id), Some(1));
        // free OCR
        q.complete_by_id(ocr.id);
        assert_eq!(q.used_weight(), 0);
        // now telea dispatches
        let d2 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d2.id, telea.id);
    }

    #[test]
    fn weight_packing_style_plus_telea() {
        let mut q = EngineQueue::new();
        let s1 = q.enqueue(tid(1), EngineKind::Style); // w2
        let s2 = q.enqueue(tid(2), EngineKind::Style); // w2
        let t1 = q.enqueue(tid(3), EngineKind::InpaintTelea); // w1
        // 2 dispatches
        assert!(q.try_pop_dispatchable().is_some()); // s1 -> used 2
        assert_eq!(q.used_weight(), 2);
        // s2 head w2 fits 2+2<=4
        assert!(q.try_pop_dispatchable().is_some()); // s2 -> used 4
        assert_eq!(q.used_weight(), 4);
        // t1 head w1 blocked (4+1>4)
        assert!(q.try_pop_dispatchable().is_none());
        q.complete_by_id(s1.id);
        assert_eq!(q.used_weight(), 2);
        // t1 now head? Actually order: pending [t1]; s2 still running. t1 w1 fits 2+1<=4
        assert!(q.try_pop_dispatchable().is_some());
        assert_eq!(q.used_weight(), 3);
        let _ = s2;
        let _ = t1;
    }

    #[test]
    fn four_telea_fill_capacity() {
        let mut q = EngineQueue::new();
        let mut ids = vec![];
        for i in 0..5 {
            ids.push(q.enqueue(tid(i), EngineKind::InpaintTelea));
        }
        for _ in 0..4 {
            assert!(q.try_pop_dispatchable().is_some());
        }
        assert_eq!(q.used_weight(), 4);
        // 5th blocked
        assert!(q.try_pop_dispatchable().is_none());
        assert_eq!(q.pending_len(), 1);
        q.complete_by_id(ids[0].id);
        assert!(q.try_pop_dispatchable().is_some());
        assert_eq!(q.used_weight(), 4);
    }

    #[test]
    fn lama_alone_blocks() {
        let mut q = EngineQueue::new();
        let lama = q.enqueue(tid(1), EngineKind::InpaintLama);
        let style = q.enqueue(tid(2), EngineKind::Style);
        q.try_pop_dispatchable().unwrap(); // lama -> used 4
        assert!(q.try_pop_dispatchable().is_none());
        q.complete_by_id(lama.id);
        assert!(q.try_pop_dispatchable().is_some());
        assert_eq!(q.running_for(tid(2), EngineKind::Style).is_some(), true);
        let _ = style;
    }

    #[test]
    fn cancel_pending_for_tab() {
        let mut q = EngineQueue::new();
        q.enqueue(tid(1), EngineKind::Ocr);
        q.enqueue(tid(2), EngineKind::Style);
        q.enqueue(tid(1), EngineKind::Style);
        assert_eq!(q.pending_len(), 3);
        let removed = q.cancel_pending_for_tab(tid(1));
        assert_eq!(removed.len(), 2);
        assert_eq!(q.pending_len(), 1);
        assert_eq!(q.pending.front().unwrap().tab_id, tid(2));
    }

    #[test]
    fn complete_by_tab_kind() {
        let mut q = EngineQueue::new();
        let _j = q.enqueue(tid(1), EngineKind::Style);
        q.try_pop_dispatchable().unwrap();
        assert_eq!(q.used_weight(), 2);
        let c = q.complete(tid(1), EngineKind::Style).unwrap();
        assert_eq!(c.kind, EngineKind::Style);
        assert_eq!(q.used_weight(), 0);
    }
}
