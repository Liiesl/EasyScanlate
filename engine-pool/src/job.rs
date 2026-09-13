//! Unified job description for the engine pool.
//!
//! This is the single message vocabulary between the app and the pool:
//! every queue entry is a [`QueuedJob`] carrying a [`JobKind`].
//! OCR manual + auto share one kind (`JobKind::Ocr`) with an [`OcrMode`]
//! discriminator; segment, styling and inpaint are the other kinds.

use easyscanlate_model::{EntryId, Quad};

/// Owner of a job. The pool is UI-agnostic: the app maps `TabId.0` <-> `OwnerId.0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OwnerId(pub u64);

impl std::fmt::Display for OwnerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Owner({})", self.0)
    }
}

/// OCR mode: auto pipeline vs manual selection. Both share weight 4 / priority 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrMode {
    Auto,
    Manual,
}

/// Unified job kind. This is the enum describing the job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    /// Unified manual + auto OCR (`OcrMode` discriminates).
    Ocr(OcrMode),
    /// Segmentation engine (SFX filter).
    Segment,
    /// Style auto-detect / classify.
    Styling,
    /// Inpaint for a specific backend.
    Inpaint(easyscanlate_settings::InpaintBackend),
}

impl JobKind {
    /// Convenience: auto OCR.
    pub fn ocr_auto() -> Self {
        Self::Ocr(OcrMode::Auto)
    }
    /// Convenience: manual OCR.
    pub fn ocr_manual() -> Self {
        Self::Ocr(OcrMode::Manual)
    }
    /// Any OCR (manual or auto).
    pub fn is_ocr(self) -> bool {
        matches!(self, Self::Ocr(_))
    }

    pub fn weight(self) -> u8 {
        match self {
            // Manual + auto share the OCR weight.
            Self::Ocr(_) => 4,
            Self::Segment => 4,
            Self::Styling => 2,
            Self::Inpaint(backend) => match backend {
                easyscanlate_settings::InpaintBackend::Telea => 1,
                easyscanlate_settings::InpaintBackend::Lama => 4,
                easyscanlate_settings::InpaintBackend::Aot => 3,
            },
        }
    }

    /// Lower value = higher priority (run sooner). SJF-inspired.
    pub fn priority(self) -> u8 {
        match self {
            Self::Inpaint(easyscanlate_settings::InpaintBackend::Telea) => 0,
            Self::Styling => 1,
            Self::Segment => 2,
            Self::Ocr(_) => 3,
            Self::Inpaint(easyscanlate_settings::InpaintBackend::Aot) => 4,
            Self::Inpaint(easyscanlate_settings::InpaintBackend::Lama) => 5,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ocr(_) => "OCR",
            Self::Segment => "SEGMENT",
            Self::Styling => "STYLE",
            Self::Inpaint(backend) => match backend {
                easyscanlate_settings::InpaintBackend::Telea => "INPAINT telea",
                easyscanlate_settings::InpaintBackend::Lama => "INPAINT lama",
                easyscanlate_settings::InpaintBackend::Aot => "INPAINT aot-gan",
            },
        }
    }
}

/// One auto-inpaint job: image index + entry + source path + quad.
/// Moved here from `app::tab` so the pool owns the payload vocabulary.
#[derive(Debug, Clone)]
pub struct InpaintAutoJob {
    pub index: usize,
    pub id: EntryId,
    pub path: String,
    pub quad: Quad,
}

/// One entry in the pool (waiting or running).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedJob {
    pub id: u64,
    pub owner: OwnerId,
    pub kind: JobKind,
}

impl QueuedJob {
    pub fn weight(&self) -> u8 {
        self.kind.weight()
    }
    pub fn priority(&self) -> u8 {
        self.kind.priority()
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub enum AcquireResult {
    Acquired(QueuedJob),
    Queued(QueuedJob, usize),
}
