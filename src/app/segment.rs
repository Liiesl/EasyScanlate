use iced::Task;
use scanlateit_model::EntryId;
#[cfg(feature = "segment")]
use scanlateit_segment::Engine as SegmentEngine;

use super::{App, Message};

pub fn start_segment_filter(app: &mut App) -> Task<Message> {
    start_segment_filter_for(app, app.active_tab().id)
}
pub fn start_segment_filter_for(app: &mut App, tab_id: crate::app::tab::TabId) -> Task<Message> {
    #[cfg(feature = "segment")]
    {
        let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i)=>i, None=>return Task::none() };
        let tab = &app.tabs[idx];
        if tab.images.is_empty() {
            return Task::none();
        }
        if !scanlateit_settings::get(|s| s.auto_sfx_filter) {
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
                app.tabs[idx].status = "Filtering SFX via segmentation...".to_string();
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
                app.tabs[idx].status = "Loading segmentation model...".to_string();
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
    use scanlateit_segment::filter::{DetBox, sfx_filter_indexes};
    use scanlateit_segment::grid::{build_grid_canvas_with_loader, grid_det_to_page, plan_grids};
    use scanlateit_segment::SegClass;
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
        let img = scanlateit_ocr::load_rgb(path).unwrap_or_else(|| image::RgbImage::new(1, 1));
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
pub fn handle_engine_ready(app: &mut App, result: Result<SegmentEngine, String>) -> Task<Message> {
    handle_engine_ready_for(app, app.active_tab().id, result)
}
#[cfg(feature = "segment")]
pub fn handle_engine_ready_for(app: &mut App, tab_id: crate::app::tab::TabId, result: Result<SegmentEngine, String>) -> Task<Message> {
    match result {
        Ok(engine) => {
            app.engines.segment = Some(engine.clone());
            let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i)=>i, None=>return Task::none()};
            app.tabs[idx].segment_filtering = false;
            start_segment_filter_for(app, tab_id)
        }
        Err(e) => {
            let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i)=>i, None=>return Task::none()};
            app.tabs[idx].segment_filtering = false;
            app.tabs[idx].status = e.clone();
            Task::none()
        }
    }
}

#[cfg(feature = "segment")]
pub fn handle_filtered(
    app: &mut App,
    result: Result<Vec<(usize, EntryId)>, String>,
) -> Task<Message> {
    handle_filtered_for(app, app.active_tab().id, result)
}
#[cfg(feature = "segment")]
pub fn handle_filtered_for(
    app: &mut App,
    tab_id: crate::app::tab::TabId,
    result: Result<Vec<(usize, EntryId)>, String>,
) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i)=>i, None=>return Task::none()};
    app.tabs[idx].segment_filtering = false;
    let is_pipeline = {
        #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
        { app.tabs[idx].pipeline_active }
        #[cfg(not(all(feature = "styling", feature = "inpaint", feature = "segment")))]
        { false }
    };
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
                let (need_style_inpaint, need_inpaint_solo) = scanlateit_settings::get(|s| {
                    let need_style = s.auto_style_detect && s.auto_inpaint;
                    let need_solo = s.auto_inpaint && !s.auto_style_detect;
                    (need_style, need_solo)
                });
                if need_style_inpaint {
                    #[cfg(all(feature = "styling", feature = "inpaint"))]
                    {
                        return super::styling::classify_for(app, tab_id);
                    }
                } else if need_inpaint_solo {
                    let eff = scanlateit_settings::get(|s| {
                        if !s.auto_style_detect && s.auto_inpaint_model == scanlateit_settings::AutoInpaintModel::Mixed {
                            scanlateit_settings::AutoInpaintModel::Telea
                        } else {
                            s.auto_inpaint_model
                        }
                    });
                    #[cfg(feature = "inpaint")]
                    {
                        return super::inpaint::dispatch_auto_solo_for(app, tab_id, eff);
                    }
                } else {
                    #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
                    {
                        app.tabs[idx].pipeline_active = false;
                    }
                    let need_style_only = scanlateit_settings::get(|s| s.auto_style_detect && !s.auto_inpaint);
                    if need_style_only {
                        #[cfg(feature = "styling")]
                        {
                            return super::styling::classify_for(app, tab_id);
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
    Task::none()
}
