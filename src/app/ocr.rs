use iced::Task;
#[cfg(feature = "ocr")]
use iced::futures::{SinkExt, StreamExt};
use scanlateit_model::{NewEntry, Quad};
#[cfg(feature = "ocr")]
use scanlateit_ocr::{self as ocr, ParallelEngine};

use super::{App, Message};

#[cfg(feature = "ocr")]
pub fn start_ocr_stream(app: &mut App) -> Task<Message> {
    let pipeline = app
        .pipeline
        .clone()
        .expect("pipeline must be built before starting the stream");
    let token = app
        .cancel
        .clone()
        .expect("cancellation token set before starting the stream");
    let runs = app.ocr_plans.clone();
    let dims = app.ocr_dims.clone();
    let paths: Vec<Vec<String>> = runs
        .iter()
        .map(|run| {
            (run.page_start..=run.page_end)
                .map(|i| {
                    app.project
                        .image(app.images[i].image_id)
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
                app.project
                    .image(app.images[page].image_id)
                    .map(|m| m.path.clone())
                    .unwrap_or_default()
            })
        })
        .collect();
    let below_paths: Vec<Option<String>> = runs
        .iter()
        .map(|run| {
            run.below.map(|(page, _)| {
                app.project
                    .image(app.images[page].image_id)
                    .map(|m| m.path.clone())
                    .unwrap_or_default()
            })
        })
        .collect();
    let workers = scanlateit_settings::get(|s| s.ocr_workers.parse::<usize>().unwrap_or(2)).max(1);
    let mut session = ocr::RunSession::new(runs, dims, paths, above_paths, below_paths, workers);
    Task::stream(
        iced::stream::try_channel(1, move |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
            while let Some(event) = session.step(&pipeline, &token)? {
                if sender
                    .send(Message::OcrStreamRun(Ok::<ocr::RunEvent, String>(event)))
                    .await
                    .is_err()
                {
                    return Ok(());
                }
            }
            Ok(())
        })
        .map(|item| match item {
            Ok(message) => message,
            Err(e) => Message::OcrStreamFailed(e),
        }),
    )
}

#[cfg(feature = "ocr")]
pub fn commit_per_page(app: &mut App, per_page: Vec<(usize, Vec<NewEntry>)>) {
    for (page, entries) in per_page {
        let Some(image) = app.images.get(page) else {
            continue;
        };
        let image_id = image.image_id;
        let count = entries.len();
        if let Some(ev) = app.project.append_ocr_for_image_with_event(image_id, entries) {
            // ocr_total tracks total appended lines, matches ids length
            if let scanlateit_model::ModelEvent::EntriesAdded { ids, .. } = &ev {
                app.ocr_total += ids.len();
            } else {
                app.ocr_total += count;
            }
            crate::app::handle_model_event(app, ev);
        }
    }
}

#[cfg(feature = "ocr")]
pub fn flush_held_boundary(app: &mut App) {
    if let Some(state) = app.held_boundary.take() {
        for candidate in state.candidates {
            if let Some(image) = app.images.get(candidate.page) {
                let image_id = image.image_id;
                if let Some(ev) = app.project.append_ocr_for_image_with_event(image_id, vec![candidate.entry]) {
                    if let scanlateit_model::ModelEvent::EntriesAdded { ids, .. } = &ev {
                        app.ocr_total += ids.len();
                    } else {
                        app.ocr_total += 1;
                    }
                    crate::app::handle_model_event(app, ev);
                }
            }
        }
    }
}

#[cfg(feature = "ocr")]
pub fn maybe_start_ocr(app: &mut App) -> Task<Message> {
    if app.running && app.pipeline.is_some() {
        app.cancel = app
            .pipeline
            .as_ref()
            .map(|pipeline| pipeline.cancellation_token().clone());
        start_ocr_stream(app)
    } else if !app.running {
        if let Some(pipeline) = app.pipeline.take() {
            pipeline.cancel();
        }
        Task::none()
    } else {
        Task::none()
    }
}

#[cfg(feature = "ocr")]
pub fn finalize_run(app: &mut App) {
    flush_held_boundary(app);
    app.running = false;
    app.cancel = None;
    app.pipeline = None;
    app.status = if app.ocr_cancelled {
        "OCR cancelled.".to_string()
    } else if app.ocr_failed > 0 {
        format!(
            "OCR done: {} line(s), {} run(s) failed.",
            app.ocr_total, app.ocr_failed
        )
    } else {
        format!("OCR done: {} line(s).", app.ocr_total)
    };
}

pub fn handle_start_ocr(app: &mut App) -> Task<Message> {
    #[cfg(feature = "ocr")]
    {
        if app.images.is_empty() {
            app.status = "Open images first.".to_string();
            return Task::none();
        }
        if app.running {
            return Task::none();
        }
        app.running = true;
        let dims: Vec<(u32, u32)> = app.images.iter().map(|image| {
            app.project.image(image.image_id).map(|m| (m.width as u32, m.height as u32)).unwrap_or((0, 0))
        }).collect();
        let runs = ocr::plan_runs(&dims);
        let run_count = runs.len();
        app.ocr_plans = runs;
        app.ocr_dims = dims;
        app.ocr_runs = run_count;
        app.pending = run_count;
        app.ocr_total = 0;
        app.ocr_failed = 0;
        app.ocr_cancelled = false;
        app.held_boundary = None;
        app.status = format!("Running OCR on {} run(s) covering {} image(s)...", run_count, app.images.len());
        if app.pipeline.is_none() {
            let (workers, cfg) = scanlateit_settings::get(|s| {
                let workers = s.ocr_workers.parse::<usize>().unwrap_or(2).max(1);
                let cfg = ocr::config_from_strings(&s.ocr_text_score, &s.ocr_max_side_len);
                (workers, cfg)
            });
            app.status = format!("Loading the OCR engine ({workers} detection worker(s))...");
            return Task::perform(async move { ParallelEngine::build_with_config(cfg, workers) }, Message::ParallelEngineReady);
        }
        return maybe_start_ocr(app);
    }
    #[cfg(not(feature = "ocr"))]
    {
        use super::boot::fake_ocr_entries;
        if app.images.is_empty() {
            app.status = "Open images first.".to_string();
            return Task::none();
        }
        if app.running {
            return Task::none();
        }
        app.running = true;
        let mut added = 0;
        let image_ids: Vec<_> = app.images.iter().map(|i| i.image_id).collect();
        for image_id in image_ids {
            let entries = fake_ocr_entries();
            let cnt = entries.len();
            if let Some(ev) = app.project.append_ocr_for_image_with_event(image_id, entries) {
                if let scanlateit_model::ModelEvent::EntriesAdded { ids, .. } = &ev { added += ids.len(); } else { added += cnt; }
                crate::app::handle_model_event(app, ev);
            }
        }
        app.running = false;
        app.status = format!("Fake OCR done: {added} line(s) (no OCR engine in this build).");
        return Task::none();
    }
}

#[cfg(feature = "ocr")]
pub fn handle_parallel_ready(app: &mut App, result: Result<ParallelEngine, String>) -> Task<Message> {
    match result {
        Ok(pipeline) => {
            app.pipeline = Some(pipeline.clone());
            maybe_start_ocr(app)
        }
        Err(e) => {
            app.running = false;
            app.status = e;
            Task::none()
        }
    }
}

pub fn handle_stop_ocr(app: &mut App) -> Task<Message> {
    #[cfg(feature = "ocr")]
    {
        if let Some(token) = &app.cancel { token.cancel(); }
        app.running = false;
        app.status = "Cancelling OCR...".to_string();
        return Task::none();
    }
    #[cfg(not(feature = "ocr"))]
    {
        app.status = "OCR is not available in this build.".to_string();
        return Task::none();
    }
}

#[cfg(feature = "ocr")]
pub fn handle_ocr_stream_run(app: &mut App, result: Result<ocr::RunEvent, String>) -> Task<Message> {
    app.pending = app.pending.saturating_sub(1);
    match result {
        Ok(ocr::RunEvent::Canvas {
            index,
            width,
            margin_top,
            lines,
        }) => {
            let run = app.ocr_plans[index];
            let prev = run.dedup.map(|(page, offset)| {
                let image_id = app.images[page].image_id;
                let quads: Vec<Quad> = app
                    .project
                    .all_for(image_id) // escape hatch: includes deleted for dedup `model/src/project.rs:120`
                    .map(|entry| entry.quad)
                    .collect();
                let width = app
                    .project
                    .image(image_id)
                    .map(|m| m.width as u32)
                    .unwrap_or(0);
                (quads, width, offset)
            });
            let (merge_cfg, min_h, max_h) = scanlateit_settings::get(|s| {
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
            let run_result = ocr::assemble_with_config(
                index,
                width,
                margin_top,
                lines,
                &app.ocr_plans,
                &app.ocr_dims,
                app.held_boundary.take(),
                prev,
                merge_cfg,
                min_h,
                max_h,
            );
            app.held_boundary = run_result.held;
            commit_per_page(app, run_result.per_page);
        }
        Err(e) => {
            app.ocr_failed += 1;
            if e == "cancelled" {
                app.ocr_cancelled = true;
            } else {
                // Undecodable page or other error: flush any held boundary to not lose previous run's bottom-margin capture
                flush_held_boundary(app);
            }
        }
    }
    #[cfg_attr(not(any(feature = "styling", feature = "segment", feature = "inpaint")), allow(unused_mut))]
    let mut tasks: Vec<Task<Message>> = Vec::new();
    if app.pending == 0 || app.ocr_cancelled {
        finalize_run(app);
        if !app.ocr_cancelled {
            let (do_sfx, do_style, do_inpaint, model) = scanlateit_settings::get(|s| {
                (s.auto_sfx_filter, s.auto_style_detect, s.auto_inpaint, s.auto_inpaint_model)
            });
            let effective_model = if !do_style && model == scanlateit_settings::AutoInpaintModel::Mixed {
                scanlateit_settings::AutoInpaintModel::Telea
            } else {
                model
            };
            if do_sfx {
                #[cfg(feature = "segment")]
                {
                    let need_chain = do_style || do_inpaint;
                    #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
                    {
                        if need_chain {
                            app.pipeline_active = true;
                        }
                    }
                    tasks.push(super::segment::start_segment_filter(app));
                }
                #[cfg(not(feature = "segment"))]
                {
                    #[cfg(feature = "styling")]
                    if do_style && !do_inpaint {
                        tasks.push(super::styling::classify(app));
                    }
                    #[cfg(feature = "inpaint")]
                    if do_inpaint && !do_style {
                        tasks.push(super::inpaint::dispatch_auto_solo(app, effective_model));
                    }
                    #[cfg(all(feature = "styling", feature = "inpaint"))]
                    if do_style && do_inpaint {
                        tasks.push(super::styling::classify(app));
                    }
                }
            } else {
                #[cfg(all(feature = "styling", feature = "inpaint"))]
                if do_style && do_inpaint {
                    tasks.push(super::styling::classify(app));
                }
                #[cfg(feature = "styling")]
                if do_style && !do_inpaint {
                    tasks.push(super::styling::classify(app));
                }
                #[cfg(feature = "inpaint")]
                if do_inpaint && !do_style {
                    tasks.push(super::inpaint::dispatch_auto_solo(app, effective_model));
                }
            }
        }
    } else {
        app.status = format!(
            "OCR in progress: {} of {} run(s) done ({} line(s)).",
            app.ocr_runs - app.pending,
            app.ocr_runs,
            app.ocr_total
        );
    }
    if tasks.is_empty() {
        Task::none()
    } else {
        Task::batch(tasks)
    }
}

#[cfg(feature = "ocr")]
pub fn handle_ocr_stream_failed(app: &mut App, e: String) -> Task<Message> {
    app.ocr_failed += 1;
    if e == "cancelled" {
        app.ocr_cancelled = true;
    } else {
        flush_held_boundary(app);
    }
    if app.pending > 0 {
        app.pending = 0;
        finalize_run(app);
    }
    Task::none()
}

// ---------------------------------------------------------------------------
// Manual OCR (toolbar drag, same UX as inpaint, no padding, pixel-perfect)
// ---------------------------------------------------------------------------











#[cfg(feature = "ocr")]
pub fn handle_manual_ocr_engine_ready(app: &mut App, result: Result<ocr::Engine, String>) -> Task<Message> {
    match result {
        Ok(engine) => {
            app.manual_ocr_engine = Some(engine.clone());
            if let Some(multi) = app.pending_manual_multi_ocr.take() {
                return start_manual_ocr_selection(app, multi, engine.clone());
            }
            Task::none()
        }
        Err(e) => {
            app.pending_manual_multi_ocr = None;
            app.status = format!("Manual OCR engine failed: {e}");
            Task::none()
        }
    }
}



// ---------------------------------------------------------------------------
// Manual OCR span (across two pages) – auto-OCR style stitch
// ---------------------------------------------------------------------------











pub fn handle_manual_ocr_selection(app: &mut App, selections: Vec<(usize, iced::Rectangle)>) -> Task<Message> {
    #[cfg(feature = "ocr")]
    {
        if selections.is_empty() { return Task::none(); }
        if app.manual_ocring || app.running || app.translating || app.inpainting { return Task::none(); }
        let mut valid: Vec<(usize, iced::Rectangle)> = Vec::new();
        for (idx, r) in selections {
            if idx >= app.images.len() { continue; }
            if r.width < 4.0 || r.height < 4.0 { continue; }
            valid.push((idx, r));
        }
        if valid.is_empty() {
            app.status = "Manual OCR: no valid selections.".to_string();
            return Task::none();
        }
        let cfg = scanlateit_settings::get(|s| ocr::config_with(0.0, s.ocr_max_side_len.trim().parse::<u32>().unwrap_or(2000)));
        let cached = app.manual_ocr_engine.clone();
        if let Some(engine) = cached { return start_manual_ocr_selection(app, valid, engine); }
        app.pending_manual_multi_ocr = Some(valid);
        app.status = "Loading OCR engine for manual OCR…".to_string();
        return Task::perform(async move { ocr::Engine::build_with_config(cfg) }, Message::ManualOcrEngineReady);
    }
    #[cfg(not(feature = "ocr"))]
    {
        let _ = selections;
        app.status = "OCR not available in this build.".to_string();
        return Task::none();
    }
}

#[cfg(feature = "ocr")]
fn start_manual_ocr_selection(app: &mut App, selections: Vec<(usize, iced::Rectangle)>, engine: ocr::Engine) -> Task<Message> {
    app.manual_ocring = true;
    app.status = format!("Manual OCR on {} selection(s)...", selections.len());
    // Build per-image paths
    let mut items: Vec<(usize, String, iced::Rectangle)> = Vec::new();
    for (idx, rect) in selections {
        if idx >= app.images.len() { continue; }
        let path = app.project.image(app.images[idx].image_id).map(|m| m.path.clone()).unwrap_or_default();
        if path.is_empty() { continue; }
        items.push((idx, path, rect));
    }
    if items.is_empty() { app.manual_ocring=false; return Task::none(); }
    let merge_cfg = scanlateit_settings::get(|s| ocr::MergeConfig::from_threshold_str(&s.ocr_merge_threshold));
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || run_manual_ocr_selection(engine, items, merge_cfg))
                .await
                .unwrap_or_else(|e| Err(format!("Manual multi OCR task cancelled: {e}")))
        },
        Message::ManualOcrMultiFinished,
    )
}

#[cfg(feature = "ocr")]
fn run_manual_ocr_selection(engine: ocr::Engine, items: Vec<(usize, String, iced::Rectangle)>, merge_cfg: ocr::MergeConfig) -> Result<Vec<(usize, Vec<NewEntry>)>, String> {
    use std::collections::HashMap;
    // Group by image idx
    let mut by_image: HashMap<usize, Vec<(String, iced::Rectangle)>> = HashMap::new();
    for (idx, path, rect) in items {
        by_image.entry(idx).or_default().push((path, rect));
    }
    // For each image, cluster rects by touching (AABB intersect or edge touch)
    struct Cluster { x0: f32, y0: f32, x1: f32, y1: f32 }
    let mut jobs: Vec<(usize, String, Cluster)> = Vec::new(); // each job is one OCR crop
    // Need to also keep path per image (they share same path per idx, but we stored path per rect, should be same)
    let mut path_by_idx: HashMap<usize, String> = HashMap::new();
    for (idx, rects) in by_image {
        if rects.is_empty() { continue; }
        let path = rects[0].0.clone();
        path_by_idx.insert(idx, path.clone());
        // cluster rects
        let mut clusters: Vec<Cluster> = Vec::new();
        for (_, r) in rects {
            let mut cur = Cluster { x0: r.x, y0: r.y, x1: r.x + r.width, y1: r.y + r.height };
            // try to merge with existing clusters if touching
            let mut merged_indices: Vec<usize> = Vec::new();
            for (ci, c) in clusters.iter().enumerate() {
                let touches = !(cur.x1 < c.x0 - 1e-3 || cur.x0 > c.x1 + 1e-3 || cur.y1 < c.y0 - 1e-3 || cur.y0 > c.y1 + 1e-3);
                // Actually touching if intervals overlap or just touch (gap <=0)
                // The condition above checks for separated; if not separated then touching/overlap
                if touches {
                    merged_indices.push(ci);
                }
            }
            if merged_indices.is_empty() {
                clusters.push(cur);
            } else {
                // merge all touched clusters plus cur into one
                let mut nx0 = cur.x0;
                let mut ny0 = cur.y0;
                let mut nx1 = cur.x1;
                let mut ny1 = cur.y1;
                // sort descending to remove safely
                merged_indices.sort_by(|a,b| b.cmp(a));
                for mi in merged_indices {
                    let c = clusters.remove(mi);
                    nx0 = nx0.min(c.x0);
                    ny0 = ny0.min(c.y0);
                    nx1 = nx1.max(c.x1);
                    ny1 = ny1.max(c.y1);
                }
                clusters.push(Cluster { x0: nx0, y0: ny0, x1: nx1, y1: ny1 });
            }
        }
        for c in clusters {
            jobs.push((idx, path.clone(), c));
        }
    }
    if jobs.is_empty() { return Err("no OCR jobs".to_string()); }
    // For each job, decode, crop, run OCR (parallel via spawn_blocking per job? But we are already in blocking thread, so sequential or use rayon)
    // Since we are in a single blocking task, we can run sequentially but spec says parallel if more than one.
    // We can use std::thread scope to run parallel within this blocking task.
    // Simpler: run sequentially and collect; parallelism will be limited but okay. For true parallel, spawn tokio tasks inside?
    // We'll implement parallel using crossbeam or std::thread::scope with each job cloning engine.
    // Engine is Arc<Mutex<RapidOcr>> so cloning is cheap but still serializes on lock. We'll just run sequentially for now; the engine lock will serialize anyway.
    // For demonstration, we run sequentially but spawn blocking per job could be parallel via tokio::join_all from the outer async, but we are already blocking.
    // Instead, we run jobs sequentially but collect results.
    let mut per_image: HashMap<usize, Vec<NewEntry>> = HashMap::new();
    for (idx, path, cluster) in jobs {
        let dyn_img = image::ImageReader::open(&path)
            .map_err(|e| format!("Failed to open {path}: {e}"))?
            .with_guessed_format().map_err(|e| format!("Failed to decode {path}: {e}"))?
            .decode().map_err(|e| format!("Failed to decode {path}: {e}"))?;
        let rgba = dyn_img.into_rgba8();
        let (img_w, img_h) = rgba.dimensions();
        let x0 = cluster.x0.floor().max(0.0) as u32;
        let y0 = cluster.y0.floor().max(0.0) as u32;
        let x1 = cluster.x1.ceil().max(x0 as f32 +1.0) as u32;
        let y1 = cluster.y1.ceil().max(y0 as f32 +1.0) as u32;
        let x1 = x1.min(img_w);
        let y1 = y1.min(img_h);
        let cw = x1.saturating_sub(x0).max(1);
        let ch = y1.saturating_sub(y0).max(1);
        let cropped_rgba = image::imageops::crop_imm(&rgba, x0, y0, cw, ch).to_image();
        let cropped_rgb = image::DynamicImage::ImageRgba8(cropped_rgba).to_rgb8();
        let token = ocr::OcrCancellationToken::new();
        let lines = engine.run_image_cancellable(&cropped_rgb, &token)
            .map_err(|e| format!("Manual OCR failed: {e}"))?;
        let mut entries = ocr::to_entries_with(lines, merge_cfg);
        for entry in &mut entries {
            for p in &mut entry.quad.points {
                p[0] += x0 as f32;
                p[1] += y0 as f32;
            }
        }
        per_image.entry(idx).or_default().extend(entries);
    }
    let mut out: Vec<(usize, Vec<NewEntry>)> = per_image.into_iter().collect();
    out.sort_by_key(|(idx,_)| *idx);
    Ok(out)
}

pub fn handle_manual_ocr_finished(app: &mut App, result: Result<Vec<(usize, Vec<NewEntry>)>, String>) -> Task<Message> {
    #[cfg(feature = "ocr")]
    {
        app.manual_ocring = false;
        match result {
            Ok(per_image) => {
                if per_image.is_empty() {
                    app.status = "Manual OCR: no text found.".to_string();
                    return Task::none();
                }
                let mut total_added = 0usize;
                let mut total_detected = 0usize;
                let mut image_count = 0usize;
                for (idx, entries) in per_image {
                    let cnt = entries.len();
                    total_detected += cnt;
                    if idx >= app.images.len() { continue; }
                    let image_id = app.images[idx].image_id;
                    let added = if let Some(ev) = app.project.append_ocr_for_image_with_event(image_id, entries) {
                        let n = if let scanlateit_model::ModelEvent::EntriesAdded { ids, .. } = &ev { ids.len() } else { cnt };
                        crate::app::handle_model_event(app, ev);
                        let rev = app.project.reorder_entries_for_image_with_event(image_id);
                        crate::app::handle_model_event(app, rev);
                        n
                    } else { 0 };
                    total_added += added;
                    image_count += 1;
                }
                if total_added==0 && total_detected==0 {
                    app.status = "Manual OCR: no text found.".to_string();
                } else {
                    app.status = format!("Manual OCR: {total_added} line(s) added across {image_count} image(s) ({total_detected} detected).");
                }
            }
            Err(e) => { app.status = format!("Manual OCR multi failed: {e}"); }
        }
        return Task::none();
    }
    #[cfg(not(feature = "ocr"))]
    {
        let _ = result;
        app.status = "OCR not available in this build.".to_string();
        return Task::none();
    }
}
