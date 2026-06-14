//! Persisted app settings: a JSON file saved next to the executable. Loaded
//! once at boot, saved whenever the settings modal closes.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// API key for the translation provider; empty falls back to the
    /// `OPENCODE_API_KEY` environment variable.
    #[serde(default)]
    pub api_key: String,
    /// When enabled, OCR-detected entries are auto-classified by the ONNX
    /// styling model and their style set from the prediction.
    #[serde(default)]
    pub auto_style_detect: bool,
    /// Number of parallel OCR detection sessions (one thread each) feeding
    /// the single recognition session.
    #[serde(default = "default_ocr_workers")]
    pub ocr_workers: usize,
    /// When enabled, the translation model picker only lists free models.
    #[serde(default)]
    pub free_models_only: bool,
}

fn default_ocr_workers() -> usize {
    2
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            auto_style_detect: false,
            ocr_workers: default_ocr_workers(),
            free_models_only: false,
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

    #[test]
    fn settings_round_trips_through_json() {
        let settings = Settings {
            api_key: "sk-test-123".to_string(),
            auto_style_detect: true,
            ocr_workers: 3,
            free_models_only: true,
        };
        let text = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&text).unwrap();
        assert_eq!(back.api_key, "sk-test-123");
        assert_eq!(back.auto_style_detect, true);
        assert_eq!(back.ocr_workers, 3);
        assert_eq!(back.free_models_only, true);
    }

    #[test]
    fn missing_fields_default() {
        let back: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(back.api_key, "");
        assert_eq!(back.auto_style_detect, false);
        assert_eq!(back.ocr_workers, 2);
        assert_eq!(back.free_models_only, false);
    }
}