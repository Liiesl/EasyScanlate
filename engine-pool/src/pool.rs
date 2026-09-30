//! Shared heavy engines, owned once per process (one `EnginePool` per `App`).
//!
//! All engine caches live here — including styling (global like OCR/inpaint/segment,
//! not per-tab). Per-tab progress (which entries are done, pending singles,
//! counters) stays on `Tab`; only the loaded model handle + build flag are global.

use crate::queue::EngineQueue;

/// Shared auto-OCR pipeline plus the config it was built with. The engine is
/// expensive (ONNX sessions + worker threads) and reusable across tabs as
/// long as the effective config matches and the previous stream finished
/// cleanly (see `RunSession`: sequential streams re-index `0..total-1` and
/// reorder locally via `recv_unordered`, so no pipeline-side ordered state
/// leaks between tabs).
#[cfg(feature = "ocr")]
#[derive(Debug, Clone)]
pub struct OcrPipelineCache {
    pub engine: easyscanlate_ocr::ParallelEngine,
    pub workers: usize,
    pub text_score_bits: u32,
    pub max_side_len: u32,
}

#[cfg(feature = "ocr")]
impl OcrPipelineCache {
    pub fn new(
        engine: easyscanlate_ocr::ParallelEngine,
        workers: usize,
        text_score_bits: u32,
        max_side_len: u32,
    ) -> Self {
        Self {
            engine,
            workers: workers.max(1),
            text_score_bits,
            max_side_len,
        }
    }

    /// True when this cached engine was built with the same effective config.
    pub fn matches(&self, workers: usize, text_score_bits: u32, max_side_len: u32) -> bool {
        self.workers == workers.max(1)
            && self.text_score_bits == text_score_bits
            && self.max_side_len == max_side_len
    }
}

#[derive(Debug, Default)]
pub struct EnginePool {
    #[cfg(feature = "ocr")]
    pub pipeline: Option<OcrPipelineCache>,
    #[cfg(feature = "ocr")]
    pub manual_ocr: Option<easyscanlate_ocr::Engine>,
    #[cfg(feature = "inpaint")]
    pub auto_telea: Option<easyscanlate_inpaint::Engine>,
    #[cfg(feature = "inpaint")]
    pub auto_lama: Option<easyscanlate_inpaint::Engine>,
    #[cfg(feature = "inpaint")]
    pub auto_aot: Option<easyscanlate_inpaint::Engine>,
    /// Cached auto engines for the manual-only CPU backends. The auto
    /// pipeline routes `Gradient` to Harmonic today (see `Mixed` routing);
    /// only ShiftMap never routes, but the slots keep backend-conditional
    /// caching exhaustive and correct.
    #[cfg(feature = "inpaint")]
    pub auto_shiftmap: Option<easyscanlate_inpaint::Engine>,
    #[cfg(feature = "inpaint")]
    pub auto_harmonic: Option<easyscanlate_inpaint::Engine>,
    #[cfg(feature = "segment")]
    pub segment: Option<easyscanlate_segment::Engine>,
    /// Global styling engine (was per-tab `JobTracker.engine`).
    #[cfg(feature = "styling")]
    pub styling: Option<easyscanlate_styling::Engine>,
    /// True while a styling engine build task is in flight (was per-tab).
    pub styling_building: bool,
    pub queue: EngineQueue,
}

impl EnginePool {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Cloned handle to the cached auto-OCR pipeline, if any.
    #[cfg(feature = "ocr")]
    pub fn pipeline_engine(&self) -> Option<easyscanlate_ocr::ParallelEngine> {
        self.pipeline.as_ref().map(|c| c.engine.clone())
    }

    /// True when the cached pipeline exists and matches the effective config.
    #[cfg(feature = "ocr")]
    pub fn pipeline_matches(
        &self,
        workers: usize,
        text_score_bits: u32,
        max_side_len: u32,
    ) -> bool {
        self.pipeline
            .as_ref()
            .is_some_and(|c| c.matches(workers, text_score_bits, max_side_len))
    }

    /// Store a freshly built auto-OCR pipeline with its config fingerprint.
    #[cfg(feature = "ocr")]
    pub fn set_pipeline(
        &mut self,
        engine: easyscanlate_ocr::ParallelEngine,
        workers: usize,
        text_score_bits: u32,
        max_side_len: u32,
    ) {
        self.pipeline = Some(OcrPipelineCache::new(
            engine,
            workers,
            text_score_bits,
            max_side_len,
        ));
    }

    /// Drop the cached auto-OCR pipeline (config change, cancel/error in
    /// Phase 1; dropping aborts worker threads via `DetRecPipeline::cancel`).
    #[cfg(feature = "ocr")]
    pub fn clear_pipeline(&mut self) {
        self.pipeline = None;
    }

    /// Shared manual+auto inpaint lookup: manual and auto use the same
    /// per-backend slots so `Lama <-> Telea <-> Lama` (or `AOT`) switches
    /// are cache hits instead of full model reloads.
    /// `radius` only matters for `Telea`/`Harmonic` (context pad); the
    /// ONNX/`ShiftMap` backends ignore it so a radius-slider change does
    /// not evict a loaded model.
    #[cfg(feature = "inpaint")]
    pub fn shared_inpaint(
        &self,
        backend: easyscanlate_settings::InpaintBackend,
        radius: i32,
    ) -> Option<easyscanlate_inpaint::Engine> {
        match backend {
            easyscanlate_settings::InpaintBackend::Telea => {
                self.auto_telea.clone().filter(|e| e.radius() == radius)
            }
            easyscanlate_settings::InpaintBackend::Harmonic => {
                self.auto_harmonic.clone().filter(|e| e.radius() == radius)
            }
            easyscanlate_settings::InpaintBackend::Lama => self.auto_lama.clone(),
            easyscanlate_settings::InpaintBackend::Aot => self.auto_aot.clone(),
            easyscanlate_settings::InpaintBackend::ShiftMap => self.auto_shiftmap.clone(),
        }
    }

    /// Store a freshly built engine into the shared per-backend slot.
    #[cfg(feature = "inpaint")]
    pub fn set_shared_inpaint(
        &mut self,
        backend: easyscanlate_settings::InpaintBackend,
        engine: easyscanlate_inpaint::Engine,
    ) {
        match backend {
            easyscanlate_settings::InpaintBackend::Telea => {
                self.auto_telea = Some(engine);
            }
            easyscanlate_settings::InpaintBackend::Lama => {
                self.auto_lama = Some(engine);
            }
            easyscanlate_settings::InpaintBackend::Aot => {
                self.auto_aot = Some(engine);
            }
            easyscanlate_settings::InpaintBackend::ShiftMap => {
                self.auto_shiftmap = Some(engine);
            }
            easyscanlate_settings::InpaintBackend::Harmonic => {
                self.auto_harmonic = Some(engine);
            }
        }
    }

    /// True when any shared inpaint engine is cached (for status logging).
    #[cfg(feature = "inpaint")]
    pub fn has_any_shared_inpaint(&self) -> bool {
        self.auto_telea.is_some()
            || self.auto_lama.is_some()
            || self.auto_aot.is_some()
            || self.auto_shiftmap.is_some()
            || self.auto_harmonic.is_some()
    }

    /// Backend-specific shared hit test (for status logging).
    #[cfg(feature = "inpaint")]
    pub fn has_shared_inpaint(
        &self,
        backend: easyscanlate_settings::InpaintBackend,
        radius: i32,
    ) -> bool {
        self.shared_inpaint(backend, radius).is_some()
    }

    /// Global styling engine handle.
    #[cfg(feature = "styling")]
    pub fn styling_engine(&self) -> Option<&easyscanlate_styling::Engine> {
        self.styling.as_ref()
    }

    /// True while a styling build is in flight (global).
    pub fn is_styling_building(&self) -> bool {
        self.styling_building
    }

    pub fn mark_styling_building(&mut self) {
        self.styling_building = true;
    }

    /// Store the loaded styling engine. Returns true when a build was pending.
    #[cfg(feature = "styling")]
    pub fn set_styling_engine(&mut self, engine: easyscanlate_styling::Engine) -> bool {
        self.styling = Some(engine);
        let pending = self.styling_building;
        self.styling_building = false;
        pending
    }

    /// Clear the building flag after a failed build.
    pub fn fail_styling_build(&mut self) {
        self.styling_building = false;
    }
}
