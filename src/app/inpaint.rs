use iced::Task;
use scanlateit_model::{EntryId, Quad};
#[cfg(feature = "inpaint")]
use scanlateit_inpaint::Engine as InpaintEngine;
#[cfg(feature = "inpaint")]
use scanlateit_settings::InpaintBackend;
#[cfg(feature = "inpaint")]
use scanlateit_ui::loaded::InpaintLayer;
#[cfg(feature = "inpaint")]
use scanlateit_model::InpaintPatch;

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
    let cached = app.auto_telea_engine.clone().filter(|e| e.radius() == radius);
    if let Some(engine) = cached {
        app.auto_inpaint_pending += jobs.len();
        app.status = format!("Auto-inpaint (Telea) {} regions in parallel...", jobs.len());
        let tasks: Vec<Task<Message>> = jobs
            .into_iter()
            .map(|job| {
                let engine = engine.clone();
                Task::perform(
                    async move {
                        let rect = {
                            let [x0, y0, x1, y1] = job.quad.bounds();
                            [x0, y0, x1 - x0, y1 - y0]
                        };
                        let res = tokio::task::spawn_blocking(move || {
                            engine.run_blocking(&job.path, rect, &[job.quad])
                        })
                        .await
                        .unwrap_or_else(|e| Err(format!("inpaint task cancelled: {e}")));
                        (job.index, job.id, res)
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
    let cached = app.auto_lama_engine.clone().filter(|e| e.radius() == radius);
    if let Some(engine) = cached {
        app.auto_inpaint_pending += jobs.len();
        app.status = format!("Auto-inpaint (LaMa) {} regions sequentially...", jobs.len());
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let mut out: Vec<(usize, EntryId, Result<Vec<(image::RgbaImage, [f32; 4])>, String>)> = Vec::new();
                    for job in jobs {
                        let rect = {
                            let [x0, y0, x1, y1] = job.quad.bounds();
                            [x0, y0, x1 - x0, y1 - y0]
                        };
                        let r = engine.run_blocking(&job.path, rect, &[job.quad]);
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
    let cached = app.auto_aot_engine.clone().filter(|e| e.radius() == radius);
    if let Some(engine) = cached {
        app.auto_inpaint_pending += jobs.len();
        app.status = format!("Auto-inpaint (AOT-GAN) {} regions sequentially...", jobs.len());
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let mut out: Vec<(usize, EntryId, Result<Vec<(image::RgbaImage, [f32; 4])>, String>)> = Vec::new();
                    for job in jobs {
                        let rect = {
                            let [x0, y0, x1, y1] = job.quad.bounds();
                            [x0, y0, x1 - x0, y1 - y0]
                        };
                        let r = engine.run_blocking(&job.path, rect, &[job.quad]);
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
    let jobs: Vec<AutoInpaintJob> = app
        .images
        .iter()
        .enumerate()
        .flat_map(|(index, image)| {
            image
                .project
                .ocr
                .visible()
                .map(move |entry| AutoInpaintJob {
                    index,
                    id: entry.id,
                    path: image.path.clone(),
                    quad: image.project.view_quad(entry),
                })
        })
        .collect();
    if jobs.is_empty() {
        return Task::none();
    }
    for job in &jobs {
        if let Some(img) = app.images.get_mut(job.index) {
            let mut style = img.project.entry_style(job.id);
            style.bg_color = [0, 0, 0, 0];
            img.project.set_entry_style(job.id, style);
        }
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
            match app.pending_inpaint.take() {
                Some((index, path, rect, quads)) => {
                    start_inpaint(app, engine, index, path, rect, quads)
                }
                None => Task::none(),
            }
        }
        Err(e) => {
            app.pending_inpaint = None;
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
                image.project.extras.inpaint_patches.push(InpaintPatch { bounds });
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
                        image.project.extras.inpaint_patches.push(InpaintPatch { bounds });
                    }
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
                        image.project.extras.inpaint_patches.push(InpaintPatch { bounds });
                    }
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
            let Some(image) = app.images.get_mut(index) else {
                return Task::none();
            };
            let count = patches.len();
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
                image
                    .project
                    .extras
                    .inpaint_patches
                    .push(InpaintPatch { bounds });
            }
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
    let quads: Vec<Quad> = image
        .project
        .ocr
        .all()
        .map(|entry| image.project.view_quad(entry))
        .filter(|quad| quad.intersects_rect(rect_arr))
        .collect();
    if quads.is_empty() {
        app.status = "Inpaint: no OCR boxes in the range; the whole selection \
                      will be cleaned."
            .to_string();
    }
    let path = image.path.clone();
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
        let Some(image) = app.images.get(index) else {
            return Task::none();
        };
        let Some(entry) = image.project.ocr.get(id) else {
            return Task::none();
        };
        (image.path.clone(), image.project.view_quad(entry))
    };
    app.style_working.bg_color = [0, 0, 0, 0];
    if let Some(image) = app.images.get_mut(index) {
        if image.project.ocr.get(id).is_some() {
            image.project.set_entry_style(id, app.style_working.clone());
        }
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
    let Some((index, id)) = app.selected else {
        return Task::none();
    };
    app.style_working.bg_color = [0, 0, 0, 0];
    if let Some(image) = app.images.get_mut(index) {
        if image.project.ocr.get(id).is_some() {
            image.project.set_entry_style(id, app.style_working.clone());
        }
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
            let valid = app
                .images
                .get(image_index)
                .is_some_and(|img| patch_idx < img.inpaint.len() || patch_idx < img.project.extras.inpaint_patches.len());
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
    let len = image.inpaint.len().max(image.project.extras.inpaint_patches.len());
    if patch_idx >= len {
        return Task::none();
    }
    if patch_idx < image.inpaint.len() {
        image.inpaint.remove(patch_idx);
    }
    if patch_idx < image.project.extras.inpaint_patches.len() {
        image.project.extras.inpaint_patches.remove(patch_idx);
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
        let bounds = if patch_idx < image.inpaint.len() {
            image.inpaint[patch_idx].bounds
        } else if patch_idx < image.project.extras.inpaint_patches.len() {
            image.project.extras.inpaint_patches[patch_idx].bounds
        } else {
            return Task::none();
        };
        let rect = [bounds[0], bounds[1], bounds[2], bounds[3]];
        let quads: Vec<Quad> = image
            .project
            .ocr
            .all()
            .map(|e| image.project.view_quad(e))
            .filter(|q| q.intersects_rect(rect))
            .collect();
        (image.path.clone(), rect, quads)
    };
    if let Some(image) = app.images.get_mut(image_index) {
        if patch_idx < image.inpaint.len() {
            image.inpaint.remove(patch_idx);
        }
        if patch_idx < image.project.extras.inpaint_patches.len() {
            image.project.extras.inpaint_patches.remove(patch_idx);
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
