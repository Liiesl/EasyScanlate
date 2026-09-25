//! Per-project series tracking, persisted as a single `series.toml` next to
//! the main config file (e.g. `%APPDATA%\easyscanlate\config\series.toml`).
//!
//! The `.mmtl` itself is the source of truth for a project's series
//! (`Project::series` in `project.xml`); this index is a derived cache so the
//! home sidebar can group projects without opening every file. Standalone
//! projects (`series = None`) are tracked too, under the `None` group.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::now_unix_secs;

/// One tracked `.mmtl`, with its last-known series (`None` = standalone).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackedProject {
    /// Absolute path to the `.mmtl` file.
    pub path: String,
    /// File stem for display.
    pub name: String,
    /// Series name (`None` = standalone).
    #[serde(default)]
    pub series: Option<String>,
    /// Unix seconds when last opened/created.
    pub last_opened: i64,
}

/// The whole `series.toml` document.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SeriesStore {
    /// Most-recent first. Max [`MAX_TRACKED`].
    #[serde(default)]
    pub items: Vec<TrackedProject>,
    /// Collapsed series names in the home sidebar.
    #[serde(default)]
    pub collapsed: BTreeSet<String>,
}

/// Max tracked projects kept in `series.toml`.
pub const MAX_TRACKED: usize = 100;

fn normalize_series(series: Option<String>) -> Option<String> {
    series.and_then(|s| {
        let t = s.trim().to_string();
        if t.is_empty() { None } else { Some(t) }
    })
}

fn display_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

/// Path of `series.toml` next to the main config file.
pub fn series_file_path() -> PathBuf {
    super::config_dir().join("series.toml")
}

/// Variant derived from an explicit config file path (useful for tests).
pub fn series_file_for_config_path(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .map(|p| p.join("series.toml"))
        .unwrap_or_else(|| PathBuf::from("series.toml"))
}

fn load_from_path(path: &Path) -> SeriesStore {
    if !path.exists() {
        return SeriesStore::default();
    }
    confy::load_path::<SeriesStore>(path).unwrap_or_default()
}

fn store_to_path(path: &Path, store: &SeriesStore) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = confy::store_path(path, store) {
        eprintln!("[series] persist failed: {e}");
    }
}

/// Loads the series index from disk; missing/corrupt yields empty.
pub fn load_series() -> SeriesStore {
    load_from_path(&series_file_path())
}

/// Records `path` under `series` (dedup by path, most-recent first).
/// `None`/empty series tracks the project as standalone.
pub fn touch_series(path: String, series: Option<String>) {
    if path.trim().is_empty() {
        return;
    }
    let series = normalize_series(series);
    let name = display_name(&path);
    let now = now_unix_secs();
    let file = series_file_path();
    let mut store = load_from_path(&file);
    store.items.retain(|r| r.path != path);
    store.items.insert(
        0,
        TrackedProject {
            path: path.clone(),
            name,
            series,
            last_opened: now,
        },
    );
    if store.items.len() > MAX_TRACKED {
        store.items.truncate(MAX_TRACKED);
    }
    store_to_path(&file, &store);
}

/// Changes the series tag of `path` in place (keeps position and
/// `last_opened`). Inserts at front if untracked. Unlike [`touch_series`]
/// this does not bump recency — for edits like settings reassignment.
pub fn reassign(path: &str, series: Option<String>) {
    if path.trim().is_empty() {
        return;
    }
    let series = normalize_series(series);
    let file = series_file_path();
    let mut store = load_from_path(&file);
    if let Some(item) = store.items.iter_mut().find(|r| r.path == path) {
        item.series = series;
    } else {
        let name = display_name(path);
        store.items.insert(
            0,
            TrackedProject {
                path: path.to_string(),
                name,
                series,
                last_opened: now_unix_secs(),
            },
        );
        if store.items.len() > MAX_TRACKED {
            store.items.truncate(MAX_TRACKED);
        }
    }
    store_to_path(&file, &store);
}

/// Drops entries whose `.mmtl` no longer exists. Skips the write when clean.
pub fn prune_missing_series() {
    let file = series_file_path();
    let store = load_from_path(&file);
    if store
        .items
        .iter()
        .any(|r| r.path.trim().is_empty() || !Path::new(&r.path).exists())
    {
        let mut store = store;
        store
            .items
            .retain(|r| !r.path.trim().is_empty() && Path::new(&r.path).exists());
        store_to_path(&file, &store);
    }
}

/// Removes a single path from the index (e.g. open failed: moved/deleted).
pub fn remove_series_path(path: &str) {
    let file = series_file_path();
    let store = load_from_path(&file);
    if store.items.iter().any(|r| r.path == path) {
        let mut store = store;
        store.items.retain(|r| r.path != path);
        store_to_path(&file, &store);
    }
}

/// Sorted distinct non-empty series names, most-recently-touched first.
pub fn series_names() -> Vec<String> {
    let store = load_from_path(&series_file_path());
    series_names_of(&store)
}

fn series_names_of(store: &SeriesStore) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for item in &store.items {
        if let Some(s) = item.series.as_deref()
            && seen.insert(s.to_string())
        {
            out.push(s.to_string());
        }
    }
    out
}

/// Whether `name` is collapsed in the sidebar.
///
/// Kept for back-compat with existing `series.toml` files; the home sidebar
/// now collapses the `Series` group as a whole (session-only) instead.
#[allow(dead_code)]
pub fn is_collapsed(name: &str) -> bool {
    load_from_path(&series_file_path()).collapsed.contains(name)
}

/// Persists the collapsed flag for `name`.
///
/// Kept for back-compat; no longer called by the UI.
#[allow(dead_code)]
pub fn set_collapsed(name: String, collapsed: bool) {
    if name.trim().is_empty() {
        return;
    }
    let file = series_file_path();
    let mut store = load_from_path(&file);
    if collapsed {
        store.collapsed.insert(name);
    } else {
        store.collapsed.remove(&name);
    }
    store_to_path(&file, &store);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_empty_becomes_none() {
        assert_eq!(normalize_series(None), None);
        assert_eq!(normalize_series(Some("  ".to_string())), None);
        assert_eq!(
            normalize_series(Some("  Solo  ".to_string())),
            Some("Solo".to_string())
        );
    }

    #[test]
    fn names_dedupes_in_touch_order() {
        let store = SeriesStore {
            items: vec![
                TrackedProject {
                    path: "b.mmtl".to_string(),
                    name: "b.mmtl".to_string(),
                    series: Some("B".to_string()),
                    last_opened: 2,
                },
                TrackedProject {
                    path: "a.mmtl".to_string(),
                    name: "a.mmtl".to_string(),
                    series: Some("A".to_string()),
                    last_opened: 1,
                },
                TrackedProject {
                    path: "c.mmtl".to_string(),
                    name: "c.mmtl".to_string(),
                    series: None,
                    last_opened: 0,
                },
            ],
            collapsed: BTreeSet::new(),
        };
        assert_eq!(series_names_of(&store), vec!["B".to_string(), "A".to_string()]);
    }

    #[test]
    fn store_round_trips_through_toml() {
        let store = SeriesStore {
            items: vec![TrackedProject {
                path: "x.mmtl".to_string(),
                name: "x.mmtl".to_string(),
                series: Some("S".to_string()),
                last_opened: 7,
            }],
            collapsed: BTreeSet::from(["S".to_string()]),
        };
        let text = toml::to_string(&store).unwrap();
        let back: SeriesStore = toml::from_str(&text).unwrap();
        assert_eq!(back.items.len(), 1);
        assert_eq!(back.items[0].series.as_deref(), Some("S"));
        assert!(back.collapsed.contains("S"));
        // Legacy file without `series`/`collapsed` still loads.
        let legacy: SeriesStore = toml::from_str("[[items]]\npath = \"y.mmtl\"\nname = \"y.mmtl\"\nlast_opened = 1\n").unwrap();
        assert_eq!(legacy.items[0].series, None);
        assert!(legacy.collapsed.is_empty());
    }
}
