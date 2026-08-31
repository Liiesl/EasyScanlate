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

use scanlateit_ui::UiState;

use super::{App, AutoInpaintJob, Message};

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
const STITCH_W: u32 = 512;
#[cfg(feature = "inpaint")]
const STITCH_H: u32 = 512;

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
            let grouped: Result<Vec<(usize, Vec<(image::RgbaImage, [f32; 4], Option<Quad>)>)>, String> = result.map(|v| {
                let mut map: std::collections::HashMap<usize, Vec<(image::RgbaImage, [f32; 4], Option<Quad>)>> = std::collections::HashMap::new();
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
fn start_background_stitch(
    app: &mut App,
    engine: InpaintEngine,
    job: AutoInpaintJob,
    pad: f32,
    prev: Option<String>,
    next: Option<String>,
) -> Task<Message> {
    let tid = app.active_tab().id;
    app.active_tab_mut().inpainting = true;
    app.active_tab_mut().status = "inpainting background (stitched)...".to_string();
    Task::perform(
        async move {
            let result = tokio::task::spawn_blocking(move || {
                run_auto_job_with_stitch(&engine, &job, pad, prev.as_deref(), next.as_deref())
            })
            .await
            .unwrap_or_else(|e| Err(format!("inpaint task cancelled: {e}")));
            let grouped: Result<Vec<(usize, Vec<(image::RgbaImage, [f32; 4], Option<Quad>)>)>, String> = result.map(|v| {
                let mut map: std::collections::HashMap<usize, Vec<(image::RgbaImage, [f32; 4], Option<Quad>)>> = std::collections::HashMap::new();
                for (idx, img, b, q) in v {
                    map.entry(idx).or_default().push((img, b, q));
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
pub fn dispatch_auto(app: &mut App, jobs: Vec<AutoInpaintJob>, backend: InpaintBackend) -> Task<Message> {
    dispatch_auto_for(app, app.active_tab().id, jobs, backend)
}

#[cfg(feature = "inpaint")]
pub fn dispatch_auto_solo(app: &mut App, effective_model: scanlateit_settings::AutoInpaintModel) -> Task<Message> {
    dispatch_auto_solo_for(app, app.active_tab().id, effective_model)
}

#[cfg(feature = "inpaint")]
pub fn handle_inpaint_engine_ready(app: &mut App, result: Result<InpaintEngine, String>) -> Task<Message> {
    handle_inpaint_engine_ready_for(app, app.active_tab().id, result)
}

#[cfg(feature = "inpaint")]
pub fn handle_auto_engine_ready(app: &mut App, backend: InpaintBackend, result: Result<InpaintEngine, String>) -> Task<Message> {
    handle_auto_engine_ready_for(app, app.active_tab().id, backend, result)
}

#[cfg(feature = "inpaint")]
pub fn handle_auto_finished(app: &mut App, index: usize, id: EntryId, result: Result<Vec<(usize, image::RgbaImage, [f32; 4], Option<Quad>)>, String>) -> Task<Message> {
    handle_auto_finished_for(app, app.active_tab().id, index, id, result)
}

#[cfg(feature = "inpaint")]
fn apply_patches(app: &mut App, patches: Vec<(usize, image::RgbaImage, [f32; 4], Option<Quad>)>) {
    let mut pending_evs: Vec<(scanlateit_model::ImageId, [f32; 4], Option<Quad>)> = Vec::new();
    let mut affected = std::collections::HashSet::new();
    for (target_idx, patch, bounds, quad) in patches {
        let Some(image_id) = app.active_tab_mut().images.get(target_idx).map(|i| i.image_id) else { continue; };
        if let Some(image) = app.active_tab_mut().images.get_mut(target_idx) {
            let (width, height) = (patch.width(), patch.height());
            let layer = InpaintLayer {
                bounds,
                quad,
                handle: iced::widget::image::Handle::from_rgba(width, height, bytes::Bytes::from(patch.into_raw())),
                width,
                height,
            };
            image.inpaint.push(layer);
            pending_evs.push((image_id, bounds, quad));
            affected.insert(target_idx);
        }
    }
    for (image_id, bounds, quad) in pending_evs {
        let ev = app.active_tab_mut().project.add_inpaint_patch_with_bounds_and_quad(image_id, bounds, quad);
        crate::app::handle_model_event(app.active_tab_mut(), ev);
    }
    if !affected.is_empty() {
        app.active_tab_mut().show_inpaint = true;
    }
}

#[cfg(feature = "inpaint")]
pub fn handle_auto_batch(app: &mut App, batch: Vec<(usize, EntryId, Result<Vec<(usize, image::RgbaImage, [f32; 4], Option<Quad>)>, String>)>) -> Task<Message> {
    handle_auto_batch_for(app, app.active_tab().id, batch)
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
) -> Result<Vec<(usize, RgbaImage, [f32; 4], Option<Quad>)>, String> {
    let [x0, y0, x1, y1] = job.quad.bounds();
    let rect = [x0, y0, x1 - x0, y1 - y0];
    // Decode main to get dims for seam detection
    let main_rgba = image::ImageReader::open(&job.path)
        .map_err(|e| format!("Failed to open {}: {e}", job.path))?
        .with_guessed_format()
        .map_err(|e| format!("Failed to decode {}: {e}", job.path))?
        .decode()
        .map_err(|e| format!("Failed to decode {}: {e}", job.path))?
        .into_rgba8();
    let (img_w, img_h) = main_rgba.dimensions();
    let img_h_f = img_h as f32;
    // Use actual view_quad points (rotated) for seam trigger, not AABB rect.
    let min_y = job.quad.points.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
    let max_y = job.quad.points.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max);
    let need_top = if min_y < pad && prev_path.is_some() { pad - min_y } else { 0.0 };
    let need_bottom = if max_y > img_h_f - pad && next_path.is_some() { max_y + pad - img_h_f } else { 0.0 };
    eprintln!("[inpaint::seam] idx={} rect={:?} view_min_y={:.1} view_max_y={:.1} pad={} img={}x{} need_top={:.1} need_bottom={:.1} prev={} next={}", job.index, rect, min_y, max_y, pad, img_w, img_h, need_top, need_bottom, prev_path.is_some(), next_path.is_some());
    if need_top <= 0.0 && need_bottom <= 0.0 {
        eprintln!("[inpaint::seam] -> no stitch, normal run_blocking");
        let v = engine.run_blocking(&job.path, rect, &[job.quad])?;
        return Ok(v.into_iter().map(|(img, b, q)| (job.index, img, b, q)).collect());
    }
    // Unified 512 stitch for single-quad auto jobs when seam is detected.
    let exp_x0 = (rect[0] - pad).max(0.0);
    let exp_y0 = (rect[1] - pad).max(0.0);
    let exp_x1 = (rect[0] + rect[2] + pad).min(img_w as f32);
    let exp_y1 = (rect[1] + rect[3] + pad).min(img_h as f32);
    let exp_w = (exp_x1 - exp_x0).max(1.0) as u32;
    let exp_h_main = (exp_y1 - exp_y0).max(1.0) as u32;
    eprintln!("[inpaint::seam] -> STITCH 512 triggered exp=[{:.1},{:.1},{:.1},{:.1}] exp_w={} exp_h_main={}", exp_x0, exp_y0, exp_x1, exp_y1, exp_w, exp_h_main);

    // Build raws: each raw is a full image + orig rect (strip) for stitching.
    struct Raw {
        idx: usize,
        full: image::RgbaImage,
        img_w: u32,
        img_h: u32,
        orig: [u32; 4],
        quads: Vec<Quad>,
    }
    let mut raws: Vec<Raw> = Vec::new();
    let main_idx = job.index;

    // Helper to decode neighbor
    let decode = |p: &str| -> Option<RgbaImage> {
        image::ImageReader::open(p)
            .ok()?
            .with_guessed_format()
            .ok()?
            .decode()
            .ok()
            .map(|d| d.into_rgba8())
    };

    // Determine prev/next indices for raw idx
    let mut neighbor_idx_prev: Option<usize> = None;
    let mut neighbor_idx_next: Option<usize> = None;
    if need_top > 0.0 && prev_path.is_some() {
        // Find prev idx as main_idx -1 if valid, else use main_idx (fallback)
        neighbor_idx_prev = if main_idx > 0 { Some(main_idx - 1) } else { Some(main_idx) };
    }
    if need_bottom > 0.0 && next_path.is_some() {
        neighbor_idx_next = Some(main_idx + 1);
    }

    if need_top > 0.0 {
        if let Some(pp) = prev_path {
            if let Some(prev_rgba) = decode(pp) {
                let (pw, ph) = prev_rgba.dimensions();
                let take_h = (need_top as u32).min(ph);
                if take_h > 0 {
                    let w_take = exp_w.min(pw);
                    // Align x with main's expanded region center
                    let center_x_main = exp_x0 + exp_w as f32 * 0.5;
                    let mut x_src = (center_x_main - w_take as f32 * 0.5).round() as i32;
                    x_src = x_src.clamp(0, pw as i32 - w_take as i32).max(0);
                    let y_src = ph.saturating_sub(take_h);
                    let idx_prev = neighbor_idx_prev.unwrap_or(main_idx.saturating_sub(1));
                    raws.push(Raw {
                        idx: idx_prev,
                        full: prev_rgba,
                        img_w: pw,
                        img_h: ph,
                        orig: [x_src as u32, y_src, w_take, take_h],
                        quads: Vec::new(),
                    });
                }
            }
        }
    }
    // Main
    {
        let idx = main_idx;
        raws.push(Raw {
            idx,
            full: main_rgba,
            img_w,
            img_h,
            orig: [exp_x0 as u32, exp_y0 as u32, exp_w, exp_h_main],
            quads: vec![job.quad],
        });
    }
    if need_bottom > 0.0 {
        if let Some(np) = next_path {
            if let Some(next_rgba) = decode(np) {
                let (nw, nh) = next_rgba.dimensions();
                let take_h = (need_bottom as u32).min(nh);
                if take_h > 0 {
                    let w_take = exp_w.min(nw);
                    let center_x_main = exp_x0 + exp_w as f32 * 0.5;
                    let mut x_src = (center_x_main - w_take as f32 * 0.5).round() as i32;
                    x_src = x_src.clamp(0, nw as i32 - w_take as i32).max(0);
                    let idx_next = neighbor_idx_next.unwrap_or(main_idx + 1);
                    raws.push(Raw {
                        idx: idx_next,
                        full: next_rgba,
                        img_w: nw,
                        img_h: nh,
                        orig: [x_src as u32, 0, w_take, take_h],
                        quads: Vec::new(),
                    });
                }
            }
        }
    }
    // Sort by idx to keep order prev->main->next
    raws.sort_by_key(|r| r.idx);
    eprintln!("[inpaint::seam] raws={} idxs={:?} main_idx={} raws_orig={:?}", raws.len(), raws.iter().map(|r| r.idx).collect::<Vec<_>>(), main_idx, raws.iter().map(|r| r.orig).collect::<Vec<_>>());
    for r in &raws { eprintln!("[inpaint::seam] raw idx={} img={}x{} orig={:?} quads={}", r.idx, r.img_w, r.img_h, r.orig, r.quads.len()); }
    if raws.is_empty() {
        let v = engine.run_blocking(&job.path, rect, &[job.quad])?;
        return Ok(v.into_iter().map(|(img, b, q)| (job.index, img, b, q)).collect());
    }
    if raws.len() == 1 {
        // No neighbor decoded, fallback
        let v = engine.run_blocking(&job.path, rect, &[job.quad])?;
        return Ok(v.into_iter().map(|(img, b, q)| (job.index, img, b, q)).collect());
    }

    // Now build 512x512 stitched canvas from raws – unified with run_stitched_inpaint logic.
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
    if raws.len() == 2 {
        // Two pieces: allocate heights to fill 512, seam at h0 (same as run_stitched_inpaint)
        let raw_h0 = raws[0].orig[3] as i32;
        let raw_h1 = raws[1].orig[3] as i32;
        let avail_top0 = raws[0].orig[1] as i32;
        let avail_bottom1 = raws[1].img_h as i32 - (raws[1].orig[1] as i32 + raws[1].orig[3] as i32);
        let total_raw = raw_h0 + raw_h1;
        let mut h0: i32;
        let mut h1: i32;
        if total_raw >= STITCH_H as i32 {
            h0 = (STITCH_H as f32 * raw_h0 as f32 / total_raw as f32).round() as i32;
            h0 = h0.clamp(1, STITCH_H as i32 - 1);
            h1 = STITCH_H as i32 - h0;
        } else {
            let extra_needed = STITCH_H as i32 - total_raw;
            let mut extra0 = (extra_needed / 2 + extra_needed % 2).min(avail_top0);
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
            h0 = raw_h0 + extra0;
            h1 = raw_h1 + extra1;
        }
        h0 = h0.max(1).min(STITCH_H as i32);
        h1 = h1.max(1).min(STITCH_H as i32);
        let y_src0 = (raws[0].orig[1] as i32 + raws[0].orig[3] as i32 - h0).clamp(0, raws[0].img_h as i32 - h0).max(0);
        let y_src1 = raws[1].orig[1] as i32;
        for (i, r) in raws.iter().enumerate() {
            let [ox, _oy, ow, _oh] = r.orig;
            let w_src = STITCH_W.min(r.img_w);
            let center_x = ox as f32 + ow as f32 * 0.5;
            let mut x_src = (center_x - w_src as f32 * 0.5).round() as i32;
            x_src = x_src.clamp(0, r.img_w as i32 - w_src as i32).max(0);
            let (h_src, off_y, y_src) = if i == 0 { (h0 as u32, 0u32, y_src0) } else { (h1 as u32, h0 as u32, y_src1) };
            let y_src_clamped = (y_src).clamp(0, r.img_h as i32 - h_src as i32).max(0);
            pieces.push(Piece { idx: r.idx, orig: r.orig, x_src, y_src: y_src_clamped, w_src, h_src, off_y, quads: r.quads.clone() });
        }
    } else if raws.len() == 3 {
        // Three pieces (prev strip, main, next strip) – allocate edge pieces to fill 512, keep middle fixed if possible.
        let raw_h0 = raws[0].orig[3] as i32;
        let raw_h1 = raws[1].orig[3] as i32;
        let raw_h2 = raws[2].orig[3] as i32;
        let total_raw = raw_h0 + raw_h1 + raw_h2;
        let avail_top0 = raws[0].orig[1] as i32;
        let avail_bottom2 = raws[2].img_h as i32 - (raws[2].orig[1] as i32 + raws[2].orig[3] as i32);
        let mut h0 = raw_h0;
        let mut h1 = raw_h1;
        let mut h2 = raw_h2;
        if total_raw < STITCH_H as i32 {
            let extra_needed = STITCH_H as i32 - total_raw;
            // Prioritize edges, middle stays fixed unless edges capped
            let mut extra0 = (extra_needed / 2).min(avail_top0);
            let mut extra2 = (extra_needed - extra0).min(avail_bottom2);
            let mut remaining = extra_needed - extra0 - extra2;
            // If still remaining, try to expand middle both sides
            if remaining > 0 {
                let avail_top1 = raws[1].orig[1] as i32;
                let avail_bottom1 = raws[1].img_h as i32 - (raws[1].orig[1] as i32 + raws[1].orig[3] as i32);
                let max_mid_extra = avail_top1 + avail_bottom1;
                let add_mid = remaining.min(max_mid_extra);
                h1 += add_mid;
                remaining -= add_mid;
                if remaining > 0 && avail_top0 > extra0 {
                    let add = remaining.min(avail_top0 - extra0);
                    extra0 += add;
                    remaining -= add;
                }
                if remaining > 0 && avail_bottom2 > extra2 {
                    let add = remaining.min(avail_bottom2 - extra2);
                    extra2 += add;
                    remaining -= add;
                }
            }
            h0 = raw_h0 + extra0;
            h2 = raw_h2 + extra2;
        } else if total_raw > STITCH_H as i32 {
            // Proportional shrink
            let f0 = raw_h0 as f32 / total_raw as f32;
            let f1 = raw_h1 as f32 / total_raw as f32;
            h0 = (STITCH_H as f32 * f0).round() as i32; h0 = h0.clamp(1, STITCH_H as i32 - 2);
            h1 = (STITCH_H as f32 * f1).round() as i32; h1 = h1.clamp(1, STITCH_H as i32 - h0 - 1);
            h2 = STITCH_H as i32 - h0 - h1;
            h2 = h2.max(1);
        }
        h0 = h0.max(1).min(STITCH_H as i32);
        h1 = h1.max(1).min(STITCH_H as i32);
        h2 = h2.max(1).min(STITCH_H as i32);
        // y_src
        let y_src0 = (raws[0].orig[1] as i32 + raws[0].orig[3] as i32 - h0).clamp(0, raws[0].img_h as i32 - h0).max(0);
        let y_src1 = {
            // For middle, if expanded, center it
            let extra_mid = h1 - raw_h1;
            if extra_mid > 0 {
                let avail_top1 = raws[1].orig[1] as i32;
                let extra_top = (extra_mid / 2).min(avail_top1);
                (raws[1].orig[1] as i32 - extra_top).clamp(0, raws[1].img_h as i32 - h1).max(0)
            } else {
                raws[1].orig[1] as i32
            }
        };
        let y_src2 = raws[2].orig[1] as i32;
        for (i, r) in raws.iter().enumerate() {
            let [ox, _oy, ow, _oh] = r.orig;
            let w_src = STITCH_W.min(r.img_w);
            let center_x = ox as f32 + ow as f32 * 0.5;
            let mut x_src = (center_x - w_src as f32 * 0.5).round() as i32;
            x_src = x_src.clamp(0, r.img_w as i32 - w_src as i32).max(0);
            let (h_src, off_y, y_src) = match i {
                0 => (h0 as u32, 0u32, y_src0),
                1 => (h1 as u32, h0 as u32, y_src1),
                _ => (h2 as u32, (h0 + h1) as u32, y_src2),
            };
            let y_src_clamped = y_src.clamp(0, r.img_h as i32 - h_src as i32).max(0);
            pieces.push(Piece { idx: r.idx, orig: r.orig, x_src, y_src: y_src_clamped, w_src, h_src, off_y, quads: r.quads.clone() });
        }
    } else {
        // Single (fallback) – centered 512
        let r = &raws[0];
        let [ox, oy, ow, oh] = r.orig;
        let w_src = STITCH_W.min(r.img_w);
        let center_x = ox as f32 + ow as f32 * 0.5;
        let mut x_src = (center_x - w_src as f32 * 0.5).round() as i32;
        x_src = x_src.clamp(0, r.img_w as i32 - w_src as i32).max(0);
        let h_src = STITCH_H.min(r.img_h);
        let center_y = oy as f32 + oh as f32 * 0.5;
        let mut y_src = (center_y - h_src as f32 * 0.5).round() as i32;
        if oy == 0 { y_src = 0; } else if oy + oh == r.img_h { y_src = r.img_h as i32 - h_src as i32; } else { y_src = y_src.clamp(0, r.img_h as i32 - h_src as i32).max(0); }
        pieces.push(Piece { idx: r.idx, orig: r.orig, x_src, y_src, w_src, h_src, off_y: 0, quads: r.quads.clone() });
    }

    for p in &pieces { eprintln!("[inpaint::seam] piece idx={} orig={:?} x_src={} y_src={} w_src={} h_src={} off_y={} quads={}", p.idx, p.orig, p.x_src, p.y_src, p.w_src, p.h_src, p.off_y, p.quads.len()); }
    eprintln!("[inpaint::seam] stitched pieces total_h={} main_idx={} main_off_y={}", pieces.iter().map(|p| p.h_src).sum::<u32>(), main_idx, pieces.iter().find(|p| p.idx==main_idx).map(|p| p.off_y).unwrap_or(0));
    // Build stitched 512x512
    let mut stitched = image::RgbaImage::new(STITCH_W, STITCH_H);
    for p in &pieces {
        let src = &raws.iter().find(|r| r.idx == p.idx).unwrap().full;
        let crop = image::imageops::crop_imm(src, p.x_src as u32, p.y_src as u32, p.w_src, p.h_src).to_image();
        let mut placed = crop;
        if p.w_src < STITCH_W {
            let mut full_w = image::RgbaImage::new(STITCH_W, p.h_src);
            image::imageops::replace(&mut full_w, &placed, 0, 0);
            let remaining = STITCH_W - p.w_src;
            if remaining > 0 {
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
    let total_h: u32 = pieces.iter().map(|p| p.h_src).sum();
    if total_h < STITCH_H {
        let gap = STITCH_H - total_h;
        if gap > 0 {
            for y in 0..gap {
                let src_y = (total_h as i32 - 1 - (y as i32 % total_h as i32)).max(0) as u32;
                for x in 0..STITCH_W {
                    let px = stitched.get_pixel(x, src_y).clone();
                    stitched.put_pixel(x, total_h + y, px);
                }
            }
        }
    }

    // Build stitched quads – only main quad(s) matter
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
    eprintln!("[inpaint::seam] quads_stitched={} rect_stitched will be from main_piece idx={}", quads_stitched.len(), main_idx);
    for (i,q) in quads_stitched.iter().enumerate() { eprintln!("[inpaint::seam] quad_stitched {}: {:?}", i, q.points); }
    // If no quads (should not happen for auto), fallback
    if quads_stitched.is_empty() {
        let v = engine.run_blocking(&job.path, rect, &[job.quad])?;
        return Ok(v.into_iter().map(|(img, b, q)| (job.index, img, b, q)).collect());
    }
    // Compute rect in stitched space – find main piece
    let main_piece = pieces.iter().find(|p| p.idx == main_idx).unwrap();
    let rect_stitched = [
        rect[0] - main_piece.x_src as f32,
        rect[1] - main_piece.y_src as f32 + main_piece.off_y as f32,
        rect[2],
        rect[3],
    ];
    let patches = engine.run_on_image(&stitched, rect_stitched, &quads_stitched)?;
    // Map patches back – split across seam like run_stitched_inpaint, clipped to image bounds
    let mut per_image: std::collections::HashMap<usize, Vec<(RgbaImage, [f32; 4], Option<Quad>)>> = std::collections::HashMap::new();
    for (idx, (patch_img, bounds_stitched, quad_opt)) in patches.into_iter().enumerate() {
        let [bx, by, bw, bh] = bounds_stitched;
        let p: &Piece = if idx < quad_piece.len() {
            let wanted = quad_piece[idx];
            pieces.iter().find(|x| x.idx == wanted).unwrap()
        } else {
            let cy = by + bh / 2.0;
            let mut found: Option<&Piece> = None;
            for pp in &pieces {
                let py0 = pp.off_y as f32; let py1 = py0 + pp.h_src as f32;
                if cy >= py0 && cy < py1 { found = Some(pp); break; }
            }
            match found {
                Some(v) => v,
                None => {
                    let mut best: Option<&Piece> = None; let mut best_overlap: f32 = 0.0;
                    for cand in &pieces {
                        let py0 = cand.off_y as f32; let py1 = py0 + cand.h_src as f32;
                        let overlap = (by + bh).min(py1) - by.max(py0);
                        if overlap > best_overlap { best_overlap = overlap; best = Some(cand); }
                    }
                    match best { Some(v) => v, None => continue }
                }
            }
        };
        // Check if bbox straddles any seam -> split into per-piece segments
        let seams: Vec<f32> = pieces.iter().skip(1).map(|pp| pp.off_y as f32).collect();
        let mut straddles = false;
        for &seam in &seams {
            if by < seam && by + bh > seam { straddles = true; break; }
        }
        if straddles {
            let mut sorted_seams: Vec<f32> = seams.into_iter().filter(|&s| s > by && s < by + bh).collect();
            sorted_seams.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mut cur_y = by;
            let mut patch_off: f32 = 0.0;
            let mut segments: Vec<(f32, f32, f32)> = Vec::new();
            for seam in sorted_seams {
                let h = seam - cur_y;
                if h > 0.5 {
                    segments.push((cur_y, h, patch_off));
                }
                patch_off += h;
                cur_y = seam;
            }
            let last_h = by + bh - cur_y;
            if last_h > 0.5 {
                segments.push((cur_y, last_h, patch_off));
            }
            if segments.len() > 1 {
                for (seg_y, seg_h, seg_off) in segments {
                    // Find piece containing this segment
                    let piece = pieces.iter().find(|pp| {
                        let py0 = pp.off_y as f32;
                        let py1 = py0 + pp.h_src as f32;
                        seg_y >= py0 && seg_y < py1
                    }).or_else(|| {
                        pieces.iter().find(|pp| {
                            let py0 = pp.off_y as f32;
                            let py1 = py0 + pp.h_src as f32;
                            let mid = seg_y + seg_h * 0.5;
                            mid >= py0 && mid < py1
                        })
                    }).unwrap_or(p);
                    let seg_patch = image::imageops::crop_imm(&patch_img, 0, seg_off as u32, bw as u32, seg_h as u32).to_image();
                    let orig_x = bx + piece.x_src as f32;
                    let orig_y = seg_y - piece.off_y as f32 + piece.y_src as f32;
                    let (img_w_f, img_h_f) = {
                        let r = raws.iter().find(|r| r.idx == piece.idx).unwrap();
                        (r.img_w as f32, r.img_h as f32)
                    };
                    let clip_x0 = orig_x.max(0.0);
                    let clip_y0 = orig_y.max(0.0);
                    let clip_x1 = (orig_x + bw).min(img_w_f);
                    let clip_y1 = (orig_y + seg_h).min(img_h_f);
                    if clip_x1 <= clip_x0 || clip_y1 <= clip_y0 { continue; }
                    let new_w = clip_x1 - clip_x0;
                    let new_h = clip_y1 - clip_y0;
                    let crop_x = (clip_x0 - orig_x).round().max(0.0) as u32;
                    let crop_y = (clip_y0 - orig_y).round().max(0.0) as u32;
                    let clipped_patch = if crop_x != 0 || crop_y != 0 || new_w as u32 != seg_patch.width() || new_h as u32 != seg_patch.height() {
                        let cw = (new_w as u32).min(seg_patch.width().saturating_sub(crop_x));
                        let ch = (new_h as u32).min(seg_patch.height().saturating_sub(crop_y));
                        if cw == 0 || ch == 0 { continue; }
                        image::imageops::crop_imm(&seg_patch, crop_x, crop_y, cw, ch).to_image()
                    } else { seg_patch };
                    let bounds = [clip_x0, clip_y0, new_w, new_h];
                    let orig_quad = quad_opt.map(|q| {
                        let mut nq = q;
                        for pt in &mut nq.points { pt[0] += piece.x_src as f32; pt[1] += piece.y_src as f32 - piece.off_y as f32; }
                        nq
                    });
                    eprintln!("[inpaint::seam] patch split -> {} at {:?} (seg_y {:.1} h {:.1} stitched {}x{} rect_stitched {:?})", piece.idx, bounds, seg_y, seg_h, stitched.width(), stitched.height(), rect_stitched);
                    per_image.entry(piece.idx).or_default().push((clipped_patch, bounds, orig_quad));
                }
                continue;
            }
        }
        // Non-split path – single piece, clip to image bounds like manual
        let local_y = by - p.off_y as f32;
        let orig_x = bx + p.x_src as f32;
        let orig_y = local_y + p.y_src as f32;
        let (img_w_f, img_h_f) = {
            let r = raws.iter().find(|r| r.idx == p.idx).unwrap();
            (r.img_w as f32, r.img_h as f32)
        };
        let clip_x0 = orig_x.max(0.0);
        let clip_y0 = orig_y.max(0.0);
        let clip_x1 = (orig_x + bw).min(img_w_f);
        let clip_y1 = (orig_y + bh).min(img_h_f);
        if clip_x1 <= clip_x0 || clip_y1 <= clip_y0 { continue; }
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
        let orig_quad = quad_opt.map(|q| {
            let mut nq = q;
            for pt in &mut nq.points { pt[0] += p.x_src as f32; pt[1] += p.y_src as f32 - p.off_y as f32; }
            nq
        });
        per_image.entry(p.idx).or_default().push((clipped_patch, bounds, orig_quad));
        eprintln!("[inpaint::seam] patch -> {} at {:?} (stitched {}x{} rect_stitched {:?})", p.idx, bounds, stitched.width(), stitched.height(), rect_stitched);
    }
    // Flatten per_image into Vec<(target_idx, patch, bounds, quad)>
    let mut out: Vec<(usize, RgbaImage, [f32; 4], Option<Quad>)> = Vec::new();
    for (target_idx, vec) in per_image {
        for (img, bounds, quad) in vec {
            out.push((target_idx, img, bounds, quad));
        }
    }
    out.sort_by_key(|(idx, _, _, _)| *idx);
    if out.is_empty() {
        let v = engine.run_blocking(&job.path, rect, &[job.quad])?;
        return Ok(v.into_iter().map(|(img, b, q)| (job.index, img, b, q)).collect());
    }
    Ok(out)
}









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
            let style = {
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
        let (backend, radius) = scanlateit_settings::get(|s| (s.inpaint_backend, s.inpaint_radius.parse::<i32>().unwrap_or(5).max(1)));
        // queue gate for background stitch (single inpaint)
        {
            use crate::app::queue::{AcquireResult, EngineKind};
            let kind = match backend {
                InpaintBackend::Telea => EngineKind::InpaintTelea,
                InpaintBackend::Lama => EngineKind::InpaintLama,
                InpaintBackend::Aot => EngineKind::InpaintAot,
            };
            let tab_id = app.active_tab().id;
            let already_running = app.engines.queue.running_for(tab_id, kind).is_some();
            let already_queued = app.engines.queue.pending_for_tab(tab_id).iter().any(|j| j.kind == kind);
            if !already_running && !already_queued {
                match app.engines.queue.try_acquire_or_enqueue(tab_id, kind) {
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
        let cached = app.engines.inpaint.clone().filter(|engine| engine.backend() == backend && engine.radius() == radius);
        let tid = app.active_tab().id;
        return match cached {
            Some(engine) => start_background_stitch(app, engine, job, pad, prev, next),
            None => {
                app.active_tab_mut().pending_background_stitch = Some((job, pad, prev, next));
                app.active_tab_mut().status = match backend {
                    InpaintBackend::Lama => "Loading LaMa model...".to_string(),
                    InpaintBackend::Aot => "Loading AOT-GAN model...".to_string(),
                    InpaintBackend::Telea => "Inpainting background...".to_string(),
                };
                Task::perform(async move { scanlateit_inpaint::Engine::build(backend, radius) }, move |res| Message::Tab(tid, crate::app::TabMessage::InpaintEngineReady(res)))
            }
        };
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

pub fn handle_inpaint_clicked(app: &mut App, selection: Option<(usize, usize)>) -> Task<Message> {
    use super::edit::clear_editing;
    clear_editing(app);
    match selection {
        Some((image_index, patch_idx)) => {
            let (image_id, inpaint_len) = {
                let tab = app.active_tab();
                let Some(img) = tab.images.get(image_index) else {
                    drop(tab);
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
    drop(image);
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
    if let Some(img) = app.active_tab_mut().images.get_mut(image_index) {
        if patch_idx < img.inpaint.len() {
            img.inpaint.remove(patch_idx);
        }
    }
    let patch_id = app.active_tab().project.extras.inpaint_patches.iter().filter(|p| p.image_id == image_id).nth(patch_idx).map(|p| p.id);
    if let Some(id) = patch_id {
        if let Some(ev) = app.active_tab_mut().project.remove_inpaint_patch(id) {
            crate::app::handle_model_event(app.active_tab_mut(), ev);
        }
    }
    if app.active_tab_mut().selected_inpaint == Some((image_index, patch_idx)) {
        app.active_tab_mut().selected_inpaint = None;
    } else if let Some((sel_img, sel_patch)) = app.active_tab_mut().selected_inpaint {
        if sel_img == image_index && sel_patch > patch_idx {
            app.active_tab_mut().selected_inpaint = Some((sel_img, sel_patch - 1));
        }
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
            if let Some(image) = app.active_tab_mut().images.get_mut(image_index) {
                if patch_idx < image.inpaint.len() {
                    image.inpaint.remove(patch_idx);
                }
            }
            if let Some(id) = patch_id {
                if let Some(ev) = app.active_tab_mut().project.remove_inpaint_patch(id) {
                    crate::app::handle_model_event(app.active_tab_mut(), ev);
                }
            }
            let sel = app.active_tab().selected_inpaint;
            if sel == Some((image_index, patch_idx)) {
                app.active_tab_mut().selected_inpaint = None;
            } else if let Some((sel_img, sel_patch)) = sel {
                if sel_img == image_index && sel_patch > patch_idx {
                    app.active_tab_mut().selected_inpaint = Some((sel_img, sel_patch - 1));
                }
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
        // queue gate for repaint manual inpaint (single selection)
        {
            use crate::app::queue::{AcquireResult, EngineKind};
            let kind = match backend {
                scanlateit_settings::InpaintBackend::Telea => EngineKind::InpaintTelea,
                scanlateit_settings::InpaintBackend::Lama => EngineKind::InpaintLama,
                scanlateit_settings::InpaintBackend::Aot => EngineKind::InpaintAot,
            };
            let tab_id = app.active_tab().id;
            let already_running = app.engines.queue.running_for(tab_id, kind).is_some();
            let already_queued = app.engines.queue.pending_for_tab(tab_id).iter().any(|j| j.kind == kind);
            if !already_running && !already_queued {
                match app.engines.queue.try_acquire_or_enqueue(tab_id, kind) {
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
        let cached = app
            .engines
            .inpaint
            .clone()
            .filter(|engine| engine.backend() == backend && engine.radius() == radius);
        let tid = app.active_tab().id;
        match cached {
            Some(engine) => return start_inpaint(app, engine, image_index, path, rect, quads),
            None => {
                app.active_tab_mut().pending_manual_multi = Some(vec![(image_index, path, rect, quads)]);
                app.active_tab_mut().status = match backend {
                    scanlateit_settings::InpaintBackend::Lama => "Loading LaMa model...".to_string(),
                    scanlateit_settings::InpaintBackend::Aot => "Loading AOT-GAN model...".to_string(),
                    scanlateit_settings::InpaintBackend::Telea => "Inpainting...".to_string(),
                };
                return Task::perform(
                    async move { scanlateit_inpaint::Engine::build(backend, radius) },
                    move |res| Message::Tab(tid, crate::app::TabMessage::InpaintEngineReady(res)),
                );
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



#[cfg(feature = "inpaint")]
pub fn handle_inpaint_selection(app: &mut App, selections: Vec<(usize, iced::Rectangle)>) -> Task<Message> {
    {
        let tab = app.active_tab();
        eprintln!("[manual::inpaint] handle_inpaint_selection selections={} cached_engine={} running={} translating={} inpainting={}", selections.len(), app.engines.inpaint.is_some(), tab.running, tab.translating, tab.inpainting);
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
    let (backend, radius) = scanlateit_settings::get(|s| (s.inpaint_backend, s.inpaint_radius.parse::<i32>().unwrap_or(5).max(1)));
    eprintln!("[manual::inpaint] backend={:?} radius={} cached_match={}", backend, radius, app.engines.inpaint.as_ref().map(|e| e.backend()==backend && e.radius()==radius).unwrap_or(false));
    // queue gate — manual inpaint uses same backend weight/priority as auto
    {
        use crate::app::queue::{AcquireResult, EngineKind};
        let kind = match backend {
            InpaintBackend::Telea => EngineKind::InpaintTelea,
            InpaintBackend::Lama => EngineKind::InpaintLama,
            InpaintBackend::Aot => EngineKind::InpaintAot,
        };
        let tab_id = app.active_tab().id;
        // Avoid duplicate queue if already running/queued for this tab+kind
        let already_running = app.engines.queue.running_for(tab_id, kind).is_some();
        let already_queued = app.engines.queue.pending_for_tab(tab_id).iter().any(|j| j.kind == kind);
        if !already_running && !already_queued {
            match app.engines.queue.try_acquire_or_enqueue(tab_id, kind) {
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
    let cached = app.engines.inpaint.clone().filter(|e| e.backend() == backend && e.radius() == radius);
    // Store pending for engine build path (weight already reserved via queue)
    if let Some(engine) = cached {
        eprintln!("[manual::inpaint] using cached engine -> start_inpaint_selection");
        return start_inpaint_selection(app, engine, data);
    } else {
        eprintln!("[manual::inpaint] no cached engine -> pending_manual_multi len={} status loading", data.len());
        let tid = app.active_tab().id;
        app.active_tab_mut().pending_manual_multi = Some(data);
        app.active_tab_mut().status = match backend {
            InpaintBackend::Lama => "Loading LaMa model...".to_string(),
            InpaintBackend::Aot => "Loading AOT-GAN model...".to_string(),
            InpaintBackend::Telea => "Inpainting...".to_string(),
        };
        return Task::perform(async move { scanlateit_inpaint::Engine::build(backend, radius) }, move |res| Message::Tab(tid, crate::app::TabMessage::InpaintEngineReady(res)));
    }
}

#[cfg(feature = "inpaint")]
fn start_inpaint_selection(app: &mut App, engine: InpaintEngine, data: Vec<(usize, String, [f32; 4], Vec<Quad>)>) -> Task<Message> {
    eprintln!("[manual::multi] start_inpaint_selection data={} backend={:?} radius={}", data.len(), engine.backend(), engine.radius());
    for (i, (idx, path, rect, quads)) in data.iter().enumerate() {
        eprintln!("[manual::multi]   data {}: idx={} path={} rect={:?} quads={}", i, idx, path, rect, quads.len());
    }
    let tid = app.active_tab().id;
    app.active_tab_mut().inpainting = true;
    app.active_tab_mut().status = format!("Inpainting {} selection(s) (multi)...", data.len());
    Task::perform(
        async move {
            eprintln!("[manual::multi] spawn_blocking run_inpaint_selection");
            let r = tokio::task::spawn_blocking(move || run_inpaint_selection(&engine, data))
                .await
                .unwrap_or_else(|e| Err(format!("inpaint multi task cancelled: {e}")));
            eprintln!("[manual::multi] spawn_blocking done is_ok={}", r.is_ok());
            if let Err(e) = &r { eprintln!("[manual::multi] error: {}", e); }
            r
        },
        move |res| Message::Tab(tid, crate::app::TabMessage::ManualMultiInpaintFinished(res)),
    )
}

#[cfg(feature = "inpaint")]
fn run_inpaint_selection(engine: &InpaintEngine, data: Vec<(usize, String, [f32; 4], Vec<Quad>)>) -> Result<Vec<(usize, Vec<(image::RgbaImage, [f32; 4], Option<Quad>)>)>, String> {
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
    let mut per_image: HashMap<usize, Vec<(image::RgbaImage, [f32;4], Option<Quad>)>> = HashMap::new();
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
            let (side, sx_u, sy_u) = scanlateit_inpaint::manual_square_params(s.x0, s.y0, s.w, s.h, s.img_w, s.img_h);
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
                Err(e) => { eprintln!("[manual::multi]   oversized run_on_image failed: {}", e); return Err(e); }
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
                let pw = (orig_w.round() as u32).max(1).min(patch_img.width());
                let ph = (orig_h.round() as u32).max(1).min(patch_img.height());
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
            min_x: u32,
            max_x: u32,
            min_y: u32,
            max_y: u32,
            w_span: u32,
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
            let w_span = max_x - min_x;
            let h_span = max_y - min_y;
            let cx = (min_x as f32 + max_x as f32)*0.5;
            let cy = (min_y as f32 + max_y as f32)*0.5;
            let img_w = list[0].img_w;
            let img_h = list[0].img_h;
            let path = list[0].path.clone();
            img_groups.push(ImgGroup { idx:*didx, path, img_w, img_h, min_x, max_x, min_y, max_y, w_span, h_span, cx, cy });
            eprintln!("[manual::multi]   img_group idx={} bbox [{},{},{},{}] w_span={} h_span={} cx={:.1} cy={:.1} img={}x{}", didx, min_x, min_y, max_x, max_y, w_span, h_span, cx, cy, img_w, img_h);
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
                        let px = region.get_pixel(sx as u32, sy as u32).clone();
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
                Err(e) => { eprintln!("[manual::multi]   single window run_on_image failed: {}", e); return Err(e); }
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
            let extra_needed = if total_span < CANVAS { CANVAS - total_span } else { 0 };
            eprintln!("[manual::multi]   total_span={} extra_needed={}", total_span, extra_needed);
            // Distribute extra_needed based on avail per image
            // avail per image: top = min_y, bottom = img_h - max_y
            let mut extra_alloc: Vec<u32> = vec![0; img_groups.len()];
            if extra_needed > 0 {
                // Simple distribution: split extra_needed equally, capped by avail
                // First compute total avail
                let avails: Vec<u32> = img_groups.iter().map(|g| (g.min_y + (g.img_h - g.max_y)) ).collect();
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
                // for mapping sels
                min_x: u32,
                max_x: u32,
                min_y: u32,
                max_y: u32,
                cx: f32,
                cy: f32,
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
                eprintln!("[manual::multi]   piece win idx={} h_span={} extra={} top={} bottom={} y_src={} h_src={} x_src={} w_src={} off_y={} min/max [{},{}] [{},{}]", ig.idx, ig.h_span, extra, top_extra, bottom_extra, y_src, h_src, x_src, w_src, off_y, ig.min_x, ig.max_x, ig.min_y, ig.max_y);
                pieces_win.push(PieceWin { idx: ig.idx, path: ig.path.clone(), img_w: ig.img_w, img_h: ig.img_h, x_src, y_src, w_src, h_src, off_y, min_x: ig.min_x, max_x: ig.max_x, min_y: ig.min_y, max_y: ig.max_y, cx: ig.cx, cy: ig.cy });
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
                            let px = region.get_pixel(sx as u32, y as u32).clone();
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
                        let px = stitched.get_pixel(x, src_y as u32).clone();
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
                Err(e) => { eprintln!("[manual::multi]   stitch run_on_image failed: {}", e); return Err(e); }
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
    let mut out: Vec<(usize, Vec<(image::RgbaImage, [f32;4], Option<Quad>)>)> = Vec::new();
    for (idx, v) in per_image { out.push((idx, v)); }
    out.sort_by_key(|(idx,_)| *idx);
    Ok(out)
}

#[cfg(feature = "inpaint")]
pub fn handle_inpaint_finished(app: &mut App, result: Result<Vec<(usize, Vec<(image::RgbaImage, [f32;4], Option<Quad>)>)>, String>) -> Task<Message> {
    eprintln!("[manual::multi] handle_inpaint_finished result.is_ok={} inpainting was {}", result.is_ok(), app.active_tab_mut().inpainting);
    app.active_tab_mut().inpainting = false;
    match result {
        Ok(per_image_patches) => {
            eprintln!("[manual::multi] finished per_image len={}", per_image_patches.len());
            for (idx, patches) in &per_image_patches {
                eprintln!("[manual::multi]   idx={} patches={}", idx, patches.len());
                for (pi, (patch, bounds, quad)) in patches.iter().enumerate() {
                    eprintln!("[manual::multi]     patch {}: bounds={:?} patch={}x{} quad={:?}", pi, bounds, patch.width(), patch.height(), quad.map(|q| q.points));
                }
            }
            let mut total=0usize;
            let mut pending_evs = Vec::new();
            for (idx, patches) in per_image_patches {
                let Some(image_id) = app.active_tab_mut().images.get(idx).map(|i| i.image_id) else {
                    eprintln!("[manual::multi]   idx {} no image_id -> skip", idx);
                    continue;
                };
                let Some(image) = app.active_tab_mut().images.get_mut(idx) else {
                    eprintln!("[manual::multi]   idx {} no image -> skip", idx);
                    continue;
                };
                for (patch, bounds, quad) in patches {
                    total+=1;
                    eprintln!("[manual::multi]   pushing layer idx={} bounds={:?} quad={:?} patch={}x{}", idx, bounds, quad.map(|q| q.points), patch.width(), patch.height());
                    let (width,height)=(patch.width(), patch.height());
                    let layer = InpaintLayer { bounds, quad, handle: iced::widget::image::Handle::from_rgba(width,height, bytes::Bytes::from(patch.into_raw())), width, height };
                    image.inpaint.push(layer);
                    pending_evs.push((image_id, bounds, quad));
                }
            }
            for (image_id, bounds, quad) in pending_evs {
                eprintln!("[manual::multi]   add_inpaint_patch image_id={:?} bounds={:?} quad={:?}", image_id, bounds, quad.map(|q| q.points));
                let ev = app.active_tab_mut().project.add_inpaint_patch_with_bounds_and_quad(image_id, bounds, quad);
                crate::app::handle_model_event(app.active_tab_mut(), ev);
            }
            app.active_tab_mut().show_inpaint = true;
            // keep manual mode active but selections already cleared; status update
            app.active_tab_mut().status = format!("Inpainted {total} region(s) (multi).");
            eprintln!("[manual::multi] done total={} show_inpaint=true status={}", total, app.active_tab_mut().status);
        }
        Err(e) => {
            eprintln!("[manual::multi] failed: {}", e);
            app.active_tab_mut().status = format!("Multi inpaint failed: {e}");
        }
    }
    Task::none()
}

#[cfg(feature = "inpaint")]
pub fn handle_inpaint_engine_ready_for(app: &mut App, tab_id: crate::app::tab::TabId, result: Result<InpaintEngine, String>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    match result {
        Ok(engine) => {
            app.engines.inpaint = Some(engine.clone());
            let data = app.tabs[idx].pending_manual_multi.take();
            if let Some(d) = data { return start_inpaint_selection_for(app, tab_id, engine, d); }
            let bg = app.tabs[idx].pending_background_stitch.take();
            if let Some((job, pad, prev, next)) = bg { return start_background_stitch_for(app, tab_id, engine, job, pad, prev, next); }
            Task::none()
        }
        Err(e) => {
            app.tabs[idx].pending_manual_multi = None;
            app.tabs[idx].pending_background_stitch = None;
            app.tabs[idx].status = e.clone();
            // free queue weight for manual inpaint (any backend) on build failure
            let mut freed = false;
            if app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::InpaintTelea).is_some() { freed = true; }
            if app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::InpaintLama).is_some() { freed = true; }
            if app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::InpaintAot).is_some() { freed = true; }
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
pub fn handle_auto_engine_ready_for(app: &mut App, tab_id: crate::app::tab::TabId, backend: InpaintBackend, result: Result<InpaintEngine, String>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    match result {
        Ok(engine) => {
            match backend {
                InpaintBackend::Telea => {
                    app.engines.auto_telea = Some(engine.clone());
                    let jobs = app.tabs[idx].pending_auto_telea_jobs.take();
                    if let Some(j) = jobs { return dispatch_auto_for(app, tab_id, j, InpaintBackend::Telea); }
                }
                InpaintBackend::Lama => {
                    app.engines.auto_lama = Some(engine.clone());
                    let jobs = app.tabs[idx].pending_auto_lama_jobs.take();
                    if let Some(j) = jobs { return dispatch_auto_for(app, tab_id, j, InpaintBackend::Lama); }
                }
                InpaintBackend::Aot => {
                    app.engines.auto_aot = Some(engine.clone());
                    let jobs = app.tabs[idx].pending_auto_aot_jobs.take();
                    if let Some(j) = jobs { return dispatch_auto_for(app, tab_id, j, InpaintBackend::Aot); }
                }
            }
            Task::none()
        }
        Err(e) => {
            match backend {
                InpaintBackend::Telea => app.tabs[idx].pending_auto_telea_jobs = None,
                InpaintBackend::Lama => app.tabs[idx].pending_auto_lama_jobs = None,
                InpaintBackend::Aot => app.tabs[idx].pending_auto_aot_jobs = None,
            }
            app.tabs[idx].status = format!("Auto-inpaint engine failed: {e}");
            #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
            { app.tabs[idx].pipeline_active = false; }
            // free queue weight (build failed) and promote
            let kind = match backend {
                InpaintBackend::Telea => crate::app::queue::EngineKind::InpaintTelea,
                InpaintBackend::Lama => crate::app::queue::EngineKind::InpaintLama,
                InpaintBackend::Aot => crate::app::queue::EngineKind::InpaintAot,
            };
            app.engines.queue.complete(tab_id, kind);
            let promote = crate::app::queue::dispatch_pending(app);
            crate::app::queue::refresh_queued_statuses(app);
            return promote;
        }
    }
}
#[cfg(feature = "inpaint")]
pub fn handle_auto_finished_for(app: &mut App, tab_id: crate::app::tab::TabId, index: usize, id: scanlateit_model::EntryId, result: Result<Vec<(usize, image::RgbaImage, [f32; 4], Option<scanlateit_model::Quad>)>, String>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    app.tabs[idx].auto_inpaint_pending = app.tabs[idx].auto_inpaint_pending.saturating_sub(1);
    let pending = app.tabs[idx].auto_inpaint_pending;
    match result {
        Ok(patches) => {
            // apply patches to this tab
            let mut pending_evs: Vec<(scanlateit_model::ImageId, [f32;4], Option<scanlateit_model::Quad>)> = Vec::new();
            let mut affected = std::collections::HashSet::new();
            for (target_idx, patch, bounds, quad) in patches {
                let image_id_opt = app.tabs[idx].images.get(target_idx).map(|i| i.image_id);
                if let Some(image_id) = image_id_opt {
                    if let Some(image) = app.tabs[idx].images.get_mut(target_idx) {
                        let (w,h) = (patch.width(), patch.height());
                        let layer = InpaintLayer { bounds, quad, handle: iced::widget::image::Handle::from_rgba(w,h, bytes::Bytes::from(patch.into_raw())), width: w, height: h };
                        image.inpaint.push(layer);
                        pending_evs.push((image_id, bounds, quad));
                        affected.insert(target_idx);
                    }
                }
            }
            for (image_id, bounds, quad) in pending_evs {
                let ev = app.tabs[idx].project.add_inpaint_patch_with_bounds_and_quad(image_id, bounds, quad);
                crate::app::handle_model_event(&mut app.tabs[idx], ev);
            }
            if !affected.is_empty() { app.tabs[idx].show_inpaint = true; }
            if pending == 0 {
                #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
                { app.tabs[idx].pipeline_active = false; }
                let s = app.tabs[idx].status.clone();
                app.tabs[idx].status = format!("Auto-inpaint done. {}", s);
                // free queue weight for InpaintTelea (per-region batch uses telea weight)
                app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::InpaintTelea);
                // also try other backends if mistakenly reserved (e.g., mixed)
                app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::InpaintLama);
                app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::InpaintAot);
                let promote = crate::app::queue::dispatch_pending(app);
                crate::app::queue::refresh_queued_statuses(app);
                return promote;
            } else {
                let s = app.tabs[idx].status.clone();
                app.tabs[idx].status = format!("Auto-inpaint: {} remaining. {}", pending, s);
            }
        }
        Err(e) => {
            app.tabs[idx].status = format!("Auto-inpaint failed for {index}:{id:?}: {e}");
            if pending == 0 {
                #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
                { app.tabs[idx].pipeline_active = false; }
                app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::InpaintTelea);
                app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::InpaintLama);
                app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::InpaintAot);
                let promote = crate::app::queue::dispatch_pending(app);
                crate::app::queue::refresh_queued_statuses(app);
                return promote;
            }
        }
    }
    Task::none()
}
#[cfg(feature = "inpaint")]
pub fn handle_auto_batch_for(app: &mut App, tab_id: crate::app::tab::TabId, batch: Vec<(usize, scanlateit_model::EntryId, Result<Vec<(usize, image::RgbaImage, [f32; 4], Option<scanlateit_model::Quad>)>, String>)>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    for (_index, id, result) in batch {
        app.tabs[idx].auto_inpaint_pending = app.tabs[idx].auto_inpaint_pending.saturating_sub(1);
        match result {
            Ok(patches) => {
                let mut pending_evs: Vec<(scanlateit_model::ImageId, [f32;4], Option<scanlateit_model::Quad>)> = Vec::new();
                let mut affected = std::collections::HashSet::new();
                for (target_idx, patch, bounds, quad) in patches {
                    if let Some(image_id) = app.tabs[idx].images.get(target_idx).map(|i| i.image_id) {
                        if let Some(image) = app.tabs[idx].images.get_mut(target_idx) {
                            let (w,h) = (patch.width(), patch.height());
                            let layer = InpaintLayer { bounds, quad, handle: iced::widget::image::Handle::from_rgba(w,h, bytes::Bytes::from(patch.into_raw())), width: w, height: h };
                            image.inpaint.push(layer);
                            pending_evs.push((image_id, bounds, quad));
                            affected.insert(target_idx);
                        }
                    }
                }
                for (image_id, bounds, quad) in pending_evs {
                    let ev = app.tabs[idx].project.add_inpaint_patch_with_bounds_and_quad(image_id, bounds, quad);
                    crate::app::handle_model_event(&mut app.tabs[idx], ev);
                }
                if !affected.is_empty() { app.tabs[idx].show_inpaint = true; }
            }
            Err(e) => { app.tabs[idx].status = format!("Auto-inpaint batch failed for {_index}:{id:?}: {e}"); }
        }
    }
    if app.tabs[idx].auto_inpaint_pending == 0 {
        #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
        { app.tabs[idx].pipeline_active = false; }
        let s = app.tabs[idx].status.clone();
        app.tabs[idx].status = format!("Auto-inpaint batch done. {}", s);
        // free whichever inpaint backend was running (lama/aot)
        let mut freed = false;
        if app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::InpaintLama).is_some() { freed = true; }
        if app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::InpaintAot).is_some() { freed = true; }
        if app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::InpaintTelea).is_some() { freed = true; }
        if freed {
            let promote = crate::app::queue::dispatch_pending(app);
            crate::app::queue::refresh_queued_statuses(app);
            return promote;
        }
    }
    Task::none()
}
#[cfg(feature = "inpaint")]
pub fn handle_inpaint_finished_for(app: &mut App, tab_id: crate::app::tab::TabId, result: Result<Vec<(usize, Vec<(image::RgbaImage, [f32;4], Option<scanlateit_model::Quad>)>)>, String>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    app.tabs[idx].inpainting = false;
    match result {
        Ok(per_image_patches) => {
            let mut total=0usize;
            let mut pending_evs = Vec::new();
            for (idx2, patches) in per_image_patches {
                let image_id_opt = app.tabs[idx].images.get(idx2).map(|i| i.image_id);
                if image_id_opt.is_none() { continue; }
                let image_id = image_id_opt.unwrap();
                if let Some(image) = app.tabs[idx].images.get_mut(idx2) {
                    for (patch, bounds, quad) in patches {
                        total+=1;
                        let (w,h)=(patch.width(), patch.height());
                        let layer = InpaintLayer { bounds, quad, handle: iced::widget::image::Handle::from_rgba(w,h, bytes::Bytes::from(patch.into_raw())), width: w, height: h };
                        image.inpaint.push(layer);
                        pending_evs.push((image_id, bounds, quad));
                    }
                }
            }
            for (image_id, bounds, quad) in pending_evs {
                let ev = app.tabs[idx].project.add_inpaint_patch_with_bounds_and_quad(image_id, bounds, quad);
                crate::app::handle_model_event(&mut app.tabs[idx], ev);
            }
            app.tabs[idx].show_inpaint = true;
            app.tabs[idx].status = format!("Inpainted {total} region(s) (multi).");
        }
        Err(e) => { app.tabs[idx].status = format!("Multi inpaint failed: {e}"); }
    }
    // Free queue weight for manual inpaint (any backend) and promote via backfill
    let mut freed = false;
    if app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::InpaintTelea).is_some() { freed = true; }
    if app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::InpaintLama).is_some() { freed = true; }
    if app.engines.queue.complete(tab_id, crate::app::queue::EngineKind::InpaintAot).is_some() { freed = true; }
    if freed {
        let promote = crate::app::queue::dispatch_pending(app);
        crate::app::queue::refresh_queued_statuses(app);
        return promote;
    }
    Task::none()
}
#[cfg(feature = "inpaint")]
pub fn dispatch_auto_for(app: &mut App, tab_id: crate::app::tab::TabId, jobs: Vec<AutoInpaintJob>, backend: InpaintBackend) -> Task<Message> {
    if jobs.is_empty() { return Task::none(); }
    // queue gate — weights 1/4/3 backfill + priority (cap 5)
    {
        use crate::app::queue::{AcquireResult, EngineKind};
        let kind = match backend {
            InpaintBackend::Telea => EngineKind::InpaintTelea,
            InpaintBackend::Lama => EngineKind::InpaintLama,
            InpaintBackend::Aot => EngineKind::InpaintAot,
        };
        let already_reserved = app.engines.queue.running_for(tab_id, kind).is_some();
        if !already_reserved {
            match app.engines.queue.try_acquire_or_enqueue(tab_id, kind) {
                AcquireResult::Acquired(_) => {},
                AcquireResult::Queued(_, pos) => {
                    let idx_tmp = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i)=>i, None=>return Task::none() };
                    // store pending for later dispatch_pending
                    match backend {
                        InpaintBackend::Telea => app.tabs[idx_tmp].pending_auto_telea_jobs = Some(jobs),
                        InpaintBackend::Lama => app.tabs[idx_tmp].pending_auto_lama_jobs = Some(jobs),
                        InpaintBackend::Aot => app.tabs[idx_tmp].pending_auto_aot_jobs = Some(jobs),
                    }
                    app.tabs[idx_tmp].status = format!("Queued {} (pos {}, pool {}/{}) ...", kind.label(), pos, app.engines.queue.used_weight(), crate::app::queue::POOL_CAPACITY);
                    return Task::none();
                }
            }
        }
    }
    let radius = scanlateit_settings::get(|s| s.inpaint_radius.parse::<i32>().unwrap_or(5).max(1));
    let pad = auto_pad_for(backend, radius);
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    let cached: Option<InpaintEngine> = match backend {
        InpaintBackend::Telea => app.engines.auto_telea.clone().filter(|e| e.radius() == radius),
        InpaintBackend::Lama => app.engines.auto_lama.clone().filter(|e| e.radius() == radius),
        InpaintBackend::Aot => app.engines.auto_aot.clone().filter(|e| e.radius() == radius),
    };
    if let Some(engine) = cached {
        app.tabs[idx].auto_inpaint_pending += jobs.len();
        match backend {
            InpaintBackend::Telea => {
                app.tabs[idx].status = format!("Auto-inpaint (Telea) {} regions in parallel...", jobs.len());
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
                            let res = tokio::task::spawn_blocking(move || run_auto_job_with_stitch(&engine, &job, pad, prev_path.as_deref(), next_path.as_deref())).await.unwrap_or_else(|e| Err(format!("inpaint task cancelled: {e}")));
                            (jidx, jid, res)
                        },
                        move |(jidx, jid, res)| Message::Tab(tid, crate::app::TabMessage::AutoInpaintFinished(jidx, jid, res)),
                    )
                }).collect();
                Task::batch(tasks)
            }
            _ => {
                let label = match backend { InpaintBackend::Lama => "LaMa", InpaintBackend::Aot => "AOT-GAN", _=> unreachable!()};
                app.tabs[idx].status = format!("Auto-inpaint ({label}) {} regions sequentially...", jobs.len());
                let tab = &app.tabs[idx];
                let enriched: Vec<(AutoInpaintJob, Option<String>, Option<String>)> = jobs.into_iter().map(|job| {
                    let prev = if job.index>0 { tab.images.get(job.index-1).and_then(|img| tab.project.image(img.image_id).map(|m| m.path.clone())) } else { None };
                    let next = if job.index+1 < tab.images.len() { tab.images.get(job.index+1).and_then(|img| tab.project.image(img.image_id).map(|m| m.path.clone())) } else { None };
                    (job, prev, next)
                }).collect();
                let is_lama = backend == InpaintBackend::Lama;
                let tid = tab_id;
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            let mut out: Vec<(usize, scanlateit_model::EntryId, Result<Vec<(usize, image::RgbaImage, [f32;4], Option<scanlateit_model::Quad>)>, String>)> = Vec::new();
                            for (job, prev_path, next_path) in enriched {
                                let r = run_auto_job_with_stitch(&engine, &job, pad, prev_path.as_deref(), next_path.as_deref());
                                out.push((job.index, job.id, r));
                            }
                            out
                        }).await.unwrap_or_else(|e| { let msg = if is_lama { format!("lama batch cancelled: {e}") } else { format!("aot batch cancelled: {e}") }; vec![(0, scanlateit_model::EntryId(0), Err(msg))] })
                    },
                    move |batch| if is_lama { Message::Tab(tid, crate::app::TabMessage::AutoInpaintLamaBatchFinished(batch)) } else { Message::Tab(tid, crate::app::TabMessage::AutoInpaintAotBatchFinished(batch)) },
                )
            }
        }
    } else {
        match backend {
            InpaintBackend::Telea => app.tabs[idx].pending_auto_telea_jobs = Some(jobs),
            InpaintBackend::Lama => app.tabs[idx].pending_auto_lama_jobs = Some(jobs),
            InpaintBackend::Aot => app.tabs[idx].pending_auto_aot_jobs = Some(jobs),
        }
        app.tabs[idx].status = match backend { InpaintBackend::Telea => "Loading Telea for auto-inpaint...".to_string(), InpaintBackend::Lama => "Loading LaMa for auto-inpaint...".to_string(), InpaintBackend::Aot => "Loading AOT-GAN for auto-inpaint...".to_string()};
        let tid = tab_id;
        Task::perform(async move { InpaintEngine::build(backend, radius) }, move |r| Message::Tab(tid, crate::app::TabMessage::AutoInpaintEngineReady(backend, r)))
    }
}
#[cfg(feature = "inpaint")]
pub fn dispatch_auto_solo_for(app: &mut App, tab_id: crate::app::tab::TabId, effective_model: scanlateit_settings::AutoInpaintModel) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    let mut jobs: Vec<AutoInpaintJob> = Vec::new();
    {
        let tab = &app.tabs[idx];
        for (index, image) in tab.images.iter().enumerate() {
            let image_id = image.image_id;
            let path = tab.project.image(image_id).map(|m| m.path.clone()).unwrap_or_default();
            for entry in tab.project.visible_for(image_id).collect::<Vec<_>>() {
                jobs.push(AutoInpaintJob { index, id: entry.id, path: path.clone(), quad: tab.project.view_quad(entry) });
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
    let backend = match effective_model { scanlateit_settings::AutoInpaintModel::Telea=>InpaintBackend::Telea, scanlateit_settings::AutoInpaintModel::Lama=>InpaintBackend::Lama, scanlateit_settings::AutoInpaintModel::Aot=>InpaintBackend::Aot, scanlateit_settings::AutoInpaintModel::Mixed=>InpaintBackend::Telea };
    dispatch_auto_for(app, tab_id, jobs, backend)
}
#[cfg(feature = "inpaint")]
pub(crate) fn start_inpaint_selection_for(app: &mut App, tab_id: crate::app::tab::TabId, engine: InpaintEngine, data: Vec<(usize, String, [f32;4], Vec<scanlateit_model::Quad>)>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    app.tabs[idx].inpainting = true;
    app.tabs[idx].status = "inpainting...".to_string();
    let tid = tab_id;
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                let mut out: std::collections::HashMap<usize, Vec<(image::RgbaImage, [f32;4], Option<scanlateit_model::Quad>)>> = std::collections::HashMap::new();
                for (idx, path, rect, quads) in data {
                    let res = engine.run_blocking(&path, rect, &quads)?;
                    for (img,b,q) in res { out.entry(idx).or_default().push((img,b,q)); }
                }
                let mut vec: Vec<(usize, Vec<(image::RgbaImage, [f32;4], Option<scanlateit_model::Quad>)>)> = out.into_iter().collect();
                vec.sort_by_key(|(i,_)| *i);
                Ok(vec)
            }).await.unwrap_or_else(|e| Err(format!("inpaint task cancelled: {e}")))
        },
        move |res| Message::Tab(tid, crate::app::TabMessage::ManualMultiInpaintFinished(res)),
    )
}
#[cfg(feature = "inpaint")]
pub(crate) fn start_background_stitch_for(app: &mut App, tab_id: crate::app::tab::TabId, engine: InpaintEngine, job: AutoInpaintJob, pad: f32, prev: Option<String>, next: Option<String>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    app.tabs[idx].inpainting = true;
    app.tabs[idx].status = "inpainting background (stitched)...".to_string();
    let tid = tab_id;
    Task::perform(
        async move {
            let result = tokio::task::spawn_blocking(move || run_auto_job_with_stitch(&engine, &job, pad, prev.as_deref(), next.as_deref())).await.unwrap_or_else(|e| Err(format!("inpaint task cancelled: {e}")));
            let grouped: Result<Vec<(usize, Vec<(image::RgbaImage, [f32;4], Option<scanlateit_model::Quad>)>)>, String> = result.map(|v| {
                let mut map: std::collections::HashMap<usize, Vec<(image::RgbaImage, [f32;4], Option<scanlateit_model::Quad>)>> = std::collections::HashMap::new();
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
