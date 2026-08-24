use iced::Task;
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

#[cfg(feature = "ocr")]
pub fn handle_start_ocr(app: &mut App) -> Task<Message> {
    if app.images.is_empty() {
        app.status = "Open images first.".to_string();
        return Task::none();
    }
    if app.running {
        return Task::none();
    }
    app.running = true;
    let dims: Vec<(u32, u32)> = app
        .images
        .iter()
        .map(|image| {
            app.project
                .image(image.image_id)
                .map(|m| (m.width as u32, m.height as u32))
                .unwrap_or((0, 0))
        })
        .collect();
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
    app.status = format!(
        "Running OCR on {} run(s) covering {} image(s)...",
        run_count,
        app.images.len()
    );
    if app.pipeline.is_none() {
        let (workers, cfg) = scanlateit_settings::get(|s| {
            let workers = s.ocr_workers.parse::<usize>().unwrap_or(2).max(1);
            let cfg = ocr::config_from_strings(&s.ocr_text_score, &s.ocr_max_side_len);
            (workers, cfg)
        });
        app.status = format!(
            "Loading the OCR engine ({workers} detection worker(s))..."
        );
        return Task::perform(
            async move { ParallelEngine::build_with_config(cfg, workers) },
            Message::ParallelEngineReady,
        );
    }
    maybe_start_ocr(app)
}

#[cfg(not(feature = "ocr"))]
pub fn handle_start_ocr(app: &mut App) -> Task<Message> {
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
    // Collect ids first to avoid holding an immutable borrow across the mutable
    // Project mutation + handle_model_event (&mut App).
    let image_ids: Vec<_> = app.images.iter().map(|i| i.image_id).collect();
    for image_id in image_ids {
        let entries = fake_ocr_entries();
        let cnt = entries.len();
        if let Some(ev) = app.project.append_ocr_for_image_with_event(image_id, entries) {
            if let scanlateit_model::ModelEvent::EntriesAdded { ids, .. } = &ev {
                added += ids.len();
            } else {
                added += cnt;
            }
            crate::app::handle_model_event(app, ev);
        }
    }
    app.running = false;
    app.status = format!("Fake OCR done: {added} line(s) (no OCR engine in this build).");
    Task::none()
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

#[cfg(feature = "ocr")]
pub fn handle_stop_ocr(app: &mut App) -> Task<Message> {
    if let Some(token) = &app.cancel {
        token.cancel();
    }
    app.running = false;
    app.status = "Cancelling OCR...".to_string();
    Task::none()
}

#[cfg(not(feature = "ocr"))]
pub fn handle_stop_ocr(app: &mut App) -> Task<Message> {
    app.status = "OCR is not available in this build.".to_string();
    Task::none()
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
                        tasks.push(super::styling::classify_entries(app));
                    }
                    #[cfg(feature = "inpaint")]
                    if do_inpaint && !do_style {
                        tasks.push(super::inpaint::dispatch_auto_inpaint_solo(app, effective_model));
                    }
                    #[cfg(all(feature = "styling", feature = "inpaint"))]
                    if do_style && do_inpaint {
                        tasks.push(super::styling::start_pipeline_style_deferred(app));
                    }
                }
            } else {
                #[cfg(all(feature = "styling", feature = "inpaint"))]
                if do_style && do_inpaint {
                    tasks.push(super::styling::start_pipeline_style_deferred(app));
                }
                #[cfg(feature = "styling")]
                if do_style && !do_inpaint {
                    tasks.push(super::styling::classify_entries(app));
                }
                #[cfg(feature = "inpaint")]
                if do_inpaint && !do_style {
                    tasks.push(super::inpaint::dispatch_auto_inpaint_solo(app, effective_model));
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
fn start_manual_ocr(app: &mut App, index: usize, rect: iced::Rectangle, engine: ocr::Engine) -> Task<Message> {
    app.manual_ocring = true;
    app.status = format!("Manual OCR on image {} …", index + 1);
    let path = app
        .project
        .image(app.images[index].image_id)
        .map(|m| m.path.clone())
        .unwrap_or_default();
    let rect_copy = rect;
    // Manual OCR: no min/max height filter and no confidence filter (text_score = 0).
    // Keep merge threshold and max side len from settings, but force text_score 0.
    let (merge_cfg, _cfg) = scanlateit_settings::get(|s| {
        (
            ocr::MergeConfig::from_threshold_str(&s.ocr_merge_threshold),
            ocr::config_with(
                0.0,
                s.ocr_max_side_len.trim().parse::<u32>().unwrap_or(2000),
            ),
        )
    });
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || -> Result<Vec<NewEntry>, String> {
                // Decode image exactly as stored on disk (EXIF honored by OCR's loader, but for manual crop we use direct decode)
                let dyn_img = image::ImageReader::open(&path)
                    .map_err(|e| format!("Failed to open {path}: {e}"))?
                    .with_guessed_format()
                    .map_err(|e| format!("Failed to decode {path}: {e}"))?
                    .decode()
                    .map_err(|e| format!("Failed to decode {path}: {e}"))?;
                let rgba = dyn_img.into_rgba8();
                let (img_w, img_h) = rgba.dimensions();
                // Integer crop exactly covering `rect` (no padding). Use floor for origin, ceil for far edge, clamped to image bounds.
                let x0f = rect_copy.x.floor().max(0.0);
                let y0f = rect_copy.y.floor().max(0.0);
                let x1f = (rect_copy.x + rect_copy.width).ceil().max(x0f + 1.0);
                let y1f = (rect_copy.y + rect_copy.height).ceil().max(y0f + 1.0);
                let x0 = (x0f as u32).min(img_w.saturating_sub(1));
                let y0 = (y0f as u32).min(img_h.saturating_sub(1));
                let x1 = (x1f as u32).min(img_w);
                let y1 = (y1f as u32).min(img_h);
                let cw = x1.saturating_sub(x0).max(1);
                let ch = y1.saturating_sub(y0).max(1);
                // Crop without any context expansion – unlike inpaint which adds 32px.
                let cropped_rgba = image::imageops::crop_imm(&rgba, x0, y0, cw, ch).to_image();
                let cropped_rgb = image::DynamicImage::ImageRgba8(cropped_rgba).to_rgb8();
                let token = ocr::OcrCancellationToken::new();
                let lines = engine
                    .run_image_cancellable(&cropped_rgb, &token)
                    .map_err(|e| format!("Manual OCR failed: {e}"))?;
                // No height/confidence filter for manual OCR: use raw lines.
                let mut entries = ocr::to_entries_with(lines, merge_cfg);
                // Translate from crop-local to original image coordinates (pixel-perfect mapping).
                for entry in &mut entries {
                    for p in &mut entry.quad.points {
                        p[0] += x0 as f32;
                        p[1] += y0 as f32;
                    }
                }
                Ok(entries)
            })
            .await
            .unwrap_or_else(|e| Err(format!("Manual OCR task cancelled: {e}")))
        },
        move |res| Message::ManualOcrFinished(index, res),
    )
}

#[cfg(feature = "ocr")]
pub fn handle_manual_ocr_toggle(app: &mut App) -> Task<Message> {
    use super::edit::clear_editing;
    if app.manual_ocring || app.running || app.translating {
        return Task::none();
    }
    if app.images.is_empty() {
        return Task::none();
    }
    clear_editing(app);
    app.ocr_mode = !app.ocr_mode;
    if app.ocr_mode {
        // Mutually exclusive with inpaint
        app.inpaint_mode = false;
        app.status = "Manual OCR: drag a rectangle over the text to OCR; click Manual OCR again to cancel.".to_string();
    } else {
        app.status = "Manual OCR cancelled.".to_string();
    }
    Task::none()
}

#[cfg(not(feature = "ocr"))]
pub fn handle_manual_ocr_toggle(app: &mut App) -> Task<Message> {
    app.status = "OCR is not available in this build.".to_string();
    Task::none()
}

#[cfg(feature = "ocr")]
pub fn handle_manual_ocr_selection(app: &mut App, index: usize, rect: iced::Rectangle) -> Task<Message> {
    if app.manual_ocring || app.running || app.translating || app.inpainting {
        return Task::none();
    }
    if index >= app.images.len() {
        return Task::none();
    }
    if rect.width < 4.0 || rect.height < 4.0 {
        app.status = "Manual OCR: selection too small.".to_string();
        return Task::none();
    }
    // Manual OCR engine: force text_score 0 (no confidence filter), keep max_side_len from settings.
    let cfg = scanlateit_settings::get(|s| {
        ocr::config_with(
            0.0,
            s.ocr_max_side_len.trim().parse::<u32>().unwrap_or(2000),
        )
    });
    let cached = app.manual_ocr_engine.clone();
    // Simple cache: reuse if any engine exists (settings change will rebuild on next selection via pending path)
    if let Some(engine) = cached {
        return start_manual_ocr(app, index, rect, engine);
    }
    app.pending_manual_ocr = Some((index, rect));
    app.status = "Loading OCR engine for manual OCR…".to_string();
    Task::perform(async move { ocr::Engine::build_with_config(cfg) }, Message::ManualOcrEngineReady)
}

#[cfg(not(feature = "ocr"))]
pub fn handle_manual_ocr_selection(app: &mut App, _index: usize, _rect: iced::Rectangle) -> Task<Message> {
    app.status = "OCR is not available in this build.".to_string();
    Task::none()
}

#[cfg(feature = "ocr")]
pub fn handle_manual_ocr_engine_ready(app: &mut App, result: Result<ocr::Engine, String>) -> Task<Message> {
    match result {
        Ok(engine) => {
            app.manual_ocr_engine = Some(engine.clone());
            if let Some(spans) = app.pending_manual_ocr_span.take() {
                return start_manual_ocr_span(app, spans, engine.clone());
            }
            if let Some((index, rect)) = app.pending_manual_ocr.take() {
                return start_manual_ocr(app, index, rect, engine);
            }
            Task::none()
        }
        Err(e) => {
            app.pending_manual_ocr = None;
            app.pending_manual_ocr_span = None;
            app.status = format!("Manual OCR engine failed: {e}");
            Task::none()
        }
    }
}

#[cfg(feature = "ocr")]
pub fn handle_manual_ocr_finished(app: &mut App, index: usize, result: Result<Vec<NewEntry>, String>) -> Task<Message> {
    app.manual_ocring = false;
    app.ocr_mode = false;
    match result {
        Ok(entries) => {
            if entries.is_empty() {
                app.status = format!("Manual OCR: no text found in selection on image {}.", index + 1);
                return Task::none();
            }
            let count = entries.len();
            if index < app.images.len() {
                let image_id = app.images[index].image_id;
                let mut added = 0;
                if let Some(ev) = app.project.append_ocr_for_image_with_event(image_id, entries) {
                    if let scanlateit_model::ModelEvent::EntriesAdded { ids, .. } = &ev {
                        added = ids.len();
                    } else {
                        added = count;
                    }
                    crate::app::handle_model_event(app, ev);
                }
                // Request reorder by manual OCR orchestrator: keep translation reading order correct (per-image)
                let ev = app.project.reorder_entries_for_image_with_event(image_id);
                crate::app::handle_model_event(app, ev);
                app.status = format!("Manual OCR: {added} line(s) added to image {} ({} detected).", index + 1, count);
            } else {
                app.status = format!("Manual OCR: {count} line(s) detected (image no longer exists).");
            }
        }
        Err(e) => {
            app.status = format!("Manual OCR failed: {e}");
        }
    }
    Task::none()
}

// ---------------------------------------------------------------------------
// Manual OCR span (across two pages) – auto-OCR style stitch
// ---------------------------------------------------------------------------

#[cfg(feature = "ocr")]
fn start_manual_ocr_span(
    app: &mut App,
    spans: Vec<(usize, iced::Rectangle)>,
    engine: ocr::Engine,
) -> Task<Message> {
    // spans are already sorted and trimmed to 2 consecutive by the viewer; enforce again.
    let mut spans = spans;
    spans.sort_by_key(|(idx, _)| *idx);
    if spans.len() > 2 {
        spans.truncate(2);
    }
    if spans.is_empty() {
        return Task::none();
    }
    if spans.len() == 1 {
        let (idx, r) = spans.into_iter().next().unwrap();
        return start_manual_ocr(app, idx, r, engine);
    }
    app.manual_ocring = true;
    app.status = format!(
        "Manual OCR on images {} & {} (stitched)…",
        spans[0].0 + 1,
        spans[1].0 + 1
    );
    // Resolve paths and keep rect copies.
    let mut items: Vec<(usize, String, iced::Rectangle)> = Vec::new();
    for (idx, rect) in spans {
        if idx >= app.images.len() {
            continue;
        }
        let path = app
            .project
            .image(app.images[idx].image_id)
            .map(|m| m.path.clone())
            .unwrap_or_default();
        if path.is_empty() {
            continue;
        }
        items.push((idx, path, rect));
    }
    if items.len() < 2 {
        // fallback to single if one path missing
        if let Some((idx, _, rect)) = items.into_iter().next() {
            // need rect; reconstruct from original spans fallback
            return start_manual_ocr(app, idx, rect, engine);
        }
        app.manual_ocring = false;
        return Task::none();
    }
    let merge_cfg = scanlateit_settings::get(|s| ocr::MergeConfig::from_threshold_str(&s.ocr_merge_threshold));
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || -> Result<Vec<(usize, Vec<NewEntry>)>, String> {
                // Decode and crop each selection exactly (no padding), like single manual OCR.
                struct PieceMeta {
                    idx: usize,
                    x0: u32,
                    y0: u32,
                    cw: u32,
                    ch: u32,
                    crop: image::RgbaImage,
                }
                let mut pieces: Vec<PieceMeta> = Vec::new();
                for (idx, path, rect) in items {
                    let dyn_img = image::ImageReader::open(&path)
                        .map_err(|e| format!("Failed to open {path}: {e}"))?
                        .with_guessed_format()
                        .map_err(|e| format!("Failed to decode {path}: {e}"))?
                        .decode()
                        .map_err(|e| format!("Failed to decode {path}: {e}"))?;
                    let rgba = dyn_img.into_rgba8();
                    let (img_w, img_h) = rgba.dimensions();
                    let x0f = rect.x.floor().max(0.0);
                    let y0f = rect.y.floor().max(0.0);
                    let x1f = (rect.x + rect.width).ceil().max(x0f + 1.0);
                    let y1f = (rect.y + rect.height).ceil().max(y0f + 1.0);
                    let x0 = (x0f as u32).min(img_w.saturating_sub(1));
                    let y0 = (y0f as u32).min(img_h.saturating_sub(1));
                    let x1 = (x1f as u32).min(img_w);
                    let y1 = (y1f as u32).min(img_h);
                    let cw = x1.saturating_sub(x0).max(1);
                    let ch = y1.saturating_sub(y0).max(1);
                    // Clamp cw/ch to image bounds and respect MIN_OCR_EDGE already filtered
                    let crop = image::imageops::crop_imm(&rgba, x0, y0, cw, ch).to_image();
                    pieces.push(PieceMeta { idx, x0, y0, cw, ch, crop });
                }
                if pieces.len() < 2 {
                    return Err("manual OCR span: not enough valid pieces".to_string());
                }
                pieces.sort_by_key(|p| p.idx);
                // Auto-OCR style: common width = first piece's width (first page width).
                let common_w = pieces[0].cw;
                if common_w == 0 {
                    return Err("manual OCR span: zero width".to_string());
                }
                // Build scaled pieces for stitching.
                struct ScaledPiece {
                    idx: usize,
                    x0: u32,
                    y0: u32,
                    cw: u32,
                    #[allow(dead_code)]
                    ch: u32,
                    #[allow(dead_code)]
                    scaled_w: u32,
                    scaled_h: u32,
                    off_y: u32,
                    image: image::RgbaImage,
                }
                let mut scaled: Vec<ScaledPiece> = Vec::new();
                let mut total_h: u32 = 0;
                for p in pieces {
                    let scaled_h = if p.cw == common_w {
                        p.ch
                    } else {
                        ((p.ch as f32 * common_w as f32 / p.cw as f32).round().max(1.0)) as u32
                    };
                    let scaled_img = if p.cw == common_w {
                        p.crop
                    } else {
                        image::imageops::resize(&p.crop, common_w, scaled_h, image::imageops::FilterType::Triangle)
                    };
                    let off_y = total_h;
                    total_h += scaled_h;
                    scaled.push(ScaledPiece {
                        idx: p.idx,
                        x0: p.x0,
                        y0: p.y0,
                        cw: p.cw,
                        ch: p.ch,
                        scaled_w: common_w,
                        scaled_h,
                        off_y,
                        image: scaled_img,
                    });
                }
                // Stitch vertically.
                let mut stitched_rgba = image::RgbaImage::new(common_w, total_h);
                for s in &scaled {
                    image::imageops::replace(&mut stitched_rgba, &s.image, 0, s.off_y as i64);
                }
                let stitched_rgb = image::DynamicImage::ImageRgba8(stitched_rgba).to_rgb8();
                let token = ocr::OcrCancellationToken::new();
                let lines = engine
                    .run_image_cancellable(&stitched_rgb, &token)
                    .map_err(|e| format!("Manual OCR span failed: {e}"))?;
                let mut entries = ocr::to_entries_with(lines, merge_cfg);
                // Map stitched-local quads back to per-image original coordinates.
                // Use common_w as stitched width; for each entry decide piece by centroid y.
                let h0 = scaled[0].scaled_h as f32;
                let mut per_image: std::collections::HashMap<usize, Vec<NewEntry>> = std::collections::HashMap::new();
                for mut entry in entries.drain(..) {
                    // Compute centroid y in stitched space.
                    let ys: Vec<f32> = entry.quad.points.iter().map(|p| p[1]).collect();
                    let y_centroid = ys.iter().sum::<f32>() / ys.len() as f32;
                    let mut target_idx = 0usize;
                    if y_centroid >= h0 && scaled.len() > 1 {
                        target_idx = 1;
                    } else if scaled.len() > 1 {
                        // Check straddle: bbox crosses seam
                        let y0 = ys.iter().cloned().fold(f32::INFINITY, f32::min);
                        let y1 = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                        if y0 < h0 && y1 > h0 {
                            // Straddles seam – assign to side with larger overlap.
                            let above_h = h0 - y0;
                            let below_h = y1 - h0;
                            if below_h > above_h {
                                target_idx = 1;
                            } else {
                                target_idx = 0;
                            }
                            // Clip points that lie beyond target piece's range to avoid cross-piece mapping artifacts.
                            // For above choice, clamp y1 to h0; for below, clamp y0 to h0 before mapping.
                            // We'll map after clamping.
                        }
                    }
                    // Determine piece for mapping
                    let piece = if target_idx == 0 { &scaled[0] } else { &scaled[1 % scaled.len()] };
                    let factor = piece.cw as f32 / common_w as f32; // uniform scale inverse
                    let off_y = piece.off_y as f32;
                    // Map each point; also handle straddle clamp: if point y is outside piece's stitched range, clamp to edge.
                    for p in &mut entry.quad.points {
                        let y_s = p[1];
                        let is_in_piece = if target_idx == 0 { y_s < h0 } else { y_s >= h0 };
                        // For points outside target piece (straddle case), clamp y_s to seam before mapping to keep quad inside target image.
                        let y_clamped = if !is_in_piece {
                            if target_idx == 0 { (h0 - 0.5).min(y_s).max(0.0) } else { h0.max(y_s) }
                        } else {
                            y_s
                        };
                        let x_mapped = piece.x0 as f32 + p[0] * factor;
                        let y_mapped = piece.y0 as f32 + (y_clamped - off_y) * factor;
                        *p = [x_mapped, y_mapped];
                    }
                    per_image.entry(piece.idx).or_default().push(entry);
                }
                let mut out: Vec<(usize, Vec<NewEntry>)> = per_image.into_iter().collect();
                out.sort_by_key(|(idx, _)| *idx);
                // Ensure at least an entry per original span even if OCR found nothing? No – return as is, caller will show "no text".
                Ok(out)
            })
            .await
            .unwrap_or_else(|e| Err(format!("Manual OCR span task cancelled: {e}")))
        },
        Message::ManualOcrSpanFinished,
    )
}

#[cfg(feature = "ocr")]
pub fn handle_manual_ocr_span(app: &mut App, spans: Vec<(usize, iced::Rectangle)>) -> Task<Message> {
    if app.manual_ocring || app.running || app.translating || app.inpainting {
        return Task::none();
    }
    if spans.is_empty() {
        return Task::none();
    }
    let mut spans = spans;
    spans.sort_by_key(|(idx, _)| *idx);
    if spans.len() > 2 {
        spans.truncate(2);
    }
    // Require consecutive seam only.
    if spans.len() == 2 && spans[1].0 != spans[0].0 + 1 {
        app.status = "Manual OCR span: images must be consecutive.".to_string();
        return Task::none();
    }
    // Validate per-piece size (keep MIN_OCR_EDGE per piece as requested)
    for (_, r) in &spans {
        if r.width < 4.0 || r.height < 4.0 {
            app.status = "Manual OCR: selection too small.".to_string();
            return Task::none();
        }
        if r.width < 4.0 || r.height < 4.0 {
            return Task::none();
        }
    }
    if spans.len() == 1 {
        let (idx, rect) = spans.into_iter().next().unwrap();
        return handle_manual_ocr_selection(app, idx, rect);
    }
    // Build engine config same as single
    let cfg = scanlateit_settings::get(|s| {
        ocr::config_with(
            0.0,
            s.ocr_max_side_len.trim().parse::<u32>().unwrap_or(2000),
        )
    });
    let cached = app.manual_ocr_engine.clone();
    if let Some(engine) = cached {
        return start_manual_ocr_span(app, spans, engine);
    }
    app.pending_manual_ocr_span = Some(spans);
    app.status = "Loading OCR engine for manual OCR…".to_string();
    Task::perform(async move { ocr::Engine::build_with_config(cfg) }, Message::ManualOcrEngineReady)
}

#[cfg(feature = "ocr")]
pub fn handle_manual_ocr_span_finished(
    app: &mut App,
    result: Result<Vec<(usize, Vec<NewEntry>)>, String>,
) -> Task<Message> {
    app.manual_ocring = false;
    app.ocr_mode = false;
    match result {
        Ok(per_image) => {
            if per_image.is_empty() {
                app.status = "Manual OCR span: no text found.".to_string();
                return Task::none();
            }
            let image_count = per_image.len();
            let mut total_added = 0usize;
            let mut total_detected = 0usize;
            for (idx, entries) in per_image {
                let cnt = entries.len();
                total_detected += cnt;
                if idx >= app.images.len() {
                    continue;
                }
                let image_id = app.images[idx].image_id;
                let added = if let Some(ev) = app.project.append_ocr_for_image_with_event(image_id, entries) {
                    let n = if let scanlateit_model::ModelEvent::EntriesAdded { ids, .. } = &ev {
                        ids.len()
                    } else {
                        cnt
                    };
                    crate::app::handle_model_event(app, ev);
                    let rev = app.project.reorder_entries_for_image_with_event(image_id);
                    crate::app::handle_model_event(app, rev);
                    n
                } else {
                    0
                };
                total_added += added;
            }
            if total_added == 0 && total_detected == 0 {
                app.status = "Manual OCR span: no text found.".to_string();
            } else {
                app.status = format!(
                    "Manual OCR span: {total_added} line(s) added across {image_count} image(s) ({total_detected} detected)."
                );
            }
        }
        Err(e) => {
            app.status = format!("Manual OCR span failed: {e}");
        }
    }
    Task::none()
}

#[cfg(not(feature = "ocr"))]
pub fn handle_manual_ocr_span(app: &mut App, _spans: Vec<(usize, iced::Rectangle)>) -> Task<Message> {
    app.status = "OCR is not available in this build.".to_string();
    Task::none()
}

#[cfg(not(feature = "ocr"))]
pub fn handle_manual_ocr_span_finished(
    app: &mut App,
    _result: Result<Vec<(usize, Vec<NewEntry>)>, String>,
) -> Task<Message> {
    app.status = "OCR is not available in this build.".to_string();
    Task::none()
}
