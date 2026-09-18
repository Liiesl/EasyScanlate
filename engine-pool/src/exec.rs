//! UI-agnostic execution helpers (no `iced`, no `App`).
//!
//! Pure `spawn_blocking` bodies moved here from `src/app/{ocr,inpaint,segment}.rs`
//! so the pool owns exec logic. The app builds explicit payloads from `Tab`
//! state, calls these from `tokio::task::spawn_blocking`, and maps results
//! back into `TabMessage`. All rects are plain `[x, y, w, h]` / `[x0,y0,x1,y1]`
//! arrays — never `iced::Rectangle`.

use crate::job::InpaintAutoJob;

/// Telea/Harmonic context pad vs ONNX/ShiftMap context pad (was
/// `auto_pad_for` in `app::inpaint`). Harmonic diffusion is local like
/// Telea, so it shares the `radius` pad; ShiftMap uses the 32px real-pixel
/// expansion like the ONNX backends.
#[cfg(feature = "inpaint")]
pub fn inpaint_pad_for(
    backend: easyscanlate_settings::InpaintBackend,
    radius: i32,
) -> f32 {
    match backend {
        easyscanlate_settings::InpaintBackend::Telea
        | easyscanlate_settings::InpaintBackend::Harmonic => radius as f32,
        _ => 32.0,
    }
}

/// One finished auto-inpaint job: per-target patches.
/// `(target_index, rgba_patch, [x, y, w, h], quad)`.
#[cfg(feature = "inpaint")]
pub type AutoInpaintPatches =
    Vec<(usize, image::RgbaImage, [f32; 4], Option<easyscanlate_model::Quad>)>;

/// Runs one auto-inpaint job with seam-aware 512 stitching (single-quad jobs
/// that touch a page seam pull context from the neighbor page).
///
/// Moved verbatim from `src/app/inpaint.rs::run_auto_job_with_stitch`; only the
/// job type changed (`tab::AutoInpaintJob` -> [`InpaintAutoJob]`) and logging kept.
#[cfg(feature = "inpaint")]
pub fn run_auto_inpaint_job(
    engine: &easyscanlate_inpaint::Engine,
    job: &InpaintAutoJob,
    pad: f32,
    prev_path: Option<&str>,
    next_path: Option<&str>,
) -> Result<AutoInpaintPatches, String> {
    use easyscanlate_model::Quad;
    use image::RgbaImage;

    type PatchMap = std::collections::HashMap<usize, Vec<(RgbaImage, [f32; 4], Option<Quad>)>>;

    const STITCH_W: u32 = 512;
    const STITCH_H: u32 = 512;

    let [x0, y0, x1, y1] = job.quad.bounds();
    let rect = [x0, y0, x1 - x0, y1 - y0];
    let main_rgba = image::ImageReader::open(&job.path)
        .map_err(|e| format!("Failed to open {}: {e}", job.path))?
        .with_guessed_format()
        .map_err(|e| format!("Failed to decode {}: {e}", job.path))?
        .decode()
        .map_err(|e| format!("Failed to decode {}: {e}", job.path))?
        .into_rgba8();
    let (img_w, img_h) = main_rgba.dimensions();
    let img_h_f = img_h as f32;
    let min_y = job.quad.points.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
    let max_y = job.quad.points.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max);
    let min_x = job.quad.points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
    let max_x = job.quad.points.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max);
    let need_top = if min_y < pad && prev_path.is_some() { pad - min_y } else { 0.0 };
    let need_bottom = if max_y > img_h_f - pad && next_path.is_some() { max_y + pad - img_h_f } else { 0.0 };
    eprintln!(
        "[auto-inpaint::job] idx={} id={:?} path={} quad_pts={:?} quad_bounds=[{:.1},{:.1},{:.1},{:.1}] rect={:?} img={}x{} pad={} min/max_x=[{:.1},{:.1}] min/max_y=[{:.1},{:.1}] need_top={:.1} need_bottom={:.1} prev={} next={}",
        job.index,
        job.id,
        job.path,
        job.quad.points,
        x0,
        y0,
        x1,
        y1,
        rect,
        img_w,
        img_h,
        pad,
        min_x,
        max_x,
        min_y,
        max_y,
        need_top,
        need_bottom,
        prev_path.is_some(),
        next_path.is_some(),
    );
    if need_top <= 0.0 && need_bottom <= 0.0 {
        {
            let ix0 = x0.max(0.0);
            let iy0 = y0.max(0.0);
            let ix1 = x1.min(img_w as f32);
            let iy1 = y1.min(img_h_f);
            if ix1 <= ix0 || iy1 <= iy0 {
                eprintln!(
                    "[auto-inpaint::quad-outside-image] idx={} id={:?} quad_bounds=[{:.1},{:.1},{:.1},{:.1}] img={}x{} inter=[{:.1},{:.1},{:.1},{:.1}] -> direct run will miss (expected global-split reassignment upstream)",
                    job.index, job.id, x0, y0, x1, y1, img_w, img_h, ix0, iy0, ix1, iy1,
                );
            }
        }
        eprintln!(
            "[auto-inpaint::direct] idx={} id={:?} path={} rect={:?} img={}x{} (no stitch)",
            job.index, job.id, job.path, rect, img_w, img_h,
        );
        let v = engine.run_blocking(&job.path, rect, &[job.quad])?;
        eprintln!(
            "[auto-inpaint::direct] idx={} id={:?} -> {} patch(es) bounds={:?}",
            job.index,
            job.id,
            v.len(),
            v.iter().map(|(_, b, _)| *b).collect::<Vec<_>>(),
        );
        return Ok(v.into_iter().map(|(img, b, q)| (job.index, img, b, q)).collect());
    }
    {
        // Explicit outside-image diagnosis: the global-split in the app layer
        // should already have reassigned fully-outside quads, so reaching here
        // with an inverted expanded crop means a stale job slipped through.
        let ix0 = x0.max(0.0);
        let iy0 = y0.max(0.0);
        let ix1 = x1.min(img_w as f32);
        let iy1 = y1.min(img_h_f);
        if ix1 <= ix0 || iy1 <= iy0 {
            eprintln!(
                "[auto-inpaint::quad-outside-image] idx={} id={:?} quad_bounds=[{:.1},{:.1},{:.1},{:.1}] img={}x{} inter=[{:.1},{:.1},{:.1},{:.1}] -> stitch attempted but quad misses owning image (expected global-split reassignment upstream)",
                job.index, job.id, x0, y0, x1, y1, img_w, img_h, ix0, iy0, ix1, iy1,
            );
        }
    }
    let exp_x0 = (rect[0] - pad).max(0.0);
    let exp_y0 = (rect[1] - pad).max(0.0);
    let exp_x1 = (rect[0] + rect[2] + pad).min(img_w as f32);
    let exp_y1 = (rect[1] + rect[3] + pad).min(img_h as f32);
    let exp_w = (exp_x1 - exp_x0).max(1.0) as u32;
    let exp_h_main = (exp_y1 - exp_y0).max(1.0) as u32;

    struct Raw {
        idx: usize,
        full: RgbaImage,
        img_w: u32,
        img_h: u32,
        orig: [u32; 4],
        quads: Vec<Quad>,
    }
    let mut raws: Vec<Raw> = Vec::new();
    let main_idx = job.index;

    let decode = |p: &str| -> Option<RgbaImage> {
        image::ImageReader::open(p)
            .ok()?
            .with_guessed_format()
            .ok()?
            .decode()
            .ok()
            .map(|d| d.into_rgba8())
    };

    let mut neighbor_idx_prev: Option<usize> = None;
    let mut neighbor_idx_next: Option<usize> = None;
    if need_top > 0.0 && prev_path.is_some() {
        neighbor_idx_prev = if main_idx > 0 { Some(main_idx - 1) } else { Some(main_idx) };
    }
    if need_bottom > 0.0 && next_path.is_some() {
        neighbor_idx_next = Some(main_idx + 1);
    }

    if need_top > 0.0
        && let Some(pp) = prev_path
            && let Some(prev_rgba) = decode(pp) {
                let (pw, ph) = prev_rgba.dimensions();
                let take_h = (need_top as u32).min(ph);
                if take_h > 0 {
                    let w_take = exp_w.min(pw);
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
    {
        raws.push(Raw {
            idx: main_idx,
            full: main_rgba,
            img_w,
            img_h,
            orig: [exp_x0 as u32, exp_y0 as u32, exp_w, exp_h_main],
            quads: vec![job.quad],
        });
    }
    if need_bottom > 0.0
        && let Some(np) = next_path
            && let Some(next_rgba) = decode(np) {
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
    raws.sort_by_key(|r| r.idx);
    eprintln!(
        "[auto-inpaint::raws] idx={} id={:?} raws={} main_idx={} exp=[{:.1},{:.1},{:.1},{:.1}] exp_w={} exp_h_main={} details={:?}",
        job.index,
        job.id,
        raws.len(),
        main_idx,
        exp_x0,
        exp_y0,
        exp_x1,
        exp_y1,
        exp_w,
        exp_h_main,
        raws
            .iter()
            .map(|r| (r.idx, r.img_w, r.img_h, r.orig, r.quads.len()))
            .collect::<Vec<_>>(),
    );
    if raws.is_empty() || raws.len() == 1 {
        eprintln!(
            "[auto-inpaint::raws-fallback] idx={} id={:?} raws={} -> direct run_blocking",
            job.index,
            job.id,
            raws.len(),
        );
        let v = engine.run_blocking(&job.path, rect, &[job.quad])?;
        eprintln!(
            "[auto-inpaint::raws-fallback] idx={} id={:?} -> {} patch(es)",
            job.index,
            job.id,
            v.len(),
        );
        return Ok(v.into_iter().map(|(img, b, q)| (job.index, img, b, q)).collect());
    }

    #[allow(dead_code)]
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
    } else {
        // 3+ raws: proportional fill (same as app version, condensed).
        let total_raw: i32 = raws.iter().map(|r| r.orig[3] as i32).sum();
        let mut hs: Vec<i32> = if total_raw >= STITCH_H as i32 {
            raws.iter().map(|r| ((STITCH_H as f32 * r.orig[3] as f32 / total_raw as f32).round() as i32).max(1)).collect()
        } else {
            raws.iter().map(|r| r.orig[3] as i32).collect()
        };
        let sum: i32 = hs.iter().sum();
        if sum != STITCH_H as i32 && !hs.is_empty() {
            let last = hs.len() - 1;
            hs[last] += STITCH_H as i32 - sum;
        }
        let mut off_y = 0u32;
        for (r, h_src) in raws.iter().zip(hs) {
            let h_src = h_src.max(1).min(STITCH_H as i32) as u32;
            let w_src = STITCH_W.min(r.img_w);
            let center_x = r.orig[0] as f32 + r.orig[2] as f32 * 0.5;
            let mut x_src = (center_x - w_src as f32 * 0.5).round() as i32;
            x_src = x_src.clamp(0, r.img_w as i32 - w_src as i32).max(0);
            let y_src = (r.orig[1] as i32).clamp(0, r.img_h as i32 - h_src as i32).max(0);
            pieces.push(Piece { idx: r.idx, orig: r.orig, x_src, y_src, w_src, h_src, off_y, quads: r.quads.clone() });
            off_y += h_src;
        }
    }

    let mut stitched = RgbaImage::new(STITCH_W, STITCH_H);
    for p in &pieces {
        let src = &raws.iter().find(|r| r.idx == p.idx).unwrap().full;
        let crop = image::imageops::crop_imm(src, p.x_src as u32, p.y_src as u32, p.w_src, p.h_src).to_image();
        let mut placed = crop;
        if p.w_src < STITCH_W {
            let mut full_w = RgbaImage::new(STITCH_W, p.h_src);
            image::imageops::replace(&mut full_w, &placed, 0, 0);
            let remaining = STITCH_W - p.w_src;
            if remaining > 0 {
                for y in 0..p.h_src {
                    for x in 0..remaining {
                        let src_x = (p.w_src as i32 - 1 - (x as i32 % p.w_src as i32)).max(0) as u32;
                        let px = *placed.get_pixel(src_x, y);
                        full_w.put_pixel(p.w_src + x, y, px);
                    }
                }
            }
            placed = full_w;
        }
        image::imageops::replace(&mut stitched, &placed, 0, p.off_y as i64);
    }

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
    eprintln!(
        "[auto-inpaint::pieces] idx={} id={:?} pieces={} details={:?}",
        job.index,
        job.id,
        pieces.len(),
        pieces
            .iter()
            .map(|p| (p.idx, p.orig, p.x_src, p.y_src, p.w_src, p.h_src, p.off_y, p.quads.len()))
            .collect::<Vec<_>>(),
    );
    eprintln!(
        "[auto-inpaint::quads-stitched] idx={} id={:?} n={} pts={:?} bounds={:?} piece={:?}",
        job.index,
        job.id,
        quads_stitched.len(),
        quads_stitched.iter().map(|q| q.points).collect::<Vec<_>>(),
        quads_stitched.iter().map(|q| q.bounds()).collect::<Vec<_>>(),
        quad_piece,
    );
    if quads_stitched.is_empty() {
        eprintln!(
            "[auto-inpaint::empty-stitched] idx={} id={:?} no quads on pieces -> direct run_blocking rect={:?}",
            job.index, job.id, rect,
        );
        let v = engine.run_blocking(&job.path, rect, &[job.quad])?;
        eprintln!(
            "[auto-inpaint::empty-stitched] idx={} id={:?} -> {} patch(es)",
            job.index,
            job.id,
            v.len(),
        );
        return Ok(v.into_iter().map(|(img, b, q)| (job.index, img, b, q)).collect());
    }
    let main_piece = pieces.iter().find(|p| p.idx == main_idx).unwrap();
    let rect_stitched = [
        rect[0] - main_piece.x_src as f32,
        rect[1] - main_piece.y_src as f32 + main_piece.off_y as f32,
        rect[2],
        rect[3],
    ];
    eprintln!(
        "[auto-inpaint::stitched-run] idx={} id={:?} main_piece idx={} x_src={} y_src={} off_y={} rect={:?} -> rect_stitched={:?} stitched=512x512",
        job.index, job.id, main_piece.idx, main_piece.x_src, main_piece.y_src, main_piece.off_y, rect, rect_stitched,
    );
    let patches = engine.run_on_image(&stitched, rect_stitched, &quads_stitched)?;
    eprintln!(
        "[auto-inpaint::stitched-result] idx={} id={:?} -> {} raw patch(es) bounds={:?}",
        job.index,
        job.id,
        patches.len(),
        patches.iter().map(|(_, b, _)| *b).collect::<Vec<_>>(),
    );
    let mut per_image: PatchMap = std::collections::HashMap::new();
    for (idx, (patch_img, bounds_stitched, quad_opt)) in patches.into_iter().enumerate() {
        let [bx, by, bw, bh] = bounds_stitched;
        let patch_y1 = by + bh;
        // Geometric divide: one stitched bbox may span the seam, so intersect
        // it with every piece's stitched interval and emit one per-page patch
        // per intersected piece. Single jobs can therefore yield multi-target
        // patches while stitch assembly above stays unchanged.
        let mut emitted_any = false;
        for p in &pieces {
            let py0 = p.off_y as f32;
            let py1 = py0 + p.h_src as f32;
            let oy0 = by.max(py0);
            let oy1 = patch_y1.min(py1);
            if oy1 <= oy0 {
                continue;
            }
            let sy0 = (oy0 - by).round().max(0.0) as u32;
            let mut sh = (oy1 - oy0).round().max(0.0) as u32;
            if sh == 0 || sy0 >= patch_img.height() {
                continue;
            }
            sh = sh.min(patch_img.height().saturating_sub(sy0));
            if sh == 0 {
                continue;
            }
            let slice_img = if sy0 != 0 || sh != patch_img.height() || patch_img.width() == 0 {
                image::imageops::crop_imm(&patch_img, 0, sy0, patch_img.width(), sh).to_image()
            } else {
                patch_img.clone()
            };
            let orig_x = bx + p.x_src as f32;
            let orig_y = (oy0 - p.off_y as f32) + p.y_src as f32;
            let (img_w_f, img_h_f) = {
                let r = raws.iter().find(|r| r.idx == p.idx).unwrap();
                (r.img_w as f32, r.img_h as f32)
            };
            let clip_x0 = orig_x.max(0.0);
            let clip_y0 = orig_y.max(0.0);
            let clip_x1 = (orig_x + bw).min(img_w_f);
            let clip_y1 = (orig_y + sh as f32).min(img_h_f);
            eprintln!(
                "[auto-inpaint::map] idx={} id={:?} patch#{} stitched_bounds=[{:.1},{:.1},{:.1},{:.1}] slice=[{:.1},{:.1}] -> piece idx={} x_src={} y_src={} off_y={} orig=[{:.1},{:.1},{:.1},{:.1}] clip=[{:.1},{:.1},{:.1},{:.1}] img={:.0}x{:.0}",
                job.index, job.id, idx, bx, by, bw, bh, oy0, oy1, p.idx, p.x_src, p.y_src, p.off_y, orig_x, orig_y, bw, sh as f32, clip_x0, clip_y0, clip_x1, clip_y1, img_w_f, img_h_f,
            );
            if clip_x1 <= clip_x0 || clip_y1 <= clip_y0 {
                eprintln!(
                    "[auto-inpaint::map-skip] idx={} id={:?} patch#{} piece idx={} clipped empty -> skipped",
                    job.index, job.id, idx, p.idx,
                );
                continue;
            }
            let new_w = clip_x1 - clip_x0;
            let new_h = clip_y1 - clip_y0;
            let crop_x = (clip_x0 - orig_x).round().max(0.0) as u32;
            let crop_y = (clip_y0 - orig_y).round().max(0.0) as u32;
            let clipped_patch = if crop_x != 0 || crop_y != 0 || new_w as u32 != slice_img.width() || new_h as u32 != slice_img.height() {
                let cw = (new_w as u32).min(slice_img.width().saturating_sub(crop_x));
                let ch = (new_h as u32).min(slice_img.height().saturating_sub(crop_y));
                if cw == 0 || ch == 0 {
                    eprintln!(
                        "[auto-inpaint::map-skip] idx={} id={:?} patch#{} piece idx={} crop empty cw={} ch={} (crop_x={} crop_y={} patch={}x{}) -> skipped",
                        job.index, job.id, idx, p.idx, cw, ch, crop_x, crop_y, slice_img.width(), slice_img.height(),
                    );
                    continue;
                }
                image::imageops::crop_imm(&slice_img, crop_x, crop_y, cw, ch).to_image()
            } else { slice_img };
            let bounds = [clip_x0, clip_y0, new_w, new_h];
            let orig_quad = quad_opt.map(|q| {
                let mut nq = q;
                for pt in &mut nq.points { pt[0] += p.x_src as f32; pt[1] += p.y_src as f32 - p.off_y as f32; }
                nq
            });
            per_image.entry(p.idx).or_default().push((clipped_patch, bounds, orig_quad));
            emitted_any = true;
        }
        if !emitted_any {
            eprintln!(
                "[auto-inpaint::map-skip] idx={} id={:?} patch#{} stitched_bounds=[{:.1},{:.1},{:.1},{:.1}] no piece intersected -> skipped",
                job.index, job.id, idx, bx, by, bw, bh,
            );
        }
    }
    let mut out: AutoInpaintPatches = Vec::new();
    for (target_idx, vec) in per_image {
        for (img, bounds, quad) in vec {
            out.push((target_idx, img, bounds, quad));
        }
    }
    out.sort_by_key(|(idx, _, _, _)| *idx);
    eprintln!(
        "[auto-inpaint::done] idx={} id={:?} stitched patches={} targets={:?} bounds={:?}",
        job.index,
        job.id,
        out.len(),
        out.iter().map(|(ti, _, _, _)| *ti).collect::<Vec<_>>(),
        out.iter().map(|(_, _, b, _)| *b).collect::<Vec<_>>(),
    );
    if out.is_empty() {
        eprintln!(
            "[auto-inpaint::fallback] idx={} id={:?} stitched empty -> direct run_blocking path={} rect={:?}",
            job.index, job.id, job.path, rect,
        );
        let v = engine.run_blocking(&job.path, rect, &[job.quad])?;
        eprintln!(
            "[auto-inpaint::fallback] idx={} id={:?} -> {} patch(es) bounds={:?}",
            job.index,
            job.id,
            v.len(),
            v.iter().map(|(_, b, _)| *b).collect::<Vec<_>>(),
        );
        return Ok(v.into_iter().map(|(img, b, q)| (job.index, img, b, q)).collect());
    }
    Ok(out)
}

/// One manual OCR selection: image index, path, `[x, y, w, h]` rect.
#[cfg(feature = "ocr")]
pub type ManualOcrItem = (usize, String, [f32; 4]);

/// Runs manual OCR over clustered selections (single + seam-stitched multi).
/// UI-agnostic port of `app::ocr::run_manual_ocr_selection`: rects are plain
/// arrays, no `iced::Rectangle`, no `App`.
#[cfg(feature = "ocr")]
pub fn run_manual_ocr_selection(
    engine: &easyscanlate_ocr::Engine,
    items: Vec<ManualOcrItem>,
    merge_cfg: easyscanlate_ocr::MergeConfig,
) -> Result<Vec<(usize, Vec<easyscanlate_model::NewEntry>)>, String> {
    use std::collections::HashMap;
    use easyscanlate_model::NewEntry;

    let mut by_image: HashMap<usize, Vec<(String, [f32; 4])>> = HashMap::new();
    for (idx, path, rect) in items {
        by_image.entry(idx).or_default().push((path, rect));
    }
    struct Cluster { x0: f32, y0: f32, x1: f32, y1: f32 }
    let mut jobs: Vec<(usize, String, Cluster)> = Vec::new();
    for (idx, rects) in by_image {
        if rects.is_empty() { continue; }
        let path = rects[0].0.clone();
        let mut clusters: Vec<Cluster> = Vec::new();
        for (_, r) in rects {
            let cur = Cluster { x0: r[0], y0: r[1], x1: r[0] + r[2], y1: r[1] + r[3] };
            let mut merged_indices: Vec<usize> = Vec::new();
            for (ci, c) in clusters.iter().enumerate() {
                let touches = !(cur.x1 < c.x0 - 1e-3 || cur.x0 > c.x1 + 1e-3 || cur.y1 < c.y0 - 1e-3 || cur.y0 > c.y1 + 1e-3);
                if touches {
                    merged_indices.push(ci);
                }
            }
            if merged_indices.is_empty() {
                clusters.push(cur);
            } else {
                let mut nx0 = cur.x0;
                let mut ny0 = cur.y0;
                let mut nx1 = cur.x1;
                let mut ny1 = cur.y1;
                merged_indices.sort_by(|a,b| b.cmp(a));
                for mi in merged_indices {
                    let c = clusters.remove(mi);
                    nx0 = nx0.min(c.x0);
                    ny0 = ny0.min(c.y0);
                    nx1 = nx1.max(c.x1);
                    ny1 = ny1.max(c.y1);
                }
                clusters.push(Cluster { x0: nx0, y0: ny0, x1: nx1, y1: ny1 });
            }
        }
        for c in clusters {
            jobs.push((idx, path.clone(), c));
        }
    }
    if jobs.is_empty() { return Err("no OCR jobs".to_string()); }
    let mut decoded: Vec<(usize, String, u32, u32, u32, u32, u32, u32, image::RgbaImage)> = Vec::new();
    for (idx, path, cluster) in jobs {
        let dyn_img = image::ImageReader::open(&path)
            .map_err(|e| format!("Failed to open {path}: {e}"))?
            .with_guessed_format().map_err(|e| format!("Failed to decode {path}: {e}"))?
            .decode().map_err(|e| format!("Failed to decode {path}: {e}"))?;
        let rgba = dyn_img.into_rgba8();
        let (img_w, img_h) = rgba.dimensions();
        let x0 = cluster.x0.floor().max(0.0) as u32;
        let y0 = cluster.y0.floor().max(0.0) as u32;
        let x1 = cluster.x1.ceil().max(x0 as f32 +1.0) as u32;
        let y1 = cluster.y1.ceil().max(y0 as f32 +1.0) as u32;
        let x1 = x1.min(img_w);
        let y1 = y1.min(img_h);
        let cw = x1.saturating_sub(x0).max(1);
        let ch = y1.saturating_sub(y0).max(1);
        let x0 = x0.min(img_w.saturating_sub(1));
        let y0 = y0.min(img_h.saturating_sub(1));
        let cw = cw.min(img_w.saturating_sub(x0).max(1));
        let ch = ch.min(img_h.saturating_sub(y0).max(1));
        let crop = image::imageops::crop_imm(&rgba, x0, y0, cw, ch).to_image();
        decoded.push((idx, path, x0, y0, cw, ch, img_w, img_h, crop));
    }
    if decoded.is_empty() { return Err("no OCR jobs".to_string()); }
    decoded.sort_by(|a, b| a.0.cmp(&b.0).then(a.3.cmp(&b.3)).then(a.2.cmp(&b.2)));
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for i in 0..decoded.len() {
        if i == 0 {
            groups.push(vec![0]);
            continue;
        }
        let prev_i = *groups.last().and_then(|g| g.last()).unwrap_or(&0);
        let (p_idx, _, p_x0, p_y0, p_cw, p_ch, p_img_w, p_img_h, _) = &decoded[prev_i];
        let (c_idx, _, c_x0, c_y0, c_cw, _c_ch, c_img_w, _, _) = &decoded[i];
        let consecutive = *c_idx == *p_idx + 1;
        let mut stitch = false;
        if consecutive && *p_img_w > 0 && *c_img_w > 0 {
            let p_x0n = *p_x0 as f32 / *p_img_w as f32;
            let p_x1n = (*p_x0 + *p_cw) as f32 / *p_img_w as f32;
            let c_x0n = *c_x0 as f32 / *c_img_w as f32;
            let c_x1n = (*c_x0 + *c_cw) as f32 / *c_img_w as f32;
            let overlap = (p_x1n.min(c_x1n) - p_x0n.max(c_x0n)).max(0.0);
            let min_w = (p_x1n - p_x0n).min(c_x1n - c_x0n).max(1e-6);
            let x_overlap = overlap / min_w > 0.5;
            let prev_touches_bottom = (*p_y0 + *p_ch) as i32 >= *p_img_h as i32 - 2;
            let cur_touches_top = *c_y0 as i32 <= 2;
            stitch = x_overlap && prev_touches_bottom && cur_touches_top;
        }
        if stitch {
            if let Some(g) = groups.last_mut() { g.push(i); }
        } else {
            groups.push(vec![i]);
        }
    }
    let mut per_image: HashMap<usize, Vec<NewEntry>> = HashMap::new();
    for g in groups {
        if g.len() == 1 {
            let (idx, _, x0, y0, _, _, _, _, crop_rgba) = &decoded[g[0]];
            let cropped_rgb = image::DynamicImage::ImageRgba8(crop_rgba.clone()).to_rgb8();
            let token = easyscanlate_ocr::OcrCancellationToken::new();
            let lines = engine.run_image_cancellable(&cropped_rgb, &token)
                .map_err(|e| format!("Manual OCR failed: {e}"))?;
            let mut entries = easyscanlate_ocr::to_entries_with(lines, merge_cfg);
            for entry in &mut entries {
                for p in &mut entry.quad.points {
                    p[0] += *x0 as f32;
                    p[1] += *y0 as f32;
                }
            }
            per_image.entry(*idx).or_default().extend(entries);
        } else {
            let common_w = decoded[g[0]].4;
            if common_w == 0 { continue; }
            let mut scaled: Vec<(usize, u32, u32, u32, u32, u32, u32, image::RgbaImage)> = Vec::new();
            let mut total_h: u32 = 0;
            for pi in &g {
                let (idx, _, x0, y0, cw, ch, _, _, crop) = &decoded[*pi];
                let scaled_h = if *cw == common_w {
                    *ch
                } else {
                    ((*ch as f32 * common_w as f32 / *cw as f32).round().max(1.0)) as u32
                };
                let scaled_img = if *cw == common_w {
                    crop.clone()
                } else {
                    image::imageops::resize(crop, common_w, scaled_h, image::imageops::FilterType::Triangle)
                };
                let off_y = total_h;
                total_h += scaled_h;
                scaled.push((*idx, *x0, *y0, *cw, *ch, scaled_h, off_y, scaled_img));
            }
            if total_h == 0 { continue; }
            let mut stitched_rgba = image::RgbaImage::new(common_w, total_h);
            for (_, _, _, _, _, _, off_y, img) in &scaled {
                image::imageops::replace(&mut stitched_rgba, img, 0, *off_y as i64);
            }
            let stitched_rgb = image::DynamicImage::ImageRgba8(stitched_rgba).to_rgb8();
            let token = easyscanlate_ocr::OcrCancellationToken::new();
            let lines = engine.run_image_cancellable(&stitched_rgb, &token)
                .map_err(|e| format!("Manual OCR span failed: {e}"))?;
            let mut entries = easyscanlate_ocr::to_entries_with(lines, merge_cfg);
            for mut entry in entries.drain(..) {
                let ys: Vec<f32> = entry.quad.points.iter().map(|p| p[1]).collect();
                if ys.is_empty() { continue; }
                let y0e = ys.iter().cloned().fold(f32::INFINITY, f32::min);
                let y1e = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                let mut best: usize = 0;
                let mut best_overlap: f32 = -1.0;
                for (pi, (_, _, _, _, _, sh, off, _)) in scaled.iter().enumerate() {
                    let lo = *off as f32;
                    let hi = (*off + *sh) as f32;
                    let overlap = (y1e.min(hi) - y0e.max(lo)).max(0.0);
                    if overlap > best_overlap {
                        best_overlap = overlap;
                        best = pi;
                    }
                }
                if best_overlap <= 0.0 {
                    let yc = ys.iter().sum::<f32>() / ys.len() as f32;
                    for (pi, (_, _, _, _, _, sh, off, _)) in scaled.iter().enumerate() {
                        let lo = *off as f32;
                        let hi = (*off + *sh) as f32;
                        if yc >= lo && yc < hi {
                            best = pi;
                            break;
                        }
                    }
                    if yc >= total_h as f32 {
                        best = scaled.len() - 1;
                    }
                }
                let (t_idx, t_x0, t_y0, t_cw, _, _, t_off, _) = &scaled[best];
                let t_idx = *t_idx;
                let factor = *t_cw as f32 / common_w as f32;
                let lo = *t_off as f32;
                let hi = (*t_off + scaled[best].5) as f32;
                for p in &mut entry.quad.points {
                    let y_clamped = p[1].clamp(lo, hi);
                    let x_mapped = *t_x0 as f32 + p[0] * factor;
                    let y_mapped = *t_y0 as f32 + (y_clamped - *t_off as f32) * factor;
                    *p = [x_mapped, y_mapped];
                }
                per_image.entry(t_idx).or_default().push(entry);
            }
        }
    }
    let mut out: Vec<(usize, Vec<NewEntry>)> = per_image.into_iter().collect();
    out.sort_by_key(|(idx,_)| *idx);
    Ok(out)
}

/// Runs segment-grid SFX filtering for a single grid canvas. Returns this
/// grid's deletions. One grid's detection failure is isolated here so the
/// stream can count it as one failed grid and continue with the rest.
#[cfg(feature = "segment")]
pub fn run_segment_grid(
    engine: &easyscanlate_segment::Engine,
    run: &easyscanlate_segment::grid::GridRun,
    dims: &[(u32, u32)],
    paths: &[String],
    ocr_boxes: &[Vec<([f32; 4], easyscanlate_model::EntryId)>],
) -> Result<Vec<(usize, easyscanlate_model::EntryId)>, String> {
    use easyscanlate_segment::filter::{DetBox, sfx_filter_indexes};
    use easyscanlate_segment::grid::{build_grid_canvas_with_loader, grid_det_to_page};
    use easyscanlate_segment::SegClass;
    let mut loader = |page_idx: usize| -> image::RgbImage {
        let path = match paths.get(page_idx) {
            Some(p) => p,
            None => return image::RgbImage::new(1, 1),
        };
        #[cfg(feature = "ocr")]
        let img = easyscanlate_ocr::load_rgb(path).unwrap_or_else(|| image::RgbImage::new(1, 1));
        #[cfg(not(feature = "ocr"))]
        let img = image::open(path).map(|i| i.to_rgb8()).unwrap_or_else(|_| image::RgbImage::new(1, 1));
        img
    };
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
    let mut to_delete: Vec<(usize, easyscanlate_model::EntryId)> = Vec::new();
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
    Ok(to_delete)
}
