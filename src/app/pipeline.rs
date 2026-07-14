use iced::Task;
use scanlateit_model::{EntryId, EntryStyle, Quad};
#[cfg(feature = "inpaint")]
use scanlateit_settings::InpaintBackend;

use super::{App, AutoInpaintJob, Message};

#[cfg(all(feature = "styling", feature = "inpaint"))]
pub fn dispatch_pipeline_inpaint_after_style(
    app: &mut App,
    buffered: Vec<(usize, EntryId, Result<(EntryStyle, scanlateit_styling::StylePrediction), String>, Quad, String)>,
) -> Task<Message> {
    let results = if buffered.is_empty() && app.pipeline_style_results.is_empty() {
        Vec::new()
    } else if !buffered.is_empty() {
        buffered
    } else {
        std::mem::take(&mut app.pipeline_style_results)
    };
    let mut telea_jobs: Vec<AutoInpaintJob> = Vec::new();
    let mut lama_jobs: Vec<AutoInpaintJob> = Vec::new();
    let effective_model = scanlateit_settings::get(|s| {
        if !s.auto_style_detect && s.auto_inpaint_model == scanlateit_settings::AutoInpaintModel::Mixed {
            scanlateit_settings::AutoInpaintModel::Telea
        } else {
            s.auto_inpaint_model
        }
    });
    let has_inpaint = scanlateit_settings::get(|s| s.auto_inpaint);
    if !has_inpaint {
        app.pipeline_style_pending = 0;
        #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
        {
            app.pipeline_active = false;
        }
        for (index, id, result, quad, _path) in results {
            if let Ok((_, pred)) = result {
                let applied = pred.to_entry_style_for_auto(EntryStyle::default());
                if let Some(image) = app.images.get_mut(index) {
                    image.project.set_entry_style(id, applied);
                    let _ = quad;
                }
            }
        }
        app.status = "Applied deferred styles (no auto-inpaint).".to_string();
        return Task::none();
    }
    for (index, id, result, quad, path) in results {
        let (_style, pred) = match result {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[pipeline] style failed for {index}:{id:?}: {e}");
                continue;
            }
        };
        let applied = pred.to_entry_style_for_auto(EntryStyle::default());
        if let Some(image) = app.images.get_mut(index) {
            image.project.set_entry_style(id, applied);
        }
        let need = match pred.bg_type {
            scanlateit_styling::BgType::Solid => None,
            scanlateit_styling::BgType::Gradient => Some(match effective_model {
                scanlateit_settings::AutoInpaintModel::Mixed => InpaintBackend::Telea,
                scanlateit_settings::AutoInpaintModel::Telea => InpaintBackend::Telea,
                scanlateit_settings::AutoInpaintModel::Lama => InpaintBackend::Lama,
            }),
            scanlateit_styling::BgType::Artwork => Some(match effective_model {
                scanlateit_settings::AutoInpaintModel::Mixed => InpaintBackend::Lama,
                scanlateit_settings::AutoInpaintModel::Telea => InpaintBackend::Telea,
                scanlateit_settings::AutoInpaintModel::Lama => InpaintBackend::Lama,
            }),
        };
        if let Some(backend) = need {
            let job = AutoInpaintJob { index, id, path: path.clone(), quad };
            match backend {
                InpaintBackend::Telea => telea_jobs.push(job),
                InpaintBackend::Lama => lama_jobs.push(job),
            }
        }
    }
    app.pipeline_style_pending = 0;
    let mut tasks: Vec<Task<Message>> = Vec::new();
    if !telea_jobs.is_empty() {
        tasks.push(super::inpaint::dispatch_auto_telea_jobs(app, telea_jobs));
    }
    if !lama_jobs.is_empty() {
        tasks.push(super::inpaint::dispatch_auto_lama_jobs(app, lama_jobs));
    }
    if tasks.is_empty() {
        #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
        {
            app.pipeline_active = false;
        }
        app.status = "Pipeline done: styles applied (solid bg, no inpaint needed).".to_string();
        return Task::none();
    }
    Task::batch(tasks)
}
