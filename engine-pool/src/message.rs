//! Unified message between the engine pool and the app.
//!
//! Instead of ~15 ad-hoc `TabMessage` variants (`ParallelEngineReady`,
//! `ManualOcrEngineReady`, `InpaintEngineReady`, ...), pool completions are a
//! single envelope carrying the [`crate::job::JobKind`] that produced them.
//! The app maps this into `iced::Task<Message>` in its adapter layer; the pool
//! itself never depends on `iced`.

use crate::job::{JobKind, OwnerId};

/// One finished queue slot: which job, for which owner, of which kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineJobDone {
    pub job_id: u64,
    pub owner: OwnerId,
    pub kind: JobKind,
}

/// A freshly built engine, tagged by the unified [`JobKind`] it serves.
/// OCR manual + auto are distinct payloads under one `JobKind::Ocr` kind.
#[derive(Debug, Clone)]
pub enum BuiltEngine {
    #[cfg(feature = "ocr")]
    OcrParallel(easyscanlate_ocr::ParallelEngine),
    #[cfg(feature = "ocr")]
    OcrManual(easyscanlate_ocr::Engine),
    #[cfg(feature = "segment")]
    Segment(easyscanlate_segment::Engine),
    #[cfg(feature = "styling")]
    Styling(easyscanlate_styling::Engine),
    #[cfg(feature = "inpaint")]
    Inpaint {
        backend: easyscanlate_settings::InpaintBackend,
        engine: easyscanlate_inpaint::Engine,
    },
}

/// Unified completion outcome. Payloads stay strongly typed per subsystem so
/// existing handlers keep their logic; only the envelope is unified.
#[derive(Debug, Clone)]
pub enum EngineOutcome {
    /// Engine (model) finished loading — or failed with a message.
    /// Covers OCR parallel + manual (unified `JobKind::Ocr`), segment, styling,
    /// inpaint (all backends via `JobKind::Inpaint`).
    EngineReady {
        job_id: u64,
        owner: OwnerId,
        kind: JobKind,
        result: Result<BuiltEngine, String>,
    },
    /// A unit of work finished (stream item or single shot).
    /// The job slot stays alive until the app calls `queue.complete`.
    UnitFinished {
        job_id: u64,
        owner: OwnerId,
        kind: JobKind,
    },
}

impl EngineOutcome {
    pub fn job_id(&self) -> u64 {
        match self {
            Self::EngineReady { job_id, .. } => *job_id,
            Self::UnitFinished { job_id, .. } => *job_id,
        }
    }
    pub fn owner(&self) -> OwnerId {
        match self {
            Self::EngineReady { owner, .. } => *owner,
            Self::UnitFinished { owner, .. } => *owner,
        }
    }
    pub fn kind(&self) -> JobKind {
        match self {
            Self::EngineReady { kind, .. } => *kind,
            Self::UnitFinished { kind, .. } => *kind,
        }
    }
}
