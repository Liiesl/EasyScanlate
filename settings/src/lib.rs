//! Persisted app settings, owned by this crate and shared by every other
//! crate directly (no per-field routing through the app). Backed by
//! [`confy`]: the store lives in the OS config dir
//! (`%APPDATA%\easyscanlate\config\default-config.toml` on Windows,
//! `~/.config/easyscanlate/config/default-config.toml` on Linux), so the same
//! code works cross-platform.
//!
//! A process-wide store is initialized once at boot with [`init`]; any crate
//! then reads through [`get`] and mutates + persists through [`modify`]
//! (write-through: every `modify` saves to disk immediately). Closures must
//! not call back into [`get`]/[`modify`] (re-entrant locking would deadlock).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use easyscanlate_model::EntryStyle;
use serde::{Deserialize, Serialize};

/// The confy application name; decides the config directory name.
const APP_NAME: &str = "easyscanlate";

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
    /// AOT-GAN ONNX model: faster + lower memory than LaMa, variable
    /// resolution up to 1024 with pad=8, DirectML with CPU fallback.
    Aot,
}

impl fmt::Display for InpaintBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Telea => "Telea (inpaint crate)",
            Self::Lama => "LaMa (ONNX)",
            Self::Aot => "AOT-GAN (ONNX)",
        })
    }
}

/// Which inpaint model the **auto** post-OCR pipeline uses. Distinct from
/// [`InpaintBackend`] (manual tool) because `Mixed` is a bg-aware routing:
/// `Solid`→no inpaint, `Gradient`→Telea, `Artwork`→LaMa (Aot available
/// as explicit choice; user requested to keep Lama in Mixed for now).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutoInpaintModel {
    Telea,
    Lama,
    Aot,
    #[default]
    Mixed,
}

impl fmt::Display for AutoInpaintModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Telea => "Telea",
            Self::Lama => "LaMa",
            Self::Aot => "AOT-GAN",
            Self::Mixed => "Mixed (bg-aware)",
        })
    }
}

/// How many preset slots the app starts with: five built-in styles plus
/// three empty slots.
pub const INITIAL_PRESET_SLOTS: usize = 8;

fn default_style_presets() -> StylePresets {
    StylePresets::default_presets()
}

/// The style-preset slot list, persisted per-user in `settings` (not per-project in `model`).
/// `None` = empty slot. "+" fills the first empty slot or appends.
///
/// TOML has no `null`, so `Vec<Option<T>>` cannot be serialized directly
/// (`UnsupportedNone`). We store only the filled presets compactly as
/// `Vec<EntryStyle>`; on load we pad trailing `None`s to at least
/// `INITIAL_PRESET_SLOTS` so the UI still renders empty placeholder tiles.
#[derive(Debug, Clone, PartialEq)]
pub struct StylePresets(Vec<Option<EntryStyle>>);

impl Serialize for StylePresets {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Preserve middle `None`s as `{ empty = true }` so order/holes survive,
        // but trim trailing `None`s for compactness (they are re-padded on load).
        // This avoids `Vec<Option<T>>`'s `UnsupportedNone` (TOML has no null).
        #[derive(Serialize)]
        #[serde(untagged)]
        enum SerSlot<'a> {
            Filled(&'a EntryStyle),
            Empty { empty: bool },
        }
        let mut slots: Vec<SerSlot> = self
            .0
            .iter()
            .map(|o| match o {
                None => SerSlot::Empty { empty: true },
                Some(st) => SerSlot::Filled(st),
            })
            .collect();
        // Trim trailing empties — they will be re-added as placeholders.
        while matches!(slots.last(), Some(SerSlot::Empty { empty: true })) {
            slots.pop();
        }
        slots.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for StylePresets {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Flexible: handle compact `Vec<EntryStyle>`, trimmed `Vec<Slot>`,
        // and legacy full `Vec<Option<EntryStyle>>` (via toml::Value inspection).
        let values = Vec::<toml::Value>::deserialize(deserializer)?;
        let mut out: Vec<Option<EntryStyle>> = Vec::new();
        for val in values {
            if let Some(tbl) = val.as_table() {
                // `{ empty = true }` placeholder
                if tbl.get("empty").and_then(|v| v.as_bool()).unwrap_or(false) {
                    out.push(None);
                    continue;
                }
                // `{ empty = false, style = { ... } }` wrapper
                if let Some(style_val) = tbl.get("style") {
                    match style_val.clone().try_into::<EntryStyle>() {
                        Ok(st) => out.push(Some(st)),
                        Err(e) => return Err(serde::de::Error::custom(e)),
                    }
                    continue;
                }
                // If table has `empty = false` without `style`, treat as None? fallback
                if tbl.contains_key("empty") {
                    // empty == false but no style -> treat as None placeholder? Should not happen.
                    out.push(None);
                    continue;
                }
            }
            // Otherwise treat whole value as `EntryStyle` (compact case or direct style table)
            if val.is_table() {
                let style: EntryStyle = val.clone().try_into().map_err(serde::de::Error::custom)?;
                out.push(Some(style));
            } else {
                out.push(None);
            }
        }
        if out.len() < INITIAL_PRESET_SLOTS {
            out.resize_with(INITIAL_PRESET_SLOTS, || None);
        }
        Ok(Self(out))
    }
}

impl Default for StylePresets {
    fn default() -> Self {
        Self::default_presets()
    }
}

impl StylePresets {
    pub fn default_presets() -> Self {
        let mut presets = Vec::with_capacity(INITIAL_PRESET_SLOTS);
        let mut preset = EntryStyle::default();
        presets.push(Some(preset.clone()));
        preset.bg_color = [0, 0, 0, 255];
        preset.text_color = [255, 255, 255, 255];
        presets.push(Some(preset.clone()));
        preset.bg_color = [0, 0, 0, 0];
        preset.text_color = [0, 0, 0, 255];
        presets.push(Some(preset.clone()));
        preset.text_color = [255, 255, 255, 255];
        presets.push(Some(preset.clone()));
        preset.bg_color = [255, 0, 0, 255];
        presets.push(Some(preset));
        presets.resize(INITIAL_PRESET_SLOTS, None);
        Self(presets)
    }

    pub fn get(&self, index: usize) -> Option<EntryStyle> {
        self.0.get(index).cloned().flatten()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn as_slice(&self) -> &[Option<EntryStyle>] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn add(&mut self, style: EntryStyle) {
        if let Some(slot) = self.0.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(style);
        } else {
            self.0.push(Some(style));
        }
    }

    pub fn replace(&mut self, index: usize, style: EntryStyle) {
        if let Some(slot) = self.0.get_mut(index) {
            *slot = Some(style);
        }
    }

    pub fn remove(&mut self, index: usize) {
        if let Some(slot) = self.0.get_mut(index) {
            *slot = None;
        }
    }
}

/// One entry in the homepage's recent-projects list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentProject {
    /// Absolute path to the `.mmtl` file.
    pub path: String,
    /// File stem for display (e.g. `tesies.mmtl`).
    pub name: String,
    /// Unix seconds when last opened/created.
    pub last_opened: i64,
}

pub fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn format_relative(last_opened: i64) -> String {
    let now = now_unix_secs();
    let diff = (now - last_opened).max(0);
    const MIN: i64 = 60;
    const HOUR: i64 = 3600;
    const DAY: i64 = 86400;
    const WEEK: i64 = 7 * DAY;
    const MONTH: i64 = 30 * DAY;
    const YEAR: i64 = 365 * DAY;
    if diff < MIN {
        "just now".to_string()
    } else if diff < HOUR {
        let m = diff / MIN;
        if m == 1 { "1 minute ago".to_string() } else { format!("{m} minutes ago") }
    } else if diff < DAY {
        let h = diff / HOUR;
        if h == 1 { "1 hour ago".to_string() } else { format!("{h} hours ago") }
    } else if diff < WEEK {
        let d = diff / DAY;
        if d == 1 { "1 day ago".to_string() } else { format!("{d} days ago") }
    } else if diff < MONTH {
        let w = diff / WEEK;
        if w == 1 { "1 week ago".to_string() } else { format!("{w} weeks ago") }
    } else if diff < YEAR {
        let mo = diff / MONTH;
        if mo == 1 { "1 month ago".to_string() } else { format!("{mo} months ago") }
    } else {
        let y = diff / YEAR;
        if y == 1 { "1 year ago".to_string() } else { format!("{y} years ago") }
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
    /// Recent projects for the homepage, most-recent first. Max 20.
    #[serde(default)]
    pub recent_projects: Vec<RecentProject>,
    /// User style presets (per-user, not per-project). Persisted here so
    /// `model` stays pure project data.
    #[serde(default = "default_style_presets")]
    pub style_presets: StylePresets,
    /// Whether to check GitHub releases for updates automatically at
    /// startup. When disabled, updates are only checked via the manual
    /// "Check for updates" button in Settings → Updates.
    #[serde(default = "default_true")]
    pub auto_check_updates: bool,
    /// Whether the first-run onboarding wizard has been completed. `false` on
    /// fresh installs triggers the blocking onboarding overlay (models +
    /// preferences) before the editor is usable.
    #[serde(default)]
    pub onboarding_completed: bool,
    /// Monotonic version of the onboarding flow. Bumped when the wizard
    /// structure changes so existing installs can be re-prompted if needed.
    /// `0` means never completed, `1` is the current version.
    #[serde(default)]
    pub onboarding_version: u32,
    /// Stored translation connections, keyed by provider id (`openai`,
    /// `deepseek`, `custom-openai`, ...). A provider is "connected" when it
    /// has an entry here; disconnect removes the entry.
    #[serde(default)]
    pub connections: BTreeMap<String, Connection>,
    /// Per-provider set of hidden model ids (Manage Models overlay). A model
    /// is hidden by the user to filter unused entries; deprecated and non-text
    /// models are always filtered out and never stored here. When a provider is
    /// first fetched, older paid family members are auto-hidden (free and
    /// `*-latest` stay visible; newest per family via `release_date`/
    /// `last_updated` stays visible) – this is the default hidden set
    /// produced by `default_hidden_ids` / `default_hidden_ids_for_models`.
    /// Clearing the entry shows all usable models.
    #[serde(default)]
    pub hidden_models: BTreeMap<String, BTreeSet<String>>,
}

pub const CURRENT_ONBOARDING_VERSION: u32 = 1;

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
            recent_projects: Vec::new(),
            style_presets: default_style_presets(),
            auto_check_updates: true,
            onboarding_completed: false,
            onboarding_version: 0,
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

/// Returns the absolute path to the confy configuration file.
///
/// On Windows this is typically `%APPDATA%\easyscanlate\config\default-config.toml`,
/// on Linux `~/.config/easyscanlate/config/default-config.toml`.
pub fn config_file_path() -> std::path::PathBuf {
    confy::get_configuration_file_path(APP_NAME, None).unwrap_or_else(|_| {
        // Fallback: should never happen, but keep a deterministic path for tests.
        std::path::PathBuf::from("easyscanlate/default-config.toml")
    })
}

/// Returns the directory containing the confy configuration file.
pub fn config_dir() -> std::path::PathBuf {
    config_file_path()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

/// Returns the directory where ONNX models are persisted.
///
/// This is a sibling `models/` directory next to the settings file, e.g.
/// `%APPDATA%\easyscanlate\config\models\` on Windows and
/// `~/.config/easyscanlate/config/models/` on Linux. It is the canonical
/// location for all downloaded models and is created on demand.
pub fn models_dir() -> std::path::PathBuf {
    config_dir().join("models")
}

/// Like [`models_dir`] but derived from an explicit config file path (useful for tests).
pub fn models_dir_for_config_path(config_path: &std::path::Path) -> std::path::PathBuf {
    config_path
        .parent()
        .map(|p| p.join("models"))
        .unwrap_or_else(|| std::path::PathBuf::from("models"))
}

/// Returns the full path for a model file inside [`models_dir`].
pub fn model_path(filename: &str) -> std::path::PathBuf {
    models_dir().join(filename)
}

/// Ensures [`models_dir`] exists, creating it recursively if needed.
pub fn ensure_models_dir() -> std::io::Result<std::path::PathBuf> {
    let dir = models_dir();
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Marks onboarding as completed at the current version.
pub fn mark_onboarding_completed() {
    let _ = modify(|s| {
        s.onboarding_completed = true;
        s.onboarding_version = CURRENT_ONBOARDING_VERSION;
    });
}

/// Resets onboarding so the wizard will show again on next boot or immediately.
pub fn reset_onboarding() {
    let _ = modify(|s| {
        s.onboarding_completed = false;
        s.onboarding_version = 0;
    });
}

/// Resolves a model file path preferring the persisted `models_dir()` (where
/// onboarding downloads land) over the legacy crate-relative `../models`
/// folder used in development. Returns the first existing file, or the
/// canonical `models_dir` path if neither exists yet (the download target).
pub fn resolve_model_path(filename: &str) -> std::path::PathBuf {
    let canonical = model_path(filename);
    if canonical.exists() {
        return canonical;
    }
    // Legacy fallback: <workspace>/models/<filename> (ocr/inpaint/etc crate-relative)
    // Keep for dev where models are checked into /models.
    let legacy = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../models")
        .join(filename);
    // Also try exe-relative models/ (installer layout)
    let exe_legacy = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("models").join(filename)));
    if legacy.exists() {
        return legacy;
    }
    if let Some(p) = exe_legacy.as_ref().filter(|p| p.exists()) {
        return p.clone();
    }
    // Fallback to legacy koharu alt used by segment crate
    let alt = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../onnx-text-styling-classification/panel-bubble-sfx-det")
        .join(filename);
    if alt.exists() {
        return alt;
    }
    canonical
}

/// Variant of `resolve_model_path` that also checks a legacy filename (e.g.
/// `yolo26s-seg.onnx` replaced by `koharu-yolo26s-seg.onnx`).
pub fn resolve_model_path_with_legacy(filename: &str, legacy_filename: Option<&str>) -> std::path::PathBuf {
    let p = resolve_model_path(filename);
    if p.exists() {
        return p;
    }
    if let Some(legacy) = legacy_filename {
        let lp = resolve_model_path(legacy);
        if lp.exists() {
            return lp;
        }
    }
    p
}

/// Bump `recent_projects` with `path`, moving it to front and updating
/// `last_opened`. Keeps at most 20 entries.
pub fn touch_recent(path: String) {
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());
    let now = now_unix_secs();
    let _ = modify(|s| {
        s.recent_projects.retain(|r| r.path != path);
        s.recent_projects.insert(
            0,
            RecentProject {
                path: path.clone(),
                name,
                last_opened: now,
            },
        );
        if s.recent_projects.len() > 20 {
            s.recent_projects.truncate(20);
        }
    });
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
            recent_projects: Vec::new(),
            style_presets: StylePresets::default_presets(),
            auto_check_updates: true,
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
        assert_eq!(back.style_presets, StylePresets::default_presets());
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

    #[test]
    fn onboarding_fields_round_trip() {
        let settings = Settings {
            onboarding_completed: true,
            onboarding_version: CURRENT_ONBOARDING_VERSION,
            ..Settings::default()
        };
        let text = toml::to_string(&settings).unwrap();
        let back: Settings = toml::from_str(&text).unwrap();
        assert!(back.onboarding_completed);
        assert_eq!(back.onboarding_version, CURRENT_ONBOARDING_VERSION);
        let empty: Settings = toml::from_str("").unwrap();
        assert!(!empty.onboarding_completed);
        assert_eq!(empty.onboarding_version, 0);
    }
}
