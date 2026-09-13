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
    pub inpaint: Option<easyscanlate_inpaint::Engine>,
    #[cfg(feature = "inpaint")]
    pub auto_telea: Option<easyscanlate_inpaint::Engine>,
    #[cfg(feature = "inpaint")]
    pub auto_lama: Option<easyscanlate_inpaint::Engine>,
    #[cfg(feature = "inpaint")]
    pub auto_aot: Option<easyscanlate_inpaint::Engine>,
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
