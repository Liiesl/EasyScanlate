//! App adapter over the unified engine pool (iced `Task` mapping).
//!
//! Queue accounting (`EngineQueue`), job vocabulary (`JobKind`, `QueuedJob`,
//! `OwnerId`) and cached engines (`EnginePool`) live in
//! `easyscanlate-engine-pool` (UI-agnostic). This module re-exports them,
//! converts `TabId` <-> `OwnerId` at the boundary, and owns the only
//! iced-aware piece: `dispatch_pending` / `refresh_queued_statuses` which turn
//! dispatched jobs into `Task<Message>`.

use iced::Task;

use super::tab::TabId;

pub use easyscanlate_engine_pool::{
    AcquireResult, BuiltEngine, EngineOutcome, JobKind, OcrMode, OwnerId, QueuedJob,
    POOL_CAPACITY,
};

/// Build a unified engine-pool completion message for `tid`.
/// Callers capture `job.id` / `job.owner` / `job.kind` (or the `Acquired(job)`)
/// so the outcome correlates with the queue slot for `complete_by_id`.
pub fn engine_ready_msg(
    tid: TabId,
    job_id: u64,
    kind: JobKind,
    result: Result<BuiltEngine, String>,
) -> crate::app::Message {
    crate::app::Message::Tab(
        tid,
        crate::app::TabMessage::EnginePool(EngineOutcome::EngineReady {
            job_id,
            owner: owner_of(tid),
            kind,
            result,
        }),
    )
}

/// `TabId` -> pool owner.
pub fn owner_of(tab_id: TabId) -> OwnerId {
    OwnerId(tab_id.0)
}

/// Pool owner -> `TabId`.
pub fn tab_of(owner: OwnerId) -> TabId {
    TabId(owner.0)
}

/// Refresh status text for all tabs that are queued (pending). Call after
/// any queue mutation (complete / dispatch) to keep UI in sync. This helper
/// avoids borrow-checker conflicts that arise from iterating `pending_jobs()`
/// while mutably borrowing `app.tabs`.
pub fn refresh_queued_statuses(app: &mut crate::app::App) {
    let pending = app.engines.queue.pending_jobs();
    let used = app.engines.queue.used_weight();
    for (pos, job) in pending.iter().enumerate() {
        let tab_id = tab_of(job.owner);
        let idx_opt = app.tabs.iter().position(|t| t.id == tab_id);
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
        let tab_id = tab_of(job.owner);
        let idx_opt = app.tabs.iter().position(|t| t.id == tab_id);
        if idx_opt.is_none() {
            // tab closed while queued
            app.engines.queue.complete_by_id(job.id);
            continue;
        }
        let idx = idx_opt.unwrap();
        // update status to running
        app.tabs[idx].status = format!(
            "{} running (pool {}/{})",
            job.kind.label(),
            app.engines.queue.used_weight(),
            POOL_CAPACITY
        );
        let task = match job.kind {
            JobKind::Ocr(_) => dispatch_ocr(app, tab_id, &job),
            JobKind::Segment => dispatch_segment(app, tab_id, &job),
            JobKind::Styling => dispatch_style(app, tab_id, &job),
            JobKind::Inpaint(backend) => dispatch_inpaint(app, tab_id, &job, backend),
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
fn dispatch_ocr(
    app: &mut crate::app::App,
    tab_id: super::tab::TabId,
    job: &QueuedJob,
) -> Task<crate::app::Message> {
    let job_id = job.id;
    let kind = job.kind;
    // Authoritative mode comes from the queued job, not tab-state sniffing.
    let is_manual = matches!(kind, JobKind::Ocr(OcrMode::Manual));
    if is_manual {
        let idx_opt = app.tabs.iter().position(|t| t.id == tab_id);
        if let Some(idx) = idx_opt
            && app.tabs[idx].pending_manual_multi_ocr.is_some()
        {
            let data = app.tabs[idx].pending_manual_multi_ocr.take().unwrap();
            let cached = app.engines.manual_ocr.clone();
            if let Some(engine) = cached {
                return crate::app::ocr::start_manual_ocr_selection(app, tab_id, data, engine);
            } else {
                let cfg = easyscanlate_settings::get(|s| {
                    easyscanlate_ocr::config_with(
                        0.0,
                        s.ocr_max_side_len.trim().parse::<u32>().unwrap_or(2000),
                    )
                });
                app.tabs[idx].pending_manual_multi_ocr = Some(data);
                // Model load counts as the run itself so buttons disable during it.
                app.tabs[idx].manual_ocring = true;
                app.tabs[idx].status = "Loading OCR engine for manual OCR…".to_string();
                let tid = tab_id;
                return Task::perform(
                    async move {
                        match easyscanlate_ocr::Engine::build_with_config(cfg) {
                            Ok(engine) => {
                                engine_ready_msg(tid, job_id, kind, Ok(BuiltEngine::OcrManual(engine)))
                            }
                            Err(e) => engine_ready_msg(tid, job_id, kind, Err(e)),
                        }
                    },
                    |msg| msg,
                );
            }
        }
        // Manual job with no pending payload (e.g. tab closed between enqueue
        // and dispatch): release is handled by completion paths; nothing to do.
        return Task::none();
    }
    // otherwise pipeline OCR
    if app.engines.pipeline.is_some() {
        return crate::app::ocr::maybe_start_ocr(app, tab_id);
    }
    // need to build pipeline
    let (workers, cfg) = easyscanlate_settings::get(|s| {
        let workers = s.ocr_workers.parse::<usize>().unwrap_or(2).max(1);
        let cfg = easyscanlate_ocr::config_from_strings(&s.ocr_text_score, &s.ocr_max_side_len);
        (workers, cfg)
    });
    let tid = tab_id;
    Task::perform(
        async move {
            match easyscanlate_ocr::ParallelEngine::build_with_config(cfg, workers) {
                Ok(engine) => engine_ready_msg(tid, job_id, kind, Ok(BuiltEngine::OcrParallel(engine))),
                Err(e) => engine_ready_msg(tid, job_id, kind, Err(e)),
            }
        },
        |msg| msg,
    )
}

#[cfg(not(feature = "ocr"))]
fn dispatch_ocr(
    _app: &mut crate::app::App,
    _tab_id: super::tab::TabId,
    _job: &QueuedJob,
) -> Task<crate::app::Message> {
    Task::none()
}

fn dispatch_segment(
    app: &mut crate::app::App,
    tab_id: super::tab::TabId,
    _job: &QueuedJob,
) -> Task<crate::app::Message> {
    crate::app::segment::start_segment_filter(app, tab_id)
}

fn dispatch_style(
    app: &mut crate::app::App,
    tab_id: super::tab::TabId,
    _job: &QueuedJob,
) -> Task<crate::app::Message> {
    #[cfg(feature = "styling")]
    {
        crate::app::styling::classify(app, tab_id)
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
    job: &QueuedJob,
    backend: easyscanlate_settings::InpaintBackend,
) -> Task<crate::app::Message> {
    let job_id = job.id;
    let kind = job.kind;
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
            return crate::app::inpaint::dispatch_auto(app, tab_id, jobs, backend);
        }
        // Fallback: manual inpaint queued via Inpaint(backend) kind
        if app.tabs[idx].pending_manual_multi.is_some() {
            let data = app.tabs[idx].pending_manual_multi.take().unwrap();
            // Need to dispatch manual inpaint now that weight is reserved (queue already popped to running)
            // Check cached engine for this backend; if not cached, build it (weight remains reserved)
            let radius = easyscanlate_settings::get(|s| s.inpaint_radius.parse::<i32>().unwrap_or(5).max(1));
            let cached = app.engines.inpaint.clone().filter(|e| e.backend() == backend && e.radius() == radius);
            if let Some(engine) = cached {
                // use helper that takes tab_id-aware start
                return crate::app::inpaint::start_inpaint_selection(app, tab_id, engine, data);
            } else {
                // store back for engine-ready path and build
                app.tabs[idx].pending_manual_multi = Some(data);
                // Model load counts as the run itself so buttons disable during it.
                app.tabs[idx].inpainting = true;
                app.tabs[idx].status = match backend {
                    easyscanlate_settings::InpaintBackend::Lama => "Loading LaMa model...".to_string(),
                    easyscanlate_settings::InpaintBackend::Aot => "Loading AOT-GAN model...".to_string(),
                    easyscanlate_settings::InpaintBackend::Telea => "Inpainting...".to_string(),
                };
                let tid = tab_id;
                return Task::perform(
                    async move {
                        match easyscanlate_inpaint::Engine::build(backend, radius) {
                            Ok(engine) => engine_ready_msg(
                                tid,
                                job_id,
                                kind,
                                Ok(BuiltEngine::Inpaint { backend, engine }),
                            ),
                            Err(e) => engine_ready_msg(tid, job_id, kind, Err(e)),
                        }
                    },
                    |msg| msg,
                );
            }
        }
        // Also handle background stitch pending (single inpaint queued)
        if app.tabs[idx].pending_background_stitch.is_some() {
            let (job, pad, prev, next) = app.tabs[idx].pending_background_stitch.take().unwrap();
            let radius = easyscanlate_settings::get(|s| s.inpaint_radius.parse::<i32>().unwrap_or(5).max(1));
            let cached = app.engines.inpaint.clone().filter(|e| e.backend() == backend && e.radius() == radius);
            if let Some(engine) = cached {
                return crate::app::inpaint::start_background_stitch(app, tab_id, engine, job, pad, prev, next);
            } else {
                app.tabs[idx].pending_background_stitch = Some((job, pad, prev, next));
                // Model load counts as the run itself so buttons disable during it.
                app.tabs[idx].inpainting = true;
                app.tabs[idx].status = match backend {
                    easyscanlate_settings::InpaintBackend::Lama => "Loading LaMa model...".to_string(),
                    easyscanlate_settings::InpaintBackend::Aot => "Loading AOT-GAN model...".to_string(),
                    easyscanlate_settings::InpaintBackend::Telea => "Inpainting background...".to_string(),
                };
                let tid = tab_id;
                return Task::perform(
                    async move {
                        match easyscanlate_inpaint::Engine::build(backend, radius) {
                            Ok(engine) => engine_ready_msg(
                                tid,
                                job_id,
                                kind,
                                Ok(BuiltEngine::Inpaint { backend, engine }),
                            ),
                            Err(e) => engine_ready_msg(tid, job_id, kind, Err(e)),
                        }
                    },
                    |msg| msg,
                );
            }
        }
        Task::none()
    }
    #[cfg(not(feature = "inpaint"))]
    {
        let _ = (app, tab_id, job, backend);
        return Task::none();
    }
}
