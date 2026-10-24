//! Unified engine pool: one queue + one message vocabulary for OCR/segment/styling/inpaint.
//!
//! Subcrates (`ocr`, `segment`, `styling`, `inpaint`, `model-store`) live under
//! `engine-pool/` and are re-exported here. The app talks to the pool only via
//! [`job::JobKind`] / [`job::QueuedJob`] / [`message::EngineOutcome`]; the pool
//! never depends on `iced` or `App` (UI-agnostic futures: callers wrap
//! [`exec`] helpers in their own tasks).

pub mod exec;
pub mod job;
pub mod message;
pub mod pool;
pub mod queue;

#[cfg(feature = "inpaint")]
pub use exec::run_auto_inpaint_job;
#[cfg(feature = "segment")]
pub use exec::run_segment_grid;
#[cfg(feature = "inpaint")]
pub use exec::{AutoInpaintPatches, inpaint_pad_for};
#[cfg(feature = "ocr")]
pub use exec::{ManualOcrItem, run_manual_ocr_selection};
pub use job::{AcquireResult, InpaintAutoJob, JobKind, OcrMode, OwnerId, QueuedJob};
pub use message::{BuiltEngine, EngineJobDone, EngineOutcome};
pub use pool::EnginePool;
#[cfg(feature = "ocr")]
pub use pool::OcrPipelineCache;
pub use queue::{EngineQueue, POOL_CAPACITY};

// Re-export engines so the app has one import root (old paths keep working).
#[cfg(feature = "inpaint")]
pub use easyscanlate_inpaint as inpaint;
#[cfg(feature = "model-store")]
pub use easyscanlate_models as models;
#[cfg(feature = "ocr")]
pub use easyscanlate_ocr as ocr;
#[cfg(feature = "segment")]
pub use easyscanlate_segment as segment;
#[cfg(feature = "styling")]
pub use easyscanlate_styling as styling;
