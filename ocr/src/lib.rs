//! Wraps the rapidocr engine: configuration, lifecycle and conversion of raw
//! engine output into the model's append-only entries. The rest of the app
//! never touches rapidocr types directly.

use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use image::{imageops, RgbImage};
use rapidocr_core::config::{
    DetConfig, InferenceOptions, LimitType, PipelineConfig, RapidOcrConfig, RecConfig,
};
use rapidocr_core::pipeline::{DetRecPipeline, PipelineError};
pub use rapidocr_core::types::OcrLine;
use rapidocr_core::{is_cancelled_error, RapidOcr};

pub use rapidocr_core::OcrCancellationToken;

pub mod session;
pub use session::{RunEvent, RunSession};

use scanlateit_model::{EntrySource, NewEntry, Project, Quad};

const MODEL_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../models");

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

/// Cloneable handle to the shared OCR engine. Only one run may execute at a
/// time; runs are cancellable via [`OcrCancellationToken`].
#[derive(Clone)]
pub struct Engine(Arc<Mutex<RapidOcr>>);

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Engine")
    }
}

impl Engine {
    pub fn build() -> Result<Self, String> {
        RapidOcr::new(config())
            .map(|ocr| Self(Arc::new(Mutex::new(ocr))))
            .map_err(|e| format!("Engine init failed: {e}"))
    }

    /// Runs OCR on an image file. On cancel returns `Err("cancelled")`.
    pub fn run_path_cancellable(
        &self,
        path: &str,
        token: &OcrCancellationToken,
    ) -> Result<Vec<OcrLine>, String> {
        let mut engine = self
            .0
            .lock()
            .map_err(|e| format!("Engine lock poisoned: {e}"))?;
        engine
            .run_path_cancellable(path, token)
            .map(|output| output.lines)
            .map_err(|e| {
                if is_cancelled_error(&e) {
                    "cancelled".to_string()
                } else {
                    format!("OCR failed: {e}")
                }
            })
    }

    /// Runs OCR on an in-memory image. On cancel returns `Err("cancelled")`.
    pub fn run_image_cancellable(
        &self,
        image: &RgbImage,
        token: &OcrCancellationToken,
    ) -> Result<Vec<OcrLine>, String> {
        let mut engine = self
            .0
            .lock()
            .map_err(|e| format!("Engine lock poisoned: {e}"))?;
        engine
            .run_image_cancellable(image, token)
            .map(|output| output.lines)
            .map_err(|e| {
                if is_cancelled_error(&e) {
                    "cancelled".to_string()
                } else {
                    format!("OCR failed: {e}")
                }
            })
    }
}

/// Cloneable handle to the parallel OCR pipeline: `workers` detection
/// sessions on dedicated threads feeding one recognition session, results
/// returned strictly in submission order. Only one stream of runs may
/// execute at a time; runs are cancellable via [`OcrCancellationToken`].
#[derive(Clone)]
pub struct ParallelEngine(Arc<DetRecPipeline>);

impl fmt::Debug for ParallelEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ParallelEngine")
    }
}

impl ParallelEngine {
    /// Builds `workers` detection sessions plus one recognition session, one
    /// thread per session. Each session uses a single ONNX Runtime intra-op
    /// thread, so the whole pipeline fits a modest CPU.
    pub fn build(workers: usize) -> Result<Self, String> {
        let cfg = config();
        let inference = InferenceOptions {
            intra_threads: 1,
            inter_threads: 1,
            ..cfg.inference
        };
        let pipeline = DetRecPipeline::new(&cfg, inference, inference, workers)
            .map_err(|e| format!("Parallel engine init failed: {e}"))?;
        Ok(Self(Arc::new(pipeline)))
    }

    /// Submits one canvas for detection and recognition, tagged with the
    /// caller's run index. Results arrive in ascending run order via
    /// [`ParallelEngine::recv`].
    pub fn submit(&self, run: usize, canvas: RgbImage) -> Result<(), String> {
        self.0.submit(run, canvas).map_err(|e| e.to_string())
    }

    /// Blocks until the next run's lines are ready, in submission order.
    /// On cancel returns `Err("cancelled")`.
    pub fn recv(&self) -> Result<(usize, Vec<OcrLine>), String> {
        self.0.recv().map_err(|e| match e {
            PipelineError::Cancelled => "cancelled".to_string(),
            other => other.to_string(),
        })
    }

    /// Blocks until any run's lines are ready, regardless of order.
    /// The caller reorders to restore `0,1,2…` commit order. On cancel
    /// returns `Err("cancelled")`.
    pub fn recv_unordered(&self) -> Result<(usize, Vec<OcrLine>), String> {
        self.0.recv_unordered().map_err(|e| match e {
            PipelineError::Cancelled => "cancelled".to_string(),
            other => other.to_string(),
        })
    }

    /// Cancels in-flight inference; the workers exit once they observe the
    /// cancellation at their next checkpoint.
    pub fn cancel(&self) {
        self.0.cancel();
    }

    /// The pipeline's cancellation token; cancelling it aborts in-flight
    /// inference and makes [`ParallelEngine::recv`] return `"cancelled"`.
    pub fn cancellation_token(&self) -> &OcrCancellationToken {
        self.0.cancellation_token()
    }
}

fn config() -> RapidOcrConfig {
    let model_dir = PathBuf::from(MODEL_DIR);
    RapidOcrConfig {
        pipeline: PipelineConfig::without_cls(),
        inference: InferenceOptions {
            intra_threads: 4,
            ..Default::default()
        },
        text_score: 0.5,
        min_side_len: 30,
        max_side_len: 2000,
        min_height: 30,
        width_height_ratio: 8.0,
        det: Some(DetConfig {
            model_path: model_dir.join("PP-OCRv6_det_tiny.onnx"),
            limit_side_len: 736,
            limit_type: LimitType::Min,
            mean: [0.5, 0.5, 0.5],
            std: [0.5, 0.5, 0.5],
            thresh: 0.3,
            box_thresh: 0.5,
            max_candidates: 1000,
            unclip_ratio: 1.6,
            min_size: 3,
            input_limits: Default::default(),
        }),
        cls: None,
        rec: Some(RecConfig {
            model_path: model_dir.join("korean_PP-OCRv5_rec_mobile.onnx"),
            dict_path: model_dir.join("korean_dict.txt"),
            image_shape: [3, 48, 320],
            batch_size: 6,
        }),
    }
}

/// Tuning for merging nearby OCR text boxes into one entry.
///
/// Every margin is a ratio of the box's height, so the grouping is invariant
/// under image resolution changes: doubling the pixel dimensions doubles the
/// allowed gaps and the same lines still merge.
#[derive(Debug, Clone, Copy)]
pub struct MergeConfig {
    /// Side margin added to each box, as a ratio of its height.
    pub expand_x: f32,
    /// Top/bottom margin added to each box, as a ratio of its height.
    pub expand_y: f32,
}

impl Default for MergeConfig {
    fn default() -> Self {
        Self {
            expand_x: 0.5,
            expand_y: 0.5,
        }
    }
}

/// Convert raw engine output into model entries (auto-OCR source). Safe to
/// call from a background task; the model is untouched until appended.
///
/// Nearby text boxes are merged first (see [`MergeConfig::default`]).
pub fn to_entries(lines: Vec<OcrLine>) -> Vec<NewEntry> {
    to_entries_with(lines, MergeConfig::default())
}

/// Like [`to_entries`], but with explicit merge tuning.
pub fn to_entries_with(lines: Vec<OcrLine>, cfg: MergeConfig) -> Vec<NewEntry> {
    merge_lines(lines, cfg)
        .into_iter()
        .map(|line| NewEntry {
            source: EntrySource::AutoOcr,
            text: line.text,
            score: line.score,
            quad: Quad { points: line.bbox.points },
        })
        .collect()
}

/// Loads an image the same way the OCR engine does: EXIF orientation applied,
/// alpha flattened onto a contrast background. Returns `None` when the file
/// cannot be decoded (missing, corrupt, unsupported).
pub fn load_rgb(path: &str) -> Option<RgbImage> {
    rapidocr_core::image_ops::load_rgb_image(path).ok()
}

/// One OCR run: a contiguous span of page content.
///
/// A run covers pages `page_start..=page_end` stacked at a common width; the
/// `band` is the fraction of that stacked body the run actually OCRs (the
/// whole body `(0.0, 1.0)` for normal runs, a chunk `(c/k, (c+1)/k)` for a
/// split of a too-tall page). Margins are stitched from the bands above and
/// below, and duplicates in the top margin are deduped against the stored
/// entries of the page whose content touches the band's top edge.
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
    /// The page whose stored entries the top-margin duplicates are deduped
    /// against, plus the offset in that page's pixel space of the band's top
    /// edge. `None` only for the very first run.
    pub dedup: Option<(usize, u32)>,
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
            let height = dims[i].1;
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
                let dedup = if chunk > 0 {
                    Some((i, (band.0 * height as f32).round() as u32))
                } else {
                    i.checked_sub(1).map(|p| (p, dims[p].1))
                };
                runs.push(RunPlan {
                    page_start: i,
                    page_end: i,
                    band,
                    above,
                    below,
                    dedup,
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
                dedup: i.checked_sub(1).map(|p| (p, dims[p].1)),
            });
            i = j + 1;
        }
    }
    runs
}

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

/// Stitches `margin` pixels of `prev`'s bottom and `next`'s top around `cur`,
/// so a speech bubble cut by the page boundary still appears whole. Strips are
/// scaled to `cur`'s width; every page in the result shares one coordinate
/// space (pixels of `cur`).
pub fn stitch(prev: Option<&RgbImage>, cur: &RgbImage, next: Option<&RgbImage>, margin: u32) -> RgbImage {
    let width = cur.width();
    let top = prev.and_then(|p| bottom_margin_strip(p, (0.0, 1.0), width, margin));
    let bottom = next.and_then(|n| top_margin_strip(n, (0.0, 1.0), width, margin));
    stack_run(top, &[cur.clone()], bottom, width, (0.0, 1.0))
}

/// A bubble captured in a run's bottom margin strip: the merged line's bounds
/// in the producing run's canvas space plus the entry ready to append, in the
/// producing run's last page's pixel space with a quad possibly past that
/// page's bottom edge. The next run re-detects the same bubble in its top
/// margin; [`resolve_boundary`] keeps the fuller capture.
#[derive(Debug, Clone)]
pub struct BoundaryCandidate {
    /// AABB of the merged line in the producing run's canvas space.
    pub canvas_quad: [f32; 4],
    /// Ready-to-append entry in the last page's pixel space, quad possibly
    /// past its bottom edge.
    pub entry: NewEntry,
    /// The page the entry is anchored to.
    pub page: usize,
}

/// Boundary candidates of one run, together with the canvas metrics needed to
/// map them into the next run's canvas space ([`transform_candidates`]).
#[derive(Debug, Clone)]
pub struct BoundaryState {
    pub candidates: Vec<BoundaryCandidate>,
    /// The producing run's canvas width.
    pub width: u32,
    /// The producing run's canvas-space y of its bottom edge (the seam
    /// between its last page and the next run's first).
    pub boundary: u32,
}

/// The payload of a finished OCR run: entries grouped per page plus the
/// boundary candidates held for the next run (`None` for the last run, whose
/// bottom edge is the book's end).
#[derive(Debug, Clone)]
pub struct RunResult {
    pub per_page: Vec<(usize, Vec<NewEntry>)>,
    pub held: Option<BoundaryState>,
}

impl RunResult {
    /// Appends this run's per-page entries to the projects (indexed by page)
    /// and returns the appended count.
    pub fn commit_entries(&self, projects: &mut [Project]) -> usize {
        self.per_page
            .iter()
            .map(|(page, entries)| {
                projects
                    .get_mut(*page)
                    .map_or(0, |project| project.append_ocr(entries.clone()))
            })
            .sum()
    }
}

impl BoundaryState {
    /// Appends every held candidate to its page's project and returns the
    /// appended count. Used when a run fails, is cancelled or never starts:
    /// the captured bubbles must not be lost.
    pub fn commit(&self, projects: &mut [Project]) -> usize {
        self.candidates
            .iter()
            .map(|candidate| {
                projects
                    .get_mut(candidate.page)
                    .map_or(0, |project| project.append_ocr(vec![candidate.entry.clone()]))
            })
            .sum()
    }
}

/// Assembles one run's raw lines into a commit-ready [`RunResult`], strictly
/// in run order on the UI thread: merges nearby boxes, resolves the previous
/// run's held boundary candidates against this run's re-detections in its top
/// margin, dedups against the committed quads of the page above, distributes
/// the survivors to their pages and holds this run's own boundary candidates
/// for the next run.
///
/// `prev` is the dedup target: the committed quads of the page above, its
/// width and the offset of this run's canvas top edge in that page's pixel
/// space (`(quads, prev_width, prev_offset)`).
pub fn assemble(
    index: usize,
    width: u32,
    margin_top: u32,
    lines: Vec<OcrLine>,
    plans: &[RunPlan],
    dims: &[(u32, u32)],
    held: Option<BoundaryState>,
    prev: Option<(Vec<Quad>, u32, u32)>,
) -> RunResult {
    let run = plans[index];
    let run_dims: Vec<(usize, u32, u32)> = (run.page_start..=run.page_end)
        .map(|i| (i, dims[i].0, dims[i].1))
        .collect();
    let merged = merge(lines, MergeConfig::default());
    let (resolved, kept) = match &held {
        Some(state) => {
            let transformed = transform_candidates(
                &state.candidates,
                state.width,
                state.boundary,
                width,
                margin_top,
            );
            let resolution = resolve_boundary(&state.candidates, &transformed, merged);
            (resolution.append, resolution.kept)
        }
        None => (Vec::new(), merged),
    };
    let deduped = match &prev {
        Some((quads, prev_width, offset)) => {
            dedup_with_previous(kept, quads, *prev_width, *offset, width)
        }
        None => kept,
    };
    let out = distribute(deduped, &run_dims, run.band, margin_top);
    let mut per_page = out.per_page;
    for candidate in resolved {
        match per_page
            .iter_mut()
            .find(|(page, _)| *page == candidate.page)
        {
            Some((_, entries)) => entries.push(candidate.entry),
            None => per_page.push((candidate.page, vec![candidate.entry])),
        }
    }
    per_page.sort_by_key(|(page, _)| *page);
    eprintln!(
        "[ocr-run {index}] final per-page: {:?}",
        per_page
            .iter()
            .map(|(p, e)| format!("p{p}:{}", e.len()))
            .collect::<Vec<_>>()
    );
    let held = (!out.held.is_empty()).then(|| BoundaryState {
        candidates: out.held,
        width,
        boundary: out.boundary,
    });
    RunResult { per_page, held }
}

/// One OCR run's distributed output: per-page entries plus the boundary
/// candidates held for the next run.
#[derive(Debug, Clone)]
pub struct RunOutput {
    pub per_page: Vec<(usize, Vec<NewEntry>)>,
    pub held: Vec<BoundaryCandidate>,
    /// The canvas-space y of the run's bottom edge (`margin_top` + body
    /// height at the canvas width), identical to the seam of the following
    /// run's top margin.
    pub boundary: u32,
}

/// Maps a merged OCR run over the stitched canvas back to per-page entries in
/// each page's native pixel space.
///
/// `pages` lists the covered pages' indices and native `(width, height)` in
/// order; the canvas is `width` wide (the first page's width) with the run's
/// body starting `margin_top` pixels down and occupying the `band` fraction
/// of the stacked body. Entries above the body belong to the band above the
/// run and are assigned to the page holding the band's top edge, with quads
/// past that edge (the caller dedups them against that page's store).
/// Entries whose quad extends past the run's bottom edge — bubbles half-on/
/// half-off the seam or entirely in the bottom margin strip — are held as
/// [`BoundaryCandidate`]s instead of being stored: the next run re-detects
/// them and [`resolve_boundary`] decides which page and which capture wins.
pub fn distribute(
    lines: Vec<OcrLine>,
    pages: &[(usize, u32, u32)],
    band: (f32, f32),
    margin_top: u32,
) -> RunOutput {
    if pages.is_empty() {
        return RunOutput {
            per_page: Vec::new(),
            held: Vec::new(),
            boundary: margin_top,
        };
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
    let mut held: Vec<BoundaryCandidate> = Vec::new();
    for line in lines {
        let bounds = box_bounds(&line.bbox);
        let y0 = bounds[1];
        if bounds[3] > boundary {
            let t = page_at(band_bottom);
            let scale = scaled[t].0;
            let dy = (band_bottom - offset(t) - margin_top as f32 - total) * scale;
            held.push(BoundaryCandidate {
                canvas_quad: bounds,
                entry: NewEntry {
                    source: EntrySource::AutoOcr,
                    text: line.text,
                    score: line.score,
                    quad: Quad {
                        points: line.bbox.points.map(|[x, y]| [x * scale, y * scale + dy]),
                    },
                },
                page: pages[t].0,
            });
            continue;
        }
        let (target, dy) = if y0 < margin_top as f32 {
            let t = page_at(band_top);
            (t, (band_top - offset(t) - margin_top as f32) * scaled[t].0)
        } else {
            let t = page_at(y0 - margin_top as f32);
            (t, ((band_top - offset(t)).max(0.0) - margin_top as f32 - offset(t)) * scaled[t].0)
        };
        let scale = scaled[target].0;
        per_page[target].push(NewEntry {
            source: EntrySource::AutoOcr,
            text: line.text,
            score: line.score,
            quad: Quad {
                points: line.bbox.points.map(|[x, y]| [x * scale, y * scale + dy]),
            },
        });
    }

    RunOutput {
        per_page: pages
            .iter()
            .zip(per_page)
            .filter(|(_, entries)| !entries.is_empty())
            .map(|((index, _, _), entries)| (*index, entries))
            .collect(),
        held,
        boundary: boundary.round() as u32,
    }
}

/// Maps boundary candidates of the producing run into the next run's canvas
/// space: both canvases contain the same pixels at the seam, so `y' = (y -
/// `prev_boundary`) * scale + `new_margin_top` with `scale = new_width /
/// prev_width` is exact — the seam sits at the new canvas's top-margin strip
/// height, not at 0.
pub fn transform_candidates(
    candidates: &[BoundaryCandidate],
    prev_width: u32,
    prev_boundary: u32,
    new_width: u32,
    new_margin_top: u32,
) -> Vec<[f32; 4]> {
    let scale = new_width as f32 / prev_width.max(1) as f32;
    candidates
        .iter()
        .map(|candidate| {
            let [x0, y0, x1, y1] = candidate.canvas_quad;
            [
                x0 * scale,
                (y0 - prev_boundary as f32) * scale + new_margin_top as f32,
                x1 * scale,
                (y1 - prev_boundary as f32) * scale + new_margin_top as f32,
            ]
        })
        .collect()
}

/// Outcome of [`resolve_boundary`].
#[derive(Debug, Clone)]
pub struct Resolution {
    /// Candidates to append to their pages: the candidate's capture beat its
    /// re-detection, or no re-detection appeared and the capture is flushed
    /// as-is. Entries are ready in their pages' pixel space.
    pub append: Vec<BoundaryCandidate>,
    /// Lines that survived: a re-detection beat its candidate, or the line
    /// matched no candidate. They continue through dedup and distribute.
    pub kept: Vec<OcrLine>,
}

/// Resolves the producing run's held boundary candidates against this run's
/// re-detections in its top margin.
///
/// `transformed` are the candidates' quads mapped into this run's canvas
/// space by [`transform_candidates`]; each is greedily matched to the merged
/// line whose bbox overlaps it most (each line matched at most once). Per
/// pair the fuller capture wins — the capture anchored on the page containing
/// more of the bubble shows more of it, so comparing captured areas picks the
/// true read. On equal captures the page holding more of the bubble (its
/// extent above vs below the seam) wins, which also fixes bubbles entirely
/// inside the margin onto the page below. A winning candidate's entry is
/// returned for append; a winning re-detection survives and flows through
/// [`distribute`], which already stores top-margin entries past the seam on
/// the run's first page. Unmatched candidates and lines are kept.
pub fn resolve_boundary(
    candidates: &[BoundaryCandidate],
    transformed: &[[f32; 4]],
    lines: Vec<OcrLine>,
) -> Resolution {
    let mut kept = lines;
    // `matched[i]`: line already paired with a candidate (not re-matchable);
    // `dropped[i]`: line consumed by a winning candidate (removed from kept).
    let mut matched = vec![false; kept.len()];
    let mut dropped = vec![false; kept.len()];
    let mut append: Vec<BoundaryCandidate> = Vec::new();

    for (candidate, quad) in candidates.iter().zip(transformed) {
        let mut best = None;
        let mut best_overlap = 0.0f32;
        for (i, line) in kept.iter().enumerate() {
            if matched[i] {
                continue;
            }
            let overlap = overlap_area(*quad, box_bounds(&line.bbox));
            if overlap > best_overlap {
                best_overlap = overlap;
                best = Some(i);
            }
        }
        let Some(i) = best else {
            append.push(candidate.clone());
            continue;
        };
        matched[i] = true;
        let cand_area = quad_area(*quad);
        let line_area = quad_area(box_bounds(&kept[i].bbox));
        let bigger = cand_area.max(line_area);
        // Within 10% the captures are effectively equal (OCR jitter): the
        // page holding more of the bubble decides. Beyond it the fuller
        // capture wins outright.
        let tie = (cand_area - line_area).abs() <= 0.1 * bigger;
        let below_wins = if tie {
            let above = -quad[1].min(0.0);
            let below = quad[3].max(0.0);
            below >= above
        } else {
            line_area > cand_area
        };
        let merged_text = merge_texts(&kept[i].text, &candidate.entry.text);
        if below_wins {
            // The re-detection won: it survives with the candidate's extra
            // text and bbox merged in, then flows through dedup and
            // distribute.
            kept[i].text = merged_text;
            let [x0, y0, x1, y1] = box_bounds(&kept[i].bbox);
            kept[i].bbox = rapidocr_core::types::Quad::from_xyxy(
                x0.min(quad[0]),
                y0.min(quad[1]),
                x1.max(quad[2]),
                y1.max(quad[3]),
            );
        } else {
            // The candidate's capture won: drop the re-detection and append
            // the ready-to-append entry, carrying the re-detection's fuller
            // text when it covers the candidate's.
            dropped[i] = true;
            let mut entry = candidate.clone();
            entry.entry.text = merge_texts(&candidate.entry.text, &kept[i].text);
            append.push(entry);
        }
    }

    Resolution {
        append,
        kept: kept
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !dropped[*i])
            .map(|(_, line)| line)
            .collect(),
    }
}

/// Area of an AABB (zero for degenerate boxes).
fn quad_area(bounds: [f32; 4]) -> f32 {
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

/// Area of the intersection of two AABBs (zero when disjoint).
fn overlap_area(a: [f32; 4], b: [f32; 4]) -> f32 {
    let w = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let h = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    w * h
}

/// Deduplicates `lines` against the stored quads of the page content directly
/// above the run.
///
/// The run's top margin re-detects bubbles already captured by the run that
/// covered the band above (that page's stored entries include quads past its
/// own band edge into this run's band), so both copies describe the same
/// bubble and the copy here is dropped when its AABB overlaps a previous quad
/// transformed into this run's canvas space: scaled by the width ratio and
/// shifted up by `prev_offset` — the offset in the previous page's pixel
/// space of the top edge of this run's canvas (the previous page's height for
/// whole-page runs, the chunk's top for splits of a too-tall page). Only
/// entries near the run's top edge can match: transformed previous quads all
/// sit at `y <= 0` plus the margin overlap.
pub fn dedup_with_previous(
    lines: Vec<OcrLine>,
    prev_quads: &[Quad],
    prev_width: u32,
    prev_offset: u32,
    cur_width: u32,
) -> Vec<OcrLine> {
    if prev_quads.is_empty() || prev_width == 0 || cur_width == 0 {
        return lines;
    }
    let scale = cur_width as f32 / prev_width as f32;
    let prev: Vec<[f32; 4]> = prev_quads
        .iter()
        .map(|quad| {
            let [min_x, min_y, max_x, max_y] = quad.bounds();
            [
                min_x * scale,
                (min_y - prev_offset as f32) * scale,
                max_x * scale,
                (max_y - prev_offset as f32) * scale,
            ]
        })
        .collect();
    let overlaps = |bounds: [f32; 4]| -> bool {
        // A couple of pixels of slack absorbs OCR jitter between runs.
        let padded = [bounds[0] - 2.0, bounds[1] - 2.0, bounds[2] + 2.0, bounds[3] + 2.0];
        prev.iter().any(|p| {
            !(p[2] < padded[0] || p[0] > padded[2] || p[3] < padded[1] || p[1] > padded[3])
        })
    };
    lines
        .into_iter()
        .filter(|line| !overlaps(box_bounds(&line.bbox)))
        .collect()
}

/// AABBs of a detected text box as `[min_x, min_y, max_x, max_y]`.
fn box_bounds(quad: &rapidocr_core::types::Quad) -> [f32; 4] {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for point in &quad.points {
        min_x = min_x.min(point[0]);
        min_y = min_y.min(point[1]);
        max_x = max_x.max(point[0]);
        max_y = max_y.max(point[1]);
    }
    [min_x, min_y, max_x, max_y]
}

/// One cluster of merged lines: the union AABB plus the member lines.
#[derive(Default)]
struct Group {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    lines: Vec<OcrLine>,
}

impl Group {
    fn intersect(&self, other: [f32; 4]) -> bool {
        !(self.max_x < other[0]
            || self.min_x > other[2]
            || self.max_y < other[1]
            || self.min_y > other[3])
    }
}

/// Groups nearby OCR lines into one merged line per cluster, in the canvas's
/// coordinate space.
///
/// Two boxes count as nearby when their AABBs intersect after each side is
/// expanded by a configurable fraction of its own height. Overlaps, small
/// gaps and near-distance boxes therefore all merge, and merging is
/// transitive: A near B and B near C also pulls A and C together. Merged
/// text is joined with single spaces in reading order (top-to-bottom, then
/// left-to-right); the merged score is the mean of its lines.
pub fn merge(lines: Vec<OcrLine>, cfg: MergeConfig) -> Vec<OcrLine> {
    merge_lines(lines, cfg)
}

fn merge_lines(lines: Vec<OcrLine>, cfg: MergeConfig) -> Vec<OcrLine> {
    let mut lines = lines;
    lines.sort_by(|a, b| {
        let [ax0, ay0, _, ay1] = box_bounds(&a.bbox);
        let [bx0, by0, _, by1] = box_bounds(&b.bbox);
        let a_mid = (ay0 + ay1) / 2.0;
        let b_mid = (by0 + by1) / 2.0;
        a_mid.total_cmp(&b_mid).then(ax0.total_cmp(&bx0))
    });

    let mut groups: Vec<Group> = Vec::new();
    for line in lines {
        let [lx0, ly0, lx1, ly1] = box_bounds(&line.bbox);
        let height = (ly1 - ly0).max(1.0);
        let expanded = [
            lx0 - cfg.expand_x * height,
            ly0 - cfg.expand_y * height,
            lx1 + cfg.expand_x * height,
            ly1 + cfg.expand_y * height,
        ];

        let mut hits: Vec<usize> = groups
            .iter()
            .enumerate()
            .filter(|(_, group)| group.intersect(expanded))
            .map(|(index, _)| index)
            .collect();

        if hits.is_empty() {
            groups.push(Group {
                min_x: lx0,
                min_y: ly0,
                max_x: lx1,
                max_y: ly1,
                lines: vec![line],
            });
        } else {
            let first = hits.remove(0);
            let mut target = std::mem::take(&mut groups[first]);
            hits.sort_by(|a, b| b.cmp(a));
            for index in hits {
                let merged = groups.remove(index);
                target.min_x = target.min_x.min(merged.min_x);
                target.min_y = target.min_y.min(merged.min_y);
                target.max_x = target.max_x.max(merged.max_x);
                target.max_y = target.max_y.max(merged.max_y);
                target.lines.extend(merged.lines);
            }
            target.min_x = target.min_x.min(lx0);
            target.min_y = target.min_y.min(ly0);
            target.max_x = target.max_x.max(lx1);
            target.max_y = target.max_y.max(ly1);
            target.lines.push(line);
            groups[first] = target;
        }
    }

    groups
        .into_iter()
        .map(|group| {
            let text = group
                .lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let score = group
                .lines
                .iter()
                .map(|line| line.score)
                .sum::<f32>()
                / group.lines.len().max(1) as f32;
            OcrLine {
                bbox: rapidocr_core::types::Quad::from_xyxy(
                    group.min_x, group.min_y, group.max_x, group.max_y,
                ),
                text,
                score,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, x0: f32, y0: f32, x1: f32, y1: f32, score: f32) -> OcrLine {
        OcrLine {
            bbox: rapidocr_core::types::Quad::from_xyxy(x0, y0, x1, y1),
            text: text.to_string(),
            score,
        }
    }

    #[test]
    fn merges_gapped_lines_into_one_entry() {
        let lines = vec![
            line("first", 10.0, 10.0, 60.0, 30.0, 0.9),
            line("second", 65.0, 10.0, 120.0, 30.0, 0.7),
        ];
        let entries = to_entries(lines);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "first second");
        assert!((entries[0].score - 0.8).abs() < 1e-6);
        let [min_x, min_y, max_x, max_y] = entries[0].quad.bounds();
        assert_eq!([min_x, min_y, max_x, max_y], [10.0, 10.0, 120.0, 30.0]);
    }

    #[test]
    fn keeps_far_apart_boxes_separate() {
        let lines = vec![
            line("first", 10.0, 10.0, 60.0, 30.0, 0.9),
            line("second", 200.0, 10.0, 260.0, 30.0, 0.7),
        ];
        let entries = to_entries(lines);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "first");
        assert_eq!(entries[1].text, "second");
    }

    #[test]
    fn merging_is_transitive() {
        let lines = vec![
            line("a", 10.0, 10.0, 40.0, 30.0, 0.9),
            line("c", 80.0, 10.0, 110.0, 30.0, 0.7),
            line("b", 45.0, 10.0, 75.0, 30.0, 0.8),
        ];
        let entries = to_entries(lines);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "a b c");
    }

    #[test]
    fn grouping_is_resolution_invariant() {
        let low = vec![
            line("first", 10.0, 10.0, 60.0, 30.0, 0.9),
            line("second", 65.0, 10.0, 120.0, 30.0, 0.7),
            line("far", 200.0, 10.0, 260.0, 30.0, 0.8),
        ];
        let high = vec![
            line("first", 20.0, 20.0, 120.0, 60.0, 0.9),
            line("second", 130.0, 20.0, 240.0, 60.0, 0.7),
            line("far", 400.0, 20.0, 520.0, 60.0, 0.8),
        ];
        let low_groups: Vec<String> = to_entries(low)
            .into_iter()
            .map(|e| e.text)
            .collect();
        let high_groups: Vec<String> = to_entries(high)
            .into_iter()
            .map(|e| e.text)
            .collect();
        assert_eq!(low_groups, high_groups);
        assert_eq!(low_groups, vec!["first second".to_string(), "far".to_string()]);
    }

    #[test]
    fn zero_expansion_merges_only_touching_or_overlapping() {
        let cfg = MergeConfig {
            expand_x: 0.0,
            expand_y: 0.0,
        };
        let touching = vec![
            line("a", 10.0, 10.0, 60.0, 30.0, 0.9),
            line("b", 60.0, 10.0, 120.0, 30.0, 0.7),
        ];
        assert_eq!(to_entries_with(touching, cfg).len(), 1);

        let gapped = vec![
            line("a", 10.0, 10.0, 60.0, 30.0, 0.9),
            line("b", 65.0, 10.0, 120.0, 30.0, 0.7),
        ];
        assert_eq!(to_entries_with(gapped, cfg).len(), 2);
    }

    #[test]
    fn empty_lines_yield_no_entries() {
        assert!(to_entries(vec![]).is_empty());
    }

    fn rgb(w: u32, h: u32, fill: [u8; 3]) -> RgbImage {
        use image::Rgb;
        RgbImage::from_pixel(w, h, Rgb(fill))
    }

    fn quad_xyxy(x0: f32, y0: f32, x1: f32, y1: f32) -> Quad {
        Quad {
            points: [[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
        }
    }

    #[test]
    fn stitch_places_strips_around_the_page() {
        let prev = rgb(100, 50, [10, 0, 0]);
        let cur = rgb(100, 40, [0, 20, 0]);
        let next = rgb(100, 60, [0, 0, 30]);
        let out = stitch(Some(&prev), &cur, Some(&next), 20);
        assert_eq!((out.width(), out.height()), (100, 20 + 40 + 20));
        assert_eq!(out.get_pixel(50, 0).0, [10, 0, 0], "top strip from prev");
        assert_eq!(out.get_pixel(50, 20).0, [0, 20, 0], "page body");
        assert_eq!(out.get_pixel(50, 60).0, [0, 0, 30], "bottom strip from next");
    }

    #[test]
    fn stitch_without_neighbors_is_the_plain_page() {
        let cur = rgb(80, 50, [5, 5, 5]);
        let out = stitch(None, &cur, None, 200);
        assert_eq!((out.width(), out.height()), (80, 50));
    }

    #[test]
    fn stitch_scales_strips_to_page_width() {
        let prev = rgb(200, 100, [9, 9, 9]);
        let cur = rgb(100, 40, [0, 20, 0]);
        let out = stitch(Some(&prev), &cur, None, 100);
        assert_eq!((out.width(), out.height()), (100, 50 + 40));
        assert_eq!(out.get_pixel(50, 25).0, [9, 9, 9], "strip scaled to width 100");
    }

    #[test]
    fn stitch_drops_strips_smaller_than_the_margin() {
        let short = rgb(100, 10, [7, 7, 7]);
        let cur = rgb(100, 40, [0, 20, 0]);
        let out = stitch(Some(&short), &cur, Some(&short), 200);
        assert_eq!((out.width(), out.height()), (100, 40), "margins skipped");
    }

    #[test]
    fn stitched_entries_are_translated_into_page_coordinates() {
        let lines = vec![
            line("above", 10.0, 150.0, 90.0, 180.0, 0.9),
            line("middle", 10.0, 250.0, 90.0, 280.0, 0.9),
            line("below", 10.0, 650.0, 90.0, 680.0, 0.9),
        ];
        let out = distribute(merge(lines, MergeConfig::default()), &[(0, 100, 400)], (0.0, 1.0), 200);
        let entries = &out.per_page[0].1;
        let bounds: Vec<[f32; 4]> = entries.iter().map(|e| e.quad.bounds()).collect();
        assert_eq!(bounds[0], [10.0, -50.0, 90.0, -20.0], "top margin, out of page");
        assert_eq!(bounds[1], [10.0, 50.0, 90.0, 80.0], "page body");
        assert_eq!(out.held.len(), 1, "bottom margin must be held, not stored");
        assert_eq!(out.held[0].canvas_quad, [10.0, 650.0, 90.0, 680.0]);
        assert_eq!(out.held[0].page, 0);
        assert_eq!(out.held[0].entry.quad.bounds(), [10.0, 450.0, 90.0, 480.0]);
        assert_eq!(out.boundary, 600, "seam at the page's bottom edge");
    }

    #[test]
    fn stitched_merging_keeps_boundary_bubbles_whole() {
        let lines = vec![
            line("first", 10.0, 180.0, 60.0, 210.0, 0.9),
            line("half", 10.0, 215.0, 60.0, 240.0, 0.7),
        ];
        let per_page = distribute(merge(lines, MergeConfig::default()), &[(0, 100, 400)], (0.0, 1.0), 200).per_page;
        assert_eq!(per_page[0].1.len(), 1, "split bubble must merge into one entry");
        assert_eq!(per_page[0].1[0].text, "first half");
    }

    #[test]
    fn distribute_maps_entries_to_their_pages_in_native_pixels() {
        // Two pages stitched at 100px wide: 100x200 (page 0) + 100x300 (page 1).
        let lines = vec![
            line("p0", 10.0, 210.0, 90.0, 240.0, 0.9),
            line("p1", 10.0, 450.0, 90.0, 480.0, 0.9),
            line("below", 10.0, 710.0, 90.0, 730.0, 0.9),
        ];
        let out = distribute(lines, &[(0, 100, 200), (1, 100, 300)], (0.0, 1.0), 200);
        assert_eq!(out.per_page.len(), 2);
        assert_eq!(out.per_page[0].0, 0);
        assert_eq!(out.per_page[1].0, 1);
        assert_eq!(out.per_page[0].1.len(), 1);
        assert_eq!(out.per_page[1].1.len(), 1);
        assert_eq!(out.per_page[0].1[0].text, "p0");
        assert_eq!(out.per_page[0].1[0].quad.bounds(), [10.0, 10.0, 90.0, 40.0]);
        assert_eq!(out.per_page[1].1[0].text, "p1");
        assert_eq!(out.per_page[1].1[0].quad.bounds(), [10.0, 50.0, 90.0, 80.0]);
        assert_eq!(out.held.len(), 1, "bottom margin entry held for the next run");
        assert_eq!(out.held[0].page, 1);
        assert_eq!(out.held[0].entry.text, "below");
        assert_eq!(out.held[0].entry.quad.bounds(), [10.0, 310.0, 90.0, 330.0]);
        assert_eq!(out.held[0].canvas_quad, [10.0, 710.0, 90.0, 730.0]);
        assert_eq!(out.boundary, 700);
    }

    #[test]
    fn distribute_scales_back_entries_of_narrower_pages() {
        // Page 0 is 200 wide, page 1 is 100 wide; the canvas is 200 wide, so
        // page 1 is scaled up 2x and its entries must scale back by 0.5.
        let lines = vec![line("w", 40.0, 410.0, 120.0, 430.0, 0.9)];
        let per_page = distribute(lines, &[(0, 200, 200), (1, 100, 150)], (0.0, 1.0), 200).per_page;
        assert_eq!(per_page.len(), 1);
        assert_eq!(per_page[0].0, 1);
        assert_eq!(per_page[0].1[0].quad.bounds(), [20.0, 5.0, 60.0, 15.0]);
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
        let per_page = distribute(lines, &[(0, 100, 600)], (0.5, 1.0), 200).per_page;
        assert_eq!(per_page.len(), 1);
        let entries = &per_page[0].1;
        assert_eq!(entries[0].text, "chunk");
        assert_eq!(entries[0].quad.bounds(), [10.0, 350.0, 90.0, 380.0]);
        assert_eq!(entries[1].text, "edge");
        assert_eq!(entries[1].quad.bounds(), [10.0, 290.0, 90.0, 310.0]);
    }

    #[test]
    fn distribute_without_lines_yields_no_entries() {
        assert!(distribute(vec![], &[(0, 100, 200)], (0.0, 1.0), 200).per_page.is_empty());
        let out = distribute(vec![line("x", 0.0, 0.0, 10.0, 10.0, 0.9)], &[], (0.0, 1.0), 200);
        assert!(out.per_page.is_empty());
        assert!(out.held.is_empty());
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
        assert_eq!(runs[0].dedup, None);
        assert_eq!(runs[1].above, Some((0, (0.0, 1.0))));
        assert_eq!(runs[1].below, Some((2, (0.0, 1.0))));
        assert_eq!(runs[1].dedup, Some((0, 3000)));
        assert_eq!(runs[2].dedup, Some((1, 2400)));
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
        assert_eq!(runs[0].dedup, None);
        assert_eq!((runs[1].page_start, runs[1].page_end), (2, 2));
        assert_eq!(runs[1].above, Some((1, (0.0, 1.0))));
        assert_eq!(runs[1].dedup, Some((1, 1400)));
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
        assert_eq!(runs[1].dedup, Some((0, 1200)));
        assert_eq!(runs[2].above, Some((1, (0.0, 0.5))));
        assert_eq!(runs[2].dedup, Some((1, 4000)));
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
        assert_eq!(runs[0].dedup, None);
        assert_eq!(runs[1].band, (0.5, 1.0));
        assert_eq!(runs[1].above, Some((0, (0.0, 0.5))));
        assert_eq!(runs[1].dedup, Some((0, 4000)));
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
    fn dedup_drops_the_repeat_of_a_spanning_bubble() {
        // Previous page (height 500) captured the bubble in its bottom margin
        // with a quad sticking 50px past its own bottom edge.
        let prev = vec![quad_xyxy(20.0, 450.0, 80.0, 550.0)];
        let cur = vec![
            line("span", 20.0, -50.0, 80.0, 50.0, 0.9),
            line("own", 20.0, 120.0, 80.0, 150.0, 0.9),
        ];
        let kept = dedup_with_previous(cur, &prev, 100, 500, 100);
        assert_eq!(kept.len(), 1, "spanning bubble deduped against previous page");
        assert_eq!(kept[0].text, "own");
    }

    #[test]
    fn dedup_scales_coordinates_for_different_page_widths() {
        // Previous page is twice as wide; its bottom margin maps to the
        // current page's space via the same scale the stitch used.
        let prev = vec![quad_xyxy(80.0, 450.0, 160.0, 540.0)];
        let cur = vec![line("span", 40.0, -40.0, 80.0, 20.0, 0.9)];
        let kept = dedup_with_previous(cur, &prev, 200, 500, 100);
        assert!(kept.is_empty(), "scaled prev quad must still overlap the copy");
    }

    #[test]
    fn dedup_uses_chunk_offset_inside_the_same_page() {
        // Page height 600 split into two 300px chunks. The first chunk's run
        // stored bubbles past its own band edge (into the second chunk); the
        // second chunk dedups against them with offset = 300.
        let prev = vec![quad_xyxy(10.0, 290.0, 60.0, 330.0)];
        let cur = vec![line("span", 10.0, -10.0, 60.0, 30.0, 0.9)];
        let kept = dedup_with_previous(cur, &prev, 100, 300, 100);
        assert!(kept.is_empty(), "chunk boundary duplicate must be dropped");
    }

    #[test]
    fn dedup_keeps_distinct_bubbles_even_in_the_strip() {
        let prev = vec![quad_xyxy(10.0, 460.0, 40.0, 480.0)];
        let cur = vec![line("other", 60.0, 10.0, 90.0, 40.0, 0.9)];
        let kept = dedup_with_previous(cur, &prev, 100, 500, 100);
        assert_eq!(kept.len(), 1, "non-overlapping boxes must survive");
    }

    #[test]
    fn dedup_without_previous_data_keeps_everything() {
        let lines = vec![line("x", 0.0, -10.0, 10.0, 10.0, 0.9)];
        assert_eq!(dedup_with_previous(lines.clone(), &[], 100, 500, 100).len(), 1);
        assert_eq!(dedup_with_previous(lines, &[quad_xyxy(0.0, 0.0, 1.0, 1.0)], 0, 500, 100).len(), 1);
    }

    fn candidate(canvas: [f32; 4], page: usize) -> BoundaryCandidate {
        candidate_text(canvas, page, "held")
    }

    fn candidate_text(canvas: [f32; 4], page: usize, text: &str) -> BoundaryCandidate {
        BoundaryCandidate {
            canvas_quad: canvas,
            entry: NewEntry {
                source: EntrySource::AutoOcr,
                text: text.to_string(),
                score: 0.9,
                quad: Quad {
                    points: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                },
            },
            page,
        }
    }

    /// Resolves `candidates` (already transformed) against `lines` and returns
    /// `(appended pages' entries texts, kept lines' texts)`.
    fn resolve(
        candidates: &[BoundaryCandidate],
        transformed: &[[f32; 4]],
        lines: Vec<OcrLine>,
    ) -> (Vec<String>, Vec<String>) {
        let out = resolve_boundary(candidates, transformed, lines);
        (
            out.append.iter().map(|c| c.entry.text.clone()).collect(),
            out.kept.iter().map(|l| l.text.clone()).collect(),
        )
    }

    #[test]
    fn transform_candidates_maps_candidates_into_the_next_canvas() {
        let candidates = vec![candidate([10.0, 590.0, 90.0, 650.0], 0)];
        let transformed = transform_candidates(&candidates, 100, 600, 200, 0);
        assert_eq!(transformed, vec![[20.0, -20.0, 180.0, 100.0]], "scale + seam shift");
    }

    #[test]
    fn transform_candidates_keeps_the_seam_at_zero() {
        // Different widths: everything above the seam maps negative, below
        // positive; the seam itself maps to 0.
        let candidates = vec![candidate([0.0, 100.0, 50.0, 700.0], 0)];
        let transformed = transform_candidates(&candidates, 200, 600, 100, 0);
        assert_eq!(transformed, vec![[0.0, -250.0, 25.0, 50.0]]);
    }

    #[test]
    fn transform_candidates_shifts_by_the_new_canvas_top_margin() {
        // The next run's canvas starts with its own top-margin strip, so the
        // seam lands at the strip's height, not at 0.
        let candidates = vec![candidate([10.0, 590.0, 90.0, 650.0], 0)];
        let transformed = transform_candidates(&candidates, 100, 600, 200, 320);
        assert_eq!(transformed, vec![[20.0, 300.0, 180.0, 420.0]]);
    }

    #[test]
    fn resolve_keeps_the_fuller_capture_on_the_page_with_more_area() {
        // Bubble mostly in page k (top -150..-60) with its bottom cut in the
        // candidate's canvas (ends at +90): the re-detection shows it fully
        // (to +90) but crops the top at -100. The candidate is fuller.
        let candidates = vec![candidate([0.0, -150.0, 100.0, 90.0], 0)];
        let (append, kept) = resolve(&candidates, &[[0.0, -150.0, 100.0, 90.0]], vec![
            line("redo", 0.0, -100.0, 100.0, 90.0, 0.9),
        ]);
        assert_eq!(append, vec!["held".to_string()], "candidate capture wins");
        assert!(kept.is_empty(), "losing re-detection must be dropped");
    }

    #[test]
    fn resolve_prefers_the_re_detection_when_it_is_fuller() {
        // Bubble mostly in page k+1 (bottom to +150) with its top cut in the
        // re-detection's canvas (starts at -50); the candidate's capture was
        // cropped at the margin (+100). The re-detection is fuller.
        let candidates = vec![candidate([0.0, -50.0, 100.0, 100.0], 0)];
        let (append, kept) = resolve(&candidates, &[[0.0, -50.0, 100.0, 100.0]], vec![
            line("redo", 0.0, -50.0, 100.0, 150.0, 0.9),
        ]);
        assert!(append.is_empty(), "candidate must lose");
        assert_eq!(kept, vec!["redo".to_string()], "re-detection flows to distribute");
    }

    #[test]
    fn resolve_sends_equal_captures_to_the_page_holding_more_of_the_bubble() {
        // Small bubble fully inside the overlap band: both captures are full
        // (equal areas), so the page with more of the bubble decides.
        let mostly_above = vec![candidate([0.0, -100.0, 100.0, 90.0], 0)];
        let (append, kept) = resolve(&mostly_above, &[[0.0, -100.0, 100.0, 90.0]], vec![
            line("redo", 0.0, -100.0, 100.0, 90.0, 0.9),
        ]);
        assert_eq!(append, vec!["held".to_string()], "more area above the seam");
        assert!(kept.is_empty());

        let mostly_below = vec![candidate([0.0, -90.0, 100.0, 100.0], 0)];
        let (append, kept) = resolve(&mostly_below, &[[0.0, -90.0, 100.0, 100.0]], vec![
            line("redo", 0.0, -90.0, 100.0, 100.0, 0.9),
        ]);
        assert!(append.is_empty(), "more area below the seam");
        assert_eq!(kept, vec!["redo".to_string()]);
    }

    #[test]
    fn resolve_flushes_unmatched_candidates_and_keeps_unmatched_lines() {
        let candidates = vec![candidate([0.0, -100.0, 100.0, 90.0], 0)];
        let (append, kept) = resolve(&candidates, &[[0.0, -100.0, 100.0, 90.0]], vec![
            line("own", 0.0, 500.0, 100.0, 530.0, 0.9),
        ]);
        assert_eq!(append, vec!["held".to_string()], "no re-detection: flushed");
        assert_eq!(kept, vec!["own".to_string()]);
    }

    #[test]
    fn resolve_matches_each_line_once() {
        // Two candidates but only one re-detection: the second candidate is
        // flushed (its entry kept), the consumed line loses.
        let candidates = vec![
            candidate([0.0, -150.0, 100.0, 90.0], 0),
            candidate([200.0, -150.0, 300.0, 90.0], 0),
        ];
        let transformed = vec![[0.0, -150.0, 100.0, 90.0], [200.0, -150.0, 300.0, 90.0]];
        let (append, kept) = resolve(&candidates, &transformed, vec![
            line("redo", 0.0, -100.0, 100.0, 90.0, 0.9),
        ]);
        assert_eq!(append.len(), 2, "winner plus the unmatched flush");
        assert!(kept.is_empty());
    }

    #[test]
    fn resolve_merges_the_full_containing_text_into_a_winning_candidate() {
        // The candidate's capture is fuller but was cut differently: the
        // re-detection read the leading dots the candidate missed. The
        // candidate wins the area but its text must carry the dots.
        let candidates = vec![candidate_text([0.0, -150.0, 100.0, 90.0], 0, "떠나시는 겁니까")];
        let (append, kept) = resolve(&candidates, &[[0.0, -150.0, 100.0, 90.0]], vec![
            line("..떠나시는 겁니까", 0.0, -100.0, 100.0, 90.0, 0.9),
        ]);
        assert_eq!(append, vec!["..떠나시는 겁니까".to_string()]);
        assert!(kept.is_empty(), "losing re-detection must be dropped");
    }

    #[test]
    fn resolve_merges_the_candidate_text_into_a_winning_redetection() {
        // The re-detection is fuller; the candidate's text adds a part the
        // re-detection's canvas cut. The kept line must carry both.
        let candidates = vec![candidate_text([0.0, -150.0, 100.0, 90.0], 0, "떠나시는")];
        let (append, kept) = resolve(&candidates, &[[0.0, -150.0, 100.0, 90.0]], vec![
            line("..떠나시는 겁니까", 0.0, -150.0, 100.0, 200.0, 0.9),
        ]);
        assert!(append.is_empty());
        assert_eq!(kept, vec!["..떠나시는 겁니까".to_string()]);
    }

    #[test]
    fn resolve_unions_the_winning_redetection_bbox_with_the_candidate() {
        let candidates = vec![candidate([0.0, -150.0, 100.0, 90.0], 0)];
        let out = resolve_boundary(&candidates, &[[0.0, -150.0, 100.0, 90.0]], vec![
            line("redo", 0.0, -150.0, 100.0, 200.0, 0.9),
        ]);
        assert!(out.append.is_empty());
        assert_eq!(out.kept.len(), 1);
        assert_eq!(
            box_bounds(&out.kept[0].bbox),
            [0.0, -150.0, 100.0, 200.0],
            "union of the candidate and re-detection boxes"
        );
    }

    #[test]
    fn resolve_keeps_the_winner_text_when_neither_capture_contains_the_other() {
        // The re-detection wins; the candidate's text is a cut misread of the
        // same sliver, not extra text — it must not be concatenated in.
        let candidates = vec![candidate_text([0.0, -150.0, 100.0, 90.0], 0, "전하를 지켜즈게")];
        let (append, kept) = resolve(&candidates, &[[0.0, -150.0, 100.0, 90.0]], vec![
            line("..전하를 지켜주게 숙빈", 0.0, -150.0, 100.0, 200.0, 0.9),
        ]);
        assert!(append.is_empty());
        assert_eq!(kept, vec!["..전하를 지켜주게 숙빈".to_string()]);
    }

    fn entry(text: &str) -> NewEntry {
        NewEntry {
            source: EntrySource::AutoOcr,
            text: text.to_string(),
            score: 0.9,
            quad: quad_xyxy(0.0, 0.0, 10.0, 10.0),
        }
    }

    fn plan(page_start: usize, page_end: usize, band: (f32, f32), dedup: Option<(usize, u32)>) -> RunPlan {
        RunPlan {
            page_start,
            page_end,
            band,
            above: None,
            below: None,
            dedup,
        }
    }

    #[test]
    fn assemble_maps_a_whole_page_run_to_per_page_entries() {
        let plans = vec![plan(0, 1, (0.0, 1.0), None)];
        let dims = [(100, 200), (100, 300)];
        let lines = vec![
            line("p0", 10.0, 210.0, 90.0, 240.0, 0.9),
            line("p1", 10.0, 450.0, 90.0, 480.0, 0.9),
        ];
        let result = assemble(0, 100, 200, lines, &plans, &dims, None, None);
        assert_eq!(result.per_page.len(), 2);
        assert_eq!(result.per_page[0].0, 0);
        assert_eq!(result.per_page[0].1[0].text, "p0");
        assert_eq!(result.per_page[0].1[0].quad.bounds(), [10.0, 10.0, 90.0, 40.0]);
        assert_eq!(result.per_page[1].0, 1);
        assert_eq!(result.per_page[1].1[0].text, "p1");
        assert_eq!(result.per_page[1].1[0].quad.bounds(), [10.0, 50.0, 90.0, 80.0]);
        assert!(result.held.is_none());
    }

    #[test]
    fn assemble_resolves_held_boundary_candidates_against_the_next_run() {
        // Run 0 covers page 0 (100x400) and holds a bubble captured in its
        // bottom margin (the capture is held, not stored); run 1 re-detects
        // the same bubble in its top margin.
        let plans = vec![
            plan(0, 0, (0.0, 1.0), None),
            plan(1, 1, (0.0, 1.0), Some((0, 400))),
        ];
        let dims = [(100, 400), (100, 400)];
        let first = assemble(
            0,
            100,
            200,
            vec![line("held", 10.0, 590.0, 90.0, 640.0, 0.9)],
            &plans,
            &dims,
            None,
            None,
        );
        assert!(first.per_page.is_empty(), "bottom-margin bubbles are held, not stored");
        let held = first.held.expect("run 0 holds its bottom-margin capture");
        assert_eq!(held.candidates.len(), 1);
        assert_eq!(held.width, 100);
        assert_eq!(held.boundary, 600);

        // Run 1's canvas starts with a 40px top strip: the seam sits at y 40
        // and page 0 committed nothing (the capture was held, not stored).
        let second = assemble(
            1,
            100,
            40,
            vec![line("redo", 10.0, 30.0, 90.0, 80.0, 0.9)],
            &plans,
            &dims,
            Some(held),
            Some((Vec::new(), 100, 400)),
        );
        assert_eq!(second.per_page.len(), 1);
        assert_eq!(second.per_page[0].0, 1);
        assert_eq!(second.per_page[0].1[0].text, "redo", "re-detection wins the tie");
        assert_eq!(second.per_page[0].1[0].quad.bounds(), [10.0, -10.0, 90.0, 40.0]);
        assert!(second.held.is_none());
    }

    #[test]
    fn assemble_dedups_against_the_previous_page_quads() {
        // Page 0's committed store has an entry whose quad sticks 50px past
        // the page's bottom edge; run 1's top strip re-detects it as a
        // duplicate and must drop the copy.
        let plans = vec![plan(1, 1, (0.0, 1.0), Some((0, 500)))];
        let dims = [(100, 500), (100, 500)];
        let prev_quads = vec![quad_xyxy(20.0, 450.0, 80.0, 550.0)];
        let result = assemble(
            0,
            100,
            50,
            vec![
                line("span", 20.0, -50.0, 80.0, 50.0, 0.9),
                line("own", 20.0, 120.0, 80.0, 150.0, 0.9),
            ],
            &plans,
            &dims,
            None,
            Some((prev_quads, 100, 500)),
        );
        assert_eq!(result.per_page.len(), 1);
        assert_eq!(result.per_page[0].0, 1);
        assert_eq!(result.per_page[0].1.len(), 1);
        assert_eq!(result.per_page[0].1[0].text, "own");
        assert_eq!(result.per_page[0].1[0].quad.bounds(), [20.0, 70.0, 80.0, 100.0]);
        assert!(result.held.is_none());
    }

    #[test]
    fn assemble_maps_chunk_bands_into_the_page() {
        // A too-tall page split in half: the run covers band 0.5..1.0 of page
        // 0 with a 200px top strip.
        let plans = vec![plan(0, 0, (0.5, 1.0), Some((0, 300)))];
        let dims = [(100, 600)];
        let result = assemble(
            0,
            100,
            200,
            vec![
                line("chunk", 10.0, 250.0, 90.0, 280.0, 0.9),
                line("edge", 10.0, 190.0, 90.0, 210.0, 0.9),
            ],
            &plans,
            &dims,
            None,
            None,
        );
        assert_eq!(result.per_page.len(), 1);
        let entries = &result.per_page[0].1;
        assert_eq!(entries[0].text, "edge");
        assert_eq!(entries[0].quad.bounds(), [10.0, 290.0, 90.0, 310.0]);
        assert_eq!(entries[1].text, "chunk");
        assert_eq!(entries[1].quad.bounds(), [10.0, 350.0, 90.0, 380.0]);
        assert!(result.held.is_none());
    }

    #[test]
    fn commit_entries_appends_per_page_entries_and_counts() {
        let mut projects = [Project::new(), Project::new()];
        let result = RunResult {
            per_page: vec![
                (0, vec![entry("a")]),
                (1, vec![entry("b"), entry("c")]),
                (9, vec![entry("x")]),
            ],
            held: None,
        };
        assert_eq!(result.commit_entries(&mut projects), 3, "out-of-range page skipped");
        assert_eq!(projects[0].ocr.visible_count(), 1);
        assert_eq!(projects[1].ocr.visible_count(), 2);
    }

    #[test]
    fn boundary_state_commit_appends_held_candidates() {
        let mut projects = [Project::new()];
        let state = BoundaryState {
            candidates: vec![
                BoundaryCandidate { canvas_quad: [0.0, 0.0, 1.0, 1.0], entry: entry("a"), page: 0 },
                BoundaryCandidate { canvas_quad: [0.0, 0.0, 1.0, 1.0], entry: entry("b"), page: 0 },
                BoundaryCandidate { canvas_quad: [0.0, 0.0, 1.0, 1.0], entry: entry("c"), page: 5 },
            ],
            width: 100,
            boundary: 600,
        };
        assert_eq!(state.commit(&mut projects), 2, "out-of-range page skipped");
        assert_eq!(projects[0].ocr.visible_count(), 2);
        let texts: Vec<String> = projects[0].ocr.all().map(|e| e.text.clone()).collect();
        assert_eq!(texts, vec!["a".to_string(), "b".to_string()]);
    }
}