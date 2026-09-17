use iced::Task;
#[cfg(feature = "ocr")]
use iced::futures::{SinkExt, StreamExt};
use easyscanlate_model::{NewEntry, Quad};
// Needed only by the `not(ocr)` fake-OCR fallback (`is_bulk_busy` via `UiState`).
#[cfg(not(feature = "ocr"))]
use easyscanlate_ui::UiState;
#[cfg(feature = "ocr")]
use easyscanlate_ocr::{self as ocr, ParallelEngine};

use super::{App, Message};
use crate::app::queue::owner_of;

#[cfg(feature = "ocr")]
pub fn start_ocr_stream(app: &mut App, tab_id: super::tab::TabId) -> Task<Message> {
    let pipeline = app
        .engines
        .pipeline
        .clone()
        .expect("pipeline must be built before starting the stream");
    let tab = app.tab_by_id(tab_id).expect("tab must exist for ocr stream");
    let token = tab
        .cancel
        .clone()
        .expect("cancellation token set before starting the stream");
    let runs = tab.ocr_plans.clone();
    let dims = tab.ocr_dims.clone();
    let paths: Vec<Vec<String>> = runs
        .iter()
        .map(|run| {
            (run.page_start..=run.page_end)
                .map(|i| {
                    tab.project
                        .image(tab.images[i].image_id)
                        .map(|m| m.path.clone())
                        .unwrap_or_default()
                })
                .collect()
        })
        .collect();
    let above_paths: Vec<Option<String>> = runs
        .iter()
        .map(|run| {
            run.above.map(|(page, _)| {
                tab.project
                    .image(tab.images[page].image_id)
                    .map(|m| m.path.clone())
                    .unwrap_or_default()
            })
        })
        .collect();
    let below_paths: Vec<Option<String>> = runs
        .iter()
        .map(|run| {
            run.below.map(|(page, _)| {
                tab.project
                    .image(tab.images[page].image_id)
                    .map(|m| m.path.clone())
                    .unwrap_or_default()
            })
        })
        .collect();
    let workers = easyscanlate_settings::get(|s| s.ocr_workers.parse::<usize>().unwrap_or(2)).max(1);
    let mut session = ocr::RunSession::new(runs, dims, paths, above_paths, below_paths, workers);
    Task::stream(
        iced::stream::try_channel(1, move |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
            while let Some(event) = session.step(&pipeline, &token)? {
                if sender
                    .send(Message::Tab(tab_id, crate::app::TabMessage::OcrStreamRun(Ok::<ocr::RunEvent, String>(event))))
                    .await
                    .is_err()
                {
                    return Ok(());
                }
            }
            Ok(())
        })
        .map(move |item| match item {
            Ok(message) => message,
            Err(e) => Message::Tab(tab_id, crate::app::TabMessage::OcrStreamFailed(e)),
        }),
    )
}

#[cfg(feature = "ocr")]
pub fn maybe_start_ocr(app: &mut App, tab_id: super::tab::TabId) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    if app.tabs[idx].running && app.engines.pipeline.is_some() {
        let token = app.engines.pipeline.as_ref().map(|pipeline| pipeline.cancellation_token().clone());
        app.tabs[idx].cancel = token;
        start_ocr_stream(app, tab_id)
    } else if !app.tabs[idx].running {
        if let Some(pipeline) = app.engines.pipeline.take() {
            pipeline.cancel();
        }
        Task::none()
    } else {
        Task::none()
    }
}

pub fn handle_start_ocr(app: &mut App) -> Task<Message> {
    #[cfg(feature = "ocr")]
    {
        if app.active_tab_mut().images.is_empty() {
            app.active_tab_mut().status = "Open images first.".to_string();
            return Task::none();
        }
        let tab_id = app.active_tab().id;
        // per-tab guard: same tab already busy or already queued/running in global pool
        let already_busy = {
            let tab = app.tab_by_id(tab_id).unwrap();
            tab.running
                || app.engines.queue.is_tab_queued(owner_of(tab_id))
                || app.engines.queue.is_tab_running(owner_of(tab_id))
                || {
                    // also check is_bulk_busy for this tab (inpaint/style/segment etc)
                    let idx = app.tabs.iter().position(|t| t.id == tab_id).unwrap();
                    let t = &app.tabs[idx];
                    t.translating
                        || t.inpainting
                        || {
                            #[cfg(feature = "ocr")]
                            { t.manual_ocring }
                            #[cfg(not(feature = "ocr"))]
                            { false }
                        }
                        || {
                            #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
                            { t.pipeline_active }
                            #[cfg(not(all(feature = "styling", feature = "inpaint", feature = "segment")))]
                            {
                                #[cfg(all(feature = "styling", feature = "inpaint"))]
                                { t.pipeline_style_pending > 0 }
                                #[cfg(not(all(feature = "styling", feature = "inpaint")))]
                                { false }
                            }
                        }
                        || {
                            #[cfg(feature = "inpaint")]
                            { t.auto_inpaint_pending > 0 }
                            #[cfg(not(feature = "inpaint"))]
                            { false }
                        }
                        || {
                            #[cfg(feature = "segment")]
                            { t.segment_filtering }
                            #[cfg(not(feature = "segment"))]
                            { false }
                        }
                        || {
                            #[cfg(feature = "styling")]
                            { t.pipeline_style_pending > 0 || app.engines.is_styling_building() }
                            #[cfg(not(feature = "styling"))]
                            { false }
                        }
                }
        };
        if already_busy {
            if !app.tab_by_id(tab_id).unwrap().running {
                app.tab_by_id_mut(tab_id).unwrap().status = "Wait for current task to finish.".to_string();
            }
            return Task::none();
        }
        // prepare OCR plans (must be done before queue decision so dispatch can use them)
        let dims: Vec<(u32, u32)> = {
            let tab = app.tab_by_id(tab_id).unwrap();
            tab.images
                .iter()
                .map(|image| {
                    tab.project
                        .image(image.image_id)
                        .map(|m| (m.width as u32, m.height as u32))
                        .unwrap_or((0, 0))
                })
                .collect()
        };
        let runs = ocr::plan_runs(&dims);
        let run_count = runs.len();
        {
            let tab = app.tab_by_id_mut(tab_id).unwrap();
            tab.running = true;
            tab.ocr_plans = runs;
            tab.ocr_dims = dims;
            tab.ocr_runs = run_count;
            tab.pending = run_count;
            tab.ocr_total = 0;
            tab.ocr_failed = 0;
            tab.ocr_cancelled = false;
            tab.held_boundary = None;
            #[cfg(feature = "segment")]
            {
                tab.pipeline_seg_done = false;
                tab.segment_pending = 0;
                tab.segment_total = 0;
                tab.segment_failed = 0;
                tab.segment_removed = 0;
            }
            #[cfg(feature = "inpaint")]
            {
                tab.auto_inpaint_pending = 0;
                tab.auto_inpaint_total = 0;
                tab.auto_inpaint_failed = 0;
                tab.manual_inpaint_pending = 0;
                tab.manual_inpaint_total = 0;
                tab.manual_inpaint_failed = 0;
            }
        }
        // queue gate — weight 4, backfill + priority (cap 5)
        use crate::app::queue::{AcquireResult, JobKind, OcrMode, engine_ready_msg, owner_of};
        use easyscanlate_engine_pool::BuiltEngine;
        match app.engines.queue.try_acquire_or_enqueue(owner_of(tab_id), JobKind::Ocr(OcrMode::Auto)) {
            AcquireResult::Acquired(job) => {
                let job_id = job.id;
                let kind = job.kind;
                let idx = app.tabs.iter().position(|t| t.id == tab_id).unwrap();
                app.tabs[idx].status = format!(
                    "OCR running (pool {}/{}) on {} run(s)...",
                    app.engines.queue.used_weight(),
                    crate::app::queue::POOL_CAPACITY,
                    run_count
                );
                if app.engines.pipeline.is_none() {
                    let (workers, cfg) = easyscanlate_settings::get(|s| {
                        let workers = s.ocr_workers.parse::<usize>().unwrap_or(2).max(1);
                        let cfg = ocr::config_from_strings(&s.ocr_text_score, &s.ocr_max_side_len);
                        (workers, cfg)
                    });
                    app.tabs[idx].status =
                        format!("Loading the OCR engine ({workers} detection worker(s))... pool {}/{}", app.engines.queue.used_weight(), crate::app::queue::POOL_CAPACITY);
                    let tid = tab_id;
                    return Task::perform(
                        async move {
                            match ParallelEngine::build_with_config(cfg, workers) {
                                Ok(engine) => engine_ready_msg(
                                    tid,
                                    job_id,
                                    kind,
                                    Ok(BuiltEngine::OcrParallel(engine)),
                                ),
                                Err(e) => engine_ready_msg(tid, job_id, kind, Err(e)),
                            }
                        },
                        |msg| msg,
                    );
                }
                maybe_start_ocr(app, tab_id)
            }
            AcquireResult::Queued(_, pos) => {
                let idx = app.tabs.iter().position(|t| t.id == tab_id).unwrap();
                app.tabs[idx].status = format!(
                    "Queued OCR (pos {}, pool {}/{}) — {} run(s) waiting...",
                    pos,
                    app.engines.queue.used_weight(),
                    crate::app::queue::POOL_CAPACITY,
                    run_count
                );
                Task::none()
            }
        }
    }
    #[cfg(not(feature = "ocr"))]
    {
        use super::boot::fake_ocr_entries;
        if app.active_tab_mut().images.is_empty() {
            app.active_tab_mut().status = "Open images first.".to_string();
            return Task::none();
        }
        if app.active_tab_mut().running || app.active_state().is_bulk_busy() {
            if !app.active_tab_mut().running {
                app.active_tab_mut().status = "Wait for current task to finish.".to_string();
            }
            return Task::none();
        }
        app.active_tab_mut().running = true;
        let mut added = 0;
        let image_ids: Vec<_> = app.active_tab_mut().images.iter().map(|i| i.image_id).collect();
        for image_id in image_ids {
            let entries = fake_ocr_entries();
            let cnt = entries.len();
            if let Some(ev) = app.active_tab_mut().project.append_ocr_for_image_with_event(image_id, entries) {
                if let easyscanlate_model::ModelEvent::EntriesAdded { ids, .. } = &ev { added += ids.len(); } else { added += cnt; }
                crate::app::handle_model_event(app.active_tab_mut(), ev);
            }
        }
        app.active_tab_mut().running = false;
        app.active_tab_mut().status = format!("Fake OCR done: {added} line(s) (no OCR engine in this build).");
        return Task::none();
    }
}

#[cfg(feature = "ocr")]
pub fn handle_parallel_ready(app: &mut App, tab_id: super::tab::TabId, result: Result<ParallelEngine, String>) -> Task<Message> {
    match result {
        Ok(pipeline) => {
            app.engines.pipeline = Some(pipeline.clone());
            maybe_start_ocr(app, tab_id)
        }
        Err(e) => {
            if let Some(tab) = app.tab_by_id_mut(tab_id) {
                tab.running = false;
                tab.status = e.clone();
            }
            // free queue weight (build failed) and promote pending
            app.engines.queue.complete_ocr(crate::app::queue::owner_of(tab_id));
            let promote = crate::app::queue::dispatch_pending(app);
            crate::app::queue::refresh_queued_statuses(app);
            promote
        }
    }
}

pub fn handle_stop_ocr(app: &mut App) -> Task<Message> {
    #[cfg(feature = "ocr")]
    {
        let tab_id = app.active_tab().id;
        if let Some(token) = &app.active_tab_mut().cancel { token.cancel(); }
        // free queue weight if running/queued
        let was_running = app.engines.queue.complete_ocr(crate::app::queue::owner_of(tab_id)).is_some();
        let was_queued = if app.engines.queue.is_tab_queued(owner_of(tab_id)) {
            let removed = app.engines.queue.cancel_pending_for_tab(owner_of(tab_id));
            !removed.is_empty()
        } else { false };
        if was_running || was_queued {
            let promote = crate::app::queue::dispatch_pending(app);
            app.active_tab_mut().running = false;
            app.active_tab_mut().status = "Cancelling OCR...".to_string();
            return promote;
        }
        app.active_tab_mut().running = false;
        app.active_tab_mut().status = "Cancelling OCR...".to_string();
        Task::none()
    }
    #[cfg(not(feature = "ocr"))]
    {
        app.active_tab_mut().status = "OCR is not available in this build.".to_string();
        return Task::none();
    }
}

#[cfg(feature = "ocr")]
pub fn handle_ocr_stream_run(app: &mut App, tab_id: super::tab::TabId, result: Result<ocr::RunEvent, String>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    app.tabs[idx].pending = app.tabs[idx].pending.saturating_sub(1);
    match result {
        Ok(ocr::RunEvent::Canvas {
            index,
            width,
            margin_top,
            lines,
        }) => {
            let run = app.tabs[idx].ocr_plans[index];
            let prev = run.dedup.map(|(page, offset)| {
                let tab = &app.tabs[idx];
                let image_id = tab.images[page].image_id;
                let quads: Vec<Quad> = tab
                    .project
                    .all_for(image_id)
                    .map(|entry| entry.quad)
                    .collect();
                let width = tab
                    .project
                    .image(image_id)
                    .map(|m| m.width as u32)
                    .unwrap_or(0);
                (quads, width, offset)
            });
            let (merge_cfg, min_h, max_h) = easyscanlate_settings::get(|s| {
                (
                    ocr::MergeConfig::from_threshold_str(&s.ocr_merge_threshold),
                    s.ocr_min_text_height
                        .trim()
                        .parse::<f32>()
                        .unwrap_or(40.0),
                    s.ocr_max_text_height
                        .trim()
                        .parse::<f32>()
                        .unwrap_or(100.0),
                )
            });
            let (plans, dims, held) = {
                let tab = &mut app.tabs[idx];
                (tab.ocr_plans.clone(), tab.ocr_dims.clone(), tab.held_boundary.take())
            };
            let run_result = ocr::assemble_with_config(
                index,
                width,
                margin_top,
                lines,
                &plans,
                &dims,
                held,
                prev,
                merge_cfg,
                min_h,
                max_h,
            );
            app.tabs[idx].held_boundary = run_result.held;
            // commit per page to this specific tab
            let per_page = run_result.per_page;
            for (page, entries) in per_page {
                let image_id = match app.tabs[idx].images.get(page).map(|im| im.image_id) { Some(id) => id, None => continue };
                let cnt = entries.len();
                if let Some(ev) = app.tabs[idx].project.append_ocr_for_image_with_event(image_id, entries) {
                    if let easyscanlate_model::ModelEvent::EntriesAdded { ids, .. } = &ev { app.tabs[idx].ocr_total += ids.len(); } else { app.tabs[idx].ocr_total += cnt; }
                    let ev_clone = ev;
                    crate::app::handle_model_event(&mut app.tabs[idx], ev_clone);
                }
            }
        }
        Err(e) => {
            app.tabs[idx].ocr_failed += 1;
            if e == "cancelled" {
                app.tabs[idx].ocr_cancelled = true;
            } else {
                // flush held boundary for this tab
                if let Some(state) = app.tabs[idx].held_boundary.take() {
                    for candidate in state.candidates {
                        let img_len = app.tabs[idx].images.len();
                        if candidate.page >= img_len { continue; }
                        let image_id = app.tabs[idx].images[candidate.page].image_id;
                        let cnt = 1;
                        if let Some(ev) = app.tabs[idx].project.append_ocr_for_image_with_event(image_id, vec![candidate.entry]) {
                            if let easyscanlate_model::ModelEvent::EntriesAdded { ids, .. } = &ev { app.tabs[idx].ocr_total += ids.len(); } else { app.tabs[idx].ocr_total += cnt; }
                            crate::app::handle_model_event(&mut app.tabs[idx], ev);
                        }
                    }
                }
            }
        }
    }
    // finalize and auto pipeline need tab-aware versions
    #[cfg_attr(not(any(feature = "styling", feature = "segment", feature = "inpaint")), allow(unused_mut))]
    let mut tasks: Vec<Task<Message>> = Vec::new();
    let pending = app.tabs[idx].pending;
    let cancelled = app.tabs[idx].ocr_cancelled;
    if pending == 0 || cancelled {
        // finalize for this tab
        {
            let tab = &mut app.tabs[idx];
            let held = tab.held_boundary.take();
            if let Some(state) = held {
                for candidate in state.candidates {
                    if candidate.page >= tab.images.len() { continue; }
                    let image_id = tab.images[candidate.page].image_id;
                    if let Some(ev) = tab.project.append_ocr_for_image_with_event(image_id, vec![candidate.entry]) {
                        if let easyscanlate_model::ModelEvent::EntriesAdded { ids, .. } = &ev { tab.ocr_total += ids.len(); } else { tab.ocr_total += 1; }
                        crate::app::handle_model_event(tab, ev);
                    }
                }
            }
            let (total, failed, cancelled) = (tab.ocr_total, tab.ocr_failed, tab.ocr_cancelled);
            tab.running = false;
            tab.cancel = None;
            tab.status = if cancelled { "OCR cancelled.".to_string() } else if failed>0 { format!("OCR done: {} line(s), {} run(s) failed.", total, failed) } else { format!("OCR done: {} line(s).", total) };
        }
        app.engines.pipeline = None;
        // free OCR weight and update queued positions
        app.engines.queue.complete_ocr(crate::app::queue::owner_of(tab_id));
        crate::app::queue::refresh_queued_statuses(app);
        let cancelled_now = app.tabs[idx].ocr_cancelled;
        if !cancelled_now {
            let (do_sfx, do_style, do_inpaint, model) = easyscanlate_settings::get(|s| {
                (s.auto_sfx_filter, s.auto_style_detect, s.auto_inpaint, s.auto_inpaint_model)
            });
            let effective_model = model;
            // Enqueue next pipeline stages via queue (backfill + priority, weight-checked)
            use crate::app::queue::{AcquireResult, JobKind, owner_of};
            if do_sfx {
                #[cfg(feature = "segment")]
                {
                    let need_chain = do_style || do_inpaint;
                    #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
                    {
                        if need_chain {
                            app.tabs[idx].pipeline_active = true;
                        }
                    }
                    match app.engines.queue.try_acquire_or_enqueue(owner_of(tab_id), JobKind::Segment) {
                        AcquireResult::Acquired(_) => {
                            tasks.push(super::segment::start_segment_filter(app, tab_id));
                        }
                        AcquireResult::Queued(_, pos) => {
                            let used = app.engines.queue.used_weight();
                            if let Some(t) = app.tab_by_id_mut(tab_id) {
                                t.status = format!(
                                    "Queued {} (pos {}, pool {}/{}) ...",
                                    JobKind::Segment.label(),
                                    pos,
                                    used,
                                    crate::app::queue::POOL_CAPACITY
                                );
                            }
                        }
                    }
                }
                #[cfg(not(feature = "segment"))]
                {
                    #[cfg(feature = "styling")]
                    if do_style && !do_inpaint {
                        match app.engines.queue.try_acquire_or_enqueue(owner_of(tab_id), JobKind::Styling) {
                            AcquireResult::Acquired(_) => tasks.push(super::styling::classify(app, tab_id)),
                            AcquireResult::Queued(_, pos) => {
                                let used = app.engines.queue.used_weight();
                                if let Some(t) = app.tab_by_id_mut(tab_id) {
                                    t.status = format!("Queued {} (pos {}, pool {}/{}) ...", JobKind::Styling.label(), pos, used, crate::app::queue::POOL_CAPACITY);
                                }
                            }
                        }
                    }
                    #[cfg(feature = "inpaint")]
                    if do_inpaint && !do_style {
                        let kind = match effective_model {
                            easyscanlate_settings::AutoInpaintModel::Telea => JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Telea),
                            easyscanlate_settings::AutoInpaintModel::Lama => JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Lama),
                            easyscanlate_settings::AutoInpaintModel::Aot => JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Aot),
                            easyscanlate_settings::AutoInpaintModel::Mixed => JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Harmonic),
                        };
                        match app.engines.queue.try_acquire_or_enqueue(owner_of(tab_id), kind) {
                            AcquireResult::Acquired(_) => tasks.push(super::inpaint::dispatch_auto_solo(app, tab_id, effective_model)),
                            AcquireResult::Queued(_, pos) => {
                                let used = app.engines.queue.used_weight();
                                if let Some(t) = app.tab_by_id_mut(tab_id) {
                                    t.status = format!("Queued {} (pos {}, pool {}/{}) ...", kind.label(), pos, used, crate::app::queue::POOL_CAPACITY);
                                }
                            }
                        }
                    }
                    #[cfg(all(feature = "styling", feature = "inpaint"))]
                    if do_style && do_inpaint {
                        match app.engines.queue.try_acquire_or_enqueue(owner_of(tab_id), JobKind::Styling) {
                            AcquireResult::Acquired(_) => tasks.push(super::styling::classify(app, tab_id)),
                            AcquireResult::Queued(_, pos) => {
                                let used = app.engines.queue.used_weight();
                                if let Some(t) = app.tab_by_id_mut(tab_id) {
                                    t.status = format!("Queued {} (pos {}, pool {}/{}) ...", JobKind::Styling.label(), pos, used, crate::app::queue::POOL_CAPACITY);
                                }
                            }
                        }
                    }
                }
            } else {
                #[cfg(all(feature = "styling", feature = "inpaint"))]
                if do_style && do_inpaint {
                    match app.engines.queue.try_acquire_or_enqueue(owner_of(tab_id), JobKind::Styling) {
                        AcquireResult::Acquired(_) => tasks.push(super::styling::classify(app, tab_id)),
                        AcquireResult::Queued(_, pos) => {
                                let used = app.engines.queue.used_weight();
                                if let Some(t) = app.tab_by_id_mut(tab_id) {
                                    t.status = format!("Queued {} (pos {}, pool {}/{}) ...", JobKind::Styling.label(), pos, used, crate::app::queue::POOL_CAPACITY);
                                }
                            }
                    }
                }
                #[cfg(feature = "styling")]
                if do_style && !do_inpaint {
                    match app.engines.queue.try_acquire_or_enqueue(owner_of(tab_id), JobKind::Styling) {
                        AcquireResult::Acquired(_) => tasks.push(super::styling::classify(app, tab_id)),
                        AcquireResult::Queued(_, pos) => {
                                let used = app.engines.queue.used_weight();
                                if let Some(t) = app.tab_by_id_mut(tab_id) {
                                    t.status = format!("Queued {} (pos {}, pool {}/{}) ...", JobKind::Styling.label(), pos, used, crate::app::queue::POOL_CAPACITY);
                                }
                            }
                    }
                }
                #[cfg(feature = "inpaint")]
                if do_inpaint && !do_style {
                    let kind = match effective_model {
                        easyscanlate_settings::AutoInpaintModel::Telea => JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Telea),
                        easyscanlate_settings::AutoInpaintModel::Lama => JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Lama),
                        easyscanlate_settings::AutoInpaintModel::Aot => JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Aot),
                        easyscanlate_settings::AutoInpaintModel::Mixed => JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Harmonic),
                    };
                    match app.engines.queue.try_acquire_or_enqueue(owner_of(tab_id), kind) {
                        AcquireResult::Acquired(_) => tasks.push(super::inpaint::dispatch_auto_solo(app, tab_id, effective_model)),
                        AcquireResult::Queued(_, pos) => {
                                let used = app.engines.queue.used_weight();
                                if let Some(t) = app.tab_by_id_mut(tab_id) {
                                    t.status = format!("Queued {} (pos {}, pool {}/{}) ...", kind.label(), pos, used, crate::app::queue::POOL_CAPACITY);
                                }
                            }
                    }
                }
            }
        }
        // also dispatch any other pending jobs that now fit (backfill priority scan)
        tasks.push(crate::app::queue::dispatch_pending(app));
    } else {
        let (runs, pending, total) = { let tab = &app.tabs[idx]; (tab.ocr_runs, tab.pending, tab.ocr_total) };
        app.tabs[idx].status = format!(
            "OCR in progress: {} of {} run(s) done ({} line(s)).",
            runs - pending,
            runs,
            total
        );
    }
    if tasks.is_empty() {
        Task::none()
    } else {
        Task::batch(tasks)
    }
}

#[cfg(feature = "ocr")]
pub fn handle_ocr_stream_failed(app: &mut App, tab_id: super::tab::TabId, e: String) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i)=>i, None=>return Task::none()};
    app.tabs[idx].ocr_failed += 1;
    if e == "cancelled" {
        app.tabs[idx].ocr_cancelled = true;
    } else {
        // flush
        if let Some(state) = app.tabs[idx].held_boundary.take() {
            for candidate in state.candidates {
                if candidate.page >= app.tabs[idx].images.len() { continue; }
                let image_id = app.tabs[idx].images[candidate.page].image_id;
                if let Some(ev) = app.tabs[idx].project.append_ocr_for_image_with_event(image_id, vec![candidate.entry]) {
                    if let easyscanlate_model::ModelEvent::EntriesAdded { ids, .. } = &ev { app.tabs[idx].ocr_total += ids.len(); } else { app.tabs[idx].ocr_total += 1; }
                    crate::app::handle_model_event(&mut app.tabs[idx], ev);
                }
            }
        }
    }
    if app.tabs[idx].pending > 0 {
        app.tabs[idx].pending = 0;
        // finalize
        let (total, failed, cancelled) = { let t=&app.tabs[idx]; (t.ocr_total, t.ocr_failed, t.ocr_cancelled) };
        app.tabs[idx].running = false;
        app.tabs[idx].cancel = None;
        app.engines.pipeline = None;
        app.tabs[idx].status = if cancelled { "OCR cancelled.".to_string() } else if failed>0 { format!("OCR done: {} line(s), {} run(s) failed.", total, failed) } else { format!("OCR done: {} line(s).", total) };
        app.engines.queue.complete_ocr(crate::app::queue::owner_of(tab_id));
        let promote = crate::app::queue::dispatch_pending(app);
        crate::app::queue::refresh_queued_statuses(app);
        return promote;
    }
    Task::none()
}

// ---------------------------------------------------------------------------
// Manual OCR (toolbar drag, same UX as inpaint, no padding, pixel-perfect)
// ---------------------------------------------------------------------------


#[cfg(feature = "ocr")]
pub fn handle_manual_ocr_engine_ready(app: &mut App, tab_id: super::tab::TabId, result: Result<ocr::Engine, String>) -> Task<Message> {
    match result {
        Ok(engine) => {
            app.engines.manual_ocr = Some(engine.clone());
            if let Some(tab) = app.tab_by_id_mut(tab_id)
                && let Some(multi) = tab.pending_manual_multi_ocr.take() {
                    return start_manual_ocr_selection(app, tab_id, multi, engine.clone());
                }
            if let Some(tab) = app.tab_by_id_mut(tab_id) {
                tab.manual_ocring = false;
            }
            Task::none()
        }
        Err(e) => {
            if let Some(tab) = app.tab_by_id_mut(tab_id) {
                tab.pending_manual_multi_ocr = None;
                tab.manual_ocring = false;
                tab.status = format!("Manual OCR engine failed: {e}");
            }
            // free queue weight on build failure (manual OCR)
            if app.engines.queue.complete_ocr(crate::app::queue::owner_of(tab_id)).is_some() {
                let promote = crate::app::queue::dispatch_pending(app);
                crate::app::queue::refresh_queued_statuses(app);
                return promote;
            }
            Task::none()
        }
    }
}


// ---------------------------------------------------------------------------
// Manual OCR span (across two pages) – auto-OCR style stitch
// ---------------------------------------------------------------------------


pub fn handle_manual_ocr_selection(app: &mut App, tab_id: super::tab::TabId, selections: Vec<(usize, iced::Rectangle)>) -> Task<Message> {
    #[cfg(feature = "ocr")]
    {
        if selections.is_empty() { return Task::none(); }
        // check bulk busy for that tab
        if let Some(tab) = app.tab_by_id(tab_id) {
            if tab.running || tab.translating || tab.inpainting { return Task::none(); }
            #[cfg(feature = "ocr")]
            if tab.manual_ocring {
                if let Some(tab) = app.tab_by_id_mut(tab_id) {
                    tab.status = "Wait for current task to finish.".to_string();
                }
                return Task::none();
            }
        }
        let len = app.tab_by_id(tab_id).map(|t| t.images.len()).unwrap_or(0);
        let mut valid: Vec<(usize, iced::Rectangle)> = Vec::new();
        for (idx, r) in selections {
            if idx >= len { continue; }
            if r.width < 4.0 || r.height < 4.0 { continue; }
            valid.push((idx, r));
        }
        // Keep one entry point capable of both single and stitched multi-image:
        // order so pieces of one seam-crossing drag become adjacent (idx, y, x).
        valid.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(a.1.y.total_cmp(&b.1.y))
                .then(a.1.x.total_cmp(&b.1.x))
        });
        if valid.is_empty() {
            if let Some(tab) = app.tab_by_id_mut(tab_id) { tab.status = "Manual OCR: no valid selections.".to_string(); }
            return Task::none();
        }
        // queue gate — manual OCR uses OCR weight/priority (w4, prio3)
        let mut manual_job_id: Option<u64> = None;
        {
            use crate::app::queue::{AcquireResult, JobKind, OcrMode, owner_of};
            let kind = JobKind::Ocr(OcrMode::Manual);
            let already_running = app.engines.queue.running_ocr_for(owner_of(tab_id)).is_some();
            let already_queued = app.engines.queue.pending_for_tab(owner_of(tab_id)).iter().any(|j| j.kind.is_ocr());
            if !already_running && !already_queued {
                match app.engines.queue.try_acquire_or_enqueue(owner_of(tab_id), kind) {
                    AcquireResult::Acquired(job) => {
                        manual_job_id = Some(job.id);
                    }
                    AcquireResult::Queued(_, pos) => {
                        let used = app.engines.queue.used_weight();
                        if let Some(tab) = app.tab_by_id_mut(tab_id) {
                            tab.pending_manual_multi_ocr = Some(valid);
                            tab.status = format!(
                                "Queued {} (pos {}, pool {}/{}) ...",
                                kind.label(),
                                pos,
                                used,
                                crate::app::queue::POOL_CAPACITY
                            );
                        }
                        return Task::none();
                    }
                }
            } else if already_queued || already_running {
                if let Some(tab) = app.tab_by_id_mut(tab_id) {
                    tab.status = "Wait for current task to finish.".to_string();
                }
                return Task::none();
            }
        }
        let cfg = easyscanlate_settings::get(|s| ocr::config_with(0.0, s.ocr_max_side_len.trim().parse::<u32>().unwrap_or(2000)));
        let cached = app.engines.manual_ocr.clone();
        if let Some(engine) = cached { return start_manual_ocr_selection(app, tab_id, valid, engine); }
        if let Some(tab) = app.tab_by_id_mut(tab_id) {
            tab.pending_manual_multi_ocr = Some(valid);
            // Model load counts as the run itself so buttons disable during it.
            tab.manual_ocring = true;
            tab.status = "Loading OCR engine for manual OCR…".to_string();
        }
        // Correlate the build with the queue slot reserved above (or the
        // running OCR slot if reservation pre-existed).
        let job_id = manual_job_id.or_else(|| {
            app.engines
                .queue
                .running_ocr_for(crate::app::queue::owner_of(tab_id))
                .map(|j| j.id)
        });
        let kind = crate::app::queue::JobKind::Ocr(crate::app::queue::OcrMode::Manual);
        Task::perform(
            async move {
                use crate::app::queue::engine_ready_msg;
                use easyscanlate_engine_pool::BuiltEngine;
                match ocr::Engine::build_with_config(cfg) {
                    Ok(engine) => engine_ready_msg(
                        tab_id,
                        job_id.unwrap_or(0),
                        kind,
                        Ok(BuiltEngine::OcrManual(engine)),
                    ),
                    Err(e) => engine_ready_msg(tab_id, job_id.unwrap_or(0), kind, Err(e)),
                }
            },
            |msg| msg,
        )
    }
    #[cfg(not(feature = "ocr"))]
    {
        let _ = selections;
        app.active_tab_mut().status = "OCR not available in this build.".to_string();
        return Task::none();
    }
}

#[cfg(feature = "ocr")]
pub(crate) fn start_manual_ocr_selection(app: &mut App, tab_id: super::tab::TabId, selections: Vec<(usize, iced::Rectangle)>, engine: ocr::Engine) -> Task<Message> {
    // Same single entry point handles single + stitched: order selections so
    // seam-crossing pieces stay adjacent for the stitch pass in the worker.
    let mut selections = selections;
    selections.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.y.total_cmp(&b.1.y))
            .then(a.1.x.total_cmp(&b.1.x))
    });
    if let Some(tab) = app.tab_by_id_mut(tab_id) {
        tab.manual_ocring = true;
        tab.status = format!("Manual OCR on {} selection(s)...", selections.len());
    }
    // Build per-image paths as pool items ([x, y, w, h] arrays, no iced types
    // cross the exec boundary).
    let mut items: Vec<easyscanlate_engine_pool::ManualOcrItem> = Vec::new();
    {
        let tab = match app.tab_by_id(tab_id) { Some(t) => t, None => return Task::none() };
        for (idx, rect) in selections {
            if idx >= tab.images.len() { continue; }
            let path = tab.project.image(tab.images[idx].image_id).map(|m| m.path.clone()).unwrap_or_default();
            if path.is_empty() { continue; }
            items.push((idx, path, [rect.x, rect.y, rect.width, rect.height]));
        }
    }
    if items.is_empty() {
        if let Some(tab) = app.tab_by_id_mut(tab_id) { tab.manual_ocring=false; }
        return Task::none();
    }
    let merge_cfg = easyscanlate_settings::get(|s| ocr::MergeConfig::from_threshold_str(&s.ocr_merge_threshold));
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                easyscanlate_engine_pool::run_manual_ocr_selection(&engine, items, merge_cfg)
            })
            .await
            .unwrap_or_else(|e| Err(format!("Manual multi OCR task cancelled: {e}")))
        },
        move |res| Message::Tab(tab_id, crate::app::TabMessage::ManualOcrMultiFinished(res)),
    )
}

#[cfg(feature = "ocr")]
pub fn handle_manual_ocr_finished(app: &mut App, tab_id: super::tab::TabId, result: Result<Vec<(usize, Vec<NewEntry>)>, String>) -> Task<Message> {
    #[cfg(feature = "ocr")]
    {
        if let Some(tab) = app.tab_by_id_mut(tab_id) { tab.manual_ocring = false; }
        // Free queue weight for manual OCR (same kind as pipeline OCR) and backfill promote
        let freed = app.engines.queue.complete_ocr(crate::app::queue::owner_of(tab_id)).is_some();
        let mut promote_task = Task::none();
        let mut refresh_needed = false;
        if freed {
            promote_task = crate::app::queue::dispatch_pending(app);
            refresh_needed = true;
        }
        match result {
            Ok(per_image) => {
                if per_image.is_empty() {
                    if let Some(tab) = app.tab_by_id_mut(tab_id) { tab.status = "Manual OCR: no text found.".to_string(); }
                    if refresh_needed { crate::app::queue::refresh_queued_statuses(app); }
                    return promote_task;
                }
                let mut total_added = 0usize;
                let mut total_detected = 0usize;
                let mut image_count = 0usize;
                for (idx, entries) in per_image {
                    let cnt = entries.len();
                    total_detected += cnt;
                    let len = app.tab_by_id(tab_id).map(|t| t.images.len()).unwrap_or(0);
                    if idx >= len { continue; }
                    let image_id = app.tab_by_id(tab_id).unwrap().images[idx].image_id;
                    let tab = app.tab_by_id_mut(tab_id).unwrap();
                    let added = if let Some(ev) = tab.project.append_ocr_for_image_with_event(image_id, entries) {
                        let n = if let easyscanlate_model::ModelEvent::EntriesAdded { ids, .. } = &ev { ids.len() } else { cnt };
                        let ev2 = ev;
                        crate::app::handle_model_event(tab, ev2);
                        let rev = tab.project.reorder_entries_for_image_with_event(image_id);
                        crate::app::handle_model_event(tab, rev);
                        n
                    } else { 0 };
                    total_added += added;
                    image_count += 1;
                }
                if let Some(tab) = app.tab_by_id_mut(tab_id) {
                    if total_added==0 && total_detected==0 {
                        tab.status = "Manual OCR: no text found.".to_string();
                    } else {
                        tab.status = format!("Manual OCR: {total_added} line(s) added across {image_count} image(s) ({total_detected} detected).");
                    }
                }
            }
            Err(e) => { if let Some(tab) = app.tab_by_id_mut(tab_id) { tab.status = format!("Manual OCR multi failed: {e}"); } }
        }
        if refresh_needed { crate::app::queue::refresh_queued_statuses(app); }
        promote_task
    }
    #[cfg(not(feature = "ocr"))]
    {
        let _ = result;
        app.active_tab_mut().status = "OCR not available in this build.".to_string();
        return Task::none();
    }
}
