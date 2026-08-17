use iced::Task;
use scanlateit_model::{EntryId, Quad};
#[cfg(feature = "inpaint")]
use scanlateit_inpaint::Engine as InpaintEngine;
#[cfg(feature = "inpaint")]
use scanlateit_settings::InpaintBackend;
#[cfg(feature = "inpaint")]
use scanlateit_ui::loaded::InpaintLayer;
#[cfg(feature = "inpaint")]
use image::RgbaImage;

use super::{App, AutoInpaintJob, Message};

#[cfg(feature = "inpaint")]
pub fn start_inpaint(
    app: &mut App,
    engine: InpaintEngine,
    index: usize,
    path: String,
    rect: [f32; 4],
    quads: Vec<Quad>,
) -> Task<Message> {
    app.inpainting = true;
    app.status = "inpainting...".to_string();
    Task::perform(
        async move {
            let result = tokio::task::spawn_blocking(move || {
                engine.run_blocking(&path, rect, &quads)
            })
            .await
            .unwrap_or_else(|e| Err(format!("inpaint task cancelled: {e}")));
            (index, result)
        },
        |(index, result)| Message::InpaintFinished(index, result),
    )
}

#[cfg(feature = "inpaint")]
pub fn dispatch_auto_telea_jobs(app: &mut App, jobs: Vec<AutoInpaintJob>) -> Task<Message> {
    if jobs.is_empty() {
        return Task::none();
    }
    let radius = scanlateit_settings::get(|s| s.inpaint_radius.parse::<i32>().unwrap_or(5).max(1));
    let pad = auto_pad_for(InpaintBackend::Telea, radius);
    let cached = app.auto_telea_engine.clone().filter(|e| e.radius() == radius);
    if let Some(engine) = cached {
        app.auto_inpaint_pending += jobs.len();
        app.status = format!("Auto-inpaint (Telea) {} regions in parallel...", jobs.len());
        // Precompute neighbor paths for seam stitching
        let neighbor_map: std::collections::HashMap<usize, (Option<String>, Option<String>)> = {
            let mut map = std::collections::HashMap::new();
            for job in &jobs {
                let prev = if job.index > 0 {
                    app.images.get(job.index - 1).and_then(|img| app.project.image(img.image_id).map(|m| m.path.clone()))
                } else { None };
                let next = if job.index + 1 < app.images.len() {
                    app.images.get(job.index + 1).and_then(|img| app.project.image(img.image_id).map(|m| m.path.clone()))
                } else { None };
                map.insert(job.index, (prev, next));
            }
            map
        };
        let tasks: Vec<Task<Message>> = jobs
            .into_iter()
            .map(|job| {
                let engine = engine.clone();
                let pad = pad;
                let (prev_path, next_path) = neighbor_map.get(&job.index).cloned().unwrap_or((None, None));
                let idx = job.index;
                let id = job.id;
                Task::perform(
                    async move {
                        let res = tokio::task::spawn_blocking(move || {
                            run_auto_job_with_stitch(&engine, &job, pad, prev_path.as_deref(), next_path.as_deref())
                        })
                        .await
                        .unwrap_or_else(|e| Err(format!("inpaint task cancelled: {e}")));
                        (idx, id, res)
                    },
                    |(idx, id, res)| Message::AutoInpaintFinished(idx, id, res),
                )
            })
            .collect();
        Task::batch(tasks)
    } else {
        app.pending_auto_telea_jobs = Some(jobs);
        app.status = "Loading Telea for auto-inpaint...".to_string();
        Task::perform(
            async move { InpaintEngine::build(InpaintBackend::Telea, radius) },
            move |r| Message::AutoInpaintEngineReady(InpaintBackend::Telea, r),
        )
    }
}

#[cfg(feature = "inpaint")]
pub fn dispatch_auto_lama_jobs(app: &mut App, jobs: Vec<AutoInpaintJob>) -> Task<Message> {
    if jobs.is_empty() {
        return Task::none();
    }
    let radius = scanlateit_settings::get(|s| s.inpaint_radius.parse::<i32>().unwrap_or(5).max(1));
    let pad = auto_pad_for(InpaintBackend::Lama, radius);
    let cached = app.auto_lama_engine.clone().filter(|e| e.radius() == radius);
    if let Some(engine) = cached {
        app.auto_inpaint_pending += jobs.len();
        app.status = format!("Auto-inpaint (LaMa) {} regions sequentially...", jobs.len());
        // Precompute neighbor paths per job for seam stitching
        let enriched: Vec<(AutoInpaintJob, Option<String>, Option<String>)> = jobs.into_iter().map(|job| {
            let prev = if job.index > 0 {
                app.images.get(job.index - 1).and_then(|img| app.project.image(img.image_id).map(|m| m.path.clone()))
            } else { None };
            let next = if job.index + 1 < app.images.len() {
                app.images.get(job.index + 1).and_then(|img| app.project.image(img.image_id).map(|m| m.path.clone()))
            } else { None };
            (job, prev, next)
        }).collect();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let mut out: Vec<(usize, EntryId, Result<Vec<(image::RgbaImage, [f32; 4])>, String>)> = Vec::new();
                    for (job, prev_path, next_path) in enriched {
                        let r = run_auto_job_with_stitch(&engine, &job, pad, prev_path.as_deref(), next_path.as_deref());
                        out.push((job.index, job.id, r));
                    }
                    out
                })
                .await
                .unwrap_or_else(|e| vec![(0, EntryId(0), Err(format!("lama batch cancelled: {e}")))])
            },
            Message::AutoInpaintLamaBatchFinished,
        )
    } else {
        app.pending_auto_lama_jobs = Some(jobs);
        app.status = "Loading LaMa for auto-inpaint...".to_string();
        Task::perform(
            async move { InpaintEngine::build(InpaintBackend::Lama, radius) },
            move |r| Message::AutoInpaintEngineReady(InpaintBackend::Lama, r),
        )
    }
}

#[cfg(feature = "inpaint")]
pub fn dispatch_auto_aot_jobs(app: &mut App, jobs: Vec<AutoInpaintJob>) -> Task<Message> {
    if jobs.is_empty() {
        return Task::none();
    }
    let radius = scanlateit_settings::get(|s| s.inpaint_radius.parse::<i32>().unwrap_or(5).max(1));
    let pad = auto_pad_for(InpaintBackend::Aot, radius);
    let cached = app.auto_aot_engine.clone().filter(|e| e.radius() == radius);
    if let Some(engine) = cached {
        app.auto_inpaint_pending += jobs.len();
        app.status = format!("Auto-inpaint (AOT-GAN) {} regions sequentially...", jobs.len());
        let enriched: Vec<(AutoInpaintJob, Option<String>, Option<String>)> = jobs.into_iter().map(|job| {
            let prev = if job.index > 0 {
                app.images.get(job.index - 1).and_then(|img| app.project.image(img.image_id).map(|m| m.path.clone()))
            } else { None };
            let next = if job.index + 1 < app.images.len() {
                app.images.get(job.index + 1).and_then(|img| app.project.image(img.image_id).map(|m| m.path.clone()))
            } else { None };
            (job, prev, next)
        }).collect();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let mut out: Vec<(usize, EntryId, Result<Vec<(image::RgbaImage, [f32; 4])>, String>)> = Vec::new();
                    for (job, prev_path, next_path) in enriched {
                        let r = run_auto_job_with_stitch(&engine, &job, pad, prev_path.as_deref(), next_path.as_deref());
                        out.push((job.index, job.id, r));
                    }
                    out
                })
                .await
                .unwrap_or_else(|e| vec![(0, EntryId(0), Err(format!("aot batch cancelled: {e}")))])
            },
            Message::AutoInpaintAotBatchFinished,
        )
    } else {
        app.pending_auto_aot_jobs = Some(jobs);
        app.status = "Loading AOT-GAN for auto-inpaint...".to_string();
        Task::perform(
            async move { InpaintEngine::build(InpaintBackend::Aot, radius) },
            move |r| Message::AutoInpaintEngineReady(InpaintBackend::Aot, r),
        )
    }
}

#[cfg(feature = "inpaint")]
pub fn dispatch_auto_inpaint_solo(app: &mut App, effective_model: scanlateit_settings::AutoInpaintModel) -> Task<Message> {
    let mut jobs: Vec<AutoInpaintJob> = Vec::new();
    for (index, image) in app.images.iter().enumerate() {
        let image_id = image.image_id;
        let path = app
            .project
            .image(image_id)
            .map(|m| m.path.clone())
            .unwrap_or_default();
        for entry in app.project.visible_for(image_id).collect::<Vec<_>>() {
            jobs.push(AutoInpaintJob {
                index,
                id: entry.id,
                path: path.clone(),
                quad: app.project.view_quad(entry),
            });
        }
    }
    if jobs.is_empty() {
        return Task::none();
    }
    for job in &jobs {
        let mut style = app.project.entry_style(job.id);
        style.bg_color = [0, 0, 0, 0];
        let ev = app.project.set_entry_style_with_event(job.id, style);
        crate::app::handle_model_event(app, ev);
    }
    match effective_model {
        scanlateit_settings::AutoInpaintModel::Telea => dispatch_auto_telea_jobs(app, jobs),
        scanlateit_settings::AutoInpaintModel::Lama => dispatch_auto_lama_jobs(app, jobs),
        scanlateit_settings::AutoInpaintModel::Aot => dispatch_auto_aot_jobs(app, jobs),
        scanlateit_settings::AutoInpaintModel::Mixed => dispatch_auto_telea_jobs(app, jobs),
    }
}

#[cfg(feature = "inpaint")]
pub fn handle_inpaint_engine_ready(app: &mut App, result: Result<InpaintEngine, String>) -> Task<Message> {
    match result {
        Ok(engine) => {
            app.inpaint_engine = Some(engine.clone());
            if let Some(spans) = app.pending_inpaint_span.take() {
                return start_inpaint_span(app, engine, spans);
            }
            match app.pending_inpaint.take() {
                Some((index, path, rect, quads)) => {
                    start_inpaint(app, engine, index, path, rect, quads)
                }
                None => Task::none(),
            }
        }
        Err(e) => {
            app.pending_inpaint = None;
            app.pending_inpaint_span = None;
            app.status = e;
            Task::none()
        }
    }
}

#[cfg(feature = "inpaint")]
pub fn handle_auto_engine_ready(app: &mut App, backend: InpaintBackend, result: Result<InpaintEngine, String>) -> Task<Message> {
    match result {
        Ok(engine) => {
            match backend {
                InpaintBackend::Telea => {
                    app.auto_telea_engine = Some(engine.clone());
                    if let Some(jobs) = app.pending_auto_telea_jobs.take() {
                        return dispatch_auto_telea_jobs(app, jobs);
                    }
                }
                InpaintBackend::Lama => {
                    app.auto_lama_engine = Some(engine.clone());
                    if let Some(jobs) = app.pending_auto_lama_jobs.take() {
                        return dispatch_auto_lama_jobs(app, jobs);
                    }
                }
                InpaintBackend::Aot => {
                    app.auto_aot_engine = Some(engine.clone());
                    if let Some(jobs) = app.pending_auto_aot_jobs.take() {
                        return dispatch_auto_aot_jobs(app, jobs);
                    }
                }
            }
            Task::none()
        }
        Err(e) => {
            match backend {
                InpaintBackend::Telea => { app.pending_auto_telea_jobs = None; }
                InpaintBackend::Lama => { app.pending_auto_lama_jobs = None; }
                InpaintBackend::Aot => { app.pending_auto_aot_jobs = None; }
            }
            app.status = format!("Auto-inpaint engine failed: {e}");
            #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
            {
                app.pipeline_active = false;
            }
            Task::none()
        }
    }
}

#[cfg(feature = "inpaint")]
pub fn handle_auto_finished(app: &mut App, index: usize, id: EntryId, result: Result<Vec<(image::RgbaImage, [f32; 4])>, String>) -> Task<Message> {
    app.auto_inpaint_pending = app.auto_inpaint_pending.saturating_sub(1);
    let pending = app.auto_inpaint_pending;
    match result {
        Ok(patches) => {
            let Some(image_id) = app.images.get(index).map(|i| i.image_id) else {
                return Task::none();
            };
            let mut pending_evs = Vec::new();
            {
                let Some(image) = app.images.get_mut(index) else {
                    return Task::none();
                };
                for (patch, bounds) in patches {
                    let (width, height) = (patch.width(), patch.height());
                    let layer = InpaintLayer {
                        bounds,
                        handle: iced::widget::image::Handle::from_rgba(width, height, bytes::Bytes::from(patch.into_raw())),
                        width,
                        height,
                    };
                    image.inpaint.push(layer);
                    pending_evs.push(app.project.add_inpaint_patch(image_id, bounds));
                }
            }
            for ev in pending_evs {
                crate::app::handle_model_event(app, ev);
            }
            app.show_inpaint = true;
            if pending == 0 {
                #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
                {
                    app.pipeline_active = false;
                }
                app.status = format!("Auto-inpaint done. {}", app.status);
            } else {
                app.status = format!("Auto-inpaint: {} remaining. {}", pending, app.status);
            }
        }
        Err(e) => {
            app.status = format!("Auto-inpaint failed for {index}:{id:?}: {e}");
            if pending == 0 {
                #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
                { app.pipeline_active = false; }
            }
        }
    }
    Task::none()
}

#[cfg(feature = "inpaint")]
pub fn handle_auto_lama_batch(app: &mut App, batch: Vec<(usize, EntryId, Result<Vec<(image::RgbaImage, [f32; 4])>, String>)>) -> Task<Message> {
    for (index, id, result) in batch {
        app.auto_inpaint_pending = app.auto_inpaint_pending.saturating_sub(1);
        match result {
            Ok(patches) => {
                let Some(image_id) = app.images.get(index).map(|i| i.image_id) else { continue; };
                let mut pending_evs = Vec::new();
                {
                    if let Some(image) = app.images.get_mut(index) {
                        for (patch, bounds) in patches {
                            let (width, height) = (patch.width(), patch.height());
                            let layer = InpaintLayer {
                                bounds,
                                handle: iced::widget::image::Handle::from_rgba(width, height, bytes::Bytes::from(patch.into_raw())),
                                width,
                                height,
                            };
                            image.inpaint.push(layer);
                            pending_evs.push(app.project.add_inpaint_patch(image_id, bounds));
                        }
                    }
                }
                for ev in pending_evs { crate::app::handle_model_event(app, ev); }
                if app.images.get(index).is_some() {
                    app.show_inpaint = true;
                }
            }
            Err(e) => {
                app.status = format!("Auto-inpaint (LaMa) failed for {index}:{id:?}: {e}");
            }
        }
    }
    if app.auto_inpaint_pending == 0 {
        #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
        { app.pipeline_active = false; }
        app.status = format!("Auto-inpaint (LaMa batch) done. {}", app.status);
    }
    Task::none()
}

#[cfg(feature = "inpaint")]
pub fn handle_auto_aot_batch(app: &mut App, batch: Vec<(usize, EntryId, Result<Vec<(image::RgbaImage, [f32; 4])>, String>)>) -> Task<Message> {
    for (index, id, result) in batch {
        app.auto_inpaint_pending = app.auto_inpaint_pending.saturating_sub(1);
        match result {
            Ok(patches) => {
                let Some(image_id) = app.images.get(index).map(|i| i.image_id) else { continue; };
                let mut pending_evs = Vec::new();
                {
                    if let Some(image) = app.images.get_mut(index) {
                        for (patch, bounds) in patches {
                            let (width, height) = (patch.width(), patch.height());
                            let layer = InpaintLayer {
                                bounds,
                                handle: iced::widget::image::Handle::from_rgba(width, height, bytes::Bytes::from(patch.into_raw())),
                                width,
                                height,
                            };
                            image.inpaint.push(layer);
                            pending_evs.push(app.project.add_inpaint_patch(image_id, bounds));
                        }
                    }
                }
                for ev in pending_evs { crate::app::handle_model_event(app, ev); }
                if app.images.get(index).is_some() {
                    app.show_inpaint = true;
                }
            }
            Err(e) => {
                app.status = format!("Auto-inpaint (AOT) failed for {index}:{id:?}: {e}");
            }
        }
    }
    if app.auto_inpaint_pending == 0 {
        #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
        { app.pipeline_active = false; }
        app.status = format!("Auto-inpaint (AOT batch) done. {}", app.status);
    }
    Task::none()
}

#[cfg(feature = "inpaint")]
pub fn handle_inpaint_finished(app: &mut App, index: usize, result: Result<Vec<(image::RgbaImage, [f32; 4])>, String>) -> Task<Message> {
    app.inpainting = false;
    match result {
        Ok(patches) => {
            let Some(image_id) = app.images.get(index).map(|i| i.image_id) else {
                return Task::none();
            };
            let count = patches.len();
            let mut pending_evs = Vec::new();
            {
                let Some(image) = app.images.get_mut(index) else {
                    return Task::none();
                };
                for (patch, bounds) in patches {
                    let (width, height) = (patch.width(), patch.height());
                    let layer = InpaintLayer {
                        bounds,
                        handle: iced::widget::image::Handle::from_rgba(
                            width,
                            height,
                            bytes::Bytes::from(patch.into_raw()),
                        ),
                        width,
                        height,
                    };
                    image.inpaint.push(layer);
                    pending_evs.push(app.project.add_inpaint_patch(image_id, bounds));
                }
            }
            for ev in pending_evs { crate::app::handle_model_event(app, ev); }
            app.inpaint_mode = false;
            app.show_inpaint = true;
            app.status = format!("Inpainted {count} region(s).");
        }
        Err(e) => {
            app.status = e;
        }
    }
    Task::none()
}

#[cfg(feature = "inpaint")]
fn crop_spec_stitched(rect: [f32; 4], width: u32, height: u32) -> [u32; 4] {
    let [x0, y0, x1, y1] = [rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]];
    let x = x0.floor().clamp(0.0, width as f32 - 1.0) as u32;
    let y = y0.floor().clamp(0.0, height as f32 - 1.0) as u32;
    let x1c = x1.ceil().clamp(x as f32 + 1.0, width as f32) as u32;
    let y1c = y1.ceil().clamp(y as f32 + 1.0, height as f32) as u32;
    [x, y, x1c - x, y1c - y]
}

#[cfg(feature = "inpaint")]
pub fn start_inpaint_span(
    app: &mut App,
    engine: InpaintEngine,
    spans: Vec<(usize, String, [f32; 4], Vec<Quad>)>,
) -> Task<Message> {
    app.inpainting = true;
    app.status = "inpainting (stitched)...".to_string();
    Task::perform(
        async move {
            let result = tokio::task::spawn_blocking(move || run_stitched_inpaint(&engine, spans))
                .await
                .unwrap_or_else(|e| Err(format!("inpaint span task cancelled: {e}")));
            result
        },
        Message::InpaintSpanFinished,
    )
}

#[cfg(feature = "inpaint")]
fn run_stitched_inpaint(
    engine: &InpaintEngine,
    spans: Vec<(usize, String, [f32; 4], Vec<Quad>)>,
) -> Result<Vec<(usize, Vec<(image::RgbaImage, [f32; 4])>)>, String> {
    if spans.is_empty() {
        return Err("no spans".to_string());
    }
    const STITCH_W: u32 = 512;
    const STITCH_H: u32 = 512;
    // Decode full images and collect orig rects.
    struct Raw {
        idx: usize,
        full: image::RgbaImage,
        img_w: u32,
        img_h: u32,
        orig: [u32; 4], // [ox,oy,ow,oh] selection in image pixels
        quads: Vec<Quad>,
    }
    let mut raws: Vec<Raw> = Vec::new();
    for (idx, path, rect_arr, quads) in spans {
        let rgba = image::ImageReader::open(&path)
            .map_err(|e| format!("Failed to open {path}: {e}"))?
            .with_guessed_format()
            .map_err(|e| format!("Failed to decode {path}: {e}"))?
            .decode()
            .map_err(|e| format!("Failed to decode {path}: {e}"))?
            .into_rgba8();
        let (img_w, img_h) = rgba.dimensions();
        let [ox, oy, ow, oh] = crop_spec_stitched(rect_arr, img_w, img_h);
        if ow == 0 || oh == 0 {
            continue;
        }
        raws.push(Raw { idx, full: rgba, img_w, img_h, orig: [ox, oy, ow, oh], quads });
    }
    if raws.is_empty() {
        return Err("no valid crops".to_string());
    }
    raws.sort_by_key(|r| r.idx);
    if raws.len() > 2 {
        raws.truncate(2);
    }
    // Synthesize quads for mixed empty/non-empty so both tiles get inpainted.
    let has_any = raws.iter().any(|r| !r.quads.is_empty());
    let has_empty = raws.iter().any(|r| r.quads.is_empty());
    if has_any && has_empty {
        for r in &mut raws {
            if r.quads.is_empty() {
                let [ox, oy, ow, oh] = r.orig;
                let q = Quad { points: [[ox as f32, oy as f32], [ox as f32+ow as f32, oy as f32], [ox as f32+ow as f32, oy as f32+oh as f32], [ox as f32, oy as f32+oh as f32]] };
                r.quads.push(q);
            }
        }
    }

    // For single span, fall back to 512-wide centered handling but keep height 512 logic for consistency.
    // For two spans, allocate heights to fill 512 with seam in middle, sticking bottom/top.
    struct Piece {
        idx: usize,
        orig: [u32; 4],
        x_src: i32,
        y_src: i32,
        w_src: u32,
        h_src: u32,
        off_y: u32,
        quads: Vec<Quad>,
    }

    let mut pieces: Vec<Piece> = Vec::new();

    if raws.len() == 1 {
        // Single piece stitched as 512x512 centered on selection
        let r = &raws[0];
        let [ox, oy, ow, oh] = r.orig;
        // Width 512 centered
        let w_src = STITCH_W.min(r.img_w);
        let center_x = ox as f32 + ow as f32 * 0.5;
        let mut x_src = (center_x - w_src as f32 * 0.5).round() as i32;
        x_src = x_src.clamp(0, r.img_w as i32 - w_src as i32).max(0);
        // Height 512 centered on selection, sticking if at edge
        let h_src = STITCH_H.min(r.img_h);
        let center_y = oy as f32 + oh as f32 * 0.5;
        let mut y_src = (center_y - h_src as f32 * 0.5).round() as i32;
        // If selection near edge, stick to edge
        if oy == 0 {
            y_src = 0;
        } else if oy + oh == r.img_h {
            y_src = r.img_h as i32 - h_src as i32;
        } else {
            y_src = y_src.clamp(0, r.img_h as i32 - h_src as i32).max(0);
        }
        pieces.push(Piece { idx: r.idx, orig: r.orig, x_src, y_src, w_src, h_src, off_y: 0, quads: r.quads.clone() });
    } else {
        // Two pieces: allocate heights to fill 512, seam at h0
        let raw_h0 = raws[0].orig[3] as i32;
        let raw_h1 = raws[1].orig[3] as i32;
        let avail_top0 = raws[0].orig[1] as i32;
        let avail_bottom1 = raws[1].img_h as i32 - (raws[1].orig[1] as i32 + raws[1].orig[3] as i32);
        let total_raw = raw_h0 + raw_h1;
        let mut h0: i32;
        let mut h1: i32;
        if total_raw >= STITCH_H as i32 {
            // Selection itself larger than 512 - allocate proportionally (rare for seam)
            h0 = (STITCH_H as f32 * raw_h0 as f32 / total_raw as f32).round() as i32;
            h0 = h0.clamp(1, STITCH_H as i32 -1);
            h1 = STITCH_H as i32 - h0;
        } else {
            let extra_needed = STITCH_H as i32 - total_raw;
            // Aim for equal extra both sides
            let mut extra0 = (extra_needed / 2 + extra_needed %2).min(avail_top0);
            let mut extra1 = (extra_needed - extra0).min(avail_bottom1);
            let mut remaining = extra_needed - extra0 - extra1;
            if remaining > 0 && avail_top0 > extra0 {
                let add = remaining.min(avail_top0 - extra0);
                extra0 += add;
                remaining -= add;
            }
            if remaining > 0 && avail_bottom1 > extra1 {
                let add = remaining.min(avail_bottom1 - extra1);
                extra1 += add;
            }
            // If still remaining, will be padded with mirror later
            h0 = raw_h0 + extra0;
            h1 = raw_h1 + extra1;
            // If h0+h1 <512 due to avail limits, keep as is and pad stitched with mirror (allowed per spec when at edge)
            // For now, if still <512, distribute remaining as mirror padding handled later, but keep h0/h1 as computed
        }
        // Ensure at least 1
        h0 = h0.max(1).min(STITCH_H as i32);
        h1 = h1.max(1).min(STITCH_H as i32);
        // If h0+h1 !=512 due to avail limits, we will still create 512 stitched and pad gaps with mirror
        // Clamp h0 to not exceed avail + raw
        // Compute y_src per spec: bottom of image0 at seam, top of image1 at seam
        let y_src0 = (raws[0].orig[1] as i32 + raws[0].orig[3] as i32 - h0).clamp(0, raws[0].img_h as i32 - h0).max(0);
        let y_src1 = raws[1].orig[1] as i32; // top stuck
        // Width 512 centered per piece (align centers to keep seam vertical aligned)
        for (i, r) in raws.iter().enumerate() {
            let [ox, _oy, ow, _oh] = r.orig;
            let w_src = STITCH_W.min(r.img_w);
            let center_x = ox as f32 + ow as f32 * 0.5;
            let mut x_src = (center_x - w_src as f32 * 0.5).round() as i32;
            x_src = x_src.clamp(0, r.img_w as i32 - w_src as i32).max(0);
            let (h_src, off_y, y_src) = if i == 0 { (h0 as u32, 0u32, y_src0) } else { (h1 as u32, h0 as u32, y_src1) };
            let y_src_clamped = (y_src as i32).clamp(0, r.img_h as i32 - h_src as i32).max(0);
            pieces.push(Piece { idx: r.idx, orig: r.orig, x_src, y_src: y_src_clamped, w_src, h_src, off_y, quads: r.quads.clone() });
        }
    }

    // Build stitched 512x512
    let mut stitched = image::RgbaImage::new(STITCH_W, STITCH_H);
    // Initially transparent; fill gaps with mirror of nearest edge if needed
    for p in &pieces {
        let src = &raws.iter().find(|r| r.idx == p.idx).unwrap().full;
        let crop = image::imageops::crop_imm(src, p.x_src as u32, p.y_src as u32, p.w_src, p.h_src).to_image();
        // If w_src <512, need to fill remaining width with horizontal mirror of crop edge
        let mut placed = crop;
        if p.w_src < STITCH_W {
            let mut full_w = image::RgbaImage::new(STITCH_W, p.h_src);
            image::imageops::replace(&mut full_w, &placed, 0, 0);
            // Mirror fill remaining width: reflect last column
            let remaining = STITCH_W - p.w_src;
            if remaining > 0 {
                // Simple horizontal reflect of edge: copy mirrored columns
                for y in 0..p.h_src {
                    for x in 0..remaining {
                        let src_x = (p.w_src as i32 - 1 - (x as i32 % p.w_src as i32)).max(0) as u32;
                        let px = placed.get_pixel(src_x, y).clone();
                        full_w.put_pixel(p.w_src + x, y, px);
                    }
                }
            }
            placed = full_w;
        }
        image::imageops::replace(&mut stitched, &placed, 0, p.off_y as i64);
    }
    // If pieces heights sum <512 (due to avail limits), there will be a gap between h0 and 512 or at top/bottom.
    // Fill gap with vertical mirror of nearest edge (allowed when at chapter edge per spec).
    let total_h: u32 = pieces.iter().map(|p| p.h_src).sum();
    if total_h < STITCH_H {
        // Gap is at middle if h0+h1 <512? Actually h0+h1 == total_h, but stitched height is 512, so gap =512-total_h at bottom (since we placed at 0 and h0)
        // Fill remaining rows with mirrored edge of last piece's bottom
        let gap = STITCH_H - total_h;
        if gap > 0 {
            // Mirror bottom edge of stitched's existing content
            for y in 0..gap {
                let src_y = (total_h as i32 -1 - (y as i32 % total_h as i32)).max(0) as u32;
                for x in 0..STITCH_W {
                    let px = stitched.get_pixel(x, src_y).clone();
                    stitched.put_pixel(x, total_h + y, px);
                }
            }
        }
    }

    // Build stitched quads with fixed scale=1 (no resize) and track origin for correct mapping
    let mut quads_stitched: Vec<Quad> = Vec::new();
    let mut quad_piece: Vec<usize> = Vec::new();
    for p in &pieces {
        for q in &p.quads {
            let mut pts = [[0.0f32; 2]; 4];
            for (i, pt) in q.points.iter().enumerate() {
                let x_in = pt[0] - p.x_src as f32;
                let y_in = pt[1] - p.y_src as f32 + p.off_y as f32;
                pts[i] = [x_in, y_in];
            }
            quads_stitched.push(Quad { points: pts });
            quad_piece.push(p.idx);
        }
    }
    let rect_stitched = [0.0, 0.0, STITCH_W as f32, STITCH_H as f32];
    let patches = engine.run_on_image(&stitched, rect_stitched, &quads_stitched)?;
    // Map patches back - fixed 512 scale=1, no resize
    let mut per_image: std::collections::HashMap<usize, Vec<(image::RgbaImage, [f32; 4])>> = std::collections::HashMap::new();
    let is_empty_single = quads_stitched.is_empty() && patches.len() == 1 && patches[0].0.dimensions() == (STITCH_W, STITCH_H);
    if is_empty_single {
        let (img_patch, _) = &patches[0];
        for p in &pieces {
            let [ox, oy, ow, oh] = p.orig;
            // For empty, extract selection sub-crop from the 512 strip
            let y0 = p.off_y;
            let h = p.h_src;
            let slice_full = image::imageops::crop_imm(img_patch, 0, y0, STITCH_W, h).to_image();
            // Selection offset inside slice
            let sel_x = (ox as i32 - p.x_src).max(0) as u32;
            let sel_y = if p.off_y == 0 {
                // top piece: selection at bottom of its strip
                (h as i32 - oh as i32).max(0) as u32
            } else {
                // bottom piece: selection at top
                0
            };
            // Clamp sel_x+ow within 512
            let sel_x_clamped = sel_x.min(STITCH_W - ow);
            let sub = image::imageops::crop_imm(&slice_full, sel_x_clamped, sel_y, ow, oh).to_image();
            let bounds = [ox as f32, oy as f32, ow as f32, oh as f32];
            per_image.entry(p.idx).or_default().push((sub, bounds));
        }
    } else {
        for (idx, (patch_img, bounds_stitched)) in patches.into_iter().enumerate() {
            let [bx, by, bw, bh] = bounds_stitched;
            // Map by quad index (preserves origin) - fallback to center if len mismatch
            let p: &Piece = if idx < quad_piece.len() {
                let wanted = quad_piece[idx];
                pieces.iter().find(|x| x.idx == wanted).unwrap()
            } else {
                let cy = by + bh / 2.0;
                let mut found: Option<&Piece> = None;
                for pp in &pieces {
                    let py0 = pp.off_y as f32;
                    let py1 = py0 + pp.h_src as f32;
                    if cy >= py0 && cy < py1 { found = Some(pp); break; }
                }
                match found {
                    Some(v) => v,
                    None => {
                        let mut best: Option<&Piece> = None;
                        let mut best_overlap: f32 = 0.0;
                        for cand in &pieces {
                            let py0 = cand.off_y as f32;
                            let py1 = py0 + cand.h_src as f32;
                            let overlap = (by + bh).min(py1) - by.max(py0);
                            if overlap > best_overlap { best_overlap = overlap; best = Some(cand); }
                        }
                        match best { Some(v) => v, None => continue }
                    }
                }
            };
            // Seam-crossing split: if bbox straddles seam, split into two (rare)
            let py0 = p.off_y as f32;
            let py1 = py0 + p.h_src as f32;
            let seam_cross = by < py1 && by + bh > py1 && pieces.len() == 2;
            if seam_cross {
                let top_h = py1 - by;
                let bot_h = by + bh - py1;
                if top_h > 0.5 && bot_h > 0.5 {
                    let other = pieces.iter().find(|o| o.idx != p.idx).unwrap();
                    let top_patch = image::imageops::crop_imm(&patch_img, 0, 0, bw as u32, top_h as u32).to_image();
                    let bot_patch = image::imageops::crop_imm(&patch_img, 0, top_h as u32, bw as u32, bot_h as u32).to_image();
                    let bounds_top = [bx + p.x_src as f32, by + p.y_src as f32, bw, top_h];
                    let bounds_bot = [bx + other.x_src as f32, (py1 - p.off_y as f32) + other.y_src as f32, bw, bot_h];
                    per_image.entry(p.idx).or_default().push((top_patch, bounds_top));
                    per_image.entry(other.idx).or_default().push((bot_patch, bounds_bot));
                    continue;
                }
            }
            let local_y = by - p.off_y as f32;
            let orig_x = bx + p.x_src as f32;
            let orig_y = local_y + p.y_src as f32;
            // Clip to image bounds (B) - keep inside image for InpaintLayer
            let (img_w_f, img_h_f) = {
                let r = raws.iter().find(|r| r.idx == p.idx).unwrap();
                (r.img_w as f32, r.img_h as f32)
            };
            let clip_x0 = orig_x.max(0.0);
            let clip_y0 = orig_y.max(0.0);
            let clip_x1 = (orig_x + bw).min(img_w_f);
            let clip_y1 = (orig_y + bh).min(img_h_f);
            if clip_x1 <= clip_x0 || clip_y1 <= clip_y0 {
                continue;
            }
            let new_w = clip_x1 - clip_x0;
            let new_h = clip_y1 - clip_y0;
            let crop_x = (clip_x0 - orig_x).round().max(0.0) as u32;
            let crop_y = (clip_y0 - orig_y).round().max(0.0) as u32;
            let clipped_patch = if crop_x != 0 || crop_y != 0 || new_w as u32 != patch_img.width() || new_h as u32 != patch_img.height() {
                let cw = (new_w as u32).min(patch_img.width().saturating_sub(crop_x));
                let ch = (new_h as u32).min(patch_img.height().saturating_sub(crop_y));
                if cw == 0 || ch == 0 { continue; }
                image::imageops::crop_imm(&patch_img, crop_x, crop_y, cw, ch).to_image()
            } else { patch_img };
            let bounds = [clip_x0, clip_y0, new_w, new_h];
            per_image.entry(p.idx).or_default().push((clipped_patch, bounds));
        }
    }
    let mut out: Vec<(usize, Vec<(image::RgbaImage, [f32; 4])>)> = Vec::new();
    for p in &pieces {
        if let Some(v) = per_image.remove(&p.idx) {
            out.push((p.idx, v));
        }
    }
    out.sort_by_key(|(idx, _)| *idx);
    Ok(out)
}

#[cfg(feature = "inpaint")]
pub fn handle_inpaint_span(app: &mut App, spans: Vec<(usize, iced::Rectangle)>) -> Task<Message> {
    if app.inpainting || app.running || app.translating {
        return Task::none();
    }
    #[cfg(feature = "ocr")]
    if app.manual_ocring {
        return Task::none();
    }
    if spans.is_empty() {
        return Task::none();
    }
    let mut span_data: Vec<(usize, String, [f32; 4], Vec<Quad>)> = Vec::new();
    for (idx, rect) in spans {
        if idx >= app.images.len() {
            continue;
        }
        let image_id = app.images[idx].image_id;
        let rect_arr = [rect.x, rect.y, rect.width, rect.height];
        let quads: Vec<Quad> = app
            .project
            .all_for(image_id)
            .map(|e| app.project.view_quad(e))
            .filter(|q| q.intersects_rect(rect_arr))
            .collect();
        if quads.is_empty() {
            // still allow empty -> will clean whole selection
        }
        let path = app.project.image(image_id).map(|m| m.path.clone()).unwrap_or_default();
        if path.is_empty() {
            continue;
        }
        span_data.push((idx, path, rect_arr, quads));
    }
    if span_data.is_empty() {
        return Task::none();
    }
    span_data.sort_by_key(|(idx, _, _, _)| *idx);
    if span_data.len() > 2 {
        span_data.truncate(2);
    }
    if span_data.len() == 1 {
        let (idx, path, rect, quads) = span_data.into_iter().next().unwrap();
        let (backend, radius) = scanlateit_settings::get(|s| (s.inpaint_backend, s.inpaint_radius.parse::<i32>().unwrap_or(5).max(1)));
        let cached = app.inpaint_engine.clone().filter(|e| e.backend() == backend && e.radius() == radius);
        return match cached {
            Some(engine) => start_inpaint(app, engine, idx, path, rect, quads),
            None => {
                app.pending_inpaint = Some((idx, path, rect, quads));
                app.status = match backend {
                    InpaintBackend::Lama => "Loading LaMa model...".to_string(),
                    InpaintBackend::Aot => "Loading AOT-GAN model...".to_string(),
                    InpaintBackend::Telea => "Inpainting...".to_string(),
                };
                Task::perform(async move { scanlateit_inpaint::Engine::build(backend, radius) }, Message::InpaintEngineReady)
            }
        };
    }
    let (backend, radius) = scanlateit_settings::get(|s| (s.inpaint_backend, s.inpaint_radius.parse::<i32>().unwrap_or(5).max(1)));
    let cached = app.inpaint_engine.clone().filter(|e| e.backend() == backend && e.radius() == radius);
    match cached {
        Some(engine) => start_inpaint_span(app, engine, span_data),
        None => {
            app.pending_inpaint_span = Some(span_data);
            app.status = match backend {
                InpaintBackend::Lama => "Loading LaMa model...".to_string(),
                InpaintBackend::Aot => "Loading AOT-GAN model...".to_string(),
                InpaintBackend::Telea => "Inpainting...".to_string(),
            };
            Task::perform(async move { scanlateit_inpaint::Engine::build(backend, radius) }, Message::InpaintEngineReady)
        }
    }
}

#[cfg(feature = "inpaint")]
pub fn handle_inpaint_span_finished(app: &mut App, result: Result<Vec<(usize, Vec<(image::RgbaImage, [f32; 4])>)>, String>) -> Task<Message> {
    app.inpainting = false;
    match result {
        Ok(per_image_patches) => {
            let mut total = 0usize;
            let mut pending_evs = Vec::new();
            for (idx, patches) in per_image_patches {
                let Some(image_id) = app.images.get(idx).map(|i| i.image_id) else { continue; };
                let Some(image) = app.images.get_mut(idx) else { continue; };
                for (patch, bounds) in patches {
                    total += 1;
                    let (width, height) = (patch.width(), patch.height());
                    let layer = InpaintLayer { bounds, handle: iced::widget::image::Handle::from_rgba(width, height, bytes::Bytes::from(patch.into_raw())), width, height };
                    image.inpaint.push(layer);
                    pending_evs.push((image_id, bounds));
                }
            }
            for (image_id, bounds) in pending_evs {
                let ev = app.project.add_inpaint_patch(image_id, bounds);
                crate::app::handle_model_event(app, ev);
            }
            app.inpaint_mode = false;
            app.show_inpaint = true;
            app.status = format!("Inpainted {total} region(s) (stitched).");
        }
        Err(e) => {
            app.status = e;
        }
    }
    Task::none()
}

#[cfg(feature = "inpaint")]
fn auto_pad_for(backend: InpaintBackend, radius: i32) -> f32 {
    match backend {
        InpaintBackend::Telea => radius as f32,
        _ => 32.0,
    }
}

#[cfg(feature = "inpaint")]
fn run_auto_job_with_stitch(
    engine: &InpaintEngine,
    job: &AutoInpaintJob,
    pad: f32,
    prev_path: Option<&str>,
    next_path: Option<&str>,
) -> Result<Vec<(RgbaImage, [f32; 4])>, String> {
    let [x0, y0, x1, y1] = job.quad.bounds();
    let rect = [x0, y0, x1 - x0, y1 - y0];
    // Quick check: if not near top/bottom seam, use normal path
    // Need image dimensions to decide; decode main to get dims
    let main_rgba = image::ImageReader::open(&job.path)
        .map_err(|e| format!("Failed to open {}: {e}", job.path))?
        .with_guessed_format()
        .map_err(|e| format!("Failed to decode {}: {e}", job.path))?
        .decode()
        .map_err(|e| format!("Failed to decode {}: {e}", job.path))?
        .into_rgba8();
    let (img_w, img_h) = main_rgba.dimensions();
    let img_h_f = img_h as f32;
    let need_top = if rect[1] < pad && prev_path.is_some() { pad - rect[1] } else { 0.0 };
    let need_bottom = if rect[1] + rect[3] > img_h_f - pad && next_path.is_some() { rect[1] + rect[3] + pad - img_h_f } else { 0.0 };
    if need_top <= 0.0 && need_bottom <= 0.0 {
        // No seam crossing, normal
        return engine.run_blocking(&job.path, rect, &[job.quad]);
    }
    // Build stitched image: top strip (from prev) + main expanded crop + bottom strip
    // For simplicity, main expanded crop is rect expanded by pad clamped to main image
    let exp_x0 = (rect[0] - pad).max(0.0);
    let exp_y0 = (rect[1] - pad).max(0.0);
    let exp_x1 = (rect[0] + rect[2] + pad).min(img_w as f32);
    let exp_y1 = (rect[1] + rect[3] + pad).min(img_h as f32);
    let exp_w = (exp_x1 - exp_x0).max(1.0) as u32;
    let exp_h_main = (exp_y1 - exp_y0).max(1.0) as u32;
    // Use exp_w as common width
    let common_w = exp_w;
    // Prepare strips
    let mut strips: Vec<RgbaImage> = Vec::new();
    let mut total_h: u32 = 0;
    // Top strip from prev image if needed
    if need_top > 0.0 {
        if let Some(pp) = prev_path {
            if let Ok(prev_rgba) = image::ImageReader::open(pp).and_then(|r| r.with_guessed_format().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))).and_then(|r| r.decode().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))).map(|d| d.into_rgba8()) {
                let (pw, ph) = prev_rgba.dimensions();
                let ph_f = ph as f32;
                let take_h = need_top.min(ph_f) as u32;
                if take_h > 0 {
                    let y_src = (ph_f - take_h as f32).max(0.0) as u32;
                    let x_src = exp_x0.min((pw as f32 - common_w as f32).max(0.0)) as u32;
                    let w_src = common_w.min(pw - x_src);
                    let strip = image::imageops::crop_imm(&prev_rgba, x_src, y_src, w_src, take_h).to_image();
                    // Resize to common_w if w_src != common_w
                    let resized = if w_src != common_w {
                        let dyn_s = image::DynamicImage::ImageRgba8(strip);
                        dyn_s.resize_exact(common_w, take_h, image::imageops::FilterType::Triangle).into_rgba8()
                    } else { strip };
                    total_h += take_h;
                    strips.push(resized);
                }
            }
        }
    }
    // Main middle strip (expanded clamped)
    {
        let x_src = exp_x0 as u32;
        let y_src = exp_y0 as u32;
        let w_src = exp_w;
        let h_src = exp_h_main;
        let middle = image::imageops::crop_imm(&main_rgba, x_src, y_src, w_src, h_src).to_image();
        total_h += h_src;
        strips.push(middle);
    }
    // Bottom strip from next image if needed
    if need_bottom > 0.0 {
        if let Some(np) = next_path {
            if let Ok(next_rgba) = image::ImageReader::open(np).and_then(|r| r.with_guessed_format().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))).and_then(|r| r.decode().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))).map(|d| d.into_rgba8()) {
                let (nw, _nh) = next_rgba.dimensions();
                let take_h = need_bottom as u32;
                if take_h > 0 {
                    let x_src = exp_x0.min((nw as f32 - common_w as f32).max(0.0)) as u32;
                    let w_src = common_w.min(nw - x_src);
                    let strip = image::imageops::crop_imm(&next_rgba, x_src, 0, w_src, take_h.min(next_rgba.height())).to_image();
                    let resized = if w_src != common_w {
                        let dyn_s = image::DynamicImage::ImageRgba8(strip);
                        dyn_s.resize_exact(common_w, take_h, image::imageops::FilterType::Triangle).into_rgba8()
                    } else { strip };
                    total_h += take_h;
                    strips.push(resized);
                }
            }
        }
    }
    if strips.is_empty() {
        return engine.run_blocking(&job.path, rect, &[job.quad]);
    }
    // Build stitched
    let mut stitched = RgbaImage::new(common_w, total_h);
    let mut off = 0u32;
    for s in &strips {
        image::imageops::replace(&mut stitched, s, 0, off as i64);
        off += s.height();
    }
    // Quad in stitched coordinates: original quad shifted by (-exp_x0, -exp_y0 + need_top)
    let need_top_h = if need_top > 0.0 { need_top as u32 } else { 0 };
    let mut quad_stitched = job.quad.clone();
    for pt in &mut quad_stitched.points {
        pt[0] = pt[0] - exp_x0;
        pt[1] = pt[1] - exp_y0 + need_top_h as f32;
    }
    let rect_stitched = [rect[0] - exp_x0, rect[1] - exp_y0 + need_top_h as f32, rect[2], rect[3]];
    let patches = engine.run_on_image(&stitched, rect_stitched, &[quad_stitched])?;
    // Map patches back: patches bounds are in stitched coordinates of the expanded main region? For Telea, bbox_crops will return per-quad bbox within stitched's patched area.
    // The returned bounds are in stitched pixel space of the original rect? Actually engine returns bounds in stitched image coords of the quad's bbox.
    // We need to map back to original main image coords: subtract need_top and add exp offsets.
    let mut out: Vec<(RgbaImage, [f32; 4])> = Vec::new();
    for (img, b) in patches {
        let [bx, by, bw, bh] = b;
        // Only keep patches that lie within middle section (original image's expanded area) to avoid returning neighbor strips
        // Check if patch overlaps middle section
        let middle_y0 = need_top as f32;
        let middle_y1 = middle_y0 + exp_h_main as f32;
        // If patch is fully outside middle (shouldn't happen, quads are from middle), skip neighbor-only patches
        if by + bh <= middle_y0 || by >= middle_y1 {
            continue;
        }
        // Map back
        let orig_x = bx + exp_x0;
        let orig_y = by - need_top_h as f32 + exp_y0;
        // Patch image may need no resize as common_w == exp_w
        out.push((img, [orig_x, orig_y, bw, bh]));
    }
    if out.is_empty() {
        // Fallback to normal if stitching produced no middle patches
        return engine.run_blocking(&job.path, rect, &[job.quad]);
    }
    Ok(out)
}

#[cfg(feature = "inpaint")]
pub fn handle_inpaint_selection(app: &mut App, index: usize, rect: iced::Rectangle) -> Task<Message> {
    if app.inpainting || app.running || app.translating {
        return Task::none();
    }
    #[cfg(feature = "ocr")]
    if app.manual_ocring {
        return Task::none();
    }
    let rect_arr = [rect.x, rect.y, rect.width, rect.height];
    let Some(image) = app.images.get(index) else {
        return Task::none();
    };
    let image_id = image.image_id;
    let quads: Vec<Quad> = app
        .project
        .all_for(image_id) // includes deleted for inpaint intersection
        .map(|entry| app.project.view_quad(entry))
        .filter(|quad| quad.intersects_rect(rect_arr))
        .collect();
    if quads.is_empty() {
        app.status = "Inpaint: no OCR boxes in the range; the whole selection \
                      will be cleaned."
            .to_string();
    }
    let path = app
        .project
        .image(image_id)
        .map(|m| m.path.clone())
        .unwrap_or_default();
    let (backend, radius) = scanlateit_settings::get(|s| {
        (
            s.inpaint_backend,
            s.inpaint_radius.parse::<i32>().unwrap_or(5).max(1),
        )
    });
    let cached = app
        .inpaint_engine
        .clone()
        .filter(|engine| engine.backend() == backend && engine.radius() == radius);
    match cached {
        Some(engine) => start_inpaint(app, engine, index, path, rect_arr, quads),
        None => {
            app.pending_inpaint = Some((index, path, rect_arr, quads));
            app.status = match backend {
                InpaintBackend::Lama => "Loading LaMa model...".to_string(),
                InpaintBackend::Aot => "Loading AOT-GAN model...".to_string(),
                InpaintBackend::Telea => "Inpainting...".to_string(),
            };
            Task::perform(
                async move { scanlateit_inpaint::Engine::build(backend, radius) },
                Message::InpaintEngineReady,
            )
        }
    }
}

#[cfg(not(feature = "inpaint"))]
pub fn handle_inpaint_selection(app: &mut App, _index: usize, _rect: iced::Rectangle) -> Task<Message> {
    app.status = "Inpaint is not available in this build.".to_string();
    Task::none()
}

#[cfg(not(feature = "inpaint"))]
pub fn handle_inpaint_span(app: &mut App, _spans: Vec<(usize, iced::Rectangle)>) -> Task<Message> {
    app.status = "Inpaint is not available in this build.".to_string();
    Task::none()
}

#[cfg(not(feature = "inpaint"))]
pub fn handle_inpaint_span_finished(
    app: &mut App,
    _result: Result<Vec<(usize, Vec<(image::RgbaImage, [f32; 4])>)>, String>,
) -> Task<Message> {
    app.status = "Inpaint is not available in this build.".to_string();
    Task::none()
}

#[cfg(feature = "inpaint")]
pub fn handle_style_inpaint_background(app: &mut App) -> Task<Message> {
    if app.inpainting || app.running || app.translating {
        return Task::none();
    }
    let Some((index, id)) = app.selected else {
        return Task::none();
    };
    if index >= app.images.len() {
        return Task::none();
    }
    let (path, quad) = {
        let Some(entry) = app.project.entry(id) else {
            return Task::none();
        };
        let image_id = app.images[index].image_id;
        if entry.image_id != image_id {
            return Task::none();
        }
        let path = app
            .project
            .image(image_id)
            .map(|m| m.path.clone())
            .unwrap_or_default();
        (path, app.project.view_quad(entry))
    };
    app.style_working.bg_color = [0, 0, 0, 0];
    if app.project.entry(id).is_some() {
        let ev = app.project.set_entry_style_with_event(id, app.style_working.clone());
        crate::app::handle_model_event(app, ev);
    }
    let [x0, y0, x1, y1] = quad.bounds();
    let rect = [x0, y0, x1 - x0, y1 - y0];
    if rect[2] <= 0.0 || rect[3] <= 0.0 {
        app.status = "Inpaint Background: selected box is degenerate.".to_string();
        return Task::none();
    }
    let quads = vec![quad];
    let (backend, radius) = scanlateit_settings::get(|s| {
        (
            s.inpaint_backend,
            s.inpaint_radius.parse::<i32>().unwrap_or(5).max(1),
        )
    });
    let cached = app
        .inpaint_engine
        .clone()
        .filter(|engine| engine.backend() == backend && engine.radius() == radius);
    match cached {
        Some(engine) => start_inpaint(app, engine, index, path, rect, quads),
        None => {
            app.pending_inpaint = Some((index, path, rect, quads));
            app.status = match backend {
                InpaintBackend::Lama => "Loading LaMa model...".to_string(),
                InpaintBackend::Aot => "Loading AOT-GAN model...".to_string(),
                InpaintBackend::Telea => "Inpainting background...".to_string(),
            };
            Task::perform(
                async move { scanlateit_inpaint::Engine::build(backend, radius) },
                Message::InpaintEngineReady,
            )
        }
    }
}

#[cfg(not(feature = "inpaint"))]
pub fn handle_style_inpaint_background(app: &mut App) -> Task<Message> {
    let Some((_index, id)) = app.selected else {
        return Task::none();
    };
    app.style_working.bg_color = [0, 0, 0, 0];
    if app.project.entry(id).is_some() {
        let ev = app.project.set_entry_style_with_event(id, app.style_working.clone());
        crate::app::handle_model_event(app, ev);
    }
    app.status =
        "Background made transparent (inpaint not available in this build).".to_string();
    Task::none()
}

pub fn handle_inpaint_clicked(app: &mut App, selection: Option<(usize, usize)>) -> Task<Message> {
    use super::edit::clear_editing;
    clear_editing(app);
    match selection {
        Some((image_index, patch_idx)) => {
            let Some(img) = app.images.get(image_index) else {
                app.status = "That inpaint layer no longer exists.".to_string();
                return Task::none();
            };
            let image_id = img.image_id;
            let extras_len = app
                .project
                .extras
                .inpaint_patches
                .iter()
                .filter(|p| p.image_id == image_id)
                .count();
            let valid = patch_idx < img.inpaint.len() || patch_idx < extras_len;
            if !valid {
                app.status = "That inpaint layer no longer exists.".to_string();
                return Task::none();
            }
            app.selected = None;
            app.selected_inpaint = Some((image_index, patch_idx));
            app.status = format!("Inpaint {patch_idx} selected – overlays hidden.");
            if app.scheduler.needs_settle(image_index, app.images.len()) {
                return app.scheduler.schedule(image_index..image_index+1, Message::SettleElapsed);
            }
            Task::none()
        }
        None => {
            app.selected_inpaint = None;
            app.status = "Inpaint deselected – overlays shown.".to_string();
            Task::none()
        }
    }
}

pub fn handle_inpaint_delete(app: &mut App, image_index: usize, patch_idx: usize) -> Task<Message> {
    let Some(image) = app.images.get_mut(image_index) else {
        return Task::none();
    };
    let image_id = image.image_id;
    let extras_len = app
        .project
        .extras
        .inpaint_patches
        .iter()
        .filter(|p| p.image_id == image_id)
        .count();
    let len = image.inpaint.len().max(extras_len);
    if patch_idx >= len {
        return Task::none();
    }
    if patch_idx < image.inpaint.len() {
        image.inpaint.remove(patch_idx);
    }
    let patch_id = app.project.extras.inpaint_patches.iter().filter(|p| p.image_id == image_id).nth(patch_idx).map(|p| p.id);
    if let Some(id) = patch_id {
        if let Some(ev) = app.project.remove_inpaint_patch(id) {
            crate::app::handle_model_event(app, ev);
        }
    }
    if app.selected_inpaint == Some((image_index, patch_idx)) {
        app.selected_inpaint = None;
    } else if let Some((sel_img, sel_patch)) = app.selected_inpaint {
        if sel_img == image_index && sel_patch > patch_idx {
            app.selected_inpaint = Some((sel_img, sel_patch - 1));
        }
    }
    app.status = "Deleted inpaint patch.".to_string();
    Task::none()
}

pub fn handle_inpaint_repaint(app: &mut App, image_index: usize, patch_idx: usize) -> Task<Message> {
    if app.inpainting || app.running || app.translating {
        return Task::none();
    }
    let (path, rect, quads) = {
        let Some(image) = app.images.get(image_index) else {
            return Task::none();
        };
        let image_id = image.image_id;
        let extras_patch = {
            let mut seen = 0usize;
            let mut found = None;
            for p in &app.project.extras.inpaint_patches {
                if p.image_id == image_id {
                    if seen == patch_idx {
                        found = Some(p.bounds);
                        break;
                    }
                    seen += 1;
                }
            }
            found
        };
        let bounds = if patch_idx < image.inpaint.len() {
            image.inpaint[patch_idx].bounds
        } else if let Some(b) = extras_patch {
            b
        } else {
            return Task::none();
        };
        let rect = [bounds[0], bounds[1], bounds[2], bounds[3]];
        let quads: Vec<Quad> = app
            .project
            .all_for(image_id)
            .map(|e| app.project.view_quad(e))
            .filter(|q| q.intersects_rect(rect))
            .collect();
        let path = app
            .project
            .image(image_id)
            .map(|m| m.path.clone())
            .unwrap_or_default();
        (path, rect, quads)
    };
    if let Some(image) = app.images.get_mut(image_index) {
        let image_id = image.image_id;
        let patch_id = app.project.extras.inpaint_patches.iter().filter(|p| p.image_id == image_id).nth(patch_idx).map(|p| p.id);
        if patch_idx < image.inpaint.len() {
            image.inpaint.remove(patch_idx);
        }
        if let Some(id) = patch_id {
            if let Some(ev) = app.project.remove_inpaint_patch(id) {
                crate::app::handle_model_event(app, ev);
            }
        }
        if app.selected_inpaint == Some((image_index, patch_idx)) {
            app.selected_inpaint = None;
        } else if let Some((sel_img, sel_patch)) = app.selected_inpaint {
            if sel_img == image_index && sel_patch > patch_idx {
                app.selected_inpaint = Some((sel_img, sel_patch - 1));
            }
        }
    }
    #[cfg(feature = "inpaint")]
    {
        let (backend, radius) = scanlateit_settings::get(|s| {
            (
                s.inpaint_backend,
                s.inpaint_radius.parse::<i32>().unwrap_or(5).max(1),
            )
        });
        let cached = app
            .inpaint_engine
            .clone()
            .filter(|engine| engine.backend() == backend && engine.radius() == radius);
        match cached {
            Some(engine) => return start_inpaint(app, engine, image_index, path, rect, quads),
            None => {
                app.pending_inpaint = Some((image_index, path, rect, quads));
                app.status = match backend {
                    scanlateit_settings::InpaintBackend::Lama => "Loading LaMa model...".to_string(),
                    scanlateit_settings::InpaintBackend::Aot => "Loading AOT-GAN model...".to_string(),
                    scanlateit_settings::InpaintBackend::Telea => "Inpainting...".to_string(),
                };
                return Task::perform(
                    async move { scanlateit_inpaint::Engine::build(backend, radius) },
                    Message::InpaintEngineReady,
                );
            }
        }
    }
    #[cfg(not(feature = "inpaint"))]
    {
        let _ = (path, rect, quads);
        app.status = "Inpaint is not available in this build.".to_string();
        Task::none()
    }
}

pub fn handle_inpaint_toolbar(app: &mut App, image_index: usize, patch_idx: usize, action: scanlateit_ui::event::InpaintToolbarAction) -> Task<Message> {
    match action {
        scanlateit_ui::event::InpaintToolbarAction::Delete => {
            return handle_inpaint_delete(app, image_index, patch_idx);
        }
        scanlateit_ui::event::InpaintToolbarAction::Repaint => {
            return handle_inpaint_repaint(app, image_index, patch_idx);
        }
    }
}

pub fn handle_inpaint_toggle(app: &mut App) -> Task<Message> {
    use super::edit::clear_editing;
    if app.inpainting || app.running || app.translating || app.images.is_empty() {
        return Task::none();
    }
    #[cfg(feature = "ocr")]
    if app.manual_ocring {
        return Task::none();
    }
    clear_editing(app);
    app.inpaint_mode = !app.inpaint_mode;
    if app.inpaint_mode {
        app.ocr_mode = false;
        app.status = "Inpaint mode: drag a rectangle over the text to remove; \
                   click Inpaint again to cancel."
            .to_string();
    } else {
        app.status = "Inpaint mode cancelled.".to_string();
    }
    Task::none()
}
