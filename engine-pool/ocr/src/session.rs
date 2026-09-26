//! The planned run set driven to completion: windowed canvas submission,
//! ordered merged-result delivery. Iced-free: the app pumps `step()` and
//! forwards the events to its message channel.
//!
//! The app builds every canvas (pages plus margin strips) and hands the
//! finished images in via [`RunSession::submit`]. The pump only moves pixels
//! to the engine and merged lines back: each reception is filtered and merged
//! here, so [`RunEvent`] lines are always merged lines — the app never sees
//! raw detector output and the engine never sees margins, bands, or dedup.

use std::collections::{BTreeMap, VecDeque};

use image::RgbImage;

use crate::{
    MergeConfig, OcrCancellationToken, OcrLine, ParallelEngine, filter_by_bbox_height, merge,
};

/// Drives the run set with a bounded in-flight window (`workers + 1`).
/// Results are emitted strictly in index order `0,1,2…` so the app's inbox
/// never sees a gap. Merging happens here, inside the engine: the app hands
/// over canvases and gets merged lines back.
pub struct RunSession {
    merge_cfg: MergeConfig,
    min_h: f32,
    max_h: f32,
    window: usize,
    total: usize,
    submitted: usize,
    in_flight: usize,
    /// Indexed by run so unordered completions can be matched even when the
    /// pipeline completes `2` before `1`. Stores `(index, width, margin_top)`
    /// as handed over at submission.
    canvas_meta: VecDeque<(usize, u32, u32)>,
    next_emit: usize,
    buffered: BTreeMap<usize, RunEvent>,
}

impl RunSession {
    pub fn new(total: usize, workers: usize, merge_cfg: MergeConfig, min_h: f32, max_h: f32) -> Self {
        Self {
            merge_cfg,
            min_h,
            max_h,
            window: workers + 1,
            total,
            submitted: 0,
            in_flight: 0,
            canvas_meta: VecDeque::new(),
            next_emit: 0,
            buffered: BTreeMap::new(),
        }
    }

    /// The next run index the app should build a canvas for and submit, or
    /// `None` when the submission window is full (call [`RunSession::poll`]
    /// to drain) or every run is submitted.
    pub fn next_needed(&self) -> Option<usize> {
        (self.submitted < self.total && self.in_flight < self.window).then_some(self.submitted)
    }

    /// Hands one app-built canvas to the pipeline.
    pub fn submit(
        &mut self,
        pipeline: &ParallelEngine,
        index: usize,
        canvas: RgbImage,
        width: u32,
        margin_top: u32,
    ) -> Result<(), String> {
        debug_assert_eq!(index, self.submitted, "canvases must be submitted in order");
        self.canvas_meta.push_back((index, width, margin_top));
        pipeline
            .submit(index, canvas)
            .map_err(|e| format!("OCR pipeline submit failed: {e}"))?;
        self.in_flight += 1;
        self.submitted += 1;
        Ok(())
    }

    /// Advances the run set by one step: submits the next canvas while the
    /// window has room (building it via `build`), otherwise blocks on the
    /// pipeline's `recv_unordered` and reorders locally. Returns `None` when
    /// every run is done. May block (canvas build + inference recv) — call
    /// from a background task/stream, never the UI.
    pub fn step(
        &mut self,
        pipeline: &ParallelEngine,
        token: &OcrCancellationToken,
        build: &mut dyn FnMut(usize) -> Result<(RgbImage, u32, u32), String>,
    ) -> Result<Option<RunEvent>, String> {
        // Allow early cancellation check before building canvases
        token
            .checkpoint()
            .map_err(|_| "cancelled".to_string())?;
        loop {
            // Fill the submission window with app-built canvases first.
            if let Some(index) = self.next_needed() {
                let (canvas, width, margin_top) = build(index)?;
                self.submit(pipeline, index, canvas, width, margin_top)?;
                // Keep filling the window before emitting; also check if next is ready.
                if self.buffered.contains_key(&self.next_emit) {
                    let ev = self.buffered.remove(&self.next_emit).expect("present");
                    self.next_emit += 1;
                    return Ok(Some(ev));
                }
                continue;
            }
            return self.poll(pipeline);
        }
    }

    /// Emits the next ordered event if buffered, otherwise blocks on one
    /// pipeline completion, merges it, and buffers it. Returns `None` when
    /// every run is done.
    fn poll(&mut self, pipeline: &ParallelEngine) -> Result<Option<RunEvent>, String> {
        loop {
            // Emit in order if already buffered (e.g. 2 finished before 1).
            if let Some(ev) = self.buffered.remove(&self.next_emit) {
                self.next_emit += 1;
                return Ok(Some(ev));
            }
            if self.submitted == self.total && self.in_flight == 0 && self.buffered.is_empty() {
                return Ok(None);
            }
            // Need an unordered pipeline completion; merge it, then buffer it
            // until its turn.
            let (idx, raw) = pipeline.recv_unordered()?;
            self.in_flight -= 1;
            let pos = self
                .canvas_meta
                .iter()
                .position(|(i, _, _)| *i == idx)
                .expect("canvas metadata for idx must exist");
            let (_, width, margin_top) = self
                .canvas_meta
                .remove(pos)
                .expect("position was valid");
            debug_assert!(
                self.canvas_meta.iter().all(|(i, _, _)| *i != idx),
                "duplicate metadata for idx {idx}"
            );
            let filtered = filter_by_bbox_height(raw, self.min_h, self.max_h);
            let merged: Vec<OcrLine> = merge(filtered, self.merge_cfg);
            self.buffered.insert(
                idx,
                RunEvent::Canvas {
                    index: idx,
                    width,
                    margin_top,
                    lines: merged,
                },
            );
            // Loop will emit if idx == next_emit, otherwise recv next.
        }
    }
}

/// One run's outcome as the UI must see it: merged lines plus the canvas
/// metrics the app needs for assembly.
#[derive(Debug, Clone)]
pub enum RunEvent {
    /// Merged lines plus the canvas metrics the app needs for assembly.
    Canvas {
        index: usize,
        width: u32,
        margin_top: u32,
        lines: Vec<OcrLine>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_books_total_and_window() {
        // NOTE: the pump path (`step` against a real engine) is covered by
        // the app-level OCR integration smoke and the rapidocr-core e2e
        // tests; the engines cannot be constructed without the ONNX models,
        // so no engine fakes are invented here.
        let empty = RunSession::new(0, 2, MergeConfig::default(), 0.0, 10000.0);
        assert_eq!(empty.total, 0);
        assert_eq!(empty.window, 3);
        assert_eq!(empty.next_needed(), None);

        let session = RunSession::new(1, 1, MergeConfig::default(), 0.0, 10000.0);
        assert_eq!(session.total, 1);
        assert_eq!(session.window, 2);
        assert_eq!(session.next_needed(), Some(0));
        assert_eq!(session.submitted, 0);
        assert_eq!(session.in_flight, 0);
        assert!(session.canvas_meta.is_empty());
    }
}
