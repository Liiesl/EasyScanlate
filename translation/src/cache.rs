//! On-disk fallback cache for cloud provider model listings.
//!
//! One JSON file per provider under `<config_dir>/models_cache/<id>.json`.
//! Boot (and new connects) paint instantly from this cache, then a single
//! network delta refresh overwrites it when the mirror listing changed.
//! Only connected cloud gateways are cached; custom slots and local
//! providers (`ollama`/`vllm`/`llama cpp`) are endpoint-dependent and skipped.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{is_custom, is_local, Provider};

/// Schema version of the cached files. A mismatch means ignore the file.
const CACHE_VERSION: u32 = 1;

/// What is stored per provider file.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedProvider {
    version: u32,
    fetched_at: i64,
    provider: Provider,
}

fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Directory holding the per-provider cache files. Created on demand.
pub fn cache_dir() -> PathBuf {
    easyscanlate_settings::config_dir().join("models_cache")
}

/// Like [`cache_dir`] but rooted at an explicit config file path (tests).
pub fn cache_dir_for_config_path(config_path: &std::path::Path) -> PathBuf {
    config_path
        .parent()
        .map(|p| p.join("models_cache"))
        .unwrap_or_else(|| PathBuf::from("models_cache"))
}

/// File for one provider id. Returns `None` for ids that must never be
/// cached (custom slots, locals, empty or path-unsafe ids).
pub fn cache_path(id: &str) -> Option<PathBuf> {
    cache_path_in(&cache_dir(), id)
}

fn cache_path_in(dir: &std::path::Path, id: &str) -> Option<PathBuf> {
    if id.is_empty() || is_custom(id) || is_local(id) {
        return None;
    }
    if id == "." || id == ".." || id.contains("..") {
        return None;
    }
    if id.chars().any(|c| c == '/' || c == '\\' || c == ':') {
        return None;
    }
    Some(dir.join(format!("{id}.json")))
}

/// Whether two listings differ: model ids (ordered), display names, free
/// flags, family/dates, plus api/env/kind. Used for the delta: only dirty
/// providers are rewritten and re-announced.
pub fn providers_equal(a: &Provider, b: &Provider) -> bool {
    a == b
}

/// Loads one cached provider, or `None` when missing/corrupt/versioned-out.
pub fn load_cached_provider(id: &str) -> Option<Provider> {
    let path = cache_path(id)?;
    load_cached_provider_from_path(id, &path)
}

fn load_cached_provider_from_path(id: &str, path: &std::path::Path) -> Option<Provider> {
    let bytes = std::fs::read(path).ok()?;
    let cached: CachedProvider = serde_json::from_slice(&bytes).ok()?;
    if cached.version != CACHE_VERSION {
        return None;
    }
    if cached.provider.id != id {
        return None;
    }
    Some(cached.provider)
}

/// Loads every cached provider for `ids` (connected cloud gateways).
pub fn load_cached_providers(ids: &[String]) -> HashMap<String, Provider> {
    let mut out = HashMap::new();
    for id in ids {
        if let Some(provider) = load_cached_provider(id) {
            out.insert(id.clone(), provider);
        }
    }
    out
}

/// Persists one provider listing. Skips custom/local ids. Overwrites only
/// when the content differs from what is on disk (delta write). Failures
/// are logged, never fatal.
pub fn save_provider(provider: &Provider) {
    let Some(path) = cache_path(&provider.id) else {
        return;
    };
    if let Some(existing) = load_cached_provider_from_path(&provider.id, &path)
        && providers_equal(&existing, provider)
    {
        return;
    }
    let cached = CachedProvider {
        version: CACHE_VERSION,
        fetched_at: now_unix_secs(),
        provider: provider.clone(),
    };
    let Ok(json) = serde_json::to_string_pretty(&cached) else {
        return;
    };
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    // Atomic write: temp file + rename so a crash never leaves half JSON.
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_err() {
        return;
    }
    if std::fs::rename(&tmp, &path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Persists every provider in the map (delta per file).
pub fn save_providers(providers: &HashMap<String, Provider>) {
    for provider in providers.values() {
        save_provider(provider);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CompatKind, Model};

    fn sample(id: &str) -> Provider {
        Provider {
            id: id.to_string(),
            name: "Sample".to_string(),
            api: "https://example.test/v1".to_string(),
            kind: CompatKind::OpenAI,
            api_key_env: "SAMPLE_API_KEY".to_string(),
            models: vec![Model {
                id: "m1".to_string(),
                name: "M 1".to_string(),
                free: false,
                family: None,
                release_date: None,
                last_updated: None,
            }],
        }
    }

    #[test]
    fn cache_path_rejects_custom_local_and_unsafe_ids() {
        assert!(cache_path(crate::CUSTOM_OPENAI).is_none());
        assert!(cache_path(crate::LOCAL_OLLAMA).is_none());
        assert!(cache_path("").is_none());
        assert!(cache_path("../openai").is_none());
        assert!(cache_path("a/b").is_none());
        assert!(cache_path("openai").is_some());
        // Test-rooted variant keeps the same guards.
        let dir = std::path::Path::new("/tmp/x");
        assert!(cache_path_in(dir, "openai").is_some());
        assert!(cache_path_in(dir, "a/b").is_none());
    }

    #[test]
    fn providers_equal_detects_model_changes() {
        let a = sample("openai");
        let mut b = sample("openai");
        assert!(providers_equal(&a, &b));
        b.models.push(Model {
            id: "m2".to_string(),
            name: "M 2".to_string(),
            free: true,
            family: None,
            release_date: None,
            last_updated: None,
        });
        assert!(!providers_equal(&a, &b));
    }

    #[test]
    fn cached_provider_round_trips_through_json() {
        let cached = CachedProvider {
            version: CACHE_VERSION,
            fetched_at: 123,
            provider: sample("deepseek"),
        };
        let json = serde_json::to_string(&cached).unwrap();
        let back: CachedProvider = serde_json::from_str(&json).unwrap();
        assert_eq!(back.version, CACHE_VERSION);
        assert!(providers_equal(&back.provider, &cached.provider));
    }
}
