//! Persisted app settings: a JSON file saved next to the executable. Loaded
//! once at boot, saved whenever the settings modal closes.

use serde::{Deserialize, Serialize};
#[cfg(feature = "translation")]
use std::collections::BTreeMap;
use std::path::PathBuf;

#[cfg(feature = "translation")]
use scanlateit_translation::Connection;
#[cfg(feature = "inpaint")]
use scanlateit_model::InpaintBackend;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Stored translation connections, keyed by provider id (`openai`,
    /// `deepseek`, `custom-openai`, ...). A provider is "connected" when it
    /// has an entry here; disconnect removes the entry.
    #[cfg(feature = "translation")]
    #[serde(default)]
    pub connections: BTreeMap<String, Connection>,
    /// The connection used by the translation bar; `None` falls back to the
    /// first connected provider.
    #[cfg(feature = "translation")]
    #[serde(default)]
    pub last_provider: Option<String>,
    /// When enabled, OCR-detected entries are auto-classified by the ONNX
    /// styling model and their style set from the prediction.
    #[cfg(feature = "styling")]
    #[serde(default)]
    pub auto_style_detect: bool,
    /// Number of parallel OCR detection sessions (one thread each) feeding
    /// the single recognition session.
    #[cfg(feature = "ocr")]
    #[serde(default = "default_ocr_workers")]
    pub ocr_workers: usize,
    /// When enabled, the translation model picker only lists free models.
    #[cfg(feature = "translation")]
    #[serde(default)]
    pub free_models_only: bool,
    /// Which inpainting implementation is used: the pure-Rust Telea
    /// algorithm (default) or the LaMa ONNX model.
    #[cfg(feature = "inpaint")]
    #[serde(default)]
    pub inpaint_backend: InpaintBackend,
    /// The Telea interpolation radius in pixels (ignored by LaMa).
    #[cfg(feature = "inpaint")]
    #[serde(default = "default_inpaint_radius")]
    pub inpaint_radius: u8,
}

#[cfg(feature = "inpaint")]
fn default_inpaint_radius() -> u8 {
    5
}

#[cfg(feature = "ocr")]
fn default_ocr_workers() -> usize {
    2
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            #[cfg(feature = "translation")]
            connections: BTreeMap::new(),
            #[cfg(feature = "translation")]
            last_provider: None,
            #[cfg(feature = "styling")]
            auto_style_detect: false,
            #[cfg(feature = "ocr")]
            ocr_workers: default_ocr_workers(),
            #[cfg(feature = "translation")]
            free_models_only: false,
            #[cfg(feature = "inpaint")]
            inpaint_backend: InpaintBackend::default(),
            #[cfg(feature = "inpaint")]
            inpaint_radius: default_inpaint_radius(),
        }
    }
}

impl Settings {
    /// `settings.json` in the directory of the running executable.
    fn path() -> Result<PathBuf, String> {
        let mut path = std::env::current_exe()
            .map_err(|e| format!("Cannot locate executable: {e}"))?;
        path.pop();
        Ok(path.join("settings.json"))
    }

    /// Loads the settings file; a missing or corrupt file yields defaults.
    pub fn load() -> Self {
        let Ok(path) = Self::path() else {
            return Self::default();
        };
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Writes the settings JSON next to the executable.
    pub fn save(&self) -> Result<(), String> {
        let path = Self::path()?;
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize settings: {e}"))?;
        std::fs::write(&path, text)
            .map_err(|e| format!("Failed to write {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(
        feature = "translation",
        feature = "ocr",
        feature = "styling",
        feature = "inpaint"
    ))]
    #[test]
    fn settings_round_trips_through_json() {
        let settings = Settings {
            connections: BTreeMap::from([
                (
                    "deepseek".to_string(),
                    Connection {
                        api_key: "sk-test-123".to_string(),
                        base_url: None,
                        model: None,
                    },
                ),
                (
                    "custom-openai".to_string(),
                    Connection {
                        api_key: "sk-custom".to_string(),
                        base_url: Some("http://localhost:11434/v1".to_string()),
                        model: Some("llama-3.1-8b".to_string()),
                    },
                ),
            ]),
            last_provider: Some("deepseek".to_string()),
            auto_style_detect: true,
            ocr_workers: 3,
            free_models_only: true,
            inpaint_backend: InpaintBackend::Lama,
            inpaint_radius: 7,
        };
        let text = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&text).unwrap();
        assert_eq!(back.connections["deepseek"].api_key, "sk-test-123");
        assert_eq!(
            back.connections["custom-openai"].base_url.as_deref(),
            Some("http://localhost:11434/v1")
        );
        assert_eq!(
            back.connections["custom-openai"].model.as_deref(),
            Some("llama-3.1-8b")
        );
        assert_eq!(back.last_provider.as_deref(), Some("deepseek"));
        assert_eq!(back.auto_style_detect, true);
        assert_eq!(back.ocr_workers, 3);
        assert_eq!(back.free_models_only, true);
        assert_eq!(back.inpaint_backend, InpaintBackend::Lama);
        assert_eq!(back.inpaint_radius, 7);
    }

    #[cfg(all(
        feature = "translation",
        feature = "ocr",
        feature = "styling",
        feature = "inpaint"
    ))]
    #[test]
    fn missing_fields_default() {
        let back: Settings = serde_json::from_str("{}").unwrap();
        assert!(back.connections.is_empty());
        assert_eq!(back.last_provider, None);
        assert_eq!(back.auto_style_detect, false);
        assert_eq!(back.ocr_workers, 2);
        assert_eq!(back.free_models_only, false);
        assert_eq!(back.inpaint_backend, InpaintBackend::Telea);
        assert_eq!(back.inpaint_radius, 5);
    }

    #[cfg(feature = "translation")]
    #[test]
    fn legacy_api_key_field_is_ignored() {
        let back: Settings = serde_json::from_str(r#"{"api_key": "kilo"}"#).unwrap();
        assert!(back.connections.is_empty());
    }
}
