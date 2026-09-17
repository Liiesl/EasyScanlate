//! Shared heavy engines, owned once per process (one `EnginePool` per `App`).
//!
//! All engine caches live here — including styling (global like OCR/inpaint/segment,
//! not per-tab). Per-tab progress (which entries are done, pending singles,
//! counters) stays on `Tab`; only the loaded model handle + build flag are global.

use crate::queue::EngineQueue;

#[derive(Debug, Default)]
pub struct EnginePool {
    #[cfg(feature = "ocr")]
    pub pipeline: Option<easyscanlate_ocr::ParallelEngine>,
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
