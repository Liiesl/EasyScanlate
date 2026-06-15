//! The whole planned run set driven to completion: windowed canvas
//! submission, ordered result delivery, undecodable-page fallback. Iced-free:
//! the app pumps `step()` and forwards the events to its message channel.

use std::collections::VecDeque;

use crate::{
    Engine, OcrCancellationToken, OcrLine, ParallelEngine, RunPlan, RunResult, body_height,
    bottom_margin_strip, load_rgb, stack_run, to_entries, top_margin_strip, STITCH_MARGIN_RATIO,
};

/// One run's outcome as the UI must see it.
#[derive(Debug, Clone)]
pub enum RunEvent {
    /// Raw lines plus the canvas metrics the app needs for assembly.
    Canvas {
        index: usize,
        width: u32,
        margin_top: u32,
        lines: Vec<OcrLine>,
    },
    /// Fallback of the undecodable-page path: ready-to-commit per-page result.
    Fallback { index: usize, result: RunResult },
}

/// What [`build_canvas`] produced for one run.
pub(crate) enum BuiltCanvas {
    /// The stitched canvas plus its `(width, margin_top)` for submission.
    Ready(image::RgbImage, u32, u32),
    /// The undecodable-page fallback result, ready to commit directly.
    Fallback(RunResult),
}

/// Builds the stitched canvas of one run: its pages stacked at the first
/// page's width with the margin strips of the neighboring content above and
/// below (see [`crate::stack_run`]). When a page fails to decode, falls back
/// to raw per-page OCR through the old engine and returns the ready-to-commit
/// result instead (no canvas, no boundary candidates).
pub(crate) fn build_canvas(
    fallback: &Engine,
    token: &OcrCancellationToken,
    index: usize,
    run: &RunPlan,
    paths: &[String],
    above_path: Option<&str>,
    below_path: Option<&str>,
    dims: &[(u32, u32)],
) -> Result<BuiltCanvas, String> {
    let mut loaded = Vec::with_capacity(paths.len());
    for path in paths {
        match load_rgb(path) {
            Some(image) => loaded.push(image),
            None => {
                eprintln!(
                    "[ocr-run {index}] undecodable page {path}: falling back to per-page OCR"
                );
                let mut out = Vec::with_capacity(paths.len());
                for (offset, path) in paths.iter().enumerate() {
                    match fallback.run_path_cancellable(path, token) {
                        Ok(lines) => out.push((run.page_start + offset, to_entries(lines))),
                        Err(e) => return Err(e),
                    }
                }
                return Ok(BuiltCanvas::Fallback(RunResult {
                    per_page: out,
                    held: None,
                }));
            }
        }
    }
    let width = loaded[0].width();
    let body_h = body_height(&dims[run.page_start..=run.page_end], width, run.band);
    let margin = (STITCH_MARGIN_RATIO * body_h as f32).round().max(1.0) as u32;
    let above = match (above_path, run.above) {
        (Some(path), Some((_, band))) => load_rgb(path)
            .and_then(|image| top_margin_strip(&image, band, width, margin)),
        _ => None,
    };
    let below = match (below_path, run.below) {
        (Some(path), Some((_, band))) => load_rgb(path)
            .and_then(|image| bottom_margin_strip(&image, band, width, margin)),
        _ => None,
    };
    let margin_top = above.as_ref().map_or(0, |strip| strip.height());
    let canvas = stack_run(above, &loaded, below, width, run.band);
    Ok(BuiltCanvas::Ready(canvas, width, margin_top))
}

/// Drives the planned run set with a bounded in-flight window (`workers + 1`).
pub struct RunSession {
    plans: Vec<RunPlan>,
    dims: Vec<(u32, u32)>,
    paths: Vec<Vec<String>>,
    above_paths: Vec<Option<String>>,
    below_paths: Vec<Option<String>>,
    window: usize,
    dispatched: usize,
    in_flight: usize,
    canvas_meta: VecDeque<(u32, u32)>,
    total: usize,
}

impl RunSession {
    pub fn new(
        plans: Vec<RunPlan>,
        dims: Vec<(u32, u32)>,
        paths: Vec<Vec<String>>,
        above_paths: Vec<Option<String>>,
        below_paths: Vec<Option<String>>,
        workers: usize,
    ) -> Self {
        let total = plans.len();
        Self {
            plans,
            dims,
            paths,
            above_paths,
            below_paths,
            window: workers + 1,
            dispatched: 0,
            in_flight: 0,
            canvas_meta: VecDeque::new(),
            total,
        }
    }

    /// Advances the run set by one step: fills the submission window, then
    /// blocks on the pipeline's ordered `recv` when the window is full.
    /// Returns `None` when every run is done. May block (image loads +
    /// inference recv) — call from a background task/stream, never the UI.
    pub fn step(
        &mut self,
        pipeline: &ParallelEngine,
        fallback: &Engine,
        token: &OcrCancellationToken,
    ) -> Result<Option<RunEvent>, String> {
        loop {
            if self.dispatched < self.total && self.in_flight < self.window {
                let index = self.dispatched;
                let run = &self.plans[index];
                match build_canvas(
                    fallback,
                    token,
                    index,
                    run,
                    &self.paths[index],
                    self.above_paths[index].as_deref(),
                    self.below_paths[index].as_deref(),
                    &self.dims,
                ) {
                    Ok(BuiltCanvas::Ready(canvas, width, margin_top)) => {
                        self.canvas_meta.push_back((width, margin_top));
                        pipeline
                            .submit(index, canvas)
                            .map_err(|e| format!("OCR pipeline submit failed: {e}"))?;
                        self.in_flight += 1;
                        self.dispatched += 1;
                    }
                    Ok(BuiltCanvas::Fallback(result)) => {
                        self.dispatched += 1;
                        return Ok(Some(RunEvent::Fallback { index, result }));
                    }
                    Err(e) => return Err(e),
                }
                continue;
            }
            if self.dispatched == self.total && self.in_flight == 0 {
                return Ok(None);
            }
            let (idx, lines) = pipeline.recv()?;
            self.in_flight -= 1;
            let (width, margin_top) = self
                .canvas_meta
                .pop_front()
                .expect("canvas metadata arrives in submission order");
            return Ok(Some(RunEvent::Canvas {
                index: idx,
                width,
                margin_top,
                lines,
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> RunPlan {
        RunPlan {
            page_start: 0,
            page_end: 0,
            band: (0.0, 1.0),
            above: None,
            below: None,
            dedup: None,
        }
    }

    #[test]
    fn new_books_total_and_window() {
        // NOTE: the pump path (`step` against a real engine) is covered by
        // the app-level OCR integration smoke and the rapidocr-core e2e
        // tests; the engines cannot be constructed without the ONNX models,
        // so no engine fakes are invented here.
        let empty = RunSession::new(Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), 2);
        assert_eq!(empty.total, 0);
        assert_eq!(empty.window, 3);

        let session = RunSession::new(
            vec![plan()],
            vec![(100, 400)],
            vec![vec!["a.png".to_string()]],
            vec![None],
            vec![None],
            1,
        );
        assert_eq!(session.total, 1);
        assert_eq!(session.window, 2);
        assert_eq!(session.dispatched, 0);
        assert_eq!(session.in_flight, 0);
        assert!(session.canvas_meta.is_empty());
    }
}