use iced::Task;
use easyscanlate_model::EntryId;
#[cfg(feature = "segment")]
use easyscanlate_segment::Engine as SegmentEngine;

use super::{App, Message};

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
                app.tabs[idx].segment_filtering = true;
                app.tabs[idx].status = format!(
                    "Filtering SFX via segmentation... pool {}/{}",
                    app.engines.queue.used_weight(),
                    crate::app::queue::POOL_CAPACITY
                );
                return Task::perform(
                    async move {
                        let res = tokio::task::spawn_blocking(move || run_segment_filter_blocking(&engine, &dims, &paths, &ocr_boxes))
                            .await
                            .unwrap_or_else(|e| Err(format!("segment task cancelled: {e}")));
                        res
                    },
                    move |res| Message::Tab(tab_id, crate::app::TabMessage::SegmentFiltered(res)),
                );
            }
            None => {
                app.tabs[idx].segment_filtering = true;
                app.tabs[idx].status = format!(
                    "Loading segmentation model... pool {}/{}",
                    app.engines.queue.used_weight(),
                    crate::app::queue::POOL_CAPACITY
                );
                return Task::perform(async move { SegmentEngine::build() }, move |res| Message::Tab(tab_id, crate::app::TabMessage::SegmentEngineReady(res)));
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

#[cfg(feature = "segment")]
fn run_segment_filter_blocking(
    engine: &SegmentEngine,
    dims: &[(u32, u32)],
    paths: &[String],
    ocr_boxes: &[Vec<([f32; 4], EntryId)>],
) -> Result<Vec<(usize, EntryId)>, String> {
    use easyscanlate_segment::filter::{DetBox, sfx_filter_indexes};
    use easyscanlate_segment::grid::{build_grid_canvas_with_loader, grid_det_to_page, plan_grids};
    use easyscanlate_segment::SegClass;
    if dims.is_empty() {
        return Ok(Vec::new());
    }
    let runs = plan_grids(dims);
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
    let mut to_delete: Vec<(usize, EntryId)> = Vec::new();
    for run in runs.iter() {
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
        drop(canvas);
    }
    Ok(to_delete)
}

#[cfg(feature = "segment")]
pub fn handle_engine_ready(app: &mut App, tab_id: crate::app::tab::TabId, result: Result<SegmentEngine, String>) -> Task<Message> {
    match result {
        Ok(engine) => {
            app.engines.segment = Some(engine.clone());
            let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i)=>i, None=>return Task::none()};
            app.tabs[idx].segment_filtering = false;
            // engine built while weight reserved — keep reservation, directly spawn filter without re-queue
            // Temporarily clear running? No, keep reserved. Call internal spawn bypassing queue gate.
            // To bypass gate, we temporarily mark as already reserved and call spawn logic that checks gate.
            // Easier: directly spawn blocking without queue check since reservation already held.
            let engine_clone = engine.clone();
            let tab = &app.tabs[idx];
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
            app.tabs[idx].segment_filtering = true;
            app.tabs[idx].status = format!("Filtering SFX via segmentation... pool {}/{}", app.engines.queue.used_weight(), crate::app::queue::POOL_CAPACITY);
            return Task::perform(
                async move {
                    let res = tokio::task::spawn_blocking(move || run_segment_filter_blocking(&engine_clone, &dims, &paths, &ocr_boxes))
                        .await
                        .unwrap_or_else(|e| Err(format!("segment task cancelled: {e}")));
                    res
                },
                move |res| Message::Tab(tab_id, crate::app::TabMessage::SegmentFiltered(res)),
            );
        }
        Err(e) => {
            let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i)=>i, None=>return Task::none()};
            app.tabs[idx].segment_filtering = false;
            app.tabs[idx].status = e.clone();
            // free queue weight (build failed) and promote
            app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::Segment);
            let promote = crate::app::queue::dispatch_pending(app);
            crate::app::queue::refresh_queued_statuses(app);
            return promote;
        }
    }
}

#[cfg(feature = "segment")]
pub fn handle_filtered(
    app: &mut App,
    tab_id: crate::app::tab::TabId,
    result: Result<Vec<(usize, EntryId)>, String>,
) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i)=>i, None=>return Task::none()};
    app.tabs[idx].segment_filtering = false;
    app.tabs[idx].pipeline_seg_done = true;
    // free queue weight for segment (success or failure)
    app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::Segment);
    crate::app::queue::refresh_queued_statuses(app);
    let is_pipeline = {
        #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
        { app.tabs[idx].pipeline_active }
        #[cfg(not(all(feature = "styling", feature = "inpaint", feature = "segment")))]
        { false }
    };
    let mut tasks: Vec<Task<Message>> = Vec::new();
    match result {
        Ok(to_delete) => {
            let n = to_delete.len();
            for (pidx, id) in to_delete {
                if pidx < app.tabs[idx].images.len() {
                    let tab = &mut app.tabs[idx];
                    if let Some(ev) = tab.project.delete_entry_with_event(id) {
                        crate::app::handle_model_event(tab, ev);
                    }
                }
            }
            let tab_status = app.tabs[idx].status.clone();
            if n > 0 {
                app.tabs[idx].status = format!("SFX filter removed {n} entry(s). {}", tab_status);
            } else {
                app.tabs[idx].status = format!("SFX filter: no entries removed. {}", tab_status);
            }
            if is_pipeline {
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
        }
        Err(e) => {
            app.tabs[idx].status = format!("SFX filter failed: {e}");
            if is_pipeline {
                #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
                {
                    app.tabs[idx].pipeline_active = false;
                }
            }
        }
    }
    // promote any pending that now fits
    tasks.push(crate::app::queue::dispatch_pending(app));
    if tasks.is_empty() { Task::none() } else { Task::batch(tasks) }
}
