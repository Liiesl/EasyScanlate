#![cfg(feature = "ocr")]

//! App-owned OCR canvas building and assembly.
//!
//! The app builds every OCR canvas (pages stacked at a common width with
//! margin strips from the neighboring content), hands the finished images to
//! the engine, and assembles the merged lines it gets back: distribute them
//! to pages, dedup the current run against the committed model state once the
//! next run has arrived, then commit. The engine never sees margins, bands,
//! or dedup state: pixels in, merged lines out.

use image::{RgbImage, imageops};

use easyscanlate_model::{EntryId, EntrySource, NewEntry, Quad};
use easyscanlate_ocr::{DetectorQuad, OcrLine, OCR_QUAD_PAD, load_rgb};

// ---------------------------------------------------------------------------
// Run planning
// ---------------------------------------------------------------------------

/// Fraction of an OCR run's body height stitched above and below it from the
/// neighboring page content, so speech bubbles cut by the run's boundary stay
/// whole. Resolution-invariant: unlike a fixed pixel margin, it scales with
/// the page.
pub const STITCH_MARGIN_RATIO: f32 = 0.2;

/// Runs whose height/width ratio is below this are stitched with the next
/// pages until the combined ratio reaches it (vertical 2:1).
pub const MIN_ASPECT_RATIO: f32 = 2.0;

/// Runs whose height/width ratio is above this are split into equal chunks of
/// at most this ratio (vertical 6:1).
pub const MAX_ASPECT_RATIO: f32 = 6.0;

/// One OCR run: a contiguous span of page content.
///
/// A run covers pages `page_start..=page_end` stacked at a common width; the
/// `band` is the fraction of that stacked body the run actually OCRs (the
/// whole body `(0.0, 1.0)` for normal runs, a chunk `(c/k, (c+1)/k)` for a
/// split of a too-tall page). Margins are stitched from the bands above and
/// below.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunPlan {
    /// First page index covered (inclusive).
    pub page_start: usize,
    /// Last page index covered (inclusive).
    pub page_end: usize,
    /// Fraction `[0, 1]` of the stacked body this run covers.
    pub band: (f32, f32),
    /// Page whose content sits directly above the band, and that band as
    /// fractions of the page's height; its bottom [`STITCH_MARGIN_RATIO`]
    /// becomes the run's top margin strip.
    pub above: Option<(usize, (f32, f32))>,
    /// Same for the content directly below the band (its top
    /// [`STITCH_MARGIN_RATIO`] becomes the bottom margin strip).
    pub below: Option<(usize, (f32, f32))>,
}

/// Splits a book into OCR runs by aspect ratio (height/width).
///
/// A page above [`MAX_ASPECT_RATIO`] is split into equal chunks of at most
/// that ratio, one run per chunk. A page below [`MIN_ASPECT_RATIO`] is
/// stitched with the following pages until the combined ratio reaches the
/// minimum — stopping before it would exceed the maximum, and never absorbing
/// a page above the maximum, which becomes its own split run. A short run
/// left over at the end of the book is OCR'd as-is.
pub fn plan_runs(dims: &[(u32, u32)]) -> Vec<RunPlan> {
    let ratio = |i: usize| dims[i].1 as f32 / dims[i].0.max(1) as f32;
    let combined = |i: usize, j: usize| {
        let width = dims[i].0.max(1) as f32;
        dims[i..=j]
            .iter()
            .map(|&(w, h)| h as f32 * width / w.max(1) as f32)
            .sum::<f32>()
            / width
    };

    let mut runs = Vec::new();
    let mut i = 0;
    while i < dims.len() {
        if ratio(i) > MAX_ASPECT_RATIO {
            let chunks = (ratio(i) / MAX_ASPECT_RATIO).ceil() as u32;
            for chunk in 0..chunks {
                let band = (chunk as f32 / chunks as f32, (chunk + 1) as f32 / chunks as f32);
                let above = if chunk > 0 {
                    Some((i, ((chunk - 1) as f32 / chunks as f32, band.0)))
                } else {
                    i.checked_sub(1).map(|p| (p, (0.0, 1.0)))
                };
                let below = if chunk + 1 < chunks {
                    Some((i, (band.1, (chunk + 2) as f32 / chunks as f32)))
                } else {
                    (i + 1 < dims.len()).then(|| (i + 1, (0.0, 1.0)))
                };
                runs.push(RunPlan {
                    page_start: i,
                    page_end: i,
                    band,
                    above,
                    below,
                });
            }
            i += 1;
        } else {
            let mut j = i;
            while j + 1 < dims.len()
                && ratio(j + 1) <= MAX_ASPECT_RATIO
                && combined(i, j) < MIN_ASPECT_RATIO
                && combined(i, j + 1) <= MAX_ASPECT_RATIO
            {
                j += 1;
            }
            runs.push(RunPlan {
                page_start: i,
                page_end: j,
                band: (0.0, 1.0),
                above: i.checked_sub(1).map(|p| (p, (0.0, 1.0))),
                below: (j + 1 < dims.len()).then(|| (j + 1, (0.0, 1.0))),
            });
            i = j + 1;
        }
    }
    runs
}

// ---------------------------------------------------------------------------
// Canvas building
// ---------------------------------------------------------------------------

/// Crops `margin` rows starting at `y` and scales the strip to `width`.
fn strip(image: &RgbImage, margin: u32, y: u32, width: u32) -> Option<RgbImage> {
    if margin == 0 || image.height() < margin || width == 0 {
        return None;
    }
    let y = y.min(image.height() - margin);
    let crop = imageops::crop_imm(image, 0, y, image.width(), margin).to_image();
    Some(imageops::resize(
        &crop,
        width,
        (margin as f32 * width as f32 / image.width().max(1) as f32).round().max(1.0) as u32,
        imageops::FilterType::Triangle,
    ))
}

/// Crops `margin` rows at the bottom (`bottom`) or top edge of `band`
/// (fractions of the image's height) and scales the strip to `width`.
fn band_strip(image: &RgbImage, band: (f32, f32), margin: u32, width: u32, bottom: bool) -> Option<RgbImage> {
    let height = image.height() as f32;
    let band_height = (band.1 - band.0) * height;
    if band_height < 1.0 || margin == 0 || width == 0 {
        return None;
    }
    let band_top = band.0 * height;
    let y = if bottom {
        band_top + band_height - margin as f32
    } else {
        band_top
    };
    strip(image, margin, y.round().max(0.0) as u32, width)
}

/// The run's top margin strip: the bottom `margin` rows of the band of
/// `image` directly above the run, scaled to `width`, so a speech bubble cut
/// by the run's top boundary still appears whole. `band` is the covered band
/// as fractions of the image's height. Returns `None` when the crop is
/// impossible (image too small, zero width).
pub fn top_margin_strip(image: &RgbImage, band: (f32, f32), width: u32, margin: u32) -> Option<RgbImage> {
    band_strip(image, band, margin, width, true)
}

/// The run's bottom margin strip: the top `margin` rows of the band of
/// `image` directly below the run, scaled to `width`. Returns `None` when the
/// crop is impossible.
pub fn bottom_margin_strip(image: &RgbImage, band: (f32, f32), width: u32, margin: u32) -> Option<RgbImage> {
    band_strip(image, band, margin, width, false)
}

/// The part of `image` covered by `band` (fractions of its height), scaled to
/// `width`, preserving aspect ratio.
fn band_body(image: &RgbImage, band: (f32, f32), width: u32) -> RgbImage {
    let height = image.height();
    let band_top = (band.0 * height as f32).round() as u32;
    let band_bottom = (band.1 * height as f32).round() as u32;
    let band_height = band_bottom.saturating_sub(band_top);
    if band_height == 0 || width == 0 {
        return RgbImage::new(width, 0);
    }
    let crop = imageops::crop_imm(image, 0, band_top, width, band_height).to_image();
    if crop.width() == width {
        return crop;
    }
    imageops::resize(
        &crop,
        width,
        (crop.height() as f32 * width as f32 / crop.width().max(1) as f32)
            .round()
            .max(1.0) as u32,
        imageops::FilterType::Triangle,
    )
}

/// Scaled height of a run's body — the part of the stacked pages the run
/// covers — in pixels of `width`. `pages` are the covered pages' native
/// `(width, height)` in order.
pub fn body_height(pages: &[(u32, u32)], width: u32, band: (f32, f32)) -> u32 {
    if pages.is_empty() || width == 0 {
        return 0;
    }
    let stacked: f32 = pages
        .iter()
        .map(|&(w, h)| h as f32 * width as f32 / w.max(1) as f32)
        .sum();
    (stacked * (band.1 - band.0)).round() as u32
}

/// Stacks the run's canvas: the `top` margin strip, the body (every page
/// scaled to `width` and cropped to `band`), and the `bottom` margin strip.
/// Every layer shares one coordinate space (pixels of `width`).
pub fn stack_run(
    top: Option<RgbImage>,
    pages: &[RgbImage],
    bottom: Option<RgbImage>,
    width: u32,
    band: (f32, f32),
) -> RgbImage {
    let top_h = top.as_ref().map_or(0, |s| s.height());
    let bottom_h = bottom.as_ref().map_or(0, |s| s.height());
    let body: Vec<RgbImage> = pages.iter().map(|p| band_body(p, band, width)).collect();
    let body_h: u32 = body.iter().map(|b| b.height()).sum();
    let mut out = RgbImage::new(width, top_h + body_h + bottom_h);
    if let Some(strip) = top {
        imageops::replace(&mut out, &strip, 0, 0);
    }
    let mut y = top_h;
    for page in &body {
        imageops::replace(&mut out, page, 0, y as i64);
        y += page.height();
    }
    if let Some(strip) = bottom {
        imageops::replace(&mut out, &strip, 0, y as i64);
    }
    out
}

/// Everything the app needs to build one run's canvas: the plan set, the
/// native page dims, and the per-run image paths plus the neighboring paths
/// above/below each run.
#[derive(Debug, Clone)]
pub struct BuildCtx {
    pub runs: Vec<RunPlan>,
    pub dims: Vec<(u32, u32)>,
    pub paths: Vec<Vec<String>>,
    pub above: Vec<Option<String>>,
    pub below: Vec<Option<String>>,
}

/// Builds the stitched canvas of one run: its pages stacked at the first
/// page's width with the margin strips of the neighboring content above and
/// below. Returns the canvas plus its `(width, margin_top)` metrics. When a
/// page fails to decode, returns an error — the caller counts it as a failed
/// run.
pub fn build_canvas_for(ctx: &BuildCtx, index: usize) -> Result<(RgbImage, u32, u32), String> {
    let run = *ctx
        .runs
        .get(index)
        .ok_or_else(|| format!("ocr canvas out of range: {index}"))?;
    let paths = ctx
        .paths
        .get(index)
        .ok_or_else(|| format!("ocr canvas paths out of range: {index}"))?;
    let mut loaded = Vec::with_capacity(paths.len());
    for path in paths {
        match load_rgb(path) {
            Some(image) => loaded.push(image),
            None => {
                return Err(format!("undecodable page {path} (run {index})"));
            }
        }
    }
    let Some(first) = loaded.first() else {
        return Err(format!("ocr run {index} covers no pages"));
    };
    let width = first.width();
    let end = run.page_end.min(ctx.dims.len().saturating_sub(1));
    if run.page_start >= ctx.dims.len() || end < run.page_start {
        return Err(format!("ocr run {index} covers no known pages"));
    }
    let body_h = body_height(&ctx.dims[run.page_start..=end], width, run.band);
    let margin = (STITCH_MARGIN_RATIO * body_h as f32).round().max(1.0) as u32;
    let above = match (ctx.above.get(index).and_then(|o| o.as_deref()), run.above) {
        (Some(path), Some((_, band))) => {
            load_rgb(path).and_then(|image| top_margin_strip(&image, band, width, margin))
        }
        _ => None,
    };
    let below = match (ctx.below.get(index).and_then(|o| o.as_deref()), run.below) {
        (Some(path), Some((_, band))) => {
            load_rgb(path).and_then(|image| bottom_margin_strip(&image, band, width, margin))
        }
        _ => None,
    };
    let margin_top = above.as_ref().map_or(0, |strip| strip.height());
    let canvas = stack_run(above, &loaded, below, width, run.band);
    Ok((canvas, width, margin_top))
}

// ---------------------------------------------------------------------------
// Assembly: distribute + dedup (app-owned, no engine state)
// ---------------------------------------------------------------------------

/// AABBs of a detected text box as `[min_x, min_y, max_x, max_y]`.
fn box_bounds(quad: &DetectorQuad) -> [f32; 4] {
    points_bounds(&quad.points)
}

/// AABB of raw quad points as `[min_x, min_y, max_x, max_y]`.
fn points_bounds(points: &[[f32; 2]; 4]) -> [f32; 4] {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for point in points {
        min_x = min_x.min(point[0]);
        min_y = min_y.min(point[1]);
        max_x = max_x.max(point[0]);
        max_y = max_y.max(point[1]);
    }
    [min_x, min_y, max_x, max_y]
}

/// Area of an AABB (zero for degenerate boxes).
fn area(bounds: [f32; 4]) -> f32 {
    (bounds[2] - bounds[0]).max(0.0) * (bounds[3] - bounds[1]).max(0.0)
}

/// The more complete of two captures of the same bubble: when one text
/// contains the other (the seam cut one read short), the containing one wins;
/// otherwise the reads differ too much to merge safely and `winner` stays.
fn merge_texts(winner: &str, loser: &str) -> String {
    if winner.contains(loser) && !loser.is_empty() {
        winner.to_string()
    } else if loser.contains(winner) && !winner.is_empty() {
        loser.to_string()
    } else {
        winner.to_string()
    }
}

/// Maps merged OCR lines over the stitched canvas back to per-page entries in
/// each page's native pixel space.
///
/// `pages` lists the covered pages' indices and native `(width, height)` in
/// order; the canvas is `width` wide (the first page's width) with the run's
/// body starting `margin_top` pixels down and occupying the `band` fraction
/// of the stacked body. Entries above the body belong to the band above the
/// run and are assigned to the page holding the band's top edge, with quads
/// past that edge (the caller dedups them against that page's store).
/// Entries whose quad extends past the run's bottom edge — early captures of
/// the *next* page's content — are dropped: the next run owns that content
/// and re-detects it in its own body.
pub fn distribute(
    lines: Vec<OcrLine>,
    pages: &[(usize, u32, u32)],
    band: (f32, f32),
    margin_top: u32,
) -> Vec<(usize, Vec<NewEntry>)> {
    if pages.is_empty() {
        return Vec::new();
    }
    let width = pages[0].1.max(1) as f32;
    let scaled: Vec<(f32, f32)> = pages
        .iter()
        .map(|(_, w, h)| {
            let scale = *w as f32 / width;
            (scale, *h as f32 * scale)
        })
        .collect();
    let offset = |t: usize| scaled[..t].iter().map(|(_, h)| h).sum::<f32>();
    let total: f32 = scaled.iter().map(|(_, h)| h).sum();
    let band_top = band.0 * total;
    let band_bottom = band.1 * total;
    let boundary = margin_top as f32 + (band_bottom - band_top).round();
    let page_at = |y: f32| -> usize {
        scaled
            .iter()
            .enumerate()
            .position(|(t, (_, h))| y >= offset(t) && y < offset(t) + h)
            .unwrap_or(pages.len() - 1)
    };

    let mut per_page: Vec<Vec<NewEntry>> = vec![Vec::new(); pages.len()];
    for line in lines {
        let bounds = box_bounds(&line.bbox);
        if bounds[3] > boundary {
            // Past the run's bottom edge: the next run's content, dropped.
            // The next run re-detects it in its own body.
            continue;
        }
        let (target, dy) = if bounds[1] < margin_top as f32 {
            let t = page_at(band_top);
            (t, (band_top - offset(t) - margin_top as f32) * scaled[t].0)
        } else {
            let t = page_at(bounds[1] - margin_top as f32);
            (t, ((band_top - offset(t)).max(0.0) - margin_top as f32 - offset(t)) * scaled[t].0)
        };
        let scale = scaled[target].0;
        per_page[target].push(NewEntry {
            source: EntrySource::AutoOcr,
            text: line.text,
            score: line.score,
            quad: Quad {
                points: line.bbox.points.map(|[x, y]| [x * scale, y * scale + dy]),
            }
            .snap_if_near_upright()
            .inflate(OCR_QUAD_PAD),
        });
    }

    per_page
        .into_iter()
        .enumerate()
        .filter(|(_, entries)| !entries.is_empty())
        .map(|(t, entries)| (pages[t].0, entries))
        .collect()
}

/// App-side dedup target for `index`.
///
/// Chunk splits of one tall page dedup against the same page at the band's top
/// edge; whole-page runs dedup against the previous page's full height.
/// Returns `(page, offset)` where `offset` is the band-top edge in that page's
/// pixel space. `None` for the very first run.
pub fn dedup_target_for(index: usize, plans: &[RunPlan], dims: &[(u32, u32)]) -> Option<(usize, u32)> {
    let run = *plans.get(index)?;
    if run.band.0 > 0.0 {
        let h = dims.get(run.page_start)?.1;
        Some((run.page_start, (run.band.0 * h as f32).round() as u32))
    } else {
        let p = run.page_start.checked_sub(1)?;
        Some((p, dims.get(p)?.1))
    }
}

/// Outcome of [`dedup_with_previous`].
pub struct DedupOutcome {
    /// Lines that survived: no overlap with the committed page above, or a
    /// strictly fuller capture of an overlapped bubble (with the overlapped
    /// text merged in). They continue through distribute and commit.
    pub kept: Vec<OcrLine>,
    /// Committed entries of the page above that lost to a fuller re-detection
    /// and must be soft-deleted. Empty when every overlap favored the
    /// committed copy.
    pub drop_prev: Vec<EntryId>,
}

/// Deduplicates the current run's merged `lines` against the committed
/// entries of the page content directly above the run.
///
/// The run's top margin re-detects bubbles already captured by the run that
/// covered the band above, so both copies describe the same bubble. Each
/// current line whose AABB overlaps a previous quad (transformed into this
/// run's canvas space: scaled by the width ratio, shifted up by `prev_offset`
/// — the offset in the previous page's pixel space of the top edge of this
/// run's canvas — and shifted down by `margin_top`) is compared by captured
/// area: the fuller capture wins. A winning re-detection survives with the
/// committed text merged in and the committed copy is reported in
/// [`DedupOutcome::drop_prev`]; otherwise the re-detection is dropped and the
/// committed copy stands. Captures within 10% count as ties and favor the
/// committed copy, so detector jitter never churns the model.
pub fn dedup_with_previous(
    lines: Vec<OcrLine>,
    prev: &[(EntryId, String, Quad)],
    prev_width: u32,
    prev_offset: u32,
    cur_width: u32,
    margin_top: u32,
) -> DedupOutcome {
    if prev.is_empty() || prev_width == 0 || cur_width == 0 {
        return DedupOutcome {
            kept: lines,
            drop_prev: Vec::new(),
        };
    }
    let scale = cur_width as f32 / prev_width as f32;
    let mapped: Vec<(EntryId, &str, [f32; 4])> = prev
        .iter()
        .map(|(id, text, quad)| {
            let [min_x, min_y, max_x, max_y] = quad.bounds();
            (
                *id,
                text.as_str(),
                [
                    min_x * scale,
                    (min_y - prev_offset as f32) * scale + margin_top as f32,
                    max_x * scale,
                    (max_y - prev_offset as f32) * scale + margin_top as f32,
                ],
            )
        })
        .collect();
    let mut kept = Vec::with_capacity(lines.len());
    let mut drop_prev = Vec::new();
    for mut line in lines {
        let bounds = box_bounds(&line.bbox);
        // A couple of pixels of slack absorbs OCR jitter between runs.
        let padded = [bounds[0] - 2.0, bounds[1] - 2.0, bounds[2] + 2.0, bounds[3] + 2.0];
        let hits: Vec<usize> = mapped
            .iter()
            .enumerate()
            .filter(|(_, (_, _, p))| !(p[2] < padded[0] || p[0] > padded[2] || p[3] < padded[1] || p[1] > padded[3]))
            .map(|(i, _)| i)
            .collect();
        if hits.is_empty() {
            kept.push(line);
            continue;
        }
        let line_area = area(bounds);
        let wins = hits.iter().all(|&h| {
            let prev_area = area(mapped[h].2);
            line_area > prev_area && line_area - prev_area > 0.1 * line_area
        });
        if wins {
            for &h in &hits {
                line.text = merge_texts(&line.text, mapped[h].1);
                drop_prev.push(mapped[h].0);
            }
            kept.push(line);
        }
        // Otherwise the committed copy stands (ties included): drop the line.
    }
    drop_prev.sort();
    drop_prev.dedup();
    DedupOutcome { kept, drop_prev }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, x0: f32, y0: f32, x1: f32, y1: f32, score: f32) -> OcrLine {
        OcrLine {
            bbox: DetectorQuad::from_xyxy(x0, y0, x1, y1),
            text: text.to_string(),
            score,
        }
    }

    fn prev_quad(id: u64, text: &str, x0: f32, y0: f32, x1: f32, y1: f32) -> (EntryId, String, Quad) {
        (EntryId(id), text.to_string(), Quad::from_xyxy(x0, y0, x1, y1))
    }

    fn rgb(w: u32, h: u32, fill: [u8; 3]) -> RgbImage {
        RgbImage::from_pixel(w, h, image::Rgb(fill))
    }

    #[test]
    fn margin_strips_crop_from_the_requested_band_edge() {
        let mut image = rgb(100, 100, [0, 0, 0]);
        for y in 0..100 {
            let value = if y < 20 || y >= 80 { 90 } else { 10 };
            for x in 0..100 {
                image.put_pixel(x, y, image::Rgb([value, 0, 0]));
            }
        }
        let top = top_margin_strip(&image, (0.0, 1.0), 50, 20).unwrap();
        assert_eq!((top.width(), top.height()), (50, 10));
        assert_eq!(top.get_pixel(25, 5).0, [90, 0, 0], "bottom rows of the source");
        let bottom = bottom_margin_strip(&image, (0.0, 1.0), 50, 20).unwrap();
        assert_eq!(bottom.get_pixel(25, 5).0, [90, 0, 0], "top rows of the source");
    }

    #[test]
    fn top_margin_strip_crops_the_band_not_the_page() {
        let mut image = rgb(100, 100, [0, 0, 0]);
        for y in 0..100 {
            let value = if y >= 30 && y < 50 { 90 } else { 10 };
            for x in 0..100 {
                image.put_pixel(x, y, image::Rgb([value, 0, 0]));
            }
        }
        // Band 0.3..0.5 of the page: its bottom 10 rows are rows 40..50.
        let strip = top_margin_strip(&image, (0.3, 0.5), 100, 10).unwrap();
        assert_eq!((strip.width(), strip.height()), (100, 10));
        for y in 0..10 {
            assert_eq!(strip.get_pixel(50, y).0, [90, 0, 0], "row {y} must come from the band");
        }
    }

    #[test]
    fn stack_run_crops_pages_to_the_band() {
        // Page 100x400, band 0.5..1.0 -> body rows 200..400.
        let mut page = rgb(100, 400, [0, 0, 0]);
        for y in 200..400 {
            for x in 0..100 {
                page.put_pixel(x, y, image::Rgb([90, 0, 0]));
            }
        }
        let above = rgb(100, 100, [5, 5, 5]);
        let below = rgb(100, 100, [7, 7, 7]);
        let top = top_margin_strip(&above, (0.0, 1.0), 100, 20).unwrap();
        let bottom = bottom_margin_strip(&below, (0.0, 1.0), 100, 20).unwrap();
        let out = stack_run(Some(top), &[page], Some(bottom), 100, (0.5, 1.0));
        assert_eq!((out.width(), out.height()), (100, 20 + 200 + 20));
        assert_eq!(out.get_pixel(50, 10).0, [5, 5, 5], "top strip");
        assert_eq!(out.get_pixel(50, 25).0, [90, 0, 0], "body from the band");
        assert_eq!(out.get_pixel(50, 210).0, [90, 0, 0], "body still the band");
        assert_eq!(out.get_pixel(50, 230).0, [7, 7, 7], "bottom strip");
    }

    #[test]
    fn body_height_scales_pages_to_the_run_width() {
        assert_eq!(body_height(&[(100, 200), (200, 400)], 100, (0.0, 1.0)), 400);
        assert_eq!(body_height(&[(100, 400)], 100, (0.5, 1.0)), 200);
        assert_eq!(body_height(&[], 100, (0.0, 1.0)), 0);
    }

    #[test]
    fn plan_runs_keeps_in_range_pages_alone() {
        let runs = plan_runs(&[(800, 3000), (800, 2400), (800, 4800)]);
        assert_eq!(runs.len(), 3);
        for run in &runs {
            assert_eq!(run.page_start, run.page_end);
            assert_eq!(run.band, (0.0, 1.0));
        }
        assert_eq!(runs[0].above, None);
        assert_eq!(runs[1].above, Some((0, (0.0, 1.0))));
        assert_eq!(runs[1].below, Some((2, (0.0, 1.0))));
    }

    #[test]
    fn plan_runs_stitches_short_pages_until_the_minimum_ratio() {
        // 800x1200 (1.5) and 800x1400 (1.75): combined 3.25 >= 2, one run.
        let runs = plan_runs(&[(800, 1200), (800, 1400), (800, 3200)]);
        assert_eq!(runs.len(), 2);
        assert_eq!((runs[0].page_start, runs[0].page_end), (0, 1));
        assert_eq!(runs[0].band, (0.0, 1.0));
        assert_eq!(runs[0].above, None);
        assert_eq!(runs[0].below, Some((2, (0.0, 1.0))));
        assert_eq!((runs[1].page_start, runs[1].page_end), (2, 2));
        assert_eq!(runs[1].above, Some((1, (0.0, 1.0))));
    }

    #[test]
    fn plan_runs_stitches_all_following_short_pages() {
        // Three 1.25:1 pages: the first two combine to 2.5, the third is
        // too short to stand alone but has no next page.
        let runs = plan_runs(&[(800, 1000), (800, 1000), (800, 1000)]);
        assert_eq!(runs.len(), 2);
        assert_eq!((runs[0].page_start, runs[0].page_end), (0, 1));
        assert_eq!((runs[1].page_start, runs[1].page_end), (2, 2));
    }

    #[test]
    fn plan_runs_stops_short_of_exceeding_the_maximum() {
        // Adding the 800x4400 (5.5) page would push the combined ratio past
        // 6, so the short page is OCR'd alone.
        let runs = plan_runs(&[(800, 1500), (800, 4400)]);
        assert_eq!(runs.len(), 2);
        assert_eq!((runs[0].page_start, runs[0].page_end), (0, 0));
        assert_eq!((runs[1].page_start, runs[1].page_end), (1, 1));
    }

    #[test]
    fn plan_runs_never_stitches_a_page_above_the_maximum() {
        // A 800x1200 (1.5) page followed by a 800x8000 (10) page: the tall
        // page becomes its own split runs, the short page stays a short run.
        let runs = plan_runs(&[(800, 1200), (800, 8000)]);
        assert_eq!(runs.len(), 3);
        assert_eq!((runs[0].page_start, runs[0].page_end), (0, 0));
        assert_eq!(runs[1].above, Some((0, (0.0, 1.0))));
        assert_eq!(runs[2].above, Some((1, (0.0, 0.5))));
        assert_eq!(runs[2].below, None);
    }

    #[test]
    fn plan_runs_splits_tall_pages_into_in_range_chunks() {
        // 800x8000 is 10:1 -> two chunks of 5:1 each.
        let runs = plan_runs(&[(800, 8000)]);
        assert_eq!(runs.len(), 2);
        assert_eq!((runs[0].page_start, runs[0].page_end), (0, 0));
        assert_eq!(runs[0].band, (0.0, 0.5));
        assert_eq!(runs[0].above, None);
        assert_eq!(runs[0].below, Some((0, (0.5, 1.0))));
        assert_eq!(runs[1].band, (0.5, 1.0));
        assert_eq!(runs[1].above, Some((0, (0.0, 0.5))));
    }

    #[test]
    fn plan_runs_combines_widths_by_scaling_to_the_first_page() {
        // The second page is half as wide, so it contributes double its
        // height: 800x800 (1.0) + 400x1600 scaled to 800 -> combined 5.0.
        let runs = plan_runs(&[(800, 800), (400, 1600)]);
        assert_eq!(runs.len(), 1);
        assert_eq!((runs[0].page_start, runs[0].page_end), (0, 1));
    }

    #[test]
    fn distribute_maps_top_margin_past_the_edge_and_body_into_the_page() {
        let lines = vec![
            line("above", 10.0, 150.0, 90.0, 180.0, 0.9),
            line("middle", 10.0, 250.0, 90.0, 280.0, 0.9),
        ];
        let per_page = distribute(lines, &[(0, 100, 400)], (0.0, 1.0), 200);
        assert_eq!(per_page.len(), 1);
        let entries = &per_page[0].1;
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0].quad.bounds(),
            [
                10.0 - OCR_QUAD_PAD,
                -50.0 - OCR_QUAD_PAD,
                90.0 + OCR_QUAD_PAD,
                -20.0 + OCR_QUAD_PAD
            ],
            "top margin, out of page"
        );
        assert_eq!(
            entries[1].quad.bounds(),
            [
                10.0 - OCR_QUAD_PAD,
                50.0 - OCR_QUAD_PAD,
                90.0 + OCR_QUAD_PAD,
                80.0 + OCR_QUAD_PAD
            ],
            "page body"
        );
    }

    #[test]
    fn distribute_drops_lines_past_the_bottom_edge() {
        // The next run owns that content and re-detects it in its own body.
        let lines = vec![
            line("middle", 10.0, 250.0, 90.0, 280.0, 0.9),
            line("below", 10.0, 650.0, 90.0, 680.0, 0.9),
        ];
        let per_page = distribute(lines, &[(0, 100, 400)], (0.0, 1.0), 200);
        assert_eq!(per_page.len(), 1);
        assert_eq!(per_page[0].1.len(), 1);
        assert_eq!(per_page[0].1[0].text, "middle");
    }

    #[test]
    fn distribute_maps_entries_to_their_pages_in_native_pixels() {
        // Two pages stitched at 100px wide: 100x200 (page 0) + 100x300 (page 1).
        // The bottom-margin line belongs to the next run and is dropped.
        let lines = vec![
            line("p0", 10.0, 210.0, 90.0, 240.0, 0.9),
            line("p1", 10.0, 450.0, 90.0, 480.0, 0.9),
            line("below", 10.0, 710.0, 90.0, 730.0, 0.9),
        ];
        let per_page = distribute(lines, &[(0, 100, 200), (1, 100, 300)], (0.0, 1.0), 200);
        assert_eq!(per_page.len(), 2);
        assert_eq!(per_page[0].0, 0);
        assert_eq!(per_page[1].0, 1);
        assert_eq!(per_page[0].1.len(), 1);
        assert_eq!(per_page[1].1.len(), 1);
        assert_eq!(per_page[0].1[0].text, "p0");
        assert_eq!(
            per_page[0].1[0].quad.bounds(),
            [
                10.0 - OCR_QUAD_PAD,
                10.0 - OCR_QUAD_PAD,
                90.0 + OCR_QUAD_PAD,
                40.0 + OCR_QUAD_PAD
            ]
        );
        assert_eq!(per_page[1].1[0].text, "p1");
        assert_eq!(
            per_page[1].1[0].quad.bounds(),
            [
                10.0 - OCR_QUAD_PAD,
                50.0 - OCR_QUAD_PAD,
                90.0 + OCR_QUAD_PAD,
                80.0 + OCR_QUAD_PAD
            ]
        );
    }

    #[test]
    fn distribute_scales_back_entries_of_narrower_pages() {
        // Page 0 is 200 wide, page 1 is 100 wide; the canvas is 200 wide, so
        // page 1 is scaled up 2x and its entries must scale back by 0.5.
        let lines = vec![line("w", 40.0, 410.0, 120.0, 430.0, 0.9)];
        let per_page = distribute(lines, &[(0, 200, 200), (1, 100, 150)], (0.0, 1.0), 200);
        assert_eq!(per_page.len(), 1);
        assert_eq!(per_page[0].0, 1);
        assert_eq!(
            per_page[0].1[0].quad.bounds(),
            [
                20.0 - OCR_QUAD_PAD,
                5.0 - OCR_QUAD_PAD,
                60.0 + OCR_QUAD_PAD,
                15.0 + OCR_QUAD_PAD
            ]
        );
    }

    #[test]
    fn distribute_maps_chunk_bands_into_the_page() {
        // Page split in half (band 0.5..1): the canvas body covers page rows
        // 300..600, so a body entry at canvas y 250 lands at page y 350, and
        // a top-margin entry lands past the band's top edge at page y 290.
        let lines = vec![
            line("chunk", 10.0, 250.0, 90.0, 280.0, 0.9),
            line("edge", 10.0, 190.0, 90.0, 210.0, 0.9),
        ];
        let per_page = distribute(lines, &[(0, 100, 600)], (0.5, 1.0), 200);
        assert_eq!(per_page.len(), 1);
        let entries = &per_page[0].1;
        assert_eq!(entries[0].text, "chunk");
        assert_eq!(
            entries[0].quad.bounds(),
            [
                10.0 - OCR_QUAD_PAD,
                350.0 - OCR_QUAD_PAD,
                90.0 + OCR_QUAD_PAD,
                380.0 + OCR_QUAD_PAD
            ]
        );
        assert_eq!(entries[1].text, "edge");
        assert_eq!(
            entries[1].quad.bounds(),
            [
                10.0 - OCR_QUAD_PAD,
                290.0 - OCR_QUAD_PAD,
                90.0 + OCR_QUAD_PAD,
                310.0 + OCR_QUAD_PAD
            ]
        );
    }

    #[test]
    fn distribute_without_lines_yields_no_entries() {
        assert!(distribute(vec![], &[(0, 100, 200)], (0.0, 1.0), 200).is_empty());
        assert!(distribute(vec![line("x", 0.0, 0.0, 10.0, 10.0, 0.9)], &[], (0.0, 1.0), 200).is_empty());
    }

    #[test]
    fn dedup_drops_the_repeat_of_a_spanning_bubble() {
        // Previous page (height 500) captured the bubble with a quad sticking
        // 50px past its own bottom edge. Top margin is 100px, so the
        // re-detection appears around y=50..150 in the next canvas. Equal
        // captures tie and favor the committed copy.
        let prev = vec![prev_quad(7, "span", 20.0, 450.0, 80.0, 550.0)];
        let cur = vec![
            line("span", 20.0, 50.0, 80.0, 150.0, 0.9),
            line("own", 20.0, 220.0, 80.0, 250.0, 0.9),
        ];
        let out = dedup_with_previous(cur, &prev, 100, 500, 100, 100);
        assert_eq!(out.kept.len(), 1, "spanning bubble deduped against previous page");
        assert_eq!(out.kept[0].text, "own");
        assert!(out.drop_prev.is_empty());
    }

    #[test]
    fn dedup_keeps_the_fuller_redetection_and_drops_the_committed_copy() {
        // Committed quad is a 30px sliver; the re-detection shows the full
        // 100px bubble. The fuller capture wins: it survives and the sliver
        // is reported for soft-delete.
        let prev = vec![prev_quad(7, "hey", 20.0, 470.0, 80.0, 500.0)];
        let cur = vec![line("hey there", 20.0, 50.0, 80.0, 150.0, 0.9)];
        let out = dedup_with_previous(cur, &prev, 100, 500, 100, 100);
        assert_eq!(out.kept.len(), 1);
        assert_eq!(out.kept[0].text, "hey there");
        assert_eq!(out.drop_prev, vec![EntryId(7)]);
    }

    #[test]
    fn dedup_merges_containing_text_into_the_fuller_winner() {
        // The committed copy has the fuller text; the re-detection is a
        // partial read of the same bubble. The committed copy stands.
        let prev = vec![prev_quad(7, "2015년 9월18일", 20.0, 450.0, 80.0, 550.0)];
        let cur = vec![line("9월18일", 20.0, 50.0, 80.0, 150.0, 0.9)];
        let out = dedup_with_previous(cur, &prev, 100, 500, 100, 100);
        assert!(out.kept.is_empty());
        assert!(out.drop_prev.is_empty());
    }

    #[test]
    fn dedup_uses_chunk_offset_inside_the_same_page() {
        // Page height 600 split into two 300px chunks. The first chunk's run
        // stored bubbles past its own band edge (into the second chunk); the
        // second chunk dedups against them with offset = 300. Top margin 100px.
        let prev = vec![prev_quad(7, "span", 10.0, 290.0, 60.0, 330.0)];
        let cur = vec![line("span", 10.0, 90.0, 60.0, 130.0, 0.9)];
        let out = dedup_with_previous(cur, &prev, 100, 300, 100, 100);
        assert!(out.kept.is_empty(), "chunk boundary duplicate must be dropped");
        assert!(out.drop_prev.is_empty());
    }

    #[test]
    fn dedup_keeps_distinct_bubbles_even_in_the_strip() {
        let prev = vec![prev_quad(7, "old", 10.0, 460.0, 40.0, 480.0)];
        let cur = vec![line("other", 60.0, 110.0, 90.0, 140.0, 0.9)];
        let out = dedup_with_previous(cur, &prev, 100, 500, 100, 100);
        assert_eq!(out.kept.len(), 1, "non-overlapping boxes must survive");
    }

    #[test]
    fn dedup_without_previous_data_keeps_everything() {
        let lines = vec![line("x", 0.0, 90.0, 10.0, 110.0, 0.9)];
        assert_eq!(dedup_with_previous(lines.clone(), &[], 100, 500, 100, 100).kept.len(), 1);
        assert_eq!(
            dedup_with_previous(lines, &[prev_quad(7, "x", 0.0, 0.0, 1.0, 1.0)], 0, 500, 100, 100).kept.len(),
            1
        );
    }

    #[test]
    fn dedup_scales_coordinates_for_different_page_widths() {
        // Previous page is twice as wide; its quad maps into the current
        // canvas via the same scale the stitch used. Areas compare in the
        // current canvas space, so a strictly fuller re-detection wins.
        let prev = vec![prev_quad(7, "span", 80.0, 450.0, 160.0, 540.0)];
        let cur = vec![line("span", 40.0, 60.0, 80.0, 120.0, 0.9)];
        let out = dedup_with_previous(cur, &prev, 200, 500, 100, 100);
        assert_eq!(out.kept.len(), 1, "fuller re-detection wins across widths");
        assert_eq!(out.drop_prev, vec![EntryId(7)]);
    }
}
