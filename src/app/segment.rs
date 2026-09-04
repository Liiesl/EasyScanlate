use iced::Task;
#[cfg(feature = "segment")]
use iced::futures::{SinkExt, StreamExt};
use easyscanlate_model::EntryId;
#[cfg(feature = "segment")]
use easyscanlate_segment::Engine as SegmentEngine;

use super::{App, Message};

/// One granular segment stream item: grid index + this grid's deletions.
/// Emitted per finished grid (OCR-style), so one grid failure never drops the
/// other grids' deletions.
#[cfg(feature = "segment")]
pub type SegmentStreamItem = (usize, Vec<(usize, EntryId)>);

pub fn start_segment_filter(app: &mut App, tab_id: crate::app::tab::TabId) -> Task<Message> {
    #[cfg(feature = "segment")]
    {
        let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i)=>i, None=>return Task::none() };
        // queue gate — weight 4, backfill + priority (cap 5)
        {
            use crate::app::queue::{AcquireResult, EngineKind};
            // Only gate if not already running — if already running, we are in dispatch chain, so allow
            let already_reserved = app.engines.queue.running_for(tab_id, EngineKind::Segment).is_some();
            if !already_reserved {
                // if this tab is already queued/running for segment, don't duplicate
                if app.engines.queue.is_tab_queued(tab_id) || app.engines.queue.is_tab_running(tab_id) {
                    // already queued, avoid duplicate enqueue
                    // check if pending for this exact kind
                    if app.engines.queue.pending_for_tab(tab_id).iter().any(|j| j.kind == EngineKind::Segment) || app.engines.queue.running_for(tab_id, EngineKind::Segment).is_some() {
                        return Task::none();
                    }
                }
                match app.engines.queue.try_acquire_or_enqueue(tab_id, EngineKind::Segment) {
                    AcquireResult::Acquired(_) => {
                        // fall through to spawn
                    }
                    AcquireResult::Queued(_, pos) => {
                        app.tabs[idx].segment_filtering = true;
                        app.tabs[idx].status = format!(
                            "Queued {} (pos {}, pool {}/{}) ...",
                            EngineKind::Segment.label(),
                            pos,
                            app.engines.queue.used_weight(),
                            crate::app::queue::POOL_CAPACITY
                        );
                        return Task::none();
                    }
                }
            }
        }
        let tab = &app.tabs[idx];
        if tab.images.is_empty() {
            // release reservation if we acquired
            app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::Segment);
            return Task::none();
        }
        if !easyscanlate_settings::get(|s| s.auto_sfx_filter) {
            app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::Segment);
            return Task::none();
        }
        match &app.engines.segment {
            Some(engine) => {
                let engine = engine.clone();
                let dims: Vec<(u32, u32)> = tab.images.iter().map(|img| {
                    tab.project.image(img.image_id).map(|m| (m.width as u32, m.height as u32)).unwrap_or((0, 0))
                }).collect();
                let paths: Vec<String> = tab.images.iter().map(|img| {
                    tab.project.image(img.image_id).map(|m| m.path.clone()).unwrap_or_default()
                }).collect();
                let ocr_boxes: Vec<Vec<([f32; 4], EntryId)>> = tab.images.iter().map(|img| {
                    let image_id = img.image_id;
                    tab.project.visible_for(image_id).map(|e| (tab.project.view_quad(e).bounds(), e.id)).collect()
                }).collect();
                start_segment_stream(app, tab_id, engine, dims, paths, ocr_boxes)
            }
            None => {
                app.tabs[idx].segment_filtering = true;
                app.tabs[idx].status = format!(
                    "Loading segmentation model... pool {}/{}",
                    app.engines.queue.used_weight(),
                    crate::app::queue::POOL_CAPACITY
                );
                Task::perform(async move { SegmentEngine::build() }, move |res| Message::Tab(tab_id, crate::app::TabMessage::SegmentEngineReady(res)))
            }
        }
    }
    #[cfg(not(feature = "segment"))]
    {
        let _ = app;
        let _ = tab_id;
        return Task::none();
    }
}

/// Starts a granular segment stream (OCR-style): one `SegmentStreamRun` per
/// grid canvas. Each grid's deletions commit incrementally and one grid's
/// detection failure counts as one failed grid without dropping the rest.
/// Queue weight stays reserved until the last grid finalizes.
#[cfg(feature = "segment")]
fn start_segment_stream(
    app: &mut App,
    tab_id: crate::app::tab::TabId,
    engine: SegmentEngine,
    dims: Vec<(u32, u32)>,
    paths: Vec<String>,
    ocr_boxes: Vec<Vec<([f32; 4], EntryId)>>,
) -> Task<Message> {
    use easyscanlate_segment::grid::plan_grids;
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    if dims.is_empty() {
        app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::Segment);
        return Task::none();
    }
    let runs = plan_grids(&dims);
    if runs.is_empty() {
        app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::Segment);
        return Task::none();
    }
    let total = runs.len();
    // Fresh run resets granular counters (queue guarantees no concurrent run).
    if app.tabs[idx].segment_pending == 0 {
        app.tabs[idx].segment_total = 0;
        app.tabs[idx].segment_failed = 0;
        app.tabs[idx].segment_removed = 0;
    }
    app.tabs[idx].segment_total += total;
    app.tabs[idx].segment_pending += total;
    app.tabs[idx].segment_filtering = true;
    app.tabs[idx].pipeline_seg_done = false;
    app.tabs[idx].status = format!(
        "Filtering SFX via segmentation: 0 of {total} grid(s)... pool {}/{}",
        app.engines.queue.used_weight(),
        crate::app::queue::POOL_CAPACITY
    );
    let tid = tab_id;
    Task::stream(
        iced::stream::try_channel(1, move |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
            for (grid_idx, run) in runs.into_iter().enumerate() {
                let engine_clone = engine.clone();
                // Clone per-grid inputs for the blocking thread (grids borrow
                // `dims` by value here; each iteration owns its copies).
                let dims_c = dims.clone();
                let paths_c = paths.clone();
                let boxes_c = ocr_boxes.clone();
                let res = tokio::task::spawn_blocking(move || {
                    run_segment_grid(&engine_clone, &run, &dims_c, &paths_c, &boxes_c)
                })
                .await
                .unwrap_or_else(|e| Err(format!("segment task cancelled: {e}")));
                let payload: Result<SegmentStreamItem, String> =
                    res.map(|deletes| (grid_idx, deletes));
                if sender
                    .send(Message::Tab(
                        tid,
                        crate::app::TabMessage::SegmentStreamRun(payload),
                    ))
                    .await
                    .is_err()
                {
                    return Ok::<(), String>(());
                }
            }
            Ok::<(), String>(())
        })
        .map(move |item: Result<Message, String>| match item {
            Ok(message) => message,
            Err(e) => Message::Tab(tid, crate::app::TabMessage::SegmentStreamFailed(e.to_string())),
        }),
    )
}

/// Runs SFX filtering for a single grid canvas. Returns this grid's deletions.
/// One grid's detection failure is isolated here so the stream can count it
/// as one failed grid and continue with the rest (OCR-style granularity).
#[cfg(feature = "segment")]
fn run_segment_grid(
    engine: &SegmentEngine,
    run: &easyscanlate_segment::grid::GridRun,
    dims: &[(u32, u32)],
    paths: &[String],
    ocr_boxes: &[Vec<([f32; 4], EntryId)>],
) -> Result<Vec<(usize, EntryId)>, String> {
    use easyscanlate_segment::filter::{DetBox, sfx_filter_indexes};
    use easyscanlate_segment::grid::{build_grid_canvas_with_loader, grid_det_to_page};
    use easyscanlate_segment::SegClass;
    let mut loader = |page_idx: usize| -> image::RgbImage {
        let path = match paths.get(page_idx) {
            Some(p) => p,
            None => return image::RgbImage::new(1, 1),
        };
        #[cfg(feature = "ocr")]
        let img = easyscanlate_ocr::load_rgb(path).unwrap_or_else(|| image::RgbImage::new(1, 1));
        #[cfg(not(feature = "ocr"))]
        let img = image::open(path).map(|i| i.to_rgb8()).unwrap_or_else(|_| image::RgbImage::new(1, 1));
        img
    };
    let canvas = build_grid_canvas_with_loader(run, &mut loader);
    let dets = engine
        .detect_canvas(&canvas)
        .map_err(|e| format!("segment detect failed: {e}"))?;
    let mut balloons_per_page: Vec<Vec<DetBox>> = vec![Vec::new(); dims.len()];
    let mut sfx_per_page: Vec<Vec<DetBox>> = vec![Vec::new(); dims.len()];
    for det in dets {
        if let Some((page, bbox)) = grid_det_to_page(det.bbox, run, dims) {
            let db = DetBox {
                bbox,
                confidence: det.confidence,
            };
            match det.class {
                SegClass::Balloon => balloons_per_page[page].push(db),
                SegClass::Onomatopoeia => sfx_per_page[page].push(db),
                _ => {}
            }
        }
    }
    let mut touched_pages: Vec<usize> = run.cols.iter().flat_map(|c| c.pages.clone()).collect();
    touched_pages.sort_unstable();
    touched_pages.dedup();
    let mut to_delete: Vec<(usize, EntryId)> = Vec::new();
    for page in touched_pages {
        if page >= ocr_boxes.len() {
            continue;
        }
        let entries = &ocr_boxes[page];
        let bboxes: Vec<[f32; 4]> = entries.iter().map(|(bb, _)| *bb).collect();
        let idxs = sfx_filter_indexes(&bboxes, &balloons_per_page[page], &sfx_per_page[page]);
        for idx in idxs {
            let (_, id) = entries[idx];
            to_delete.push((page, id));
        }
    }
    Ok(to_delete)
}

#[cfg(feature = "segment")]
pub fn handle_engine_ready(app: &mut App, tab_id: crate::app::tab::TabId, result: Result<SegmentEngine, String>) -> Task<Message> {
    match result {
        Ok(engine) => {
            app.engines.segment = Some(engine);
            // Engine built while queue weight reserved — `start_segment_filter`
            // sees the reservation (`already_reserved`) and spawns the granular
            // stream without re-queueing.
            start_segment_filter(app, tab_id)
        }
        Err(e) => {
            let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i)=>i, None=>return Task::none()};
            app.tabs[idx].segment_filtering = false;
            app.tabs[idx].status = e.clone();
            // free queue weight (build failed) and promote
            app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::Segment);
            let promote = crate::app::queue::dispatch_pending(app);
            crate::app::queue::refresh_queued_statuses(app);
            promote
        }
    }
}

/// Chains the pipeline stages after segment finishes (style → inpaint-solo).
/// Shared by the granular stream finalize paths.
#[cfg(feature = "segment")]
fn chain_segment_next(app: &mut App, tab_id: crate::app::tab::TabId, idx: usize, is_pipeline: bool, tasks: &mut Vec<Task<Message>>) {
    if !is_pipeline {
        return;
    }
    let (need_style_inpaint, need_inpaint_solo) = easyscanlate_settings::get(|s| {
        let need_style = s.auto_style_detect && s.auto_inpaint;
        let need_solo = s.auto_inpaint && !s.auto_style_detect;
        (need_style, need_solo)
    });
    if need_style_inpaint {
        #[cfg(all(feature = "styling", feature = "inpaint"))]
        {
            use crate::app::queue::{AcquireResult, EngineKind};
            match app.engines.queue.try_acquire_or_enqueue(tab_id, EngineKind::Style) {
                AcquireResult::Acquired(_) => tasks.push(super::styling::classify(app, tab_id)),
                AcquireResult::Queued(_, pos) => {
                    let used = app.engines.queue.used_weight();
                    if let Some(t) = app.tab_by_id_mut(tab_id) {
                        t.status = format!("Queued {} (pos {}, pool {}/{}) ...", EngineKind::Style.label(), pos, used, crate::app::queue::POOL_CAPACITY);
                    }
                }
            }
        }
    } else if need_inpaint_solo {
        let eff = easyscanlate_settings::get(|s| {
            if !s.auto_style_detect && s.auto_inpaint_model == easyscanlate_settings::AutoInpaintModel::Mixed {
                easyscanlate_settings::AutoInpaintModel::Telea
            } else {
                s.auto_inpaint_model
            }
        });
        #[cfg(feature = "inpaint")]
        {
            use crate::app::queue::{AcquireResult, EngineKind};
            let kind = match eff {
                easyscanlate_settings::AutoInpaintModel::Telea => EngineKind::InpaintTelea,
                easyscanlate_settings::AutoInpaintModel::Lama => EngineKind::InpaintLama,
                easyscanlate_settings::AutoInpaintModel::Aot => EngineKind::InpaintAot,
                easyscanlate_settings::AutoInpaintModel::Mixed => EngineKind::InpaintTelea,
            };
            match app.engines.queue.try_acquire_or_enqueue(tab_id, kind) {
                AcquireResult::Acquired(_) => tasks.push(super::inpaint::dispatch_auto_solo(app, tab_id, eff)),
                AcquireResult::Queued(_, pos) => {
                    let used = app.engines.queue.used_weight();
                    if let Some(t) = app.tab_by_id_mut(tab_id) {
                        t.status = format!("Queued {} (pos {}, pool {}/{}) ...", kind.label(), pos, used, crate::app::queue::POOL_CAPACITY);
                    }
                }
            }
        }
    } else {
        #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
        {
            app.tabs[idx].pipeline_active = false;
        }
        let need_style_only = easyscanlate_settings::get(|s| s.auto_style_detect && !s.auto_inpaint);
        if need_style_only {
            #[cfg(feature = "styling")]
            {
                use crate::app::queue::{AcquireResult, EngineKind};
                match app.engines.queue.try_acquire_or_enqueue(tab_id, EngineKind::Style) {
                    AcquireResult::Acquired(_) => tasks.push(super::styling::classify(app, tab_id)),
                    AcquireResult::Queued(_, pos) => {
                    let used = app.engines.queue.used_weight();
                    if let Some(t) = app.tab_by_id_mut(tab_id) {
                        t.status = format!("Queued {} (pos {}, pool {}/{}) ...", EngineKind::Style.label(), pos, used, crate::app::queue::POOL_CAPACITY);
                    }
                }
                }
            }
        }
    }
}

/// Granular segment stream event (OCR-style): one finished grid.
/// `Ok((grid_idx, deletes))` deletes incrementally; `Err` counts one failed
/// grid without dropping the rest. Queue weight stays reserved until the last
/// grid finalizes and chains the pipeline.
#[cfg(feature = "segment")]
pub fn handle_stream_run(
    app: &mut App,
    tab_id: crate::app::tab::TabId,
    result: Result<SegmentStreamItem, String>,
) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    app.tabs[idx].segment_pending = app.tabs[idx].segment_pending.saturating_sub(1);
    let pending = app.tabs[idx].segment_pending;
    let total = app.tabs[idx].segment_total;
    match result {
        Ok((_grid_idx, to_delete)) => {
            let n = to_delete.len();
            for (pidx, id) in to_delete {
                if pidx < app.tabs[idx].images.len() {
                    let tab = &mut app.tabs[idx];
                    if let Some(ev) = tab.project.delete_entry_with_event(id) {
                        crate::app::handle_model_event(tab, ev);
                    }
                }
            }
            app.tabs[idx].segment_removed += n;
            let (removed, failed) = (app.tabs[idx].segment_removed, app.tabs[idx].segment_failed);
            if pending == 0 {
                app.tabs[idx].segment_filtering = false;
                app.tabs[idx].pipeline_seg_done = true;
                app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::Segment);
                crate::app::queue::refresh_queued_statuses(app);
                let is_pipeline = {
                    #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
                    { app.tabs[idx].pipeline_active }
                    #[cfg(not(all(feature = "styling", feature = "inpaint", feature = "segment")))]
                    { false }
                };
                if failed > 0 {
                    app.tabs[idx].status = format!("SFX filter: {removed} removed, {failed} of {total} grid(s) failed.");
                } else if removed > 0 {
                    app.tabs[idx].status = format!("SFX filter removed {removed} entry(s).");
                } else {
                    app.tabs[idx].status = "SFX filter: no entries removed.".to_string();
                }
                let mut tasks: Vec<Task<Message>> = Vec::new();
                chain_segment_next(app, tab_id, idx, is_pipeline, &mut tasks);
                tasks.push(crate::app::queue::dispatch_pending(app));
                return if tasks.is_empty() { Task::none() } else { Task::batch(tasks) };
            }
            let done = total.saturating_sub(pending);
            app.tabs[idx].status = if failed > 0 {
                format!("Filtering SFX: {done} of {total} grid(s) done, {removed} removed, {failed} failed.")
            } else {
                format!("Filtering SFX: {done} of {total} grid(s) done, {removed} removed.")
            };
        }
        Err(e) => {
            app.tabs[idx].segment_failed += 1;
            let (removed, failed) = (app.tabs[idx].segment_removed, app.tabs[idx].segment_failed);
            if pending == 0 {
                app.tabs[idx].segment_filtering = false;
                app.tabs[idx].pipeline_seg_done = true;
                app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::Segment);
                crate::app::queue::refresh_queued_statuses(app);
                let is_pipeline = {
                    #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
                    { app.tabs[idx].pipeline_active }
                    #[cfg(not(all(feature = "styling", feature = "inpaint", feature = "segment")))]
                    { false }
                };
                app.tabs[idx].status =
                    format!("SFX filter: {removed} removed, {failed} of {total} grid(s) failed (last: {e}).");
                let mut tasks: Vec<Task<Message>> = Vec::new();
                chain_segment_next(app, tab_id, idx, is_pipeline, &mut tasks);
                tasks.push(crate::app::queue::dispatch_pending(app));
                return if tasks.is_empty() { Task::none() } else { Task::batch(tasks) };
            }
            let done = total.saturating_sub(pending);
            app.tabs[idx].status =
                format!("Filtering SFX: {done} of {total} grid(s) done, {removed} removed, {failed} failed (last: {e}).");
        }
    }
    Task::none()
}

/// Fatal segment stream failure. Marks remaining grids failed, frees weight,
/// chains the pipeline once.
#[cfg(feature = "segment")]
pub fn handle_stream_failed(
    app: &mut App,
    tab_id: crate::app::tab::TabId,
    e: String,
) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    if app.tabs[idx].segment_pending > 0 {
        let remaining = app.tabs[idx].segment_pending;
        app.tabs[idx].segment_failed += remaining;
        app.tabs[idx].segment_pending = 0;
        app.tabs[idx].segment_filtering = false;
        app.tabs[idx].pipeline_seg_done = true;
        app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::Segment);
        crate::app::queue::refresh_queued_statuses(app);
        let (total, removed, failed) =
            (app.tabs[idx].segment_total, app.tabs[idx].segment_removed, app.tabs[idx].segment_failed);
        let is_pipeline = {
            #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
            { app.tabs[idx].pipeline_active }
            #[cfg(not(all(feature = "styling", feature = "inpaint", feature = "segment")))]
            { false }
        };
        app.tabs[idx].status =
            format!("SFX filter stream failed ({e}): {removed} removed, {failed} of {total} grid(s) failed.");
        let mut tasks: Vec<Task<Message>> = Vec::new();
        chain_segment_next(app, tab_id, idx, is_pipeline, &mut tasks);
        tasks.push(crate::app::queue::dispatch_pending(app));
        return if tasks.is_empty() { Task::none() } else { Task::batch(tasks) };
    }
    Task::none()
}
