use iced::Task;
#[cfg(feature = "inpaint")]
use iced::futures::{SinkExt, StreamExt};
use easyscanlate_model::Quad;
#[cfg(feature = "inpaint")]
use easyscanlate_inpaint::Engine as InpaintEngine;
#[cfg(feature = "inpaint")]
use easyscanlate_settings::InpaintBackend;
#[cfg(feature = "inpaint")]
use easyscanlate_ui::loaded::InpaintLayer;
#[cfg(feature = "inpaint")]
use image::RgbaImage;

use easyscanlate_ui::UiState;

use super::{App, Message};
#[cfg(feature = "inpaint")]
use super::{AutoInpaintJob};
use crate::app::queue::owner_of;
#[cfg(feature = "inpaint")]
use easyscanlate_engine_pool::{inpaint_pad_for as pool_pad_for, run_auto_inpaint_job};

#[cfg(feature = "inpaint")]
use super::tab::ManualInpaintSelection;

// ---------------------------------------------------------------------------
// Shared complex payload aliases (silences `clippy::type_complexity`).
// ---------------------------------------------------------------------------

/// One inpaint patch: RGBA crop, bounds, source quad.
#[cfg(feature = "inpaint")]
type InpaintPatch = (RgbaImage, [f32; 4], Option<Quad>);
/// Grouped manual result: per-image patches.
#[cfg(feature = "inpaint")]
type GroupedInpaint = Vec<(usize, Vec<InpaintPatch>)>;
/// Manual multi-inpaint async result.
#[cfg(feature = "inpaint")]
type GroupedInpaintResult = Result<GroupedInpaint, String>;
/// Per-image patch map used while stitching/splitting.
#[cfg(feature = "inpaint")]
type PatchMap = std::collections::HashMap<usize, Vec<InpaintPatch>>;
/// One auto-inpaint patch with its target image index.
#[cfg(feature = "inpaint")]
type AutoPatch = (usize, RgbaImage, [f32; 4], Option<Quad>);
/// Auto single-job async result.
#[cfg(feature = "inpaint")]
type AutoResult = Result<Vec<AutoPatch>, String>;
/// One granular auto stream item: job index, entry, per-job result.
/// Outer `Result` in the message is the per-job dispatch outcome (OCR-style);
/// inner `AutoResult::Err` is the per-job inpaint failure. Both count as failed.
#[cfg(feature = "inpaint")]
pub type AutoStreamItem = (usize, easyscanlate_model::EntryId, AutoResult);
/// Per-unit manual stream payload: partial patches + failed group count in
/// that unit (OCR-style: failures counted, successes kept).
#[cfg(feature = "inpaint")]
pub type ManualStreamPatches = (GroupedInpaint, usize);

#[cfg(feature = "inpaint")]
fn neighbor_paths(app: &App, index: usize) -> (Option<String>, Option<String>) {
    let tab = app.active_tab();
    let prev = if index > 0 {
        tab.images
            .get(index - 1)
            .and_then(|img| tab.project.image(img.image_id).map(|m| m.path.clone()))
    } else {
        None
    };
    let next = if index + 1 < tab.images.len() {
        tab.images
            .get(index + 1)
            .and_then(|img| tab.project.image(img.image_id).map(|m| m.path.clone()))
    } else {
        None
    };
    (prev, next)
}

#[cfg(feature = "inpaint")]
pub fn start_inpaint(
    app: &mut App,
    engine: InpaintEngine,
    index: usize,
    path: String,
    rect: [f32; 4],
    quads: Vec<Quad>,
) -> Task<Message> {
    let tid = app.active_tab().id;
    app.active_tab_mut().inpainting = true;
    app.active_tab_mut().status = "inpainting...".to_string();
    Task::perform(
        async move {
            let result = tokio::task::spawn_blocking(move || {
                engine.run_blocking(&path, rect, &quads)
            })
            .await
            .unwrap_or_else(|e| Err(format!("inpaint task cancelled: {e}")));
            // Convert single-index result into multi payload grouping
            let grouped: GroupedInpaintResult = result.map(|v| {
                let mut map: PatchMap = std::collections::HashMap::new();
                for (img, b, q) in v {
                    map.entry(index).or_default().push((img, b, q));
                }
                let mut out: Vec<_> = map.into_iter().collect();
                out.sort_by_key(|(idx, _)| *idx);
                out
            });
            grouped
        },
        move |res| Message::Tab(tid, crate::app::TabMessage::ManualMultiInpaintFinished(res)),
    )
}


#[cfg(feature = "inpaint")]
fn auto_pad_for(backend: InpaintBackend, radius: i32) -> f32 {
    pool_pad_for(backend, radius)
}

/// Vertical global offsets for a page stack: `offsets[i]` is the global y of
/// page `i`'s top edge (`sum(page_hs[..i])`). Same model as export's
/// `ExportSnapshot::build` so inpaint ownership matches viewer/export.
#[cfg(feature = "inpaint")]
pub(crate) fn inpaint_global_offsets(page_hs: &[f32]) -> Vec<f32> {
    let mut offsets = Vec::with_capacity(page_hs.len());
    let mut cur = 0.0f32;
    for h in page_hs {
        offsets.push(cur);
        cur += h.max(0.0);
    }
    offsets
}

/// Splits one entry quad owned by `owner_idx` across every page its global
/// bounds intersect, translating points into each page's local pixel space.
///
/// Mirrors `export.rs` intersection: `gy = owner_g0 + vy`, `gx = vx*scale_x`
/// with `scale_x = page_w/owner_w` when widths differ. Returns
/// `[(page_idx, translated_quad)]` in page order; empty when the quad
/// intersects no page (fully outside the chapter, e.g. stale OCR quad).
/// Stitch logic in `engine-pool::exec` is unchanged — it still supplies
/// cross-seam context for each resulting per-page job.
#[cfg(feature = "inpaint")]
pub(crate) fn split_quad_global(
    quad: Quad,
    owner_idx: usize,
    page_ws: &[f32],
    page_hs: &[f32],
    offsets: &[f32],
) -> Vec<(usize, Quad)> {
    let n = page_ws.len().min(page_hs.len()).min(offsets.len());
    if owner_idx >= n {
        return Vec::new();
    }
    let [vx0, vy0, vx1, vy1] = quad.bounds();
    if !(vx0.is_finite() && vy0.is_finite() && vx1.is_finite() && vy1.is_finite()) {
        return Vec::new();
    }
    if vx1 <= vx0 || vy1 <= vy0 {
        return vec![(owner_idx, quad)];
    }
    let owner_g0 = offsets[owner_idx];
    let owner_w = page_ws[owner_idx];
    if owner_w <= 1.0 {
        return vec![(owner_idx, quad)];
    }
    let gy0 = owner_g0 + vy0;
    let gy1 = owner_g0 + vy1;
    let mut out = Vec::new();
    for (i, ((w, h), g0)) in page_ws.iter().zip(page_hs.iter()).zip(offsets.iter()).enumerate() {
        if i >= n {
            break;
        }
        let page_g0 = *g0;
        let page_g1 = page_g0 + h.max(0.0);
        let scale_x = if (owner_w - *w).abs() > 0.5 && *w > 1.0 {
            *w / owner_w
        } else {
            1.0
        };
        let gx0 = vx0 * scale_x;
        let gx1 = vx1 * scale_x;
        if gx1 <= 0.0 || gx0 >= *w || gy1 <= page_g0 || gy0 >= page_g1 {
            continue;
        }
        let dy = owner_g0 - page_g0;
        let mut q = quad;
        for p in &mut q.points {
            p[0] *= scale_x;
            p[1] += dy;
        }
        out.push((i, q));
    }
    out
}

/// Single-job anchor variant of [`split_quad_global`]: returns the one page
/// with the largest global `y` overlap (translated into that page's local
/// space), or `None` when no page intersects. Used by the styling-panel
/// single-background flow so one selection stays one job while still landing
/// on the right page for beyond-quad / straddling entries.
#[cfg(feature = "inpaint")]
pub(crate) fn anchor_quad_global(
    quad: Quad,
    owner_idx: usize,
    page_ws: &[f32],
    page_hs: &[f32],
    offsets: &[f32],
) -> Option<(usize, Quad)> {
    let parts = split_quad_global(quad, owner_idx, page_ws, page_hs, offsets);
    if parts.is_empty() {
        return None;
    }
    if parts.len() == 1 {
        return Some(parts.into_iter().next().unwrap());
    }
    let [_, vy0, _, vy1] = quad.bounds();
    let owner_g0 = offsets[owner_idx];
    let gy0 = owner_g0 + vy0;
    let gy1 = owner_g0 + vy1;
    let mut best: Option<(usize, Quad, f32)> = None;
    for (i, q) in parts {
        let page_g0 = offsets[i];
        let page_g1 = page_g0 + page_hs[i].max(0.0);
        let overlap = gy1.min(page_g1) - gy0.max(page_g0);
        if best.is_none_or(|(_, _, b)| overlap > b) {
            best = Some((i, q, overlap));
        }
    }
    best.map(|(i, q, _)| (i, q))
}

/// Commits one auto job's patches to `tabs[idx]`. Returns number of patches applied.
#[cfg(feature = "inpaint")]
fn commit_auto_patches(app: &mut App, idx: usize, patches: Vec<AutoPatch>) -> usize {
    let mut pending_evs: Vec<(easyscanlate_model::ImageId, [f32; 4], Option<Quad>)> =
        Vec::new();
    let mut affected = std::collections::HashSet::new();
    let mut applied = 0usize;
    for (target_idx, patch, bounds, quad) in patches {
        let image_id_opt = app.tabs[idx].images.get(target_idx).map(|i| i.image_id);
        if let Some(image_id) = image_id_opt
            && let Some(image) = app.tabs[idx].images.get_mut(target_idx)
        {
            let (w, h) = (patch.width(), patch.height());
            let layer = InpaintLayer {
                bounds,
                quad,
                handle: iced::widget::image::Handle::from_rgba(
                    w,
                    h,
                    bytes::Bytes::from(patch.into_raw()),
                ),
                width: w,
                height: h,
            };
            image.inpaint.push(layer);
            pending_evs.push((image_id, bounds, quad));
            affected.insert(target_idx);
            applied += 1;
        }
    }
    for (image_id, bounds, quad) in pending_evs {
        let ev = app.tabs[idx]
            .project
            .add_inpaint_patch_with_bounds_and_quad(image_id, bounds, quad);
        crate::app::handle_model_event(&mut app.tabs[idx], ev);
    }
    if !affected.is_empty() {
        app.tabs[idx].show_inpaint = true;
    }
    applied
}

/// Frees whichever auto-inpaint backend weight is held. Returns true if anything freed.
#[cfg(feature = "inpaint")]
fn free_auto_queue(app: &mut App, tab_id: super::tab::TabId) -> bool {
    let mut freed = false;
    if app.engines.queue.complete(owner_of(tab_id), crate::app::queue::JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Telea)).is_some() {
        freed = true;
    }
    if app.engines.queue.complete(owner_of(tab_id), crate::app::queue::JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Lama)).is_some() {
        freed = true;
    }
    if app.engines.queue.complete(owner_of(tab_id), crate::app::queue::JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Aot)).is_some() {
        freed = true;
    }
    if app.engines.queue.complete(owner_of(tab_id), crate::app::queue::JobKind::Inpaint(easyscanlate_settings::InpaintBackend::ShiftMap)).is_some() {
        freed = true;
    }
    if app.engines.queue.complete(owner_of(tab_id), crate::app::queue::JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Harmonic)).is_some() {
        freed = true;
    }
    freed
}

/// Finalizes auto-inpaint when `pending==0`: summary status + queue promote.
/// Call only when `auto_inpaint_pending == 0`.
#[cfg(feature = "inpaint")]
fn finish_auto_stream(
    app: &mut App,
    tab_id: super::tab::TabId,
    idx: usize,
    label: &str,
) -> Task<Message> {
    #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
    {
        app.tabs[idx].pipeline_active = false;
    }
    let (total, failed) = (app.tabs[idx].auto_inpaint_total, app.tabs[idx].auto_inpaint_failed);
    let done = total.saturating_sub(failed);
    app.tabs[idx].status = if failed > 0 {
        format!("Auto-inpaint ({label}) done: {done} of {total} region(s), {failed} failed.")
    } else {
        format!("Auto-inpaint ({label}) done: {total} region(s).")
    };
    free_auto_queue(app, tab_id);
    let promote = crate::app::queue::dispatch_pending(app);
    crate::app::queue::refresh_queued_statuses(app);
    promote
}

/// Commits one manual group's patches. Returns number of patches applied.
#[cfg(feature = "inpaint")]
fn commit_manual_patches(app: &mut App, idx: usize, per_image_patches: GroupedInpaint) -> usize {
    let mut total = 0usize;
    let mut pending_evs = Vec::new();
    for (idx2, patches) in per_image_patches {
        let image_id_opt = app.tabs[idx].images.get(idx2).map(|i| i.image_id);
        if image_id_opt.is_none() {
            continue;
        }
        let image_id = image_id_opt.unwrap();
        if let Some(image) = app.tabs[idx].images.get_mut(idx2) {
            for (patch, bounds, quad) in patches {
                total += 1;
                let (w, h) = (patch.width(), patch.height());
                let layer = InpaintLayer {
                    bounds,
                    quad,
                    handle: iced::widget::image::Handle::from_rgba(
                        w,
                        h,
                        bytes::Bytes::from(patch.into_raw()),
                    ),
                    width: w,
                    height: h,
                };
                image.inpaint.push(layer);
                pending_evs.push((image_id, bounds, quad));
            }
        }
    }
    for (image_id, bounds, quad) in pending_evs {
        let ev = app.tabs[idx]
            .project
            .add_inpaint_patch_with_bounds_and_quad(image_id, bounds, quad);
        crate::app::handle_model_event(&mut app.tabs[idx], ev);
    }
    if total > 0 {
        app.tabs[idx].show_inpaint = true;
    }
    total
}

/// Frees whichever manual-inpaint backend weight is held.
#[cfg(feature = "inpaint")]
fn free_manual_queue(app: &mut App, tab_id: super::tab::TabId) -> bool {
    let mut freed = false;
    if app.engines.queue.complete(owner_of(tab_id), crate::app::queue::JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Telea)).is_some() {
        freed = true;
    }
    if app.engines.queue.complete(owner_of(tab_id), crate::app::queue::JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Lama)).is_some() {
        freed = true;
    }
    if app.engines.queue.complete(owner_of(tab_id), crate::app::queue::JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Aot)).is_some() {
        freed = true;
    }
    if app.engines.queue.complete(owner_of(tab_id), crate::app::queue::JobKind::Inpaint(easyscanlate_settings::InpaintBackend::ShiftMap)).is_some() {
        freed = true;
    }
    if app.engines.queue.complete(owner_of(tab_id), crate::app::queue::JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Harmonic)).is_some() {
        freed = true;
    }
    freed
}

#[cfg(feature = "inpaint")]
pub fn handle_style_inpaint_background(app: &mut App) -> Task<Message> {
    if app.active_state().is_bulk_busy() {
        app.active_tab_mut().status = "Wait for current task to finish.".to_string();
        return Task::none();
    }
    #[cfg(feature = "inpaint")]
    {
        if app.active_tab_mut().inpainting || app.active_tab_mut().running || app.active_tab_mut().translating {
            return Task::none();
        }
        let Some((index, id)) = app.active_tab_mut().selected else {
            return Task::none();
        };
        if index >= app.active_tab_mut().images.len() {
            return Task::none();
        }
        let (path, quad) = {
            let tab = app.active_tab();
            let Some(entry) = tab.project.entry(id) else {
                return Task::none();
            };
            let Some(img) = tab.images.get(index) else { return Task::none(); };
            let image_id = img.image_id;
            if entry.image_id != image_id {
                return Task::none();
            }
            let path = tab.project.image(image_id).map(|m| m.path.clone()).unwrap_or_default();
            let q = tab.project.view_quad(entry);
            (path, q)
        };
        {
            let _style = {
                let mut s = app.active_tab().style_working.clone();
                s.bg_color = [0, 0, 0, 0];
                s
            };
            app.active_tab_mut().style_working.bg_color = [0, 0, 0, 0];
            // Apply using cloned style to avoid double borrow
            let style_clone = app.active_tab().style_working.clone();
            if app.active_tab().project.entry(id).is_some() {
                let ev = app.active_tab_mut().project.set_entry_style_with_event(id, style_clone);
                crate::app::handle_model_event(app.active_tab_mut(), ev);
            }
        }
        let [x0, y0, x1, y1] = quad.bounds();
        let rect = [x0, y0, x1 - x0, y1 - y0];
        if rect[2] <= 0.0 || rect[3] <= 0.0 {
            app.active_tab_mut().status = "Inpaint Background: selected box is degenerate.".to_string();
            return Task::none();
        }
        // Single stays 1 job: reanchor beyond-quad / straddling entries to the
        // max-overlap page (translated into that page's local space). The
        // stitched exec run can then emit per-page patches from this 1 job.
        let (index, path, quad) = {
            let tab = app.active_tab();
            let n = tab.images.len();
            let mut page_ws = Vec::with_capacity(n);
            let mut page_hs = Vec::with_capacity(n);
            let mut page_paths = Vec::with_capacity(n);
            for img in &tab.images {
                if let Some(m) = tab.project.image(img.image_id) {
                    page_ws.push(m.width);
                    page_hs.push(m.height);
                    page_paths.push(m.path.clone());
                } else {
                    page_ws.push(0.0);
                    page_hs.push(0.0);
                    page_paths.push(String::new());
                }
            }
            let offsets = inpaint_global_offsets(&page_hs);
            match anchor_quad_global(quad, index, &page_ws, &page_hs, &offsets) {
                Some((page_idx, tquad)) => {
                    if page_idx != index {
                        eprintln!(
                            "[auto-inpaint::reanchor] id={:?} owner_idx={} -> page_idx={} quad_bounds={:?} translated={:?}",
                            id,
                            index,
                            page_idx,
                            quad.bounds(),
                            tquad.bounds(),
                        );
                    }
                    let tpath = page_paths.get(page_idx).cloned().unwrap_or(path);
                    (page_idx, tpath, tquad)
                }
                None => {
                    eprintln!(
                        "[auto-inpaint::no-intersection] idx={} id={:?} quad_bounds=[{:.1},{:.1},{:.1},{:.1}] -> skipped (no page intersects)",
                        index, id, x0, y0, x1, y1,
                    );
                    app.active_tab_mut().status =
                        "Inpaint Background: box is outside all pages.".to_string();
                    return Task::none();
                }
            }
        };
        let (backend, radius) = easyscanlate_settings::get(|s| (s.inpaint_backend, s.inpaint_radius.parse::<i32>().unwrap_or(5).max(1)));
        // queue gate for background stitch (single inpaint)
        {
            use crate::app::queue::{AcquireResult, JobKind, owner_of};
            let kind = JobKind::Inpaint(backend);
            let tab_id = app.active_tab().id;
            let already_running = app.engines.queue.running_for(owner_of(tab_id), kind).is_some();
            let already_queued = app.engines.queue.pending_for_tab(owner_of(tab_id)).iter().any(|j| j.kind == kind);
            if !already_running && !already_queued {
                match app.engines.queue.try_acquire_or_enqueue(owner_of(tab_id), kind) {
                    AcquireResult::Acquired(_) => {},
                    AcquireResult::Queued(_, pos) => {
                        let used = app.engines.queue.used_weight();
                        // store pending for later dispatch (use pending_background_stitch)
                        let pad_tmp = auto_pad_for(backend, radius);
                        let (prev_tmp, next_tmp) = neighbor_paths(app, index);
                        let job_tmp = AutoInpaintJob { index, id, path: path.clone(), quad };
                        app.active_tab_mut().pending_background_stitch = Some((job_tmp, pad_tmp, prev_tmp, next_tmp));
                        app.active_tab_mut().status = format!(
                            "Queued {} (pos {}, pool {}/{}) ...",
                            kind.label(),
                            pos,
                            used,
                            crate::app::queue::POOL_CAPACITY
                        );
                        return Task::none();
                    }
                }
            } else if already_queued || already_running {
                app.active_tab_mut().status = "Wait for current task to finish.".to_string();
                return Task::none();
            }
        }
        let pad = auto_pad_for(backend, radius);
        let (prev, next) = neighbor_paths(app, index);
        let job = AutoInpaintJob { index, id, path: path.clone(), quad };
        let cached = app.engines.shared_inpaint(backend, radius);
        let tid = app.active_tab().id;
        match cached {
            Some(engine) => start_background_stitch(app, tid, engine, job, pad, prev, next),
            None => {
                app.active_tab_mut().pending_background_stitch = Some((job, pad, prev, next));
                // Model load counts as the run itself so buttons disable during it.
                app.active_tab_mut().inpainting = true;
                app.active_tab_mut().status = match backend {
                    InpaintBackend::Lama => "Loading LaMa model...".to_string(),
                    InpaintBackend::Aot => "Loading AOT-GAN model...".to_string(),
                    InpaintBackend::Telea => "Inpainting background...".to_string(),
                    InpaintBackend::ShiftMap => "Inpainting background (ShiftMap)...".to_string(),
                    InpaintBackend::Harmonic => "Inpainting background (Harmonic)...".to_string(),
                };
                let kind = crate::app::queue::JobKind::Inpaint(backend);
                let job_id = app
                    .engines
                    .queue
                    .running_for(crate::app::queue::owner_of(tid), kind)
                    .map(|j| j.id)
                    .unwrap_or(0);
                Task::perform(
                    async move {
                        use crate::app::queue::engine_ready_msg;
                        use easyscanlate_engine_pool::BuiltEngine;
                        match easyscanlate_inpaint::Engine::build(backend, radius) {
                            Ok(engine) => engine_ready_msg(
                                tid,
                                job_id,
                                kind,
                                Ok(BuiltEngine::Inpaint { backend, engine }),
                            ),
                            Err(e) => engine_ready_msg(tid, job_id, kind, Err(e)),
                        }
                    },
                    |msg| msg,
                )
            }
        }
    }
    #[cfg(not(feature = "inpaint"))]
    {
        let Some((_index, id)) = app.active_tab().selected else {
            return Task::none();
        };
        app.active_tab_mut().style_working.bg_color = [0, 0, 0, 0];
        let style_clone = app.active_tab().style_working.clone();
        if app.active_tab().project.entry(id).is_some() {
            let ev = app.active_tab_mut().project.set_entry_style_with_event(id, style_clone);
            crate::app::handle_model_event(app.active_tab_mut(), ev);
        }
        app.active_tab_mut().status = "Background made transparent (inpaint not available in this build).".to_string();
        return Task::none();
    }
}

/// Menu pick of the styling panel's "Inpaint Background" split button:
/// persist the new default backend (select-only — starts no job; the main
/// area runs via [`handle_style_inpaint_background`], which reads the store).
pub fn handle_style_inpaint_backend_selected(
    app: &mut App,
    backend: easyscanlate_settings::InpaintBackend,
) -> Task<Message> {
    let _ = easyscanlate_settings::modify(|s| s.inpaint_backend = backend);
    app.active_tab_mut().status = format!("Inpaint backend: {backend}.");
    Task::none()
}

pub fn handle_inpaint_clicked(app: &mut App, selection: Option<(usize, usize)>) -> Task<Message> {
    use super::edit::clear_editing;
    clear_editing(app);
    match selection {
        Some((image_index, patch_idx)) => {
            let (image_id, inpaint_len) = {
                let tab = app.active_tab();
                let Some(img) = tab.images.get(image_index) else {
                    let _ = tab;
                    app.active_tab_mut().status = "That inpaint layer no longer exists.".to_string();
                    return Task::none();
                };
                (img.image_id, img.inpaint.len())
            };
            let extras_len = app
                .active_tab()
                .project
                .extras
                .inpaint_patches
                .iter()
                .filter(|p| p.image_id == image_id)
                .count();
            let valid = patch_idx < inpaint_len || patch_idx < extras_len;
            if !valid {
                app.active_tab_mut().status = "That inpaint layer no longer exists.".to_string();
                return Task::none();
            }
            app.active_tab_mut().selected = None;
            app.active_tab_mut().selected_inpaint = Some((image_index, patch_idx));
            app.active_tab_mut().status = format!("Inpaint {patch_idx} selected – overlays hidden.");
            let needs_settle = {
                let len = app.active_tab().images.len();
                app.active_tab_mut().scheduler.needs_settle(image_index, len)
            };
            if needs_settle {
                let tid = app.active_tab().id;
                return app.active_tab_mut().scheduler.schedule(image_index..image_index+1, move |seq| Message::Tab(tid, crate::app::TabMessage::SettleElapsed(seq)));
            }
            Task::none()
        }
        None => {
            app.active_tab_mut().selected_inpaint = None;
            app.active_tab_mut().status = "Inpaint deselected – overlays shown.".to_string();
            Task::none()
        }
    }
}

pub fn handle_inpaint_delete(app: &mut App, image_index: usize, patch_idx: usize) -> Task<Message> {
    if app.active_state().is_bulk_busy() {
        app.active_tab_mut().status = "Wait for current task to finish.".to_string();
        return Task::none();
    }
    let Some(image) = app.active_tab_mut().images.get_mut(image_index) else {
        return Task::none();
    };
    let image_id = image.image_id;
    let inpaint_len = image.inpaint.len();
    let _ = image;
    let extras_len = app
        .active_tab()
        .project
        .extras
        .inpaint_patches
        .iter()
        .filter(|p| p.image_id == image_id)
        .count();
    let len = inpaint_len.max(extras_len);
    if patch_idx >= len {
        return Task::none();
    }
    if let Some(img) = app.active_tab_mut().images.get_mut(image_index)
        && patch_idx < img.inpaint.len() {
            img.inpaint.remove(patch_idx);
        }
    let patch_id = app.active_tab().project.extras.inpaint_patches.iter().filter(|p| p.image_id == image_id).nth(patch_idx).map(|p| p.id);
    if let Some(id) = patch_id
        && let Some(ev) = app.active_tab_mut().project.remove_inpaint_patch(id) {
            crate::app::handle_model_event(app.active_tab_mut(), ev);
        }
    if app.active_tab_mut().selected_inpaint == Some((image_index, patch_idx)) {
        app.active_tab_mut().selected_inpaint = None;
    } else if let Some((sel_img, sel_patch)) = app.active_tab_mut().selected_inpaint
        && sel_img == image_index && sel_patch > patch_idx {
            app.active_tab_mut().selected_inpaint = Some((sel_img, sel_patch - 1));
        }
    app.active_tab_mut().status = "Deleted inpaint patch.".to_string();
    Task::none()
}

pub fn handle_inpaint_repaint(app: &mut App, image_index: usize, patch_idx: usize) -> Task<Message> {
    if app.active_state().is_bulk_busy() {
        app.active_tab_mut().status = "Wait for current task to finish.".to_string();
        return Task::none();
    }
    if app.active_tab_mut().inpainting || app.active_tab_mut().running || app.active_tab_mut().translating {
        return Task::none();
    }
    let (path, rect, quads) = {
        let image = app.active_tab().images.get(image_index).cloned().unwrap_or_else(|| panic!("inpaint repaint missing image"));
        // Use cloned image to avoid borrow across project access
        let image_id = image.image_id;
        let bounds_opt = if patch_idx < image.inpaint.len() {
            Some(image.inpaint[patch_idx].bounds)
        } else {
            None
        };
        // Need to check if image still exists (we cloned, so safe)
        if app.active_tab().images.get(image_index).is_none() {
            return Task::none();
        }
        let extras_patch = {
            let tab = app.active_tab();
            let mut seen = 0usize;
            let mut found = None;
            for p in &tab.project.extras.inpaint_patches {
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
        let bounds = if let Some(b) = bounds_opt {
            b
        } else if let Some(b) = extras_patch {
            b
        } else {
            return Task::none();
        };
        let rect = [bounds[0], bounds[1], bounds[2], bounds[3]];
        let (quads, path) = {
            let tab = app.active_tab();
            let quads: Vec<Quad> = tab
                .project
                .all_for(image_id)
                .map(|e| tab.project.view_quad(e))
                .filter(|q| q.intersects_rect(rect))
                .collect();
            let path = tab
                .project
                .image(image_id)
                .map(|m| m.path.clone())
                .unwrap_or_default();
            (quads, path)
        };
        (path, rect, quads)
    };
    {
        let image_id_opt = app.active_tab().images.get(image_index).map(|i| i.image_id);
        if let Some(image_id) = image_id_opt {
            let patch_id = app.active_tab().project.extras.inpaint_patches.iter().filter(|p| p.image_id == image_id).nth(patch_idx).map(|p| p.id);
            if let Some(image) = app.active_tab_mut().images.get_mut(image_index)
                && patch_idx < image.inpaint.len() {
                    image.inpaint.remove(patch_idx);
                }
            if let Some(id) = patch_id
                && let Some(ev) = app.active_tab_mut().project.remove_inpaint_patch(id) {
                    crate::app::handle_model_event(app.active_tab_mut(), ev);
                }
            let sel = app.active_tab().selected_inpaint;
            if sel == Some((image_index, patch_idx)) {
                app.active_tab_mut().selected_inpaint = None;
            } else if let Some((sel_img, sel_patch)) = sel
                && sel_img == image_index && sel_patch > patch_idx {
                    app.active_tab_mut().selected_inpaint = Some((sel_img, sel_patch - 1));
                }
        }
    }
    #[cfg(feature = "inpaint")]
    {
        let (backend, radius) = easyscanlate_settings::get(|s| {
            (
                s.inpaint_backend,
                s.inpaint_radius.parse::<i32>().unwrap_or(5).max(1),
            )
        });
        // queue gate for repaint manual inpaint (single selection)
        {
            use crate::app::queue::{AcquireResult, JobKind, owner_of};
            let kind = JobKind::Inpaint(backend);
            let tab_id = app.active_tab().id;
            let already_running = app.engines.queue.running_for(owner_of(tab_id), kind).is_some();
            let already_queued = app.engines.queue.pending_for_tab(owner_of(tab_id)).iter().any(|j| j.kind == kind);
            if !already_running && !already_queued {
                match app.engines.queue.try_acquire_or_enqueue(owner_of(tab_id), kind) {
                    AcquireResult::Acquired(_) => {},
                    AcquireResult::Queued(_, pos) => {
                        let used = app.engines.queue.used_weight();
                        app.active_tab_mut().pending_manual_multi = Some(vec![(image_index, path.clone(), rect, quads.clone())]);
                        app.active_tab_mut().status = format!(
                            "Queued {} (pos {}, pool {}/{}) ...",
                            kind.label(),
                            pos,
                            used,
                            crate::app::queue::POOL_CAPACITY
                        );
                        return Task::none();
                    }
                }
            } else if already_queued || already_running {
                app.active_tab_mut().status = "Wait for current task to finish.".to_string();
                return Task::none();
            }
        }
        let cached = app.engines.shared_inpaint(backend, radius);
        let tid = app.active_tab().id;
        match cached {
            Some(engine) => start_inpaint(app, engine, image_index, path, rect, quads),
            None => {
                app.active_tab_mut().pending_manual_multi = Some(vec![(image_index, path, rect, quads)]);
                // Model load counts as the run itself so buttons disable during it.
                app.active_tab_mut().inpainting = true;
                app.active_tab_mut().status = match backend {
                    easyscanlate_settings::InpaintBackend::Lama => "Loading LaMa model...".to_string(),
                    easyscanlate_settings::InpaintBackend::Aot => "Loading AOT-GAN model...".to_string(),
                    easyscanlate_settings::InpaintBackend::Telea => "Inpainting...".to_string(),
                    easyscanlate_settings::InpaintBackend::ShiftMap => "Inpainting (ShiftMap)...".to_string(),
                    easyscanlate_settings::InpaintBackend::Harmonic => "Inpainting (Harmonic)...".to_string(),
                };
                let kind = crate::app::queue::JobKind::Inpaint(backend);
                let job_id = app
                    .engines
                    .queue
                    .running_for(crate::app::queue::owner_of(tid), kind)
                    .map(|j| j.id)
                    .unwrap_or(0);
                Task::perform(
                    async move {
                        use crate::app::queue::engine_ready_msg;
                        use easyscanlate_engine_pool::BuiltEngine;
                        match easyscanlate_inpaint::Engine::build(backend, radius) {
                            Ok(engine) => engine_ready_msg(
                                tid,
                                job_id,
                                kind,
                                Ok(BuiltEngine::Inpaint { backend, engine }),
                            ),
                            Err(e) => engine_ready_msg(tid, job_id, kind, Err(e)),
                        }
                    },
                    |msg| msg,
                )
            }
        }
    }
    #[cfg(not(feature = "inpaint"))]
    {
        let _ = (path, rect, quads);
        app.active_tab_mut().status = "Inpaint is not available in this build.".to_string();
        Task::none()
    }
}

pub fn handle_inpaint_toolbar(app: &mut App, image_index: usize, patch_idx: usize, action: easyscanlate_ui::event::InpaintToolbarAction) -> Task<Message> {
    match action {
        easyscanlate_ui::event::InpaintToolbarAction::Delete => {
            handle_inpaint_delete(app, image_index, patch_idx)
        }
        easyscanlate_ui::event::InpaintToolbarAction::Repaint => {
            handle_inpaint_repaint(app, image_index, patch_idx)
        }
    }
}


#[cfg(feature = "inpaint")]
pub fn handle_inpaint_selection(app: &mut App, selections: Vec<(usize, iced::Rectangle)>) -> Task<Message> {
    {
        let tab = app.active_tab();
        eprintln!("[manual::inpaint] handle_inpaint_selection selections={} cached_any={} running={} translating={} inpainting={}", selections.len(), app.engines.has_any_shared_inpaint(), tab.running, tab.translating, tab.inpainting);
    }
    for (i, (idx, r)) in selections.iter().enumerate() {
        eprintln!("[manual::inpaint]   sel {}: idx={} rect=[{:.1},{:.1},{:.1},{:.1}] w={:.1} h={:.1}", i, idx, r.x, r.y, r.width, r.height, r.width, r.height);
    }
    if selections.is_empty() {
        eprintln!("[manual::inpaint] no selections -> none");
        return Task::none();
    }
    if app.active_state().is_bulk_busy() {
        eprintln!("[manual::inpaint] bulk busy -> none");
        return Task::none();
    }
    if app.active_tab_mut().inpainting || app.active_tab_mut().running || app.active_tab_mut().translating {
        {
            let tab = app.active_tab();
            eprintln!("[manual::inpaint] busy -> none (inpainting={} running={} translating={})", tab.inpainting, tab.running, tab.translating);
        }
        return Task::none();
    }
    #[cfg(feature = "ocr")]
    if app.active_tab().manual_ocring {
        eprintln!("[manual::inpaint] manual_ocring busy -> none");
        return Task::none();
    }
    // Build per-selection data: (idx, path, rect, quads)
    let mut data: Vec<(usize, String, [f32; 4], Vec<Quad>)> = Vec::new();
    for (idx, rect) in selections {
        let image_id_opt = app.active_tab().images.get(idx).map(|i| i.image_id);
        let Some(image_id) = image_id_opt else {
            eprintln!("[manual::inpaint] skip idx={} out of range images.len={}", idx, app.active_tab().images.len());
            continue;
        };
        let path = app.active_tab().project.image(image_id).map(|m| m.path.clone()).unwrap_or_default();
        if path.is_empty() {
            eprintln!("[manual::inpaint] skip idx={} empty path", idx);
            continue;
        }
        let rect_arr = [rect.x, rect.y, rect.width, rect.height];
        let all_count = app.active_tab().project.all_for(image_id).count();
        let quads: Vec<Quad> = {
            let tab = app.active_tab();
            tab.project.all_for(image_id)
            .map(|e| tab.project.view_quad(e))
            .filter(|q| q.intersects_rect(rect_arr))
            .collect()
        };
        eprintln!("[manual::inpaint] idx={} path={} rect={:?} all_for={} quads_intersect={} quad_bounds={:?}", idx, path, rect_arr, all_count, quads.len(), quads.iter().map(|q| q.bounds()).collect::<Vec<_>>());
        // keep even empty (will synthesize later if mixed)
        data.push((idx, path, rect_arr, quads));
    }
    if data.is_empty() {
        eprintln!("[manual::inpaint] data empty after filtering -> no valid selections");
        app.active_tab_mut().status = "No valid selections.".to_string();
        return Task::none();
    }
    data.sort_by_key(|(idx, _, _, _)| *idx);
    eprintln!("[manual::inpaint] data sorted len={} idxs={:?} rects={:?}", data.len(), data.iter().map(|(idx,_,_,_)| *idx).collect::<Vec<_>>(), data.iter().map(|(_,_,r,_)| *r).collect::<Vec<_>>());
    let (backend, radius) = easyscanlate_settings::get(|s| (s.inpaint_backend, s.inpaint_radius.parse::<i32>().unwrap_or(5).max(1)));
    eprintln!("[manual::inpaint] backend={:?} radius={} cached_match={}", backend, radius, app.engines.has_shared_inpaint(backend, radius));
    // queue gate — manual inpaint uses same backend weight/priority as auto
    {
        use crate::app::queue::{AcquireResult, JobKind, owner_of};
        let kind = JobKind::Inpaint(backend);
        let tab_id = app.active_tab().id;
        // Avoid duplicate queue if already running/queued for this tab+kind
        let already_running = app.engines.queue.running_for(owner_of(tab_id), kind).is_some();
        let already_queued = app.engines.queue.pending_for_tab(owner_of(tab_id)).iter().any(|j| j.kind == kind);
        if !already_running && !already_queued {
            match app.engines.queue.try_acquire_or_enqueue(owner_of(tab_id), kind) {
                AcquireResult::Acquired(_) => {
                    // weight reserved, proceed to engine/start
                }
                AcquireResult::Queued(_, pos) => {
                    let used = app.engines.queue.used_weight();
                    app.active_tab_mut().pending_manual_multi = Some(data);
                    app.active_tab_mut().status = format!(
                        "Queued {} (pos {}, pool {}/{}) ...",
                        kind.label(),
                        pos,
                        used,
                        crate::app::queue::POOL_CAPACITY
                    );
                    return Task::none();
                }
            }
        } else if already_queued || already_running {
            // duplicate submission while queued/running -> treat as busy
            app.active_tab_mut().status = "Wait for current task to finish.".to_string();
            return Task::none();
        }
    }
    let cached = app.engines.shared_inpaint(backend, radius);
    // Store pending for engine build path (weight already reserved via queue)
    if let Some(engine) = cached {
        eprintln!("[manual::inpaint] using cached engine -> start_inpaint_selection");
        let tid = app.active_tab().id;
        start_inpaint_selection(app, tid, engine, data)
    } else {
        eprintln!("[manual::inpaint] no cached engine -> pending_manual_multi len={} status loading", data.len());
        let tid = app.active_tab().id;
        app.active_tab_mut().pending_manual_multi = Some(data);
        // Model load counts as the run itself so buttons disable during it.
        app.active_tab_mut().inpainting = true;
        app.active_tab_mut().status = match backend {
            InpaintBackend::Lama => "Loading LaMa model...".to_string(),
            InpaintBackend::Aot => "Loading AOT-GAN model...".to_string(),
            InpaintBackend::Telea => "Inpainting...".to_string(),
            InpaintBackend::ShiftMap => "Inpainting (ShiftMap)...".to_string(),
            InpaintBackend::Harmonic => "Inpainting (Harmonic)...".to_string(),
        };
        let kind = crate::app::queue::JobKind::Inpaint(backend);
        let job_id = app
            .engines
            .queue
            .running_for(crate::app::queue::owner_of(tid), kind)
            .map(|j| j.id)
            .unwrap_or(0);
        Task::perform(
            async move {
                use crate::app::queue::engine_ready_msg;
                use easyscanlate_engine_pool::BuiltEngine;
                match easyscanlate_inpaint::Engine::build(backend, radius) {
                    Ok(engine) => engine_ready_msg(
                        tid,
                        job_id,
                        kind,
                        Ok(BuiltEngine::Inpaint { backend, engine }),
                    ),
                    Err(e) => engine_ready_msg(tid, job_id, kind, Err(e)),
                }
            },
            |msg| msg,
        )
    }
}


#[cfg(feature = "inpaint")]
fn run_inpaint_selection(engine: &InpaintEngine, data: Vec<ManualInpaintSelection>) -> Result<ManualStreamPatches, String> {
    eprintln!("[manual::multi] run_inpaint_selection enter data={} backend={:?} radius={}", data.len(), engine.backend(), engine.radius());
    for (i, (idx, path, rect, quads)) in data.iter().enumerate() {
        eprintln!("[manual::multi]   data {}: idx={} path={} rect={:?} quads={} bounds={:?}", i, idx, path, rect, quads.len(), quads.iter().map(|q| q.bounds()).collect::<Vec<_>>());
        for (qi, q) in quads.iter().enumerate() {
            eprintln!("[manual::multi]     quad {}: {:?}", qi, q.points);
        }
    }
    if data.is_empty() { return Err("no selections".to_string()); }
    use std::collections::HashMap;
    // Per-spec: selection as mask, drop OCR quads entirely (q2)
    // Sel holds raw selection in image pixels, with image dims
    #[derive(Clone)]
    struct Sel {
        idx: usize,
        path: String,
        rect: [f32;4],
        x0: u32,
        y0: u32,
        w: u32,
        h: u32,
        img_w: u32,
        img_h: u32,
    }
    let mut image_cache: HashMap<String, (image::RgbaImage, u32, u32)> = HashMap::new();
    let mut sels: Vec<Sel> = Vec::new();
    for (idx, path, rect_arr, _quads) in data {
        let need_decode = !image_cache.contains_key(&path);
        if need_decode {
            eprintln!("[manual::multi] decode path={}", path);
            let rgba = image::ImageReader::open(&path)
                .map_err(|e| format!("Failed to open {path}: {e}"))?
                .with_guessed_format().map_err(|e| format!("Failed to decode {path}: {e}"))?
                .decode().map_err(|e| format!("Failed to decode {path}: {e}"))?
                .into_rgba8();
            let (w,h)=rgba.dimensions();
            eprintln!("[manual::multi]   decoded {}x{}", w, h);
            image_cache.insert(path.clone(), (rgba,w,h));
        }
        let (_full, img_w, img_h) = image_cache.get(&path).unwrap();
        let [rx, ry, rw, rh] = rect_arr;
        eprintln!("[manual::multi] rect_arr idx={} [{:.1},{:.1},{:.1},{:.1}] img={}x{}", idx, rx, ry, rw, rh, img_w, img_h);
        let x0 = rx.floor().clamp(0.0, *img_w as f32 -1.0) as u32;
        let y0 = ry.floor().clamp(0.0, *img_h as f32 -1.0) as u32;
        let x1 = (rx+rw).ceil().clamp(x0 as f32 +1.0, *img_w as f32) as u32;
        let y1 = (ry+rh).ceil().clamp(y0 as f32 +1.0, *img_h as f32) as u32;
        let cw = x1.saturating_sub(x0);
        let ch = y1.saturating_sub(y0);
        eprintln!("[manual::multi]   -> x0={} y0={} x1={} y1={} cw={} ch={} ", x0, y0, x1, y1, cw, ch);
        if cw==0 || ch==0 {
            eprintln!("[manual::multi]   skip zero cw/ch");
            continue;
        }
        sels.push(Sel { idx, path: path.clone(), rect: rect_arr, x0, y0, w: cw, h: ch, img_w: *img_w, img_h: *img_h });
    }
    eprintln!("[manual::multi] sels built len={}", sels.len());
    for (i, s) in sels.iter().enumerate() {
        eprintln!("[manual::multi]   sel {}: idx={} x0={} y0={} w={} h={} img={}x{} rect={:?}", i, s.idx, s.x0, s.y0, s.w, s.h, s.img_w, s.img_h, s.rect);
    }
    if sels.is_empty() { return Err("no valid pieces".to_string()); }
    // sort by image idx then y then x deterministically
    sels.sort_by(|a,b| a.idx.cmp(&b.idx).then(a.y0.cmp(&b.y0)).then(a.x0.cmp(&b.x0)));
    eprintln!("[manual::multi] sels sorted");
    for (i, s) in sels.iter().enumerate() {
        eprintln!("[manual::multi]   sorted {}: idx={} x0={} y0={} w={} h={}", i, s.idx, s.x0, s.y0, s.w, s.h);
    }
    const CANVAS: u32 = 512;
    // Helper to compute group metrics: per-image bbox and total stitched height
    // Returns (max_w_span, total_h_span, oversized_present)
    let metrics = |group: &[Sel]| -> (u32, u32, bool) {
        use std::collections::HashMap;
        let mut per: HashMap<usize, (u32,u32,u32,u32)> = HashMap::new(); // minX,maxX,minY,maxY per idx
        let mut oversized = false;
        for s in group {
            // individual oversized already flagged, but also check combined
            if s.w > CANVAS || s.h > CANVAS { oversized = true; }
            let e = per.entry(s.idx).or_insert((s.x0, s.x0+s.w, s.y0, s.y0+s.h));
            e.0 = e.0.min(s.x0);
            e.1 = e.1.max(s.x0+s.w);
            e.2 = e.2.min(s.y0);
            e.3 = e.3.max(s.y0+s.h);
        }
        let mut max_w = 0u32;
        let mut total_h = 0u32;
        for (_idx, (minx, maxx, miny, maxy)) in per {
            let w = maxx - minx;
            let h = maxy - miny;
            if w > CANVAS || h > CANVAS { oversized = true; }
            max_w = max_w.max(w);
            total_h = total_h.saturating_add(h);
        }
        (max_w, total_h, oversized)
    };
    // Group nearby that fit without resizing inside 512x512 (bbox span)
    // Arbitrary N stitch: sum h_spans <=512 and max w <=512
    let mut groups: Vec<Vec<Sel>> = Vec::new();
    let mut cur: Vec<Sel> = Vec::new();
    for s in sels {
        if s.w > CANVAS || s.h > CANVAS {
            eprintln!("[manual::multi] grouping sel idx={} {}x{} > 512 -> solo oversized group", s.idx, s.w, s.h);
            if !cur.is_empty() { groups.push(std::mem::take(&mut cur)); }
            groups.push(vec![s]);
            continue;
        }
        if cur.is_empty() {
            cur.push(s);
            continue;
        }
        let mut hypo = cur.clone();
        hypo.push(s.clone());
        let (max_w, total_h, oversized) = metrics(&hypo);
        eprintln!("[manual::multi] grouping try add idx={} w={} h={} to cur len={} -> hypo max_w={} total_h={} oversized={}", s.idx, s.w, s.h, cur.len(), max_w, total_h, oversized);
        if oversized || max_w > CANVAS || total_h > CANVAS {
            eprintln!("[manual::multi]   would exceed 512 -> flush cur len={} and start new", cur.len());
            groups.push(std::mem::take(&mut cur));
            cur.push(s);
        } else {
            eprintln!("[manual::multi]   fits -> push cur");
            cur = hypo;
        }
    }
    if !cur.is_empty() { groups.push(cur); }
    eprintln!("[manual::multi] groups built len={}", groups.len());
    for (gi, g) in groups.iter().enumerate() {
        let (max_w, total_h, _) = metrics(g);
        eprintln!("[manual::multi]   group {}: sels={} max_w={} total_h={} member={:?}", gi, g.len(), max_w, total_h, g.iter().map(|s| (s.idx, s.x0, s.y0, s.w, s.h)).collect::<Vec<_>>());
    }
    let mut per_image: PatchMap = HashMap::new();
    // Granular (OCR-style): one group's inference failure counts as one failed
    // group and the stream continues with the remaining groups instead of
    // dropping every patch.
    let mut failed_groups: usize = 0;
    // helpers
    let reflect_index = |x: i64, len: i64| -> i64 {
        let period = len*2;
        let mut v = x % period;
        if v < 0 { v += period; }
        if v >= len { period - v -1 } else { v }
    };
    for (group_idx, group) in groups.into_iter().enumerate() {
        if group.is_empty() { continue; }
        eprintln!("[manual::multi] group {} processing sels={} ", group_idx, group.len());
        // Oversized solo: square crop + resize to 512 (spec q3)
        if group.len()==1 && (group[0].w > CANVAS || group[0].h > CANVAS) {
            let s = &group[0];
            eprintln!("[manual::multi] group {} oversized solo idx={} {}x{} img={}x{} rect={:?}", group_idx, s.idx, s.w, s.h, s.img_w, s.img_h, s.rect);
            // per spec: take full width or height based on larger side, crop square, resize to 512 (q3)
            // Use shared helper from inpaint crate for consistency with tests
            let (side, sx_u, sy_u) = easyscanlate_inpaint::manual_square_params(s.x0, s.y0, s.w, s.h, s.img_w, s.img_h);
            let sx = sx_u as i32;
            let sy = sy_u as i32;
            let larger_is_w = s.w >= s.h;
            let side_full = if larger_is_w { s.img_w } else { s.img_h };
            let min_dim = s.img_w.min(s.img_h);
            eprintln!("[manual::multi]   oversized side computed larger_is_w={} side_full={} side={} min_dim={} sx={} sy={}", larger_is_w, side_full, side, min_dim, sx, sy);
            let full = &image_cache.get(&s.path).unwrap().0;
            let square_rgba = image::imageops::crop_imm(full, sx as u32, sy as u32, side, side).to_image();
            let canvas_rgba = image::DynamicImage::ImageRgba8(square_rgba).resize(CANVAS, CANVAS, image::imageops::FilterType::Lanczos3).to_rgba8();
            let scale = CANVAS as f32 / side as f32;
            // mask quad in canvas coords (selection rect scaled)
            let qx = (s.x0 as i32 - sx) as f32 * scale;
            let qy = (s.y0 as i32 - sy) as f32 * scale;
            let qw = s.w as f32 * scale;
            let qh = s.h as f32 * scale;
            let quad = Quad { points: [[qx, qy], [qx+qw, qy], [qx+qw, qy+qh], [qx, qy+qh]] };
            eprintln!("[manual::multi]   canvas 512 mask quad {:?} scale={:.4} sx,sy={},{} side={}", quad.points, scale, sx, sy, side);
            let rect = [0.0, 0.0, CANVAS as f32, CANVAS as f32];
            let patches = match engine.run_on_image(&canvas_rgba, rect, &[quad]) {
                Ok(v) => { eprintln!("[manual::multi]   oversized patches={}", v.len()); v },
                Err(e) => { eprintln!("[manual::multi]   oversized run_on_image failed: {}", e); failed_groups += 1; continue; }
            };
            for (pi, (patch_img, bounds_canvas, quad_opt)) in patches.into_iter().enumerate() {
                let [bx, by, bw, bh] = bounds_canvas;
                eprintln!("[manual::multi]   oversized patch {}: bounds_canvas=[{:.1},{:.1},{:.1},{:.1}] patch={}x{} quad={:?}", pi, bx, by, bw, bh, patch_img.width(), patch_img.height(), quad_opt.map(|q| q.points));
                // map bounds back to original image coords via inverse scale
                let orig_x = sx as f32 + bx / scale;
                let orig_y = sy as f32 + by / scale;
                let orig_w = bw / scale;
                let orig_h = bh / scale;
                // resize patch back to orig size (inverse scale)
                let _pw = (orig_w.round() as u32).max(1).min(patch_img.width());
                let _ph = (orig_h.round() as u32).max(1).min(patch_img.height());
                // patch_img is bounds size in canvas coords; resize to orig_w/h
                let resized = if (orig_w as u32) != patch_img.width() || (orig_h as u32) != patch_img.height() {
                    let ow = orig_w.round().clamp(1.0, 5000.0) as u32;
                    let oh = orig_h.round().clamp(1.0, 5000.0) as u32;
                    eprintln!("[manual::multi]     resizing patch {}x{} -> {}x{} (scale 1/{:.4})", patch_img.width(), patch_img.height(), ow, oh, scale);
                    image::DynamicImage::ImageRgba8(patch_img).resize(ow, oh, image::imageops::FilterType::Lanczos3).to_rgba8()
                } else { patch_img };
                let bounds = [orig_x, orig_y, orig_w, orig_h];
                let orig_quad = quad_opt.map(|q| {
                    let mut nq = q;
                    for pt in &mut nq.points {
                        pt[0] = pt[0] / scale + sx as f32;
                        pt[1] = pt[1] / scale + sy as f32;
                    }
                    nq
                });
                eprintln!("[manual::multi]     -> orig_bounds={:?} quad={:?} resized_patch={}x{}", bounds, orig_quad.map(|q| q.points), resized.width(), resized.height());
                per_image.entry(s.idx).or_default().push((resized, bounds, orig_quad));
            }
            continue;
        }
        // Normal group: build raw window canvas 512x512 with surrounding pixels
        // Compute per distinct image bbox
        // We need distinct images sorted by idx (and for same idx, aggregated bbox already)
        // Build map distinct idx -> list of sels for that idx
        let mut per_idx_sels: HashMap<usize, Vec<&Sel>> = HashMap::new();
        for s in &group { per_idx_sels.entry(s.idx).or_default().push(s); }
        let mut distinct: Vec<usize> = per_idx_sels.keys().cloned().collect();
        distinct.sort();
        eprintln!("[manual::multi] group {} distinct images {:?} per_idx_counts {:?}", group_idx, distinct, per_idx_sels.iter().map(|(k,v)| (*k, v.len())).collect::<Vec<_>>());
        // For each distinct, compute bbox
        struct ImgGroup {
            idx: usize,
            path: String,
            img_w: u32,
            img_h: u32,
            min_y: u32,
            max_y: u32,
            h_span: u32,
            cx: f32,
            cy: f32,
        }
        let mut img_groups: Vec<ImgGroup> = Vec::new();
        for didx in &distinct {
            let list = &per_idx_sels[didx];
            let min_x = list.iter().map(|s| s.x0).min().unwrap();
            let max_x = list.iter().map(|s| s.x0 + s.w).max().unwrap();
            let min_y = list.iter().map(|s| s.y0).min().unwrap();
            let max_y = list.iter().map(|s| s.y0 + s.h).max().unwrap();
            let h_span = max_y - min_y;
            let cx = (min_x as f32 + max_x as f32)*0.5;
            let cy = (min_y as f32 + max_y as f32)*0.5;
            let img_w = list[0].img_w;
            let img_h = list[0].img_h;
            let path = list[0].path.clone();
            img_groups.push(ImgGroup { idx:*didx, path, img_w, img_h, min_y, max_y, h_span, cx, cy });
            eprintln!("[manual::multi]   img_group idx={} bbox [{},{},{},{}] h_span={} cx={:.1} cy={:.1} img={}x{}", didx, min_x, min_y, max_x, max_y, h_span, cx, cy, img_w, img_h);
        }
        // Allocate window heights to fill 512 if stitch multiple
        // For single distinct, window is single 512 window centered on bbox
        // For multiple, need to allocate h_src per image
        // Use strategy: sum h_span = total_h, extra = 512 - total_h
        // Distribute extra based on avail_top/bottom per image
        if img_groups.len()==1 {
            let ig = &img_groups[0];
            let w_src = CANVAS.min(ig.img_w);
            let h_src = CANVAS.min(ig.img_h);
            let mut x_src = (ig.cx - w_src as f32 *0.5).round() as i32;
            let mut y_src = (ig.cy - h_src as f32 *0.5).round() as i32;
            x_src = x_src.clamp(0, ig.img_w as i32 - w_src as i32).max(0);
            y_src = y_src.clamp(0, ig.img_h as i32 - h_src as i32).max(0);
            eprintln!("[manual::multi]   single-img window x_src={} y_src={} w_src={} h_src={} cx={:.1} cy={:.1}", x_src, y_src, w_src, h_src, ig.cx, ig.cy);
            let full = &image_cache.get(&ig.path).unwrap().0;
            let region = image::imageops::crop_imm(full, x_src as u32, y_src as u32, w_src, h_src).to_image();
            // Build canvas 512x512 with region centered and mirror pad
            let mut canvas = image::RgbaImage::new(CANVAS, CANVAS);
            // compute dx, dy to center small region (when img smaller than 512)
            let dx = if w_src < CANVAS { (CANVAS - w_src)/2 } else { 0 };
            let dy = if h_src < CANVAS { (CANVAS - h_src)/2 } else { 0 };
            // dx/dy placement for centering; but if w_src==512 then dx 0 (x_src already centered)
            // For w_src==512 case, dx 0 and region exactly fills width
            // For w_src<512 (narrow image), region centered, mirror both sides via reflect
            // Use reflect logic to fill canvas:
            // If w_src==CANVAS && h_src==CANVAS => canvas = region
            // Else reflect
            if w_src==CANVAS && h_src==CANVAS {
                canvas = region.clone();
            } else {
                // Place region at dx,dy then reflect
                // First fill canvas with reflected region
                for cy in 0..CANVAS as i64 {
                    let sy = reflect_index(cy - dy as i64, h_src as i64);
                    for cx in 0..CANVAS as i64 {
                        let sx = reflect_index(cx - dx as i64, w_src as i64);
                        let px = *region.get_pixel(sx as u32, sy as u32);
                        canvas.put_pixel(cx as u32, cy as u32, px);
                    }
                }
            }
            eprintln!("[manual::multi]   canvas built {}x{} region {}x{} at dx={},dy={} x_src={},y_src={}", canvas.width(), canvas.height(), region.width(), region.height(), dx, dy, x_src, y_src);
            // Build mask quads: each sel in group as quad in canvas coords
            let mut quads_canvas: Vec<Quad> = Vec::new();
            for s in &group {
                let qx = s.x0 as f32 - x_src as f32 + dx as f32;
                let qy = s.y0 as f32 - y_src as f32 + dy as f32;
                let quad = Quad { points: [[qx, qy],[qx+s.w as f32, qy],[qx+s.w as f32, qy+s.h as f32],[qx, qy+s.h as f32]] };
                eprintln!("[manual::multi]   sel idx={} orig [{},{},{}x{}] -> canvas quad {:?}", s.idx, s.x0, s.y0, s.w, s.h, quad.points);
                quads_canvas.push(quad);
            }
            let rect = [0.0,0.0,CANVAS as f32, CANVAS as f32];
            let patches = match engine.run_on_image(&canvas, rect, &quads_canvas) {
                Ok(v) => { eprintln!("[manual::multi]   single window patches={}", v.len()); v },
                Err(e) => { eprintln!("[manual::multi]   single window run_on_image failed: {}", e); failed_groups += 1; continue; }
            };
            for (pi, (patch_img, bounds_canvas, quad_opt)) in patches.into_iter().enumerate() {
                let [bx,by,bw,bh] = bounds_canvas;
                eprintln!("[manual::multi]   patch {}: bounds_canvas [{:.1},{:.1},{:.1},{:.1}] patch {}x{} quad={:?}", pi, bx,by,bw,bh, patch_img.width(), patch_img.height(), quad_opt.map(|q| q.points));
                // Map canvas bounds back to image coords
                let orig_x = bx - dx as f32 + x_src as f32;
                let orig_y = by - dy as f32 + y_src as f32;
                // clip to image bounds
                let img_w_f = ig.img_w as f32;
                let img_h_f = ig.img_h as f32;
                let clip_x0 = orig_x.max(0.0);
                let clip_y0 = orig_y.max(0.0);
                let clip_x1 = (orig_x + bw).min(img_w_f);
                let clip_y1 = (orig_y + bh).min(img_h_f);
                if clip_x1 <= clip_x0 || clip_y1 <= clip_y0 { eprintln!("[manual::multi]     skip zero clip"); continue; }
                let new_w = clip_x1 - clip_x0;
                let new_h = clip_y1 - clip_y0;
                let crop_x = (clip_x0 - orig_x).round().max(0.0) as u32;
                let crop_y = (clip_y0 - orig_y).round().max(0.0) as u32;
                let clipped = if crop_x!=0 || crop_y!=0 || new_w as u32 != patch_img.width() || new_h as u32 != patch_img.height() {
                    let cw = (new_w as u32).min(patch_img.width().saturating_sub(crop_x));
                    let ch = (new_h as u32).min(patch_img.height().saturating_sub(crop_y));
                    if cw==0||ch==0 { continue; }
                    image::imageops::crop_imm(&patch_img, crop_x, crop_y, cw, ch).to_image()
                } else { patch_img };
                let bounds = [clip_x0, clip_y0, new_w, new_h];
                let orig_quad = quad_opt.map(|q| {
                    let mut nq = q;
                    for pt in &mut nq.points { pt[0] += x_src as f32 - dx as f32; pt[1] += y_src as f32 - dy as f32; }
                    nq
                });
                eprintln!("[manual::multi]     -> per_image idx={} bounds={:?} quad={:?} clipped {}x{}", ig.idx, bounds, orig_quad.map(|q| q.points), clipped.width(), clipped.height());
                // For single distinct, all sels share same idx, but we push per patch with that idx
                // Use bounds to decide which sel? Actually per_image should be per distinct idx, but patches may correspond to each sel quad
                // We'll assign to ig.idx (since all same)
                per_image.entry(ig.idx).or_default().push((clipped, bounds, orig_quad));
            }
        } else {
            // Multi-image stitch arbitrary N
            eprintln!("[manual::multi]   multi-img stitch N={}", img_groups.len());
            // Compute per-image h_span already, sum = total_h
            let total_span: u32 = img_groups.iter().map(|g| g.h_span).sum();
            let extra_needed = CANVAS.saturating_sub(total_span);
            eprintln!("[manual::multi]   total_span={} extra_needed={}", total_span, extra_needed);
            // Distribute extra_needed based on avail per image
            // avail per image: top = min_y, bottom = img_h - max_y
            let mut extra_alloc: Vec<u32> = vec![0; img_groups.len()];
            if extra_needed > 0 {
                // Simple distribution: split extra_needed equally, capped by avail
                // First compute total avail
                let avails: Vec<u32> = img_groups.iter().map(|g| g.min_y + (g.img_h - g.max_y) ).collect();
                let total_avail: u32 = avails.iter().sum();
                eprintln!("[manual::multi]   avails {:?} total_avail {}", avails, total_avail);
                if total_avail >= extra_needed {
                    // Proportional to avail
                    let mut remaining = extra_needed;
                    for (i, avail) in avails.iter().enumerate() {
                        let share = if i == avails.len()-1 { remaining } else { (extra_needed as f32 * *avail as f32 / total_avail as f32).round() as u32 };
                        let take = share.min(*avail).min(remaining);
                        extra_alloc[i] = take;
                        remaining = remaining.saturating_sub(take);
                    }
                    // if still remaining due to rounding, distribute left
                    let mut idx = 0;
                    while remaining > 0 {
                        let avail_left = avails[idx] - extra_alloc[idx];
                        if avail_left > 0 {
                            let take = 1.min(avail_left).min(remaining);
                            extra_alloc[idx] += take;
                            remaining -= take;
                        }
                        idx = (idx+1)%avails.len();
                        if idx==0 && extra_alloc.iter().zip(&avails).all(|(a, av)| *a==*av) { break; }
                    }
                } else {
                    // Not enough avail to fill, will need mirror gap after
                    for (i, avail) in avails.iter().enumerate() {
                        extra_alloc[i] = *avail;
                    }
                }
            }
            eprintln!("[manual::multi]   extra_alloc per img {:?}", extra_alloc);
            // Now compute h_src per image = h_span + extra_alloc
            // and y_src per image
            struct PieceWin {
                idx: usize,
                path: String,
                img_w: u32,
                img_h: u32,
                x_src: i32,
                y_src: i32,
                w_src: u32,
                h_src: u32,
                off_y: u32,
            }
            let mut pieces_win: Vec<PieceWin> = Vec::new();
            let mut off_y: u32 = 0;
            for (i, ig) in img_groups.iter().enumerate() {
                let extra = extra_alloc[i];
                let h_src = ig.h_span + extra;
                // split extra into top/bottom: for first image, extra comes from top (min_y), for last from bottom, middle split
                let (top_extra, bottom_extra) = if img_groups.len()==1 {
                    // already handled
                    (0,0)
                } else if i==0 {
                    // first: expand upward as much as possible (up to min_y)
                    let top = extra.min(ig.min_y);
                    let bottom = extra - top; // should be 0 but if top capped, remainder from bottom? but first bottom is at seam, shouldn't expand downward beyond max_y? Actually extra for first should be top only, so bottom stays 0. If avail_top insufficient, we already limited extra to avail, so bottom 0.
                    (top, bottom)
                } else if i==img_groups.len()-1 {
                    let bottom = extra.min(ig.img_h - ig.max_y);
                    let top = extra - bottom;
                    (top, bottom)
                } else {
                    // middle: split equally
                    let top = (extra/2).min(ig.min_y);
                    let bottom = (extra - top).min(ig.img_h - ig.max_y);
                    let top2 = extra - bottom; // adjust if bottom capped
                    let top2 = top2.min(ig.min_y);
                    (top2, bottom)
                };
                let y_src = (ig.min_y as i32 - top_extra as i32).max(0);
                // Ensure y_src + h_src <= img_h
                let y_src = y_src.clamp(0, ig.img_h as i32 - h_src as i32).max(0);
                let w_src = CANVAS.min(ig.img_w);
                let mut x_src = (ig.cx - w_src as f32*0.5).round() as i32;
                x_src = x_src.clamp(0, ig.img_w as i32 - w_src as i32).max(0);
                eprintln!("[manual::multi]   piece win idx={} h_span={} extra={} top={} bottom={} y_src={} h_src={} x_src={} w_src={} off_y={} [{},{}]", ig.idx, ig.h_span, extra, top_extra, bottom_extra, y_src, h_src, x_src, w_src, off_y, ig.min_y, ig.max_y);
                pieces_win.push(PieceWin { idx: ig.idx, path: ig.path.clone(), img_w: ig.img_w, img_h: ig.img_h, x_src, y_src, w_src, h_src, off_y });
                off_y += h_src;
            }
            let total_h_stitched: u32 = pieces_win.iter().map(|p| p.h_src).sum();
            eprintln!("[manual::multi]   total_h_stitched={} off_y final {}", total_h_stitched, off_y);
            // Build stitched canvas 512x512
            let mut stitched = image::RgbaImage::new(CANVAS, CANVAS);
            // Vertical offset to center if total <512 (mirror pad needed)
            let vert_offset = if total_h_stitched < CANVAS { (CANVAS - total_h_stitched)/2 } else { 0 };
            eprintln!("[manual::multi]   vert_offset for centering stitched block {} total {} <512 {}", vert_offset, total_h_stitched, total_h_stitched < CANVAS);
            for pw in &pieces_win {
                let full = &image_cache.get(&pw.path).unwrap().0;
                let region = image::imageops::crop_imm(full, pw.x_src as u32, pw.y_src as u32, pw.w_src, pw.h_src).to_image();
                // Horizontal pad to 512 if w_src <512 via reflect both sides centered
                let mut region_padded = image::RgbaImage::new(CANVAS, pw.h_src);
                if pw.w_src < CANVAS {
                    let dx = (CANVAS - pw.w_src)/2;
                    // reflect fill
                    for y in 0..pw.h_src as i64 {
                        for x in 0..CANVAS as i64 {
                            let sx = reflect_index(x - dx as i64, pw.w_src as i64);
                            let px = *region.get_pixel(sx as u32, y as u32);
                            region_padded.put_pixel(x as u32, y as u32, px);
                        }
                    }
                } else {
                    region_padded = region;
                }
                let dst_y = vert_offset + pw.off_y;
                image::imageops::replace(&mut stitched, &region_padded, 0, dst_y as i64);
                eprintln!("[manual::multi]   placed piece idx={} w_src={} h_src={} x_src={} y_src={} off_y={} dst_y={} -> stitched", pw.idx, pw.w_src, pw.h_src, pw.x_src, pw.y_src, pw.off_y, dst_y);
            }
            if total_h_stitched < CANVAS {
                // Create a temporary copy of the stitched block region for reflection
                // First, we have stitched with block at vert_offset..vert_offset+total_h, gaps zero. We'll fill gaps by reflecting the block.
                // Use approach: for each y in 0..CANVAS, sy = reflect_index(y - vert_offset, total_h)
                let mut filled = image::RgbaImage::new(CANVAS, CANVAS);
                // Extract block as image for reflection?
                // For each canvas y, compute sy mirrored
                for y in 0..CANVAS as i64 {
                    let sy = reflect_index(y - vert_offset as i64, total_h_stitched as i64);
                    let src_y = vert_offset as i64 + sy;
                    for x in 0..CANVAS {
                        let px = *stitched.get_pixel(x, src_y as u32);
                        filled.put_pixel(x, y as u32, px);
                    }
                }
                stitched = filled;
                eprintln!("[manual::multi]   vertical mirror filled gaps total {} vert_offset {}", total_h_stitched, vert_offset);
            }
            // Build mask quads: each sel in group as quad in stitched coords
            let mut quads_canvas: Vec<Quad> = Vec::new();
            // Need mapping from sel to its piece window to compute canvas position
            for s in &group {
                // find piece win for this sel's idx
                let pw = pieces_win.iter().find(|p| p.idx == s.idx).unwrap();
                // For x: sel x0 - pw.x_src + dx where dx = (512 - pw.w_src)/2 if w_src<512 else 0
                let dx = if pw.w_src < CANVAS { (CANVAS - pw.w_src)/2 } else { 0 };
                let qx = s.x0 as f32 - pw.x_src as f32 + dx as f32;
                let qy = s.y0 as f32 - pw.y_src as f32 + vert_offset as f32 + pw.off_y as f32;
                let quad = Quad { points: [[qx, qy],[qx+s.w as f32, qy],[qx+s.w as f32, qy+s.h as f32],[qx, qy+s.h as f32]] };
                eprintln!("[manual::multi]   sel idx={} [{},{},{}x{}] -> canvas quad {:?} (x_src {}, y_src {}, off_y {}, dx {}, vert_offset {})", s.idx, s.x0, s.y0, s.w, s.h, quad.points, pw.x_src, pw.y_src, pw.off_y, dx, vert_offset);
                quads_canvas.push(quad);
            }
            let rect = [0.0,0.0,CANVAS as f32, CANVAS as f32];
            let patches = match engine.run_on_image(&stitched, rect, &quads_canvas) {
                Ok(v) => { eprintln!("[manual::multi]   stitch patches={}", v.len()); v },
                Err(e) => { eprintln!("[manual::multi]   stitch run_on_image failed: {}", e); failed_groups += 1; continue; }
            };
            for (pi, (patch_img, bounds_canvas, quad_opt)) in patches.into_iter().enumerate() {
                let [bx,by,bw,bh] = bounds_canvas;
                eprintln!("[manual::multi]   patch {}: bounds_canvas [{:.1},{:.1},{:.1},{:.1}] patch {}x{} quad={:?}", pi, bx,by,bw,bh, patch_img.width(), patch_img.height(), quad_opt.map(|q| q.points));
                // Map bounds_canvas back to original image: find which piece win contains the patch center or overlap
                // Use quad_piece mapping via pi index if available, else by center
                // Since we built quads_canvas in group order (same as group sels order), pi corresponds to group[pi] sel
                let s_opt = if pi < group.len() { Some(&group[pi]) } else { None };
                let target_pw: &PieceWin = if let Some(s) = s_opt {
                    pieces_win.iter().find(|p| p.idx == s.idx).unwrap()
                } else {
                    // fallback by cy
                    let cy = by + bh*0.5;
                    let mut found: Option<&PieceWin> = None;
                    for pw in &pieces_win {
                        let y0 = vert_offset as f32 + pw.off_y as f32;
                        let y1 = y0 + pw.h_src as f32;
                        if cy >= y0 && cy < y1 { found = Some(pw); break; }
                    }
                    match found {
                        Some(v)=>v,
                        None=> {
                            let mut best: Option<&PieceWin>=None;
                            let mut best_overlap=0.0;
                            for pw in &pieces_win {
                                let y0 = vert_offset as f32 + pw.off_y as f32;
                                let y1 = y0 + pw.h_src as f32;
                                let overlap = (by+bh).min(y1) - by.max(y0);
                                if overlap > best_overlap { best_overlap=overlap; best=Some(pw); }
                            }
                            match best { Some(v)=>v, None=>continue }
                        }
                    }
                };
                let dx = if target_pw.w_src < CANVAS { (CANVAS - target_pw.w_src)/2 } else { 0 };
                let orig_x = bx - dx as f32 + target_pw.x_src as f32;
                let orig_y = by - vert_offset as f32 - target_pw.off_y as f32 + target_pw.y_src as f32;
                let img_w_f = target_pw.img_w as f32;
                let img_h_f = target_pw.img_h as f32;
                let clip_x0 = orig_x.max(0.0);
                let clip_y0 = orig_y.max(0.0);
                let clip_x1 = (orig_x + bw).min(img_w_f);
                let clip_y1 = (orig_y + bh).min(img_h_f);
                if clip_x1 <= clip_x0 || clip_y1 <= clip_y0 { eprintln!("[manual::multi]     skip zero clip orig [{:.1},{:.1}] clip [{:.1},{:.1}]-[{:.1},{:.1}]", orig_x, orig_y, clip_x0, clip_y0, clip_x1, clip_y1); continue; }
                let new_w = clip_x1 - clip_x0;
                let new_h = clip_y1 - clip_y0;
                let crop_x = (clip_x0 - orig_x).round().max(0.0) as u32;
                let crop_y = (clip_y0 - orig_y).round().max(0.0) as u32;
                let clipped = if crop_x!=0 || crop_y!=0 || new_w as u32 != patch_img.width() || new_h as u32 != patch_img.height() {
                    let cw = (new_w as u32).min(patch_img.width().saturating_sub(crop_x));
                    let ch = (new_h as u32).min(patch_img.height().saturating_sub(crop_y));
                    if cw==0||ch==0 { continue; }
                    image::imageops::crop_imm(&patch_img, crop_x, crop_y, cw, ch).to_image()
                } else { patch_img };
                let bounds = [clip_x0, clip_y0, new_w, new_h];
                let orig_quad = quad_opt.map(|q| {
                    let mut nq = q;
                    for pt in &mut nq.points {
                        pt[0] = pt[0] - dx as f32 + target_pw.x_src as f32;
                        pt[1] = pt[1] - vert_offset as f32 - target_pw.off_y as f32 + target_pw.y_src as f32;
                    }
                    nq
                });
                eprintln!("[manual::multi]     -> per_image idx={} bounds={:?} quad={:?} clipped {}x{} (orig_x {:.1} orig_y {:.1} dx {} vert_offset {})", target_pw.idx, bounds, orig_quad.map(|q| q.points), clipped.width(), clipped.height(), orig_x, orig_y, dx, vert_offset);
                per_image.entry(target_pw.idx).or_default().push((clipped, bounds, orig_quad));
            }
        }
    }
    let mut out: GroupedInpaint = Vec::new();
    for (idx, v) in per_image { out.push((idx, v)); }
    out.sort_by_key(|(idx,_)| *idx);
    if out.is_empty() && failed_groups > 0 {
        return Err(format!("all {failed_groups} inpaint group(s) failed"));
    }
    Ok((out, failed_groups))
}

#[cfg(feature = "inpaint")]
pub fn handle_inpaint_engine_ready(app: &mut App, tab_id: crate::app::tab::TabId, result: Result<InpaintEngine, String>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    match result {
        Ok(engine) => {
            app.engines.set_shared_inpaint(engine.backend(), engine.clone());
            let data = app.tabs[idx].pending_manual_multi.take();
            if let Some(d) = data { return start_inpaint_selection(app, tab_id, engine, d); }
            let bg = app.tabs[idx].pending_background_stitch.take();
            if let Some((job, pad, prev, next)) = bg { return start_background_stitch(app, tab_id, engine, job, pad, prev, next); }
            app.tabs[idx].inpainting = false;
            Task::none()
        }
        Err(e) => {
            app.tabs[idx].pending_manual_multi = None;
            app.tabs[idx].pending_background_stitch = None;
            app.tabs[idx].inpainting = false;
            app.tabs[idx].status = e.clone();
            // free queue weight for manual inpaint (any backend) on build failure
            let mut freed = false;
            if app.engines.queue.complete(owner_of(tab_id), crate::app::queue::JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Telea)).is_some() { freed = true; }
            if app.engines.queue.complete(owner_of(tab_id), crate::app::queue::JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Lama)).is_some() { freed = true; }
            if app.engines.queue.complete(owner_of(tab_id), crate::app::queue::JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Aot)).is_some() { freed = true; }
            if app.engines.queue.complete(owner_of(tab_id), crate::app::queue::JobKind::Inpaint(easyscanlate_settings::InpaintBackend::ShiftMap)).is_some() { freed = true; }
            if app.engines.queue.complete(owner_of(tab_id), crate::app::queue::JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Harmonic)).is_some() { freed = true; }
            if freed {
                let promote = crate::app::queue::dispatch_pending(app);
                crate::app::queue::refresh_queued_statuses(app);
                return promote;
            }
            Task::none()
        }
    }
}
#[cfg(feature = "inpaint")]
pub fn handle_auto_engine_ready(app: &mut App, tab_id: crate::app::tab::TabId, backend: InpaintBackend, result: Result<InpaintEngine, String>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    match result {
        Ok(engine) => {
            app.tabs[idx].auto_inpaint_loading = false;
            match backend {
                InpaintBackend::Telea => {
                    app.engines.auto_telea = Some(engine.clone());
                    let jobs = app.tabs[idx].pending_auto_telea_jobs.take();
                    if let Some(j) = jobs { return dispatch_auto(app, tab_id, j, InpaintBackend::Telea); }
                }
                InpaintBackend::Lama => {
                    app.engines.auto_lama = Some(engine.clone());
                    let jobs = app.tabs[idx].pending_auto_lama_jobs.take();
                    if let Some(j) = jobs { return dispatch_auto(app, tab_id, j, InpaintBackend::Lama); }
                }
                InpaintBackend::Aot => {
                    app.engines.auto_aot = Some(engine.clone());
                    let jobs = app.tabs[idx].pending_auto_aot_jobs.take();
                    if let Some(j) = jobs { return dispatch_auto(app, tab_id, j, InpaintBackend::Aot); }
                }
                // ShiftMap is manual-only: cached like the other auto
                // engines so the match stays exhaustive; the auto pipeline
                // never routes here today (Harmonic does, via Mixed).
                InpaintBackend::ShiftMap => {
                    app.engines.auto_shiftmap = Some(engine.clone());
                    let jobs = app.tabs[idx].pending_auto_shiftmap_jobs.take();
                    if let Some(j) = jobs { return dispatch_auto(app, tab_id, j, InpaintBackend::ShiftMap); }
                }
                InpaintBackend::Harmonic => {
                    app.engines.auto_harmonic = Some(engine.clone());
                    let jobs = app.tabs[idx].pending_auto_harmonic_jobs.take();
                    if let Some(j) = jobs { return dispatch_auto(app, tab_id, j, InpaintBackend::Harmonic); }
                }
            }
            Task::none()
        }
        Err(e) => {
            match backend {
                InpaintBackend::Telea => app.tabs[idx].pending_auto_telea_jobs = None,
                InpaintBackend::Lama => app.tabs[idx].pending_auto_lama_jobs = None,
                InpaintBackend::Aot => app.tabs[idx].pending_auto_aot_jobs = None,
                InpaintBackend::ShiftMap => app.tabs[idx].pending_auto_shiftmap_jobs = None,
                InpaintBackend::Harmonic => app.tabs[idx].pending_auto_harmonic_jobs = None,
            }
            app.tabs[idx].auto_inpaint_loading = false;
            app.tabs[idx].status = format!("Auto-inpaint engine failed: {e}");
            #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
            { app.tabs[idx].pipeline_active = false; }
            // free queue weight (build failed) and promote
            let kind = crate::app::queue::JobKind::Inpaint(backend);
            app.engines.queue.complete(owner_of(tab_id), kind);
            let promote = crate::app::queue::dispatch_pending(app);
            crate::app::queue::refresh_queued_statuses(app);
            promote
        }
    }
}
#[cfg(feature = "inpaint")]
pub fn handle_auto_finished(app: &mut App, tab_id: crate::app::tab::TabId, index: usize, id: easyscanlate_model::EntryId, result: AutoResult) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    app.tabs[idx].auto_inpaint_pending = app.tabs[idx].auto_inpaint_pending.saturating_sub(1);
    let pending = app.tabs[idx].auto_inpaint_pending;
    let total = app.tabs[idx].auto_inpaint_total;
    match result {
        Ok(patches) => {
            commit_auto_patches(app, idx, patches);
            if pending == 0 {
                return finish_auto_stream(app, tab_id, idx, "Telea");
            } else {
                let done = total.saturating_sub(pending);
                let failed = app.tabs[idx].auto_inpaint_failed;
                app.tabs[idx].status = if failed > 0 {
                    format!("Auto-inpaint (Telea): {done} of {total} done, {failed} failed ({pending} remaining).")
                } else {
                    format!("Auto-inpaint (Telea): {done} of {total} done ({pending} remaining).")
                };
            }
        }
        Err(e) => {
            app.tabs[idx].auto_inpaint_failed += 1;
            let failed = app.tabs[idx].auto_inpaint_failed;
            if pending == 0 {
                return finish_auto_stream(app, tab_id, idx, "Telea");
            } else {
                let done = total.saturating_sub(pending);
                app.tabs[idx].status = format!("Auto-inpaint (Telea): {done} of {total} done, {failed} failed ({pending} remaining; last: {index}:{id:?}: {e}).");
            }
        }
    }
    Task::none()
}

/// Granular auto-inpaint stream event (OCR-style): one finished job.
/// `Ok((index, id, result))` is a completed job whose inner `result` may still
/// be per-job `Err`; outer `Err` is a per-job dispatch failure. Both count as
/// one failed job without dropping the rest of the stream.
#[cfg(feature = "inpaint")]
pub fn handle_auto_stream_run(
    app: &mut App,
    tab_id: crate::app::tab::TabId,
    result: Result<AutoStreamItem, String>,
) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    app.tabs[idx].auto_inpaint_pending = app.tabs[idx].auto_inpaint_pending.saturating_sub(1);
    let pending = app.tabs[idx].auto_inpaint_pending;
    let total = app.tabs[idx].auto_inpaint_total;
    // Backend label for status: infer from which queue weight is held.
    let label = if app.engines.queue.running_for(owner_of(tab_id), crate::app::queue::JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Lama)).is_some() {
        "LaMa"
    } else if app.engines.queue.running_for(owner_of(tab_id), crate::app::queue::JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Aot)).is_some() {
        "AOT-GAN"
    } else if app.engines.queue.running_for(owner_of(tab_id), crate::app::queue::JobKind::Inpaint(easyscanlate_settings::InpaintBackend::ShiftMap)).is_some() {
        "ShiftMap"
    } else if app.engines.queue.running_for(owner_of(tab_id), crate::app::queue::JobKind::Inpaint(easyscanlate_settings::InpaintBackend::Harmonic)).is_some() {
        "Harmonic"
    } else {
        "Telea"
    };
    match result {
        Ok((_index, _id, inner)) => match inner {
            Ok(patches) => {
                commit_auto_patches(app, idx, patches);
                if pending == 0 {
                    return finish_auto_stream(app, tab_id, idx, label);
                }
                let done = total.saturating_sub(pending);
                let failed = app.tabs[idx].auto_inpaint_failed;
                app.tabs[idx].status = if failed > 0 {
                    format!("Auto-inpaint ({label}): {done} of {total} done, {failed} failed ({pending} remaining).")
                } else {
                    format!("Auto-inpaint ({label}): {done} of {total} done ({pending} remaining).")
                };
            }
            Err(e) => {
                app.tabs[idx].auto_inpaint_failed += 1;
                let failed = app.tabs[idx].auto_inpaint_failed;
                if pending == 0 {
                    return finish_auto_stream(app, tab_id, idx, label);
                }
                let done = total.saturating_sub(pending);
                app.tabs[idx].status = format!("Auto-inpaint ({label}): {done} of {total} done, {failed} failed ({pending} remaining; last: {e}).");
            }
        },
        Err(e) => {
            app.tabs[idx].auto_inpaint_failed += 1;
            let failed = app.tabs[idx].auto_inpaint_failed;
            if pending == 0 {
                return finish_auto_stream(app, tab_id, idx, label);
            }
            let done = total.saturating_sub(pending);
            app.tabs[idx].status = format!("Auto-inpaint ({label}): {done} of {total} done, {failed} failed ({pending} remaining; dispatch: {e}).");
        }
    }
    Task::none()
}

/// Fatal auto-inpaint stream failure (channel/task aborted). Marks all
/// remaining jobs failed, frees queue weight and promotes pending work.
#[cfg(feature = "inpaint")]
pub fn handle_auto_stream_failed(
    app: &mut App,
    tab_id: crate::app::tab::TabId,
    e: String,
) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    if app.tabs[idx].auto_inpaint_pending > 0 {
        let remaining = app.tabs[idx].auto_inpaint_pending;
        app.tabs[idx].auto_inpaint_failed += remaining;
        app.tabs[idx].auto_inpaint_pending = 0;
        let (total, failed) = (app.tabs[idx].auto_inpaint_total, app.tabs[idx].auto_inpaint_failed);
        #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
        {
            app.tabs[idx].pipeline_active = false;
        }
        app.tabs[idx].status =
            format!("Auto-inpaint stream failed ({e}): {total} region(s), {failed} failed.");
        free_auto_queue(app, tab_id);
        let promote = crate::app::queue::dispatch_pending(app);
        crate::app::queue::refresh_queued_statuses(app);
        return promote;
    }
    Task::none()
}
/// Starts a granular auto-inpaint stream for LaMa/AOT (OCR-style).
/// Jobs run sequentially (ONNX session is single-threaded) but each finished
/// job emits `AutoInpaintStreamRun` immediately: progress + partial commits
/// are visible and one failure never drops the rest. Queue weight stays
/// reserved until the last event finalizes.
#[cfg(feature = "inpaint")]
fn start_auto_stream(
    app: &mut App,
    tab_id: crate::app::tab::TabId,
    engine: InpaintEngine,
    jobs: Vec<AutoInpaintJob>,
    pad: f32,
    label: &str,
    _backend: InpaintBackend,
) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    let total = jobs.len();
    app.tabs[idx].status = format!("Auto-inpaint ({label}) 0 of {total} done...");
    let tab = &app.tabs[idx];
    let enriched: Vec<(AutoInpaintJob, Option<String>, Option<String>)> = jobs
        .into_iter()
        .map(|job| {
            let prev = if job.index > 0 {
                tab.images.get(job.index - 1).and_then(|img| tab.project.image(img.image_id).map(|m| m.path.clone()))
            } else {
                None
            };
            let next = if job.index + 1 < tab.images.len() {
                tab.images.get(job.index + 1).and_then(|img| tab.project.image(img.image_id).map(|m| m.path.clone()))
            } else {
                None
            };
            (job, prev, next)
        })
        .collect();
    let tid = tab_id;
    Task::stream(
        iced::stream::try_channel(1, move |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
            for (job, prev_path, next_path) in enriched {
                let engine_clone = engine.clone();
                let jidx = job.index;
                let jid = job.id;
                let res = tokio::task::spawn_blocking(move || {
                    run_auto_inpaint_job(&engine_clone, &job, pad, prev_path.as_deref(), next_path.as_deref())
                })
                .await
                .unwrap_or_else(|e| Err(format!("inpaint task cancelled: {e}")));
                if sender
                    .send(Message::Tab(
                        tid,
                        crate::app::TabMessage::AutoInpaintStreamRun(Ok((jidx, jid, res))),
                    ))
                    .await
                    .is_err()
                {
                    return Ok::<(), String>(());
                }
            }
            Ok::<(), String>(())
        })
        .map(move |item: Result<Message, String>| match item {
            Ok(message) => message,
            Err(e) => Message::Tab(tid, crate::app::TabMessage::AutoInpaintStreamFailed(e.to_string())),
        }),
    )
}
#[cfg(feature = "inpaint")]
pub fn handle_inpaint_finished(app: &mut App, tab_id: crate::app::tab::TabId, result: GroupedInpaintResult) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    app.tabs[idx].inpainting = false;
    match result {
        Ok(per_image_patches) => {
            let total = commit_manual_patches(app, idx, per_image_patches);
            app.tabs[idx].status = format!("Inpainted {total} region(s) (multi).");
        }
        Err(e) => { app.tabs[idx].status = format!("Multi inpaint failed: {e}"); }
    }
    // Free queue weight for manual inpaint (any backend) and promote via backfill
    if free_manual_queue(app, tab_id) {
        let promote = crate::app::queue::dispatch_pending(app);
        crate::app::queue::refresh_queued_statuses(app);
        return promote;
    }
    Task::none()
}

/// Granular manual-inpaint stream event (OCR-style): one finished batch unit.
/// `Ok((patches, failed))` commits `patches` and adds `failed` to the failed
/// counter; outer `Err` counts one failed unit with no patches. Queue weight
/// stays reserved until the last unit finalizes.
#[cfg(feature = "inpaint")]
pub fn handle_manual_stream_run(
    app: &mut App,
    tab_id: crate::app::tab::TabId,
    result: Result<ManualStreamPatches, String>,
) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    app.tabs[idx].manual_inpaint_pending = app.tabs[idx].manual_inpaint_pending.saturating_sub(1);
    let pending = app.tabs[idx].manual_inpaint_pending;
    let total = app.tabs[idx].manual_inpaint_total;
    match result {
        Ok((patches, failed_in_unit)) => {
            let applied = commit_manual_patches(app, idx, patches);
            app.tabs[idx].manual_inpaint_failed += failed_in_unit;
            let failed = app.tabs[idx].manual_inpaint_failed;
            if pending == 0 {
                app.tabs[idx].inpainting = false;
                app.tabs[idx].status = if failed > 0 {
                    format!("Inpainted {applied} region(s), {failed} group(s) failed.")
                } else {
                    format!("Inpainted {applied} region(s) (multi).")
                };
                if free_manual_queue(app, tab_id) {
                    let promote = crate::app::queue::dispatch_pending(app);
                    crate::app::queue::refresh_queued_statuses(app);
                    return promote;
                }
                return Task::none();
            }
            let done = total.saturating_sub(pending);
            app.tabs[idx].status = if failed > 0 {
                format!("Inpainting: {done} of {total} done, {failed} failed ({pending} remaining).")
            } else {
                format!("Inpainting: {done} of {total} done ({pending} remaining).")
            };
        }
        Err(e) => {
            app.tabs[idx].manual_inpaint_failed += 1;
            let failed = app.tabs[idx].manual_inpaint_failed;
            if pending == 0 {
                app.tabs[idx].inpainting = false;
                app.tabs[idx].status = format!("Inpaint failed ({e}); {failed} of {total} unit(s) failed.");
                if free_manual_queue(app, tab_id) {
                    let promote = crate::app::queue::dispatch_pending(app);
                    crate::app::queue::refresh_queued_statuses(app);
                    return promote;
                }
                return Task::none();
            }
            let done = total.saturating_sub(pending);
            app.tabs[idx].status =
                format!("Inpainting: {done} of {total} done, {failed} failed ({pending} remaining; last: {e}).");
        }
    }
    Task::none()
}

#[cfg(feature = "inpaint")]
pub fn dispatch_auto(app: &mut App, tab_id: crate::app::tab::TabId, jobs: Vec<AutoInpaintJob>, backend: InpaintBackend) -> Task<Message> {
    if jobs.is_empty() { return Task::none(); }
    // queue gate — weights 1/4/3/4 backfill + priority (cap 5)
    {
        use crate::app::queue::{AcquireResult, JobKind, owner_of};
        let kind = JobKind::Inpaint(backend);
        let already_reserved = app.engines.queue.running_for(owner_of(tab_id), kind).is_some();
        if !already_reserved {
            match app.engines.queue.try_acquire_or_enqueue(owner_of(tab_id), kind) {
                AcquireResult::Acquired(_) => {},
                AcquireResult::Queued(_, pos) => {
                    let idx_tmp = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i)=>i, None=>return Task::none() };
                    // store pending for later dispatch_pending
                    match backend {
                        InpaintBackend::Telea => app.tabs[idx_tmp].pending_auto_telea_jobs = Some(jobs),
                        InpaintBackend::Lama => app.tabs[idx_tmp].pending_auto_lama_jobs = Some(jobs),
                        InpaintBackend::Aot => app.tabs[idx_tmp].pending_auto_aot_jobs = Some(jobs),
                        InpaintBackend::ShiftMap => app.tabs[idx_tmp].pending_auto_shiftmap_jobs = Some(jobs),
                        InpaintBackend::Harmonic => app.tabs[idx_tmp].pending_auto_harmonic_jobs = Some(jobs),
                    }
                    app.tabs[idx_tmp].status = format!("Queued {} (pos {}, pool {}/{}) ...", kind.label(), pos, app.engines.queue.used_weight(), crate::app::queue::POOL_CAPACITY);
                    return Task::none();
                }
            }
        }
    }
    let radius = easyscanlate_settings::get(|s| s.inpaint_radius.parse::<i32>().unwrap_or(5).max(1));
    let pad = auto_pad_for(backend, radius);
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    let cached: Option<InpaintEngine> = app.engines.shared_inpaint(backend, radius);
    if let Some(engine) = cached {
        // Fresh run resets totals; queue guarantees no concurrent same-tab run.
        if app.tabs[idx].auto_inpaint_pending == 0 {
            app.tabs[idx].auto_inpaint_total = 0;
            app.tabs[idx].auto_inpaint_failed = 0;
        }
        app.tabs[idx].auto_inpaint_pending += jobs.len();
        app.tabs[idx].auto_inpaint_total += jobs.len();
        match backend {
            InpaintBackend::Telea | InpaintBackend::Harmonic => {
                let label = match backend {
                    InpaintBackend::Telea => "Telea",
                    InpaintBackend::Harmonic => "Harmonic",
                    _ => unreachable!("parallel arm is Telea/Harmonic only"),
                };
                app.tabs[idx].status = format!("Auto-inpaint ({label}) {} regions in parallel...", jobs.len());
                let neighbor_map: std::collections::HashMap<usize, (Option<String>, Option<String>)> = {
                    let mut map = std::collections::HashMap::new();
                    let tab = &app.tabs[idx];
                    for job in &jobs {
                        let prev = if job.index>0 { tab.images.get(job.index-1).and_then(|img| tab.project.image(img.image_id).map(|m| m.path.clone())) } else { None };
                        let next = if job.index+1 < tab.images.len() { tab.images.get(job.index+1).and_then(|img| tab.project.image(img.image_id).map(|m| m.path.clone())) } else { None };
                        map.insert(job.index, (prev, next));
                    }
                    map
                };
                let tasks: Vec<Task<Message>> = jobs.into_iter().map(|job| {
                    let engine = engine.clone();
                    let (prev_path, next_path) = neighbor_map.get(&job.index).cloned().unwrap_or((None, None));
                    let tid = tab_id;
                    let jidx = job.index;
                    let jid = job.id;
                    Task::perform(
                        async move {
                            let res = tokio::task::spawn_blocking(move || run_auto_inpaint_job(&engine, &job, pad, prev_path.as_deref(), next_path.as_deref())).await.unwrap_or_else(|e| Err(format!("inpaint task cancelled: {e}")));
                            (jidx, jid, res)
                        },
                        move |(jidx, jid, res)| Message::Tab(tid, crate::app::TabMessage::AutoInpaintFinished(jidx, jid, res)),
                    )
                }).collect();
                Task::batch(tasks)
            }
            _ => {
                // Sequential stream for the model/heavy backends (LaMa,
                // AOT-GAN, ShiftMap); Telea/Harmonic fan out in parallel
                // above. ShiftMap never reaches the auto path via pipeline
                // routing today.
                let label = match backend { InpaintBackend::Lama => "LaMa", InpaintBackend::Aot => "AOT-GAN", InpaintBackend::ShiftMap => "ShiftMap", InpaintBackend::Harmonic | InpaintBackend::Telea => unreachable!("telea/harmonic take the parallel arm")};
                return start_auto_stream(app, tab_id, engine, jobs, pad, label, backend);
            }
        }
    } else {
        match backend {
            InpaintBackend::Telea => app.tabs[idx].pending_auto_telea_jobs = Some(jobs),
            InpaintBackend::Lama => app.tabs[idx].pending_auto_lama_jobs = Some(jobs),
            InpaintBackend::Aot => app.tabs[idx].pending_auto_aot_jobs = Some(jobs),
            InpaintBackend::ShiftMap => app.tabs[idx].pending_auto_shiftmap_jobs = Some(jobs),
            InpaintBackend::Harmonic => app.tabs[idx].pending_auto_harmonic_jobs = Some(jobs),
        }
        // Model load counts as the run itself so buttons disable during it.
        // (Queued stash above intentionally leaves this unset.)
        app.tabs[idx].auto_inpaint_loading = true;
        app.tabs[idx].status = match backend { InpaintBackend::Telea => "Loading Telea for auto-inpaint...".to_string(), InpaintBackend::Lama => "Loading LaMa for auto-inpaint...".to_string(), InpaintBackend::Aot => "Loading AOT-GAN for auto-inpaint...".to_string(), InpaintBackend::ShiftMap => "Loading ShiftMap for auto-inpaint...".to_string(), InpaintBackend::Harmonic => "Loading Harmonic for auto-inpaint...".to_string()};
        let kind = crate::app::queue::JobKind::Inpaint(backend);
        let job_id = app
            .engines
            .queue
            .running_for(crate::app::queue::owner_of(tab_id), kind)
            .map(|j| j.id)
            .unwrap_or(0);
        let tid = tab_id;
        Task::perform(
            async move {
                use crate::app::queue::engine_ready_msg;
                use easyscanlate_engine_pool::BuiltEngine;
                match InpaintEngine::build(backend, radius) {
                    Ok(engine) => engine_ready_msg(
                        tid,
                        job_id,
                        kind,
                        Ok(BuiltEngine::Inpaint { backend, engine }),
                    ),
                    Err(e) => engine_ready_msg(tid, job_id, kind, Err(e)),
                }
            },
            |msg| msg,
        )
    }
}
#[cfg(feature = "inpaint")]
pub fn dispatch_auto_solo(app: &mut App, tab_id: crate::app::tab::TabId, effective_model: easyscanlate_settings::AutoInpaintModel) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    let mut jobs: Vec<AutoInpaintJob> = Vec::new();
    {
        let tab = &app.tabs[idx];
        let n = tab.images.len();
        let mut page_ws = Vec::with_capacity(n);
        let mut page_hs = Vec::with_capacity(n);
        let mut page_paths = Vec::with_capacity(n);
        for img in &tab.images {
            if let Some(m) = tab.project.image(img.image_id) {
                page_ws.push(m.width);
                page_hs.push(m.height);
                page_paths.push(m.path.clone());
            } else {
                page_ws.push(0.0);
                page_hs.push(0.0);
                page_paths.push(String::new());
            }
        }
        let offsets = inpaint_global_offsets(&page_hs);
        for (index, image) in tab.images.iter().enumerate() {
            let image_id = image.image_id;
            let ocr_quad_of = |id: easyscanlate_model::EntryId| {
                tab.project
                    .entry_including_deleted(id)
                    .map(|e| e.quad)
            };
            for entry in tab.project.visible_for(image_id).collect::<Vec<_>>() {
                let vquad = tab.project.view_quad(entry);
                let oquad = ocr_quad_of(entry.id).unwrap_or(vquad);
                let parts = split_quad_global(vquad, index, &page_ws, &page_hs, &offsets);
                if parts.is_empty() {
                    let [bx0, by0, bx1, by1] = vquad.bounds();
                    let meta_h = page_hs.get(index).copied().unwrap_or(0.0);
                    eprintln!(
                        "[auto-inpaint::no-intersection] idx={} id={:?} quad_bounds=[{:.1},{:.1},{:.1},{:.1}] meta_h={:.1} ocr_bounds={:?} -> skipped (no page intersects)",
                        index,
                        entry.id,
                        bx0,
                        by0,
                        bx1,
                        by1,
                        meta_h,
                        oquad.bounds(),
                    );
                    continue;
                }
                for (page_idx, tquad) in parts {
                    let path = page_paths.get(page_idx).cloned().unwrap_or_default();
                    if page_idx != index {
                        eprintln!(
                            "[auto-inpaint::split] id={:?} owner_idx={} -> page_idx={} quad_bounds={:?} translated={:?}",
                            entry.id,
                            index,
                            page_idx,
                            vquad.bounds(),
                            tquad.bounds(),
                        );
                    }
                    jobs.push(AutoInpaintJob { index: page_idx, id: entry.id, path, quad: tquad });
                }
            }
        }
    }
    if jobs.is_empty() { return Task::none(); }
    for job in &jobs {
        let mut style = app.tabs[idx].project.entry_style(job.id);
        style.bg_color = [0,0,0,0];
        let ev = app.tabs[idx].project.set_entry_style_with_event(job.id, style);
        crate::app::handle_model_event(&mut app.tabs[idx], ev);
    }
    let backend = match effective_model { easyscanlate_settings::AutoInpaintModel::Telea=>InpaintBackend::Telea, easyscanlate_settings::AutoInpaintModel::Harmonic=>InpaintBackend::Harmonic, easyscanlate_settings::AutoInpaintModel::Lama=>InpaintBackend::Lama, easyscanlate_settings::AutoInpaintModel::Aot=>InpaintBackend::Aot, easyscanlate_settings::AutoInpaintModel::Mixed=>InpaintBackend::Harmonic };
    dispatch_auto(app, tab_id, jobs, backend)
}
#[cfg(feature = "inpaint")]
pub(crate) fn start_inpaint_selection(app: &mut App, tab_id: crate::app::tab::TabId, engine: InpaintEngine, data: Vec<ManualInpaintSelection>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    app.tabs[idx].inpainting = true;
    // Granular counters (OCR-style): one batch unit; per-group failures are
    // counted inside the worker and reported with the patches.
    if app.tabs[idx].manual_inpaint_pending == 0 {
        app.tabs[idx].manual_inpaint_total = 0;
        app.tabs[idx].manual_inpaint_failed = 0;
    }
    app.tabs[idx].manual_inpaint_total += 1;
    app.tabs[idx].manual_inpaint_pending += 1;
    app.tabs[idx].status = "inpainting...".to_string();
    let tid = tab_id;
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || run_inpaint_selection(&engine, data))
                .await
                .unwrap_or_else(|e| Err(format!("inpaint task cancelled: {e}")))
        },
        move |res| Message::Tab(tid, crate::app::TabMessage::ManualInpaintStreamRun(res)),
    )
}
#[cfg(feature = "inpaint")]
pub(crate) fn start_background_stitch(app: &mut App, tab_id: crate::app::tab::TabId, engine: InpaintEngine, job: AutoInpaintJob, pad: f32, prev: Option<String>, next: Option<String>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    app.tabs[idx].inpainting = true;
    app.tabs[idx].status = "inpainting background (stitched)...".to_string();
    let tid = tab_id;
    Task::perform(
        async move {
            let result = tokio::task::spawn_blocking(move || run_auto_inpaint_job(&engine, &job, pad, prev.as_deref(), next.as_deref())).await.unwrap_or_else(|e| Err(format!("inpaint task cancelled: {e}")));
            let grouped: GroupedInpaintResult = result.map(|v| {
                let mut map: PatchMap = std::collections::HashMap::new();
                for (idx, img,b,q) in v { map.entry(idx).or_default().push((img,b,q)); }
                let mut out: Vec<_> = map.into_iter().collect();
                out.sort_by_key(|(idx,_)| *idx);
                out
            });
            grouped
        },
        move |res| Message::Tab(tid, crate::app::TabMessage::ManualMultiInpaintFinished(res)),
    )
}

#[cfg(all(test, feature = "inpaint"))]
mod split_tests {
    use super::{anchor_quad_global, inpaint_global_offsets, split_quad_global};
    use easyscanlate_model::Quad;

    fn quad_xyxy(x0: f32, y0: f32, x1: f32, y1: f32) -> Quad {
        Quad {
            points: [[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
        }
    }

    #[test]
    fn fully_inside_stays_on_owner() {
        let ws = vec![800.0, 800.0];
        let hs = vec![2600.0, 2600.0];
        let offsets = inpaint_global_offsets(&hs);
        let parts = split_quad_global(quad_xyxy(111.0, 216.0, 259.0, 316.0), 1, &ws, &hs, &offsets);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].0, 1);
        assert_eq!(parts[0].1.bounds(), [111.0, 216.0, 259.0, 316.0]);
    }

    #[test]
    fn fully_outside_owner_reassigns_to_neighbor() {
        // idx=7 case: quad y=2816-2915 with h=2600 really lives at 216-315 in page 1.
        let ws = vec![800.0, 800.0];
        let hs = vec![2600.0, 2600.0];
        let offsets = inpaint_global_offsets(&hs);
        let parts = split_quad_global(quad_xyxy(111.6, 2816.7, 258.5, 2915.3), 0, &ws, &hs, &offsets);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].0, 1);
        let [x0, y0, x1, y1] = parts[0].1.bounds();
        assert!((y0 - 216.7).abs() < 1.0, "y0={y0}");
        assert!((y1 - 315.3).abs() < 1.0, "y1={y1}");
        assert!((x0 - 111.6).abs() < 0.01);
        assert!((x1 - 258.5).abs() < 0.01);
    }

    #[test]
    fn straddling_quad_splits_into_two_pages() {
        let ws = vec![800.0, 800.0];
        let hs = vec![2600.0, 2600.0];
        let offsets = inpaint_global_offsets(&hs);
        let parts = split_quad_global(quad_xyxy(100.0, 2550.0, 200.0, 2650.0), 0, &ws, &hs, &offsets);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].0, 0);
        assert_eq!(parts[1].0, 1);
        assert_eq!(parts[0].1.bounds(), [100.0, 2550.0, 200.0, 2650.0]);
        assert_eq!(parts[1].1.bounds(), [100.0, -50.0, 200.0, 50.0]);
    }

    #[test]
    fn outside_all_pages_returns_empty() {
        let ws = vec![800.0];
        let hs = vec![2600.0];
        let offsets = inpaint_global_offsets(&hs);
        let parts = split_quad_global(quad_xyxy(10.0, 5000.0, 50.0, 5100.0), 0, &ws, &hs, &offsets);
        assert!(parts.is_empty());
    }

    #[test]
    fn anchor_keeps_single_job_on_max_overlap_page() {
        let ws = vec![800.0, 800.0];
        let hs = vec![2600.0, 2600.0];
        let offsets = inpaint_global_offsets(&hs);
        // Fully-outside owner reanchors to neighbor as 1 job.
        let anchored = anchor_quad_global(quad_xyxy(111.6, 2816.7, 258.5, 2915.3), 0, &ws, &hs, &offsets).unwrap();
        assert_eq!(anchored.0, 1);
        // Straddle anchors to the page holding more of the quad (page 0: 50px vs page 1: 50px tie -> first max wins).
        let anchored = anchor_quad_global(quad_xyxy(100.0, 2550.0, 200.0, 2650.0), 0, &ws, &hs, &offsets).unwrap();
        assert_eq!(anchored.0, 0);
        // Outside everything anchors to nothing.
        assert!(anchor_quad_global(quad_xyxy(10.0, 5000.0, 50.0, 5100.0), 0, &ws, &hs, &offsets).is_none());
    }
}
