//! Wraps the rapidocr engine: configuration, lifecycle and conversion of raw
//! engine output into the model's append-only entries. The rest of the app
//! never touches rapidocr types directly.

use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rapidocr_core::config::{
    DetConfig, InferenceOptions, LimitType, PipelineConfig, RapidOcrConfig, RecConfig,
};
use rapidocr_core::types::OcrLine;
use rapidocr_core::{is_cancelled_error, OcrCancellationToken, RapidOcr};

use crate::model::{EntrySource, NewEntry, Quad};

const MODEL_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/models");

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

/// Convert raw engine output into model entries (auto-OCR source). Safe to
/// call from a background task; the model is untouched until appended.
pub fn to_entries(lines: Vec<OcrLine>) -> Vec<NewEntry> {
    lines
        .into_iter()
        .map(|line| NewEntry {
            source: EntrySource::AutoOcr,
            text: line.text,
            score: line.score,
            quad: Quad { points: line.bbox.points },
        })
        .collect()
}