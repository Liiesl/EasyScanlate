use iced::Task;
use easyscanlate_model::{EntryId, EntryStyle, Quad};
#[cfg(feature = "inpaint")]
use easyscanlate_settings::InpaintBackend;

use super::{App, AutoInpaintJob, Message};

#[cfg(all(feature = "styling", feature = "inpaint"))]
pub fn dispatch_inpaint(
    app: &mut App,
    buffered: Vec<(usize, EntryId, Result<(EntryStyle, easyscanlate_styling::StylePrediction), String>, Quad, String)>,
) -> Task<Message> {
    dispatch_inpaint_for(app, app.active_tab().id, buffered)
}
#[cfg(all(feature = "styling", feature = "inpaint"))]
pub fn dispatch_inpaint_for(
    app: &mut App,
    tab_id: crate::app::tab::TabId,
    buffered: Vec<(usize, EntryId, Result<(EntryStyle, easyscanlate_styling::StylePrediction), String>, Quad, String)>,
) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    let results = if buffered.is_empty() && app.tabs[idx].pipeline_style_results.is_empty() {
        Vec::new()
    } else if !buffered.is_empty() {
        buffered
    } else {
        std::mem::take(&mut app.tabs[idx].pipeline_style_results)
    };
    let mut telea_jobs: Vec<AutoInpaintJob> = Vec::new();
    let mut lama_jobs: Vec<AutoInpaintJob> = Vec::new();
    let mut aot_jobs: Vec<AutoInpaintJob> = Vec::new();
    let effective_model = easyscanlate_settings::get(|s| {
        if !s.auto_style_detect && s.auto_inpaint_model == easyscanlate_settings::AutoInpaintModel::Mixed {
            easyscanlate_settings::AutoInpaintModel::Telea
        } else {
            s.auto_inpaint_model
        }
    });
    let has_inpaint = easyscanlate_settings::get(|s| s.auto_inpaint);
    if !has_inpaint {
        app.tabs[idx].pipeline_style_pending = 0;
        #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
        {
            app.tabs[idx].pipeline_active = false;
        }
        for (_index, id, result, quad, _path) in results {
            if let Ok((_, pred)) = result {
                let applied = pred.to_entry_style_for_auto(EntryStyle::default());
                let tab = &mut app.tabs[idx];
                let ev = tab.project.set_entry_style_with_event(id, applied);
                crate::app::handle_model_event(tab, ev);
                let _ = quad;
            }
        }
        app.tabs[idx].status = "Applied deferred styles (no auto-inpaint).".to_string();
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
        {
            let tab = &mut app.tabs[idx];
            let ev = tab.project.set_entry_style_with_event(id, applied);
            crate::app::handle_model_event(tab, ev);
        }
        let need = match pred.bg_type {
            easyscanlate_styling::BgType::Solid => None,
            easyscanlate_styling::BgType::Gradient => Some(match effective_model {
                easyscanlate_settings::AutoInpaintModel::Mixed => InpaintBackend::Telea,
                easyscanlate_settings::AutoInpaintModel::Telea => InpaintBackend::Telea,
                easyscanlate_settings::AutoInpaintModel::Lama => InpaintBackend::Lama,
                easyscanlate_settings::AutoInpaintModel::Aot => InpaintBackend::Aot,
            }),
            easyscanlate_styling::BgType::Artwork => Some(match effective_model {
                easyscanlate_settings::AutoInpaintModel::Mixed => InpaintBackend::Lama,
                easyscanlate_settings::AutoInpaintModel::Telea => InpaintBackend::Telea,
                easyscanlate_settings::AutoInpaintModel::Lama => InpaintBackend::Lama,
                easyscanlate_settings::AutoInpaintModel::Aot => InpaintBackend::Aot,
            }),
        };
        if let Some(backend) = need {
            let job = AutoInpaintJob { index, id, path: path.clone(), quad };
            match backend {
                InpaintBackend::Telea => telea_jobs.push(job),
                InpaintBackend::Lama => lama_jobs.push(job),
                InpaintBackend::Aot => aot_jobs.push(job),
            }
        }
    }
    app.tabs[idx].pipeline_style_pending = 0;
    let mut tasks: Vec<Task<Message>> = Vec::new();
    if !telea_jobs.is_empty() {
        tasks.push(super::inpaint::dispatch_auto_for(app, tab_id, telea_jobs, InpaintBackend::Telea));
    }
    if !lama_jobs.is_empty() {
        tasks.push(super::inpaint::dispatch_auto_for(app, tab_id, lama_jobs, InpaintBackend::Lama));
    }
    if !aot_jobs.is_empty() {
        tasks.push(super::inpaint::dispatch_auto_for(app, tab_id, aot_jobs, InpaintBackend::Aot));
    }
    if tasks.is_empty() {
        #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
        {
            app.tabs[idx].pipeline_active = false;
        }
        app.tabs[idx].status = "Pipeline done: styles applied (solid bg, no inpaint needed).".to_string();
        return Task::none();
    }
    Task::batch(tasks)
}
