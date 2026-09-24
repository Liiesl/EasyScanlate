use iced::Task;
#[cfg(all(feature = "styling", feature = "inpaint"))]
use easyscanlate_model::EntryStyle;
#[cfg(all(feature = "styling", feature = "inpaint"))]
use super::tab::PipelineStyleItem;
#[cfg(feature = "inpaint")]
use easyscanlate_settings::InpaintBackend;

use super::{App, Message};
#[cfg(feature = "inpaint")]
use super::{AutoInpaintJob};

#[cfg(all(feature = "styling", feature = "inpaint"))]
pub fn dispatch_inpaint(
    app: &mut App,
    tab_id: crate::app::tab::TabId,
    buffered: Vec<PipelineStyleItem>,
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
    let mut harmonic_jobs: Vec<AutoInpaintJob> = Vec::new();
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
    let (page_ws, page_hs, page_paths, offsets) = {
        let tab = &app.tabs[idx];
        let n = tab.images.len();
        let mut ws = Vec::with_capacity(n);
        let mut hs = Vec::with_capacity(n);
        let mut paths = Vec::with_capacity(n);
        for img in &tab.images {
            if let Some(m) = tab.project.image(img.image_id) {
                ws.push(m.width);
                hs.push(m.height);
                paths.push(m.path.clone());
            } else {
                ws.push(0.0);
                hs.push(0.0);
                paths.push(String::new());
            }
        }
        let offsets = super::inpaint::inpaint_global_offsets(&hs);
        (ws, hs, paths, offsets)
    };
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
        let _ = path;
        let need = match pred.bg_type {
            easyscanlate_styling::BgType::Solid => None,
            easyscanlate_styling::BgType::Gradient => Some(match effective_model {
                easyscanlate_settings::AutoInpaintModel::Mixed => InpaintBackend::Harmonic,
                easyscanlate_settings::AutoInpaintModel::Telea => InpaintBackend::Telea,
                easyscanlate_settings::AutoInpaintModel::Harmonic => InpaintBackend::Harmonic,
                easyscanlate_settings::AutoInpaintModel::Lama => InpaintBackend::Lama,
                easyscanlate_settings::AutoInpaintModel::Aot => InpaintBackend::Aot,
            }),
            easyscanlate_styling::BgType::Artwork => Some(match effective_model {
                easyscanlate_settings::AutoInpaintModel::Mixed => InpaintBackend::Lama,
                easyscanlate_settings::AutoInpaintModel::Telea => InpaintBackend::Telea,
                easyscanlate_settings::AutoInpaintModel::Harmonic => InpaintBackend::Harmonic,
                easyscanlate_settings::AutoInpaintModel::Lama => InpaintBackend::Lama,
                easyscanlate_settings::AutoInpaintModel::Aot => InpaintBackend::Aot,
            }),
        };
        if let Some(backend) = need {
            let parts = super::inpaint::split_quad_global(quad, index, &page_ws, &page_hs, &offsets);
            if parts.is_empty() {
                let [bx0, by0, bx1, by1] = quad.bounds();
                eprintln!(
                    "[auto-inpaint::no-intersection] idx={} id={:?} quad_bounds=[{:.1},{:.1},{:.1},{:.1}] -> skipped (no page intersects)",
                    index, id, bx0, by0, bx1, by1,
                );
                continue;
            }
            for (page_idx, tquad) in parts {
                let tpath = page_paths.get(page_idx).cloned().unwrap_or_default();
                if page_idx != index {
                    eprintln!(
                        "[auto-inpaint::split] id={:?} owner_idx={} -> page_idx={} quad_bounds={:?} translated={:?}",
                        id,
                        index,
                        page_idx,
                        quad.bounds(),
                        tquad.bounds(),
                    );
                }
                let job = AutoInpaintJob { index: page_idx, id, path: tpath, quad: tquad };
                match backend {
                    InpaintBackend::Telea => telea_jobs.push(job),
                    InpaintBackend::Harmonic => harmonic_jobs.push(job),
                    InpaintBackend::Lama => lama_jobs.push(job),
                    InpaintBackend::Aot => aot_jobs.push(job),
                    // ShiftMap is manual-only and never arises from
                    // AutoInpaintModel routing above; keep exhaustive with its
                    // weight-class twin.
                    InpaintBackend::ShiftMap => aot_jobs.push(job),
                }
            }
        }
    }
    app.tabs[idx].pipeline_style_pending = 0;
    let mut tasks: Vec<Task<Message>> = Vec::new();
    if !telea_jobs.is_empty() {
        tasks.push(super::inpaint::dispatch_auto(app, tab_id, telea_jobs, InpaintBackend::Telea));
    }
    if !harmonic_jobs.is_empty() {
        tasks.push(super::inpaint::dispatch_auto(app, tab_id, harmonic_jobs, InpaintBackend::Harmonic));
    }
    if !lama_jobs.is_empty() {
        tasks.push(super::inpaint::dispatch_auto(app, tab_id, lama_jobs, InpaintBackend::Lama));
    }
    if !aot_jobs.is_empty() {
        tasks.push(super::inpaint::dispatch_auto(app, tab_id, aot_jobs, InpaintBackend::Aot));
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
