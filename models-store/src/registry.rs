//! Model registry: filenames, URLs and metadata for all downloadable ONNX assets.
//!
//! Persisted location is always `scanlateit_settings::models_dir()` (settings-relative).

use std::path::PathBuf;

/// Describes a single downloadable model asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec {
    /// Stable identifier, e.g. `lama-manga`.
    pub id: &'static str,
    /// Filename as stored on disk inside `models_dir()`.
    pub filename: &'static str,
    /// Download URL (kept verbatim including `?download=true` for Hugging Face).
    pub url: &'static str,
    /// Human-readable description.
    pub description: &'static str,
    /// Whether this model is currently downloadable. `false` for deferred assets (e.g. AOT).
    pub available: bool,
    /// Legacy filename that this asset replaces (if any). Used for migration checks.
    pub replaces: Option<&'static str>,
}

// ---------------------------------------------------------------------------
// Registry constants
// ---------------------------------------------------------------------------

pub const LAMA_MANGA: ModelSpec = ModelSpec {
    id: "lama-manga",
    filename: "lama-manga_int8.onnx",
    url: "https://huggingface.co/Liiesl/lama-manga-onnx-quant/resolve/main/lama-manga_int8.onnx?download=true",
    description: "LaMa manga inpainting (int8, 512px)",
    available: true,
    replaces: None,
};

pub const TEXT_STYLING: ModelSpec = ModelSpec {
    id: "text-styling",
    filename: "text_styling_model.onnx",
    url: "https://huggingface.co/Liiesl/text-styling-classificationv1/resolve/main/text_styling_model.onnx?download=true",
    description: "Text styling classifier (160x64)",
    available: true,
    replaces: None,
};

pub const KOHARU_SEG: ModelSpec = ModelSpec {
    id: "koharu-seg",
    filename: "koharu-yolo26s-seg.onnx",
    url: "https://huggingface.co/Liiesl/bubble-segment-onnx/resolve/main/koharu-yolo26s-seg.onnx?download=true",
    description: "Bubble/panel segmentation (YOLO26s-seg, replaces yolo26s-seg.onnx)",
    available: true,
    replaces: Some("yolo26s-seg.onnx"),
};

pub const KOREAN_REC: ModelSpec = ModelSpec {
    id: "korean-rec",
    filename: "korean_PP-OCRv5_rec_mobile.onnx",
    url: "https://modelscope.cn/api/v1/models/RapidAI/RapidOCR/repo?Revision=master&FilePath=onnx%2FPP-OCRv5%2Frec%2Fkorean_PP-OCRv5_rec_mobile.onnx",
    description: "Korean PP-OCRv5 recognition (mobile)",
    available: true,
    replaces: None,
};

pub const PPOCR_DET_TINY: ModelSpec = ModelSpec {
    id: "ppocr-det-tiny",
    filename: "PP-OCRv6_det_tiny.onnx",
    url: "https://modelscope.cn/api/v1/models/RapidAI/RapidOCR/repo?Revision=master&FilePath=onnx%2FPP-OCRv6%2Fdet%2FPP-OCRv6_det_tiny.onnx",
    description: "PP-OCRv6 detection tiny",
    available: true,
    replaces: None,
};

/// AOT inpainting model — not yet available, deferred.
/// Included in the registry so callers can query `is_available()` and show a placeholder.
pub const AOT_INPAINT: ModelSpec = ModelSpec {
    id: "aot-inpaint",
    filename: "inpainting_aot.onnx",
    url: "",
    description: "AOT-GAN inpainting (deferred, not yet available)",
    available: false,
    replaces: None,
};

/// All registered models in display order.
pub const MODELS: &[ModelSpec] = &[
    LAMA_MANGA,
    TEXT_STYLING,
    KOHARU_SEG,
    KOREAN_REC,
    PPOCR_DET_TINY,
    AOT_INPAINT,
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Find a model by its stable `id`.
pub fn get_model(id: &str) -> Option<&'static ModelSpec> {
    MODELS.iter().find(|m| m.id == id)
}

/// Find a model by filename (e.g. `lama-manga_int8.onnx`).
pub fn get_model_by_filename(filename: &str) -> Option<&'static ModelSpec> {
    MODELS.iter().find(|m| m.filename == filename)
}

/// Returns the models directory (settings-relative).
pub fn models_dir() -> PathBuf {
    scanlateit_settings::models_dir()
}

/// Returns the expected on-disk path for a registered model.
pub fn model_path(spec: &ModelSpec) -> PathBuf {
    models_dir().join(spec.filename)
}

/// Returns the expected on-disk path for a model id.
pub fn model_path_for_id(id: &str) -> Option<PathBuf> {
    get_model(id).map(model_path)
}

/// Whether a model id is currently downloadable.
pub fn is_available(id: &str) -> bool {
    get_model(id).is_some_and(|m| m.available)
}

/// Returns true if the model file exists on disk.
pub fn is_downloaded(spec: &ModelSpec) -> bool {
    model_path(spec).exists()
}

/// Returns true if any file for the spec or its legacy `replaces` target exists.
pub fn is_downloaded_with_legacy(spec: &ModelSpec) -> bool {
    if is_downloaded(spec) {
        return true;
    }
    if let Some(legacy) = spec.replaces {
        if models_dir().join(legacy).exists() {
            return true;
        }
    }
    false
}

/// List all models that are available but missing from disk.
pub fn missing_models() -> Vec<&'static ModelSpec> {
    MODELS
        .iter()
        .filter(|m| m.available && !is_downloaded(m))
        .collect()
}

/// List all models that are present.
pub fn present_models() -> Vec<&'static ModelSpec> {
    MODELS
        .iter()
        .filter(|m| is_downloaded(m))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_expected_ids() {
        let ids: Vec<&str> = MODELS.iter().map(|m| m.id).collect();
        assert!(ids.contains(&"lama-manga"));
        assert!(ids.contains(&"text-styling"));
        assert!(ids.contains(&"koharu-seg"));
        assert!(ids.contains(&"korean-rec"));
        assert!(ids.contains(&"ppocr-det-tiny"));
        assert!(ids.contains(&"aot-inpaint"));
    }

    #[test]
    fn aot_is_not_available() {
        assert!(!is_available("aot-inpaint"));
        assert!(is_available("lama-manga"));
    }

    #[test]
    fn koharu_replaces_legacy() {
        let m = get_model("koharu-seg").unwrap();
        assert_eq!(m.replaces, Some("yolo26s-seg.onnx"));
    }

    #[test]
    fn urls_keep_download_true() {
        assert!(LAMA_MANGA.url.contains("?download=true"));
        assert!(TEXT_STYLING.url.contains("?download=true"));
        assert!(KOHARU_SEG.url.contains("?download=true"));
    }

    #[test]
    fn models_dir_is_settings_relative() {
        let dir = models_dir();
        // Should end with "models" and be inside config_dir
        assert!(dir.ends_with("models"));
        assert_eq!(dir, scanlateit_settings::config_dir().join("models"));
    }
}
