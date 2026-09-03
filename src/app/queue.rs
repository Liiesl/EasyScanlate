//! Weighted backfill queue with priority for the engine pool.
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

use super::tab::TabId;
use iced::Task;

pub const POOL_CAPACITY: u8 = 5;

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

    /// Lower value = higher priority (run sooner). SJF-inspired.
    pub fn priority(self) -> u8 {
        match self {
            EngineKind::InpaintTelea => 0,
            EngineKind::Style => 1,
            EngineKind::Segment => 2,
            EngineKind::Ocr => 3,
            EngineKind::InpaintAot => 4,
            EngineKind::InpaintLama => 5,
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
    pub fn priority(&self) -> u8 {
        self.kind.priority()
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

    /// True if ANY pending job fits in remaining capacity (backfill).
    pub fn can_dispatch(&self) -> bool {
        let rem = self.remaining();
        self.pending.iter().any(|j| j.weight() <= rem)
    }

    /// Backfill pop: scan pending in priority order (lower priority value first,
    /// FIFO tie-break via id) and pop the first job whose weight fits.
    /// Marks it running and reserves weight. Returns the dispatched job.
    pub fn try_pop_dispatchable(&mut self) -> Option<QueuedJob> {
        let rem = self.remaining();
        if self.pending.is_empty() || rem == 0 {
            // still need to check if any fits; empty handled, but keep scan for priority
        }
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

    /// Backfill acquire: FIFO insertion + priority scan.
    /// If `kind` fits in `remaining` it is dispatched immediately via backfill
    /// (even if pending not empty), otherwise it is enqueued FIFO and returns
    /// Queued with FIFO position. Weight is reserved on Acquired.
    pub fn try_acquire_or_enqueue(&mut self, tab_id: TabId, kind: EngineKind) -> AcquireResult {
        let w = kind.weight();
        if w <= self.remaining() {
            // Check if any pending job would be preferred by priority scan and also fits.
            // Pending is FIFO, dispatch picks best priority fitting. If there exists a
            // pending fitting job with higher priority (lower value) than `kind`,
            // we should not starve it by acquiring new job immediately.
            // So we enqueue tentatively and see who would be picked, but to avoid
            // extra allocation we just scan pending.
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
            // If no pending fitting job, or new job has higher priority (lower key), acquire directly.
            // Otherwise enqueue FIFO and let the higher-priority pending be dispatched via dispatch_pending.
            if best_pending_key.is_none() || new_key < best_pending_key.unwrap() {
                let job = QueuedJob {
                    id: self.next_id,
                    tab_id,
                    kind,
                };
                self.next_id += 1;
                self.used = self.used.saturating_add(w);
                self.running.push(job.clone());
                return AcquireResult::Acquired(job);
            }
            // A pending higher-priority fitting job exists: must enqueue new job FIFO
        }
        let job = self.enqueue(tab_id, kind);
        let pos = self.position(job.id).unwrap_or(self.pending_len());
        AcquireResult::Queued(job, pos)
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

/// Try to dispatch as many pending jobs as weight allows using backfill
/// priority scan (FIFO insertion, priority dispatch). Called after enqueue
/// or after a job completes. Spawns the actual engine work for each
/// dispatched job while weight remains reserved until completion.
pub fn dispatch_pending(app: &mut crate::app::App) -> Task<crate::app::Message> {
    let mut tasks: Vec<Task<crate::app::Message>> = Vec::new();
    loop {
        // Find best fitting pending job in priority order; break if none fits.
        let candidate = { app.engines.queue.peek_dispatchable() };
        let Some(_) = candidate else { break };
        // reserve via priority pop
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
                dispatch_inpaint(app, tab_id, easyscanlate_settings::InpaintBackend::Telea)
            }
            EngineKind::InpaintLama => {
                dispatch_inpaint(app, tab_id, easyscanlate_settings::InpaintBackend::Lama)
            }
            EngineKind::InpaintAot => {
                dispatch_inpaint(app, tab_id, easyscanlate_settings::InpaintBackend::Aot)
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
    // Manual OCR has priority if pending_manual_multi_ocr exists (FIFO insertion + priority scan dispatched this job)
    let idx_opt = app.tabs.iter().position(|t| t.id == tab_id);
    if let Some(idx) = idx_opt {
        if app.tabs[idx].pending_manual_multi_ocr.is_some() {
            let data = app.tabs[idx].pending_manual_multi_ocr.take().unwrap();
            let cached = app.engines.manual_ocr.clone();
            if let Some(engine) = cached {
                return crate::app::ocr::start_manual_ocr_selection_for(app, tab_id, data, engine);
            } else {
                let cfg = easyscanlate_settings::get(|s| easyscanlate_ocr::config_with(0.0, s.ocr_max_side_len.trim().parse::<u32>().unwrap_or(2000)));
                app.tabs[idx].pending_manual_multi_ocr = Some(data);
                app.tabs[idx].status = "Loading OCR engine for manual OCR…".to_string();
                let tid = tab_id;
                return Task::perform(
                    async move { easyscanlate_ocr::Engine::build_with_config(cfg) },
                    move |res| crate::app::Message::Tab(tid, crate::app::TabMessage::ManualOcrEngineReady(res)),
                );
            }
        }
    }
    // otherwise pipeline OCR
    if app.engines.pipeline.is_some() {
        return crate::app::ocr::maybe_start_ocr_for(app, tab_id);
    }
    // need to build pipeline
    let (workers, cfg) = easyscanlate_settings::get(|s| {
        let workers = s.ocr_workers.parse::<usize>().unwrap_or(2).max(1);
        let cfg = easyscanlate_ocr::config_from_strings(&s.ocr_text_score, &s.ocr_max_side_len);
        (workers, cfg)
    });
    let tid = tab_id;
    Task::perform(
        async move { easyscanlate_ocr::ParallelEngine::build_with_config(cfg, workers) },
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
    backend: easyscanlate_settings::InpaintBackend,
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
            easyscanlate_settings::InpaintBackend::Telea => app.tabs[idx].pending_auto_telea_jobs.clone(),
            easyscanlate_settings::InpaintBackend::Lama => app.tabs[idx].pending_auto_lama_jobs.clone(),
            easyscanlate_settings::InpaintBackend::Aot => app.tabs[idx].pending_auto_aot_jobs.clone(),
        };
        if let Some(jobs) = jobs_opt {
            // Clear tab pending so re-entrance doesn't duplicate; queue holds running
            match backend {
                easyscanlate_settings::InpaintBackend::Telea => app.tabs[idx].pending_auto_telea_jobs = None,
                easyscanlate_settings::InpaintBackend::Lama => app.tabs[idx].pending_auto_lama_jobs = None,
                easyscanlate_settings::InpaintBackend::Aot => app.tabs[idx].pending_auto_aot_jobs = None,
            }
            return crate::app::inpaint::dispatch_auto_for(app, tab_id, jobs, backend);
        }
        // Fallback: manual inpaint queued via InpaintTelea/Lama/Aot kind
        if app.tabs[idx].pending_manual_multi.is_some() {
            let data = app.tabs[idx].pending_manual_multi.take().unwrap();
            // Need to dispatch manual inpaint now that weight is reserved (queue already popped to running)
            // Check cached engine for this backend; if not cached, build it (weight remains reserved)
            let radius = easyscanlate_settings::get(|s| s.inpaint_radius.parse::<i32>().unwrap_or(5).max(1));
            let cached = app.engines.inpaint.clone().filter(|e| e.backend() == backend && e.radius() == radius);
            if let Some(engine) = cached {
                // use helper that takes tab_id-aware start
                return crate::app::inpaint::start_inpaint_selection_for(app, tab_id, engine, data);
            } else {
                // store back for engine-ready path and build
                app.tabs[idx].pending_manual_multi = Some(data);
                app.tabs[idx].status = match backend {
                    easyscanlate_settings::InpaintBackend::Lama => "Loading LaMa model...".to_string(),
                    easyscanlate_settings::InpaintBackend::Aot => "Loading AOT-GAN model...".to_string(),
                    easyscanlate_settings::InpaintBackend::Telea => "Inpainting...".to_string(),
                };
                let tid = tab_id;
                return Task::perform(
                    async move { easyscanlate_inpaint::Engine::build(backend, radius) },
                    move |res| crate::app::Message::Tab(tid, crate::app::TabMessage::InpaintEngineReady(res)),
                );
            }
        }
        // Also handle background stitch pending (single inpaint queued)
        if app.tabs[idx].pending_background_stitch.is_some() {
            let (job, pad, prev, next) = app.tabs[idx].pending_background_stitch.take().unwrap();
            let radius = easyscanlate_settings::get(|s| s.inpaint_radius.parse::<i32>().unwrap_or(5).max(1));
            let cached = app.engines.inpaint.clone().filter(|e| e.backend() == backend && e.radius() == radius);
            if let Some(engine) = cached {
                return crate::app::inpaint::start_background_stitch_for(app, tab_id, engine, job, pad, prev, next);
            } else {
                app.tabs[idx].pending_background_stitch = Some((job, pad, prev, next));
                app.tabs[idx].status = match backend {
                    easyscanlate_settings::InpaintBackend::Lama => "Loading LaMa model...".to_string(),
                    easyscanlate_settings::InpaintBackend::Aot => "Loading AOT-GAN model...".to_string(),
                    easyscanlate_settings::InpaintBackend::Telea => "Inpainting background...".to_string(),
                };
                let tid = tab_id;
                return Task::perform(
                    async move { easyscanlate_inpaint::Engine::build(backend, radius) },
                    move |res| crate::app::Message::Tab(tid, crate::app::TabMessage::InpaintEngineReady(res)),
                );
            }
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
    fn priorities_match_spec() {
        assert_eq!(EngineKind::InpaintTelea.priority(), 0);
        assert_eq!(EngineKind::Style.priority(), 1);
        assert_eq!(EngineKind::Segment.priority(), 2);
        assert_eq!(EngineKind::Ocr.priority(), 3);
        assert_eq!(EngineKind::InpaintAot.priority(), 4);
        assert_eq!(EngineKind::InpaintLama.priority(), 5);
    }

    #[test]
    fn capacity_is_hard_4() {
        assert_eq!(POOL_CAPACITY, 5);
    }

    #[test]
    fn backfill_telea_behind_lama_when_capacity_allows() {
        let mut q = EngineQueue::new();
        // Simulate used=3 (e.g. Style w2 + Telea w1 already running)
        // We do this by directly dispatching then enqueueing more.
        // Simpler: enqueue Lama w4 first (prio5), then Telea w1 (prio0), with used=2.
        let s = q.enqueue(tid(10), EngineKind::Style); // w2
        let d = q.try_pop_dispatchable().unwrap();
        assert_eq!(d.id, s.id);
        assert_eq!(q.used_weight(), 2);
        // Now pending: Lama w4 (does not fit 2+4>4), Telea w1 (fits 2+1<=4)
        // FIFO insertion order: lama first, telea second
        let lama = q.enqueue(tid(1), EngineKind::InpaintLama);
        let telea = q.enqueue(tid(2), EngineKind::InpaintTelea);
        assert_eq!(q.pending_len(), 2);
        // Backfill should pick telea (prio0) before lama (prio5) even though lama arrived first
        let d2 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d2.id, telea.id, "backfill must prioritize telea over lama");
        assert_eq!(q.used_weight(), 3);
        // lama still pending, now 3+4>4 cannot dispatch
        assert!(q.try_pop_dispatchable().is_none());
        assert_eq!(q.position(lama.id), Some(1));
        assert_eq!(q.position(telea.id), None); // already running
        q.complete_by_id(telea.id);
        assert_eq!(q.used_weight(), 2);
        // lama still doesn't fit 2+4>4
        assert!(q.try_pop_dispatchable().is_none());
        q.complete_by_id(s.id);
        assert_eq!(q.used_weight(), 0);
        let d3 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d3.id, lama.id);
    }

    #[test]
    fn priority_scan_telea_before_style_before_segment() {
        let mut q = EngineQueue::new();
        // Enqueue in reverse priority order to ensure scan reorders
        let lama = q.enqueue(tid(1), EngineKind::InpaintLama); // 5
        let ocr = q.enqueue(tid(3), EngineKind::Ocr); // 3
        let seg = q.enqueue(tid(4), EngineKind::Segment); // 2
        let style = q.enqueue(tid(5), EngineKind::Style); // 1
        let telea = q.enqueue(tid(6), EngineKind::InpaintTelea); // 0
        assert_eq!(q.pending_len(), 5);
        // With used=0, first dispatch should be telea (prio0)
        let d1 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d1.id, telea.id);
        assert_eq!(q.used_weight(), 1);
        // Next best fitting is style w2 (1+2=3 fits), next after that would be lama? No lama w4 would be 1+2+4>5, seg w4 also > etc.
        let d2 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d2.id, style.id);
        assert_eq!(q.used_weight(), 3);
        // remaining 2, only w2 or w1 would fit but remaining pending are w4 -> none
        assert!(q.try_pop_dispatchable().is_none());
        // Free telea
        q.complete_by_id(telea.id);
        assert_eq!(q.used_weight(), 2);
        // Now remaining 3, but pending are w4 (seg/lama/ocr) -> none fits (2+4>5)
        assert!(q.try_pop_dispatchable().is_none());
        q.complete_by_id(style.id);
        assert_eq!(q.used_weight(), 0);
        // Now first should be seg prio2 (before ocr)
        let d3 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d3.id, seg.id);
        assert_eq!(d3.kind, EngineKind::Segment);
        let _ = (lama, ocr);
    }

    #[test]
    fn fifo_tie_break_within_same_priority() {
        let mut q = EngineQueue::new();
        let t1 = q.enqueue(tid(1), EngineKind::InpaintTelea);
        let t2 = q.enqueue(tid(2), EngineKind::InpaintTelea);
        let t3 = q.enqueue(tid(3), EngineKind::InpaintTelea);
        // All same prio0, should dispatch FIFO
        let d1 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d1.id, t1.id);
        let d2 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d2.id, t2.id);
        let d3 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d3.id, t3.id);
    }

    #[test]
    fn weight_packing_with_priority() {
        let mut q = EngineQueue::new();
        // Enqueue 2 styles w2 and one telea w1 in FIFO order
        // Pending [Style, Style, Telea] -> priority scan picks Telea first
        // With capacity 5, all three fit: 1+2+2=5
        let s1 = q.enqueue(tid(1), EngineKind::Style); // w2 prio1
        let s2 = q.enqueue(tid(2), EngineKind::Style); // w2 prio1
        let t1 = q.enqueue(tid(3), EngineKind::InpaintTelea); // w1 prio0
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
            ids.push(q.enqueue(tid(i), EngineKind::InpaintTelea));
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
        let lama = q.enqueue(tid(1), EngineKind::InpaintLama); // prio5
        let style = q.enqueue(tid(2), EngineKind::Style); // prio1
        // Used 0, style should be picked before lama despite FIFO
        let d1 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d1.id, style.id);
        assert_eq!(q.used_weight(), 2);
        // lama w4 doesn't fit 2+4>5? actually 2+4=6>5 blocked
        assert!(q.try_pop_dispatchable().is_none());
        q.complete_by_id(style.id);
        let d2 = q.try_pop_dispatchable().unwrap();
        assert_eq!(d2.id, lama.id);
        assert_eq!(q.running_for(tid(1), EngineKind::InpaintLama).is_some(), true);
        let _ = lama;
    }

    #[test]
    fn try_acquire_backfill_immediate() {
        let mut q = EngineQueue::new();
        // Acquire a Style w2 directly
        let a = q.try_acquire_or_enqueue(tid(1), EngineKind::Style);
        assert!(matches!(a, AcquireResult::Acquired(_)));
        assert_eq!(q.used_weight(), 2);
        // Enqueue Lama w4 (doesn't fit 2+4>5? 6>5) -> Queued
        let lama_res = q.try_acquire_or_enqueue(tid(2), EngineKind::InpaintLama);
        assert!(matches!(lama_res, AcquireResult::Queued(_, _)));
        assert_eq!(q.pending_len(), 1);
        // Try to acquire Telea w1 (fits 2+1<=5) -> should backfill Acquired even though pending not empty
        let telea_res = q.try_acquire_or_enqueue(tid(3), EngineKind::InpaintTelea);
        assert!(matches!(telea_res, AcquireResult::Acquired(_)), "telea should backfill when lama head blocked");
        assert_eq!(q.used_weight(), 3);
        assert_eq!(q.pending_len(), 1); // lama still pending
        // Next Telea w1 fits 3+1=4 -> Acquired (4<=5)
        let telea2 = q.try_acquire_or_enqueue(tid(4), EngineKind::InpaintTelea);
        assert!(matches!(telea2, AcquireResult::Acquired(_)));
        assert_eq!(q.used_weight(), 4);
        // Next Telea w1 fits 4+1=5 -> Acquired
        let telea3 = q.try_acquire_or_enqueue(tid(5), EngineKind::InpaintTelea);
        assert!(matches!(telea3, AcquireResult::Acquired(_)));
        assert_eq!(q.used_weight(), 5);
        // Next Style w2 would not fit 5+2>5
        let s = q.try_acquire_or_enqueue(tid(6), EngineKind::Style);
        assert!(matches!(s, AcquireResult::Queued(_, _)));
    }

    #[test]
    fn try_acquire_respects_higher_priority_pending() {
        let mut q = EngineQueue::new();
        // Fill used 2 with one style running
        let _ = q.try_acquire_or_enqueue(tid(1), EngineKind::Style);
        // Enqueue Style w2 pending (fits 2+2<=4 but we force queued by simulating? Instead enqueue directly via enqueue to create pending fitting job
        let style_pending = q.enqueue(tid(2), EngineKind::Style); // w2 prio1, pending len 1, would fit 2+2<=4 but not dispatched because we used enqueue not dispatch
        assert_eq!(q.pending_len(), 1);
        // Now try to acquire Lama w4 (prio5) which does not fit (2+4>4) -> Queued
        // But also try to acquire Aot w3 (prio4) which does not fit either? 2+3>4 so queued
        // Now try to acquire Telea w1 (prio0) which fits 2+1<=4, but there exists higher priority pending Style w2 prio1 that also fits 2+2<=4 (actually style_pending fits!)
        // Our try_acquire should detect that pending Style prio1 is higher priority than new Telea? No Telea prio0 is HIGHER than Style prio1 (0<1)
        // So Telea should still be allowed to backfill before Style? But Style arrived earlier and also fits, and has prio1 vs Telea prio0, Telea higher so it should go first - that's correct priority scan.
        // Now test opposite: pending has Telea prio0 fitting, new Style prio1 also fits but lower priority -> new should be queued, pending Telea should stay
        let mut q2 = EngineQueue::new();
        let _ = q2.try_acquire_or_enqueue(tid(10), EngineKind::Segment); // w4 used4? Actually Segment w4
        // Need used 0 for clean test: simpler scenario
        let mut q3 = EngineQueue::new();
        let _ = q3.enqueue(tid(20), EngineKind::Style); // pending style prio1 w2
        // used 0, remaining 4, style fits
        // Now try to acquire Lama prio5 w4 which also fits remaining 4, but style pending prio1 higher than lama prio5, so lama should be queued not acquired
        let lama_q = q3.try_acquire_or_enqueue(tid(21), EngineKind::InpaintLama);
        assert!(matches!(lama_q, AcquireResult::Queued(_, _)), "lama lower priority than pending style should be queued");
        // Whereas telea prio0 higher than style prio1 should be acquired
        let telea_a = q3.try_acquire_or_enqueue(tid(22), EngineKind::InpaintTelea);
        // Telea prio0 < style prio1, so telea should be acquired even though style pending exists
        assert!(matches!(telea_a, AcquireResult::Acquired(_)), "higher priority telea should backfill ahead of style");
        let _ = style_pending;
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
