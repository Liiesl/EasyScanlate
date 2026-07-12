//! Persisted app settings, owned by this crate and shared by every other
//! crate directly (no per-field routing through the app). Backed by
//! [`confy`]: the store lives in the OS config dir
//! (`%APPDATA%\scanlateit\config\default-config.toml` on Windows,
//! `~/.config/scanlateit/config/default-config.toml` on Linux), so the same
//! code works cross-platform.
//!
//! A process-wide store is initialized once at boot with [`init`]; any crate
//! then reads through [`get`] and mutates + persists through [`modify`]
//! (write-through: every `modify` saves to disk immediately). Closures must
//! not call back into [`get`]/[`modify`] (re-entrant locking would deadlock).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

/// The confy application name; decides the config directory name.
const APP_NAME: &str = "scanlateit";

fn default_aurora_color() -> String {
    "#3b0600".to_string()
}
fn default_aurora_blob_count() -> u8 {
    2
}
fn default_aurora_is_dark() -> bool {
    true
}
fn default_aurora_schema() -> u8 {
    1
}

fn default_inpaint_radius() -> String {
    "5".to_string()
}

fn default_ocr_workers() -> String {
    "2".to_string()
}

fn default_ocr_text_score() -> String {
    "0.7".to_string()
}

fn default_ocr_min_text_height() -> String {
    "40".to_string()
}

fn default_ocr_max_text_height() -> String {
    "100".to_string()
}

fn default_ocr_max_side_len() -> String {
    "2000".to_string()
}

fn default_ocr_merge_threshold() -> String {
    "0.5".to_string()
}

fn default_ui_font_size() -> u32 {
    12
}

fn default_true() -> bool {
    true
}

/// One stored translation connection: the API key plus (for custom
/// endpoints) the base URL and the single model id. Persisted one entry per
/// provider id. Owned here (the settings crate) so both the translation
/// crate and the persisted settings can share it without a dependency cycle.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Connection {
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: Option<String>,
}

/// Which inpainting implementation the app uses. Owned here (the settings
/// crate, like every other persisted knob); the model crate stays pure
/// image/data storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InpaintBackend {
    /// The pure-Rust Telea algorithm from the `inpaint` crate: no model, no
    /// download, works instantly on CPU.
    #[default]
    Telea,
    /// The LaMa ONNX model: better on complex backgrounds, needs the
    /// `lama-manga.onnx` file next to the executable.
    Lama,
}

impl fmt::Display for InpaintBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Telea => "Telea (inpaint crate)",
            Self::Lama => "LaMa (ONNX)",
        })
    }
}

/// Which inpaint model the **auto** post-OCR pipeline uses. Distinct from
/// [`InpaintBackend`] (manual tool) because `Mixed` is a bg-aware routing:
/// `Solid`→no inpaint, `Gradient`→Telea, `Artwork`→LaMa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutoInpaintModel {
    Telea,
    Lama,
    #[default]
    Mixed,
}

impl fmt::Display for AutoInpaintModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Telea => "Telea",
            Self::Lama => "LaMa",
            Self::Mixed => "Mixed (bg-aware)",
        })
    }
}

/// The whole persisted app configuration. Every field is unconditional:
/// this crate has no heavy dependencies, so subsystem features stay at the
/// app/ui level while the config always knows all values.
///
/// Field order matters: all map-valued fields (TOML tables) are declared
/// last, because TOML requires values before tables within one document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// The connection used by the translation bar; `None` falls back to the
    /// first connected provider.
    #[serde(default)]
    pub last_provider: Option<String>,
    /// When enabled, OCR-detected entries are auto-classified by the ONNX
    /// styling model and their style set from the prediction.
    #[serde(default = "default_true")]
    pub auto_style_detect: bool,
    /// Number of parallel OCR detection sessions (one thread each) feeding
    /// the single recognition session. Kept as the raw input string so a
    /// half-typed value survives editing; parsed (fallback 2) when OCR
    /// starts.
    #[serde(default = "default_ocr_workers")]
    pub ocr_workers: String,
    /// Minimum accepted recognition confidence (0.0..1.0). Raw string to survive
    /// half-typing; parsed (fallback 0.5) when OCR starts.
    #[serde(default = "default_ocr_text_score")]
    pub ocr_text_score: String,
    /// Minimum Text bbox height filter (px). Lines with bbox height < this are
    /// dropped. Raw string; parsed (fallback 40).
    #[serde(default = "default_ocr_min_text_height")]
    pub ocr_min_text_height: String,
    /// Maximum Text bbox height filter (px). Lines with bbox height > this are
    /// dropped. Raw string; parsed (fallback 100).
    #[serde(default = "default_ocr_max_text_height")]
    pub ocr_max_text_height: String,
    /// Maximum image side length before detection (max_side_len, longer side
    /// before resize). Raw string; parsed (fallback 2000).
    #[serde(default = "default_ocr_max_side_len")]
    pub ocr_max_side_len: String,
    /// Merge distance threshold as ratio of box height (0.0..2.0) applied to
    /// both axes. Raw string; parsed (fallback 0.5).
    #[serde(default = "default_ocr_merge_threshold")]
    pub ocr_merge_threshold: String,
    /// When enabled, the translation model picker only lists free models.
    #[serde(default)]
    pub free_models_only: bool,
    /// Which inpainting implementation is used: the pure-Rust Telea
    /// algorithm (default) or the LaMa ONNX model.
    #[serde(default)]
    pub inpaint_backend: InpaintBackend,
    /// The Telea interpolation radius in pixels (ignored by LaMa). Raw
    /// input string; parsed (fallback 5) when inpainting starts.
    #[serde(default = "default_inpaint_radius")]
    pub inpaint_radius: String,
    /// Aurora background theme: hex color like "#3b0600" (persisted as string for readability).
    #[serde(default = "default_aurora_color")]
    pub aurora_color: String,
    /// Number of aurora blobs (1..=5). 1 = solid overlay.
    #[serde(default = "default_aurora_blob_count")]
    pub aurora_blob_count: u8,
    /// Whether the aurora is in dark mode (dims base, brighter blobs).
    #[serde(default = "default_aurora_is_dark")]
    pub aurora_is_dark: bool,
    /// Color-theory schema 0=Vibrant,1=Analogous,2=Contrast,3=Neon.
    #[serde(default = "default_aurora_schema")]
    pub aurora_schema: u8,
    /// Base UI font size in points, like VS Code's `editor.fontSize`. Integer
    /// only, scaled everywhere that has a connection to a font (text, padding,
    /// border radius, gaps between items). Window chrome (`GAP`,
    /// `OUTER_PADDING`, modal shell, viewer constants) stays fixed.
    #[serde(default = "default_ui_font_size")]
    pub ui_font_size: u32,
    /// When enabled, OCR entries that overlap SFX outside balloons are
    /// auto-removed via the segmentation model (manga-mimic grid).
    #[serde(default = "default_true")]
    pub auto_sfx_filter: bool,
    /// When enabled, gradient/artwork bubbles get transparent bg + inpaint
    /// after style detection. Requires `auto_style_detect` for `Mixed`.
    #[serde(default = "default_true")]
    pub auto_inpaint: bool,
    /// Which model the auto-inpaint step uses; `Mixed` routes by bg type.
    #[serde(default)]
    pub auto_inpaint_model: AutoInpaintModel,
    /// Stored translation connections, keyed by provider id (`openai`,
    /// `deepseek`, `custom-openai`, ...). A provider is "connected" when it
    /// has an entry here; disconnect removes the entry.
    #[serde(default)]
    pub connections: BTreeMap<String, Connection>,
    /// Per-provider set of hidden model ids (Manage Models overlay). A model
    /// is hidden by the user to filter unused entries; deprecated models are
    /// always hidden and never stored here. The basic configuration (empty)
    /// hides nothing beyond the default latest-per-family filter.
    #[serde(default)]
    pub hidden_models: BTreeMap<String, BTreeSet<String>>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            connections: BTreeMap::new(),
            last_provider: None,
            auto_style_detect: true,
            ocr_workers: default_ocr_workers(),
            ocr_text_score: default_ocr_text_score(),
            ocr_min_text_height: default_ocr_min_text_height(),
            ocr_max_text_height: default_ocr_max_text_height(),
            ocr_max_side_len: default_ocr_max_side_len(),
            ocr_merge_threshold: default_ocr_merge_threshold(),
            free_models_only: false,
            hidden_models: BTreeMap::new(),
            inpaint_backend: InpaintBackend::default(),
            inpaint_radius: default_inpaint_radius(),
            aurora_color: default_aurora_color(),
            aurora_blob_count: default_aurora_blob_count(),
            aurora_is_dark: default_aurora_is_dark(),
            aurora_schema: default_aurora_schema(),
            ui_font_size: default_ui_font_size(),
            auto_sfx_filter: true,
            auto_inpaint: true,
            auto_inpaint_model: AutoInpaintModel::default(),
        }
    }
}

static STORE: OnceLock<RwLock<Settings>> = OnceLock::new();

fn store() -> &'static RwLock<Settings> {
    STORE.get_or_init(|| RwLock::new(load_from_disk()))
}

/// Loads the configuration from the OS config dir; a missing or corrupt
/// file yields defaults.
fn load_from_disk() -> Settings {
    confy::load(APP_NAME, None).unwrap_or_default()
}

/// Initializes the process-wide store from disk. Idempotent; call once at
/// boot before any [`get`]/[`modify`] (they self-initialize anyway).
pub fn init() {
    let _ = store();
}

/// Runs `f` with read access to the current settings. The closure must not
/// call [`get`]/[`modify`] again (the lock is held for its duration).
pub fn get<R>(f: impl FnOnce(&Settings) -> R) -> R {
    f(&store()
        .read()
        .expect("settings store lock must not be poisoned"))
}

/// Mutates the settings via `f` and writes them to disk immediately
/// (write-through). The closure must not call [`get`]/[`modify`] again.
pub fn modify(f: impl FnOnce(&mut Settings)) -> Result<(), String> {
    let mut guard = store()
        .write()
        .expect("settings store lock must not be poisoned");
    f(&mut guard);
    let result = confy::store(APP_NAME, None, &*guard).map_err(|e| e.to_string());
    if let Err(e) = &result {
        eprintln!("[settings] persist failed: {e}");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trips_through_toml() {
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
            ocr_workers: "3".to_string(),
            ocr_text_score: "0.7".to_string(),
            ocr_min_text_height: "12".to_string(),
            ocr_max_text_height: "500".to_string(),
            ocr_max_side_len: "3000".to_string(),
            ocr_merge_threshold: "0.8".to_string(),
            free_models_only: true,
            hidden_models: BTreeMap::from([(
                "deepseek".to_string(),
                BTreeSet::from(["deepseek-reasoner".to_string()]),
            )]),
            inpaint_backend: InpaintBackend::Lama,
            inpaint_radius: "7".to_string(),
            aurora_color: "#112233".to_string(),
            aurora_blob_count: 4,
            aurora_is_dark: false,
            aurora_schema: 2,
            ui_font_size: 14,
            auto_sfx_filter: true,
            auto_inpaint: true,
            auto_inpaint_model: AutoInpaintModel::Mixed,
        };
        let text = toml::to_string(&settings).unwrap();
        let back: Settings = toml::from_str(&text).unwrap();
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
        assert!(back.auto_style_detect);
        assert_eq!(back.ocr_workers, "3");
        assert_eq!(back.ocr_text_score, "0.7");
        assert_eq!(back.ocr_min_text_height, "12");
        assert_eq!(back.ocr_max_text_height, "500");
        assert_eq!(back.ocr_max_side_len, "3000");
        assert_eq!(back.ocr_merge_threshold, "0.8");
        assert!(back.free_models_only);
        assert!(back.hidden_models["deepseek"].contains("deepseek-reasoner"));
        assert_eq!(back.inpaint_backend, InpaintBackend::Lama);
        assert_eq!(back.inpaint_radius, "7");
        assert_eq!(back.aurora_color, "#112233");
        assert_eq!(back.aurora_blob_count, 4);
        assert!(!back.aurora_is_dark);
        assert_eq!(back.aurora_schema, 2);
        assert_eq!(back.ui_font_size, 14);
        assert!(back.auto_sfx_filter);
        assert!(back.auto_inpaint);
        assert_eq!(back.auto_inpaint_model, AutoInpaintModel::Mixed);
    }

    #[test]
    fn missing_fields_default() {
        let back: Settings = toml::from_str("").unwrap();
        assert!(back.connections.is_empty());
        assert_eq!(back.last_provider, None);
        assert!(back.auto_style_detect);
        assert_eq!(back.ocr_workers, "2");
        assert_eq!(back.ocr_text_score, "0.7");
        assert_eq!(back.ocr_min_text_height, "40");
        assert_eq!(back.ocr_max_text_height, "100");
        assert_eq!(back.ocr_max_side_len, "2000");
        assert_eq!(back.ocr_merge_threshold, "0.5");
        assert!(!back.free_models_only);
        assert!(back.hidden_models.is_empty());
        assert_eq!(back.inpaint_backend, InpaintBackend::Telea);
        assert_eq!(back.inpaint_radius, "5");
        assert_eq!(back.aurora_color, "#3b0600");
        assert_eq!(back.aurora_blob_count, 2);
        assert!(back.aurora_is_dark);
        assert_eq!(back.aurora_schema, 1);
        assert_eq!(back.ui_font_size, 12);
        assert!(back.auto_sfx_filter);
        assert!(back.auto_inpaint);
        assert_eq!(back.auto_inpaint_model, AutoInpaintModel::Mixed);
    }

    #[test]
    fn legacy_api_key_field_is_ignored() {
        let back: Settings = toml::from_str(r#"api_key = "kilo""#).unwrap();
        assert!(back.connections.is_empty());
    }

    #[test]
    fn round_trips_through_a_confy_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("default-config.toml");
        let settings = Settings {
            last_provider: Some("openai".to_string()),
            ..Settings::default()
        };
        confy::store_path(&path, &settings).unwrap();
        let back: Settings = confy::load_path(&path).unwrap();
        assert_eq!(back.last_provider.as_deref(), Some("openai"));
        assert_eq!(back.ocr_workers, "2");
    }
}
