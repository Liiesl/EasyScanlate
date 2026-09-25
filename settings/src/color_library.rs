//! App-level color-picker library persisted as a separate JSON file.
//!
//! The color picker Library tab (swatch sets + recent colors + active set)
//! is shared across the fill / stroke / bg inputs and survives restarts.
//! Stored at `<config_dir>/color_library.json` (sibling of the confy TOML),
//! so large libraries do not bloat `default-config.toml`.
//!
//! Types here are plain DTOs (`[u8; 4]` RGBA, `f32` offsets) with no `iced`
//! dependency; the `ui`/`app` boundary converts to/from
//! `neverliie_iced_widgets::color_picker::{SwatchSet, PickedValue}`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Max swatches per set, mirroring the widget limit.
pub const MAX_SWATCHES_PER_SET: usize = 24;
/// Max recent colors, mirroring the widget limit.
pub const MAX_RECENT: usize = 12;
/// File name inside the config dir.
pub const FILE_NAME: &str = "color_library.json";

fn default_set_name() -> String {
    "Default".to_string()
}

/// One gradient stop: position `0..=1` + RGBA bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredStop {
    #[serde(default)]
    pub offset: f32,
    #[serde(default)]
    pub rgba: [u8; 4],
}

/// A picked value: solid color or two-stop gradient.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StoredPicked {
    Solid { rgba: [u8; 4] },
    Gradient { stops: Vec<StoredStop> },
}

/// One named swatch set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredSwatchSet {
    #[serde(default = "default_set_name")]
    pub name: String,
    #[serde(default)]
    pub colors: Vec<StoredPicked>,
}

/// The whole persisted library.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredLibrary {
    #[serde(default)]
    pub sets: Vec<StoredSwatchSet>,
    #[serde(default)]
    pub recents: Vec<StoredPicked>,
    #[serde(default)]
    pub active_tab: usize,
}

impl Default for StoredLibrary {
    fn default() -> Self {
        Self {
            sets: vec![StoredSwatchSet {
                name: default_set_name(),
                colors: Vec::new(),
            }],
            recents: Vec::new(),
            active_tab: 0,
        }
    }
}

impl StoredLibrary {
    /// Clamp offsets, truncate to widget limits, restore the Default set
    /// when empty and clamp `active_tab`. Idempotent.
    pub fn sanitized(mut self) -> Self {
        for set in &mut self.sets {
            if set.name.trim().is_empty() {
                set.name = default_set_name();
            }
            for color in &mut set.colors {
                if let StoredPicked::Gradient { stops } = color {
                    for stop in stops.iter_mut() {
                        stop.offset = stop.offset.clamp(0.0, 1.0);
                    }
                }
            }
            set.colors.truncate(MAX_SWATCHES_PER_SET);
        }
        // Gradients with wrong stop counts are kept as-is; the widget layer
        // drops malformed ones on conversion (expects exactly 2 stops).
        self.recents.truncate(MAX_RECENT);
        if self.sets.is_empty() {
            self.sets.push(StoredSwatchSet {
                name: default_set_name(),
                colors: Vec::new(),
            });
        }
        if self.sets.len() == 1 {
            self.active_tab = 0;
        } else {
            self.active_tab = self.active_tab.min(self.sets.len() - 1);
        }
        self
    }
}

/// Path of the library file derived from the confy config dir.
pub fn library_path() -> PathBuf {
    super::config_dir().join(FILE_NAME)
}

/// Variant derived from an explicit config file path (useful for tests).
pub fn library_path_for_config_path(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .map(|p| p.join(FILE_NAME))
        .unwrap_or_else(|| PathBuf::from(FILE_NAME))
}

/// Loads the library from the default path; missing/corrupt → defaults.
pub fn load() -> StoredLibrary {
    load_from(&library_path())
}

/// Loads the library from an explicit path; missing/corrupt → defaults.
pub fn load_from(path: &Path) -> StoredLibrary {
    let Ok(bytes) = std::fs::read(path) else {
        return StoredLibrary::default();
    };
    serde_json::from_slice::<StoredLibrary>(&bytes)
        .map(|lib| lib.sanitized())
        .unwrap_or_default()
}

/// Saves the library (sanitized) to the default path.
pub fn save(library: &StoredLibrary) -> Result<(), String> {
    save_to(&library_path(), library)
}

/// Saves the library (sanitized) to an explicit path, creating parents.
pub fn save_to(path: &Path, library: &StoredLibrary) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let clean = library.clone().sanitized();
    let text = serde_json::to_string_pretty(&clean).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_single_empty_set() {
        let lib = StoredLibrary::default();
        assert_eq!(lib.sets.len(), 1);
        assert_eq!(lib.sets[0].name, "Default");
        assert!(lib.sets[0].colors.is_empty());
        assert!(lib.recents.is_empty());
        assert_eq!(lib.active_tab, 0);
    }

    #[test]
    fn missing_file_yields_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        assert_eq!(load_from(&path), StoredLibrary::default());
    }

    #[test]
    fn round_trips_through_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let lib = StoredLibrary {
            sets: vec![
                StoredSwatchSet {
                    name: "Mine".to_string(),
                    colors: vec![
                        StoredPicked::Solid { rgba: [255, 0, 0, 255] },
                        StoredPicked::Gradient {
                            stops: vec![
                                StoredStop { offset: 0.0, rgba: [0, 0, 0, 255] },
                                StoredStop { offset: 1.0, rgba: [255, 255, 255, 255] },
                            ],
                        },
                    ],
                },
                StoredSwatchSet { name: "Other".to_string(), colors: vec![] },
            ],
            recents: vec![StoredPicked::Solid { rgba: [1, 2, 3, 255] }],
            active_tab: 1,
        };
        save_to(&path, &lib).unwrap();
        assert_eq!(load_from(&path), lib.sanitized());
    }

    #[test]
    fn sanitizes_limits_and_empty() {
        let lib = StoredLibrary {
            sets: vec![],
            recents: vec![StoredPicked::Solid { rgba: [0, 0, 0, 255] }; MAX_RECENT + 5],
            active_tab: 99,
        };
        let clean = lib.sanitized();
        assert_eq!(clean.sets.len(), 1);
        assert_eq!(clean.active_tab, 0);
        assert_eq!(clean.recents.len(), MAX_RECENT);
    }
}
