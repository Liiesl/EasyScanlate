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
        .map(|run| (run.page_start..=run.page_end).map(|i| app.images[i].path.clone()).collect())
        .collect();
    let above_paths: Vec<Option<String>> = runs
        .iter()
        .map(|run| run.above.map(|(page, _)| app.images[page].path.clone()))
        .collect();
    let below_paths: Vec<Option<String>> = runs
        .iter()
        .map(|run| run.below.map(|(page, _)| app.images[page].path.clone()))
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
        let Some(image) = app.images.get_mut(page) else {
            continue;
        };
        app.ocr_total += image.project.append_ocr(entries);
    }
}

#[cfg(feature = "ocr")]
pub fn flush_held_boundary(app: &mut App) {
    if let Some(state) = app.held_boundary.take() {
        for candidate in state.candidates {
            if let Some(image) = app.images.get_mut(candidate.page) {
                app.ocr_total += image.project.append_ocr(vec![candidate.entry]);
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
        .map(|image| (image.width as u32, image.height as u32))
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
    for image in &mut app.images {
        added += image.project.append_ocr(fake_ocr_entries());
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
                let quads: Vec<Quad> = app.images[page]
                    .project
                    .ocr
                    .all()
                    .map(|entry| entry.quad)
                    .collect();
                (quads, app.images[page].width as u32, offset)
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
