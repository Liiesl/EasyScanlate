//! Machine translation via rig. Only OpenAI-compatible gateways are wired up
//! for now; the rest of the app only ever sees the functions in this module.
//!
//! All OCR lines of all loaded images are translated in a single request.
//! The result is a `Vec<String>` aligned with the input order, which the app
//! stores into the selected profile named `english(auto)` style.

use std::collections::{BTreeMap, HashMap};

use rig::completion::{AssistantContent, CompletionResponse};
use rig::prelude::*;
use rig::providers::openai;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Hard cap on lines per request; a single unbounded prompt is guaranteed to
/// blow the model's context window on big projects.
const MAX_LINES: usize = 1000;

/// Model listing mirror. Provider-specific paths (`/opencode`, `/kilo`, ...)
/// keep the payload small instead of the ~6 MB full index.
const MODELS_MIRROR: &str = "https://models.pileofthings.top";

/// Fallback model choices shown in the UI while the listing is unavailable.
pub const MODELS: [&str; 3] = ["big-pickle", "mimo-v2.5-free", "deepseek-v4-flash-free"];

/// Translation gateways offered in the UI (models.dev provider ids, which
/// double as the mirror path segments).
pub const PROVIDERS: [&str; 2] = ["opencode", "kilo"];

/// Target languages offered in the UI.
pub const LANGUAGES: [&str; 13] = [
    "English",
    "Korean",
    "Japanese",
    "Chinese (Simplified)",
    "Chinese (Traditional)",
    "Spanish",
    "French",
    "German",
    "Italian",
    "Portuguese",
    "Russian",
    "Thai",
    "Vietnamese",
];

/// One translation gateway: where to call, which environment variable holds
/// its API key, and the selectable model ids (already filtered and sorted,
/// or the fallback list when the mirror is unreachable).
#[derive(Debug, Clone)]
pub struct Provider {
    /// models.dev provider id.
    pub id: String,
    /// OpenAI-compatible chat completions base URL.
    pub api: String,
    /// API key environment variable.
    pub api_key_env: String,
    /// Selectable model ids.
    pub models: Vec<String>,
}

/// Profile name convention for machine translations: `english(auto)`.
pub fn profile_name(lang: &str) -> String {
    format!("{}(auto)", lang.to_lowercase())
}

const SYSTEM: &str = "You are a professional scanlation translator for comics, manga and manhwa. \
Translate every line into the requested target language; detect the source language of each line \
yourself (most lines are Korean). Preserve meaning, tone, and the exact order and number of lines. \
Do not add, merge, drop or summarize any line. Do not add commentary, explanations, notes or any \
formatting such as markdown or code blocks. Output only the translations.";

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct TranslationBatch {
    translations: Vec<String>,
}

/// Model listing wire format (models.dev schema, served by the mirror).
#[derive(Debug, Deserialize)]
struct ProviderListing {
    #[serde(default)]
    api: Option<String>,
    #[serde(default)]
    env: Vec<String>,
    #[serde(default)]
    models: BTreeMap<String, ModelInfo>,
}

#[derive(Debug, Deserialize)]
struct ModelInfo {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    last_updated: Option<String>,
    #[serde(default)]
    cost: Option<ModelCost>,
}

#[derive(Debug, Deserialize)]
struct ModelCost {
    #[serde(default)]
    input: Option<f64>,
    #[serde(default)]
    output: Option<f64>,
}

/// Fetches and filters the model list of every [`PROVIDERS`] gateway, keyed
/// by provider id. Each entry falls back to hardcoded defaults when its
/// listing is unreachable.
pub async fn fetch_all_providers() -> HashMap<String, Provider> {
    let mut providers = HashMap::new();
    for id in PROVIDERS {
        providers.insert(id.to_string(), fetch_provider(id).await);
    }
    providers
}

/// Fetches one gateway's listing from the models mirror and returns the
/// usable [`Provider`]: the API base URL and key environment variable are
/// taken from the listing itself. On any failure the hardcoded fallbacks are
/// used, so the app always has something to show.
pub async fn fetch_provider(id: &str) -> Provider {
    let fallback = fallback_provider(id);
    let url = format!("{MODELS_MIRROR}/{id}");
    let response = match reqwest::get(&url).await {
        Ok(response) => response,
        Err(e) => {
            eprintln!("[translation] {id} models fetch failed: {e}; using fallback list");
            return fallback;
        }
    };
    let listing: ProviderListing = match response.json().await {
        Ok(listing) => listing,
        Err(e) => {
            eprintln!("[translation] {id} models listing parse failed: {e}; using fallback list");
            return fallback;
        }
    };
    let models = select_models(&listing);
    eprintln!("[translation] {} model(s) loaded from {url}", models.len());
    Provider {
        id: id.to_string(),
        api: listing
            .api
            .filter(|api| !api.is_empty())
            .unwrap_or_else(|| fallback.api.clone()),
        api_key_env: listing
            .env
            .first()
            .cloned()
            .unwrap_or_else(|| fallback.api_key_env.clone()),
        models,
    }
}

/// The offline fallback for one gateway.
fn fallback_provider(id: &str) -> Provider {
    let (api, api_key_env) = match id {
        "kilo" => ("https://api.kilo.ai/api/gateway", "KILO_API_KEY"),
        _ => ("https://opencode.ai/zen/v1", "OPENCODE_API_KEY"),
    };
    Provider {
        id: id.to_string(),
        api: api.to_string(),
        api_key_env: api_key_env.to_string(),
        models: MODELS.iter().map(|m| (*m).to_string()).collect(),
    }
}

/// Applies the listing filters: drops deprecated models, keeps only the
/// newest release of each family for paid models, always lists free models
/// (input or output cost 0), and sorts the result by id.
fn select_models(listing: &ProviderListing) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    let mut latest: BTreeMap<String, (&str, &ModelInfo)> = BTreeMap::new();
    for (id, info) in &listing.models {
        if info.status.as_deref() == Some("deprecated") {
            continue;
        }
        if is_free(info) {
            ids.push(id.clone());
            continue;
        }
        let family = info.family.clone().unwrap_or_else(|| id.clone());
        let keep = match latest.get(&family) {
            Some((_, current)) if !is_newer(info, current) => false,
            _ => true,
        };
        if keep {
            latest.insert(family, (id, info));
        }
    }
    ids.extend(latest.into_values().map(|(id, _)| id.to_string()));
    ids.sort();
    ids
}

/// A model whose input or output cost is zero is free and always listed.
fn is_free(info: &ModelInfo) -> bool {
    matches!(&info.cost, Some(cost) if cost.input == Some(0.0) || cost.output == Some(0.0))
}

/// Whether `info` is a newer release than `current`; release date first,
/// then last-updated. Ties keep the existing entry.
fn is_newer(info: &ModelInfo, current: &ModelInfo) -> bool {
    match (info.release_date.as_deref(), current.release_date.as_deref()) {
        (Some(a), Some(b)) if a != b => return a > b,
        (Some(_), None) => return true,
        (None, Some(_)) => return false,
        _ => {}
    }
    match (info.last_updated.as_deref(), current.last_updated.as_deref()) {
        (Some(a), Some(b)) => a > b,
        (Some(_), None) => true,
        _ => false,
    }
}

/// Translates every line in `texts` into `target` using `model` on the given
/// gateway. `api_key` overrides the provider's environment variable when set
/// (in-memory only; never persisted). On success returns one translation per
/// input line, in the same order.
pub async fn translate_all(
    texts: &[String],
    target: &str,
    provider: &Provider,
    model: &str,
    api_key: Option<String>,
) -> Result<Vec<String>, String> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    if texts.len() > MAX_LINES {
        return Err(format!(
            "Too many lines for a single translation request ({}, max {MAX_LINES}).",
            texts.len()
        ));
    }

    let key = api_key
        .filter(|key| !key.is_empty())
        .or_else(|| std::env::var(&provider.api_key_env).ok())
        .unwrap_or_default();
    if key.is_empty() {
        return Err(format!(
            "Translation init failed: no API key for {} (set {} or enter an API key)",
            provider.id, provider.api_key_env
        ));
    }

    let client = openai::CompletionsClient::builder()
        .api_key(&key)
        .base_url(&provider.api)
        .build()
        .map_err(|e| format!("Translation init failed: {e}"))?;
    let completion = client.completion_model(model);

    let prompt = build_prompt(texts, target);
    eprintln!(
        "[translation] request: provider={} model={model} target={target} lines={}\nprompt:\n{prompt}\n---",
        provider.id,
        texts.len()
    );

    match structured(&completion, &prompt, texts.len()).await {
        Ok(translations) => {
            eprintln!("[translation] structured OK ({} lines)", translations.len());
            return Ok(translations);
        }
        Err(e) => eprintln!("[translation] structured attempt failed: {e}"),
    }

    let result = plain(&completion, &prompt, texts.len()).await;
    eprintln!(
        "[translation] plain attempt: {}",
        match &result {
            Ok(lines) => format!("OK ({} lines)", lines.len()),
            Err(e) => format!("failed: {e}"),
        }
    );
    result
}

fn build_prompt(texts: &[String], target: &str) -> String {
    let mut prompt = format!(
        "Translate the following {n} line(s) into {target}.\n\nReturn a JSON object with a \
\"translations\" array containing exactly {n} strings in the same order, one per line.\n\n",
        n = texts.len(),
    );
    for (i, text) in texts.iter().enumerate() {
        let clean = text.replace(['\r', '\n'], " ");
        prompt.push_str(&format!("{}. {clean}\n", i + 1));
    }
    prompt
}

/// Attempt 1: structured JSON output via the provider's schema support.
async fn structured(
    model: &openai::completion::CompletionModel<reqwest::Client>,
    prompt: &str,
    expected: usize,
) -> Result<Vec<String>, String> {
    let request = model
        .completion_request(prompt)
        .preamble(SYSTEM.to_string())
        .temperature(1.0)
        .output_schema(TranslationBatch::json_schema(
            &mut schemars::SchemaGenerator::default(),
        ));
    let response = request.send().await.map_err(|e| e.to_string())?;
    let output = choice_text(&response);
    eprintln!("[translation] structured response ({expected}):\n{output}\n---");
    let batch: TranslationBatch =
        serde_json::from_str(&output).map_err(|e| format!("Bad JSON response: {e}"))?;
    validate_count(batch.translations, expected)
}

/// Attempt 2: the same JSON prompt but without the `response_format` param,
/// which some endpoints (e.g. the opencode console gateway) reject. Models that
/// ignore the JSON instruction may still answer with numbered or plain lines,
/// which [`parse_response`] recovers.
async fn plain(
    model: &openai::completion::CompletionModel<reqwest::Client>,
    prompt: &str,
    expected: usize,
) -> Result<Vec<String>, String> {
    let request = model
        .completion_request(prompt)
        .preamble(SYSTEM.to_string())
        .temperature(1.0);
    let response = request.send().await.map_err(|e| e.to_string())?;
    let output = choice_text(&response);
    eprintln!("[translation] plain response ({expected} expected):\n{output}\n---");
    let translations = parse_response(&output);
    validate_count(translations, expected)
}

/// Extracts the assistant's text from a completion response.
fn choice_text(
    response: &CompletionResponse<openai::completion::CompletionResponse>,
) -> String {
    match response.choice.first_ref() {
        AssistantContent::Text(text) => text.text.clone(),
        _ => String::new(),
    }
}

/// Best-effort parsing of whatever the model actually output: a JSON object or
/// array first, then numbered lines, then plain lines as-is.
fn parse_response(output: &str) -> Vec<String> {
    parse_json_batch(output).unwrap_or_else(|| parse_numbered(output))
}

fn parse_json_batch(output: &str) -> Option<Vec<String>> {
    let trimmed = output.trim();
    let candidate = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|s| s.strip_suffix("```"))
        .unwrap_or(trimmed)
        .trim();
    if let Ok(batch) = serde_json::from_str::<TranslationBatch>(candidate) {
        return Some(batch.translations);
    }
    serde_json::from_str::<Vec<String>>(candidate).ok()
}

fn parse_numbered(output: &str) -> Vec<String> {
    output
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(|line| {
            let rest = line.trim_start_matches(['0', '1', '2', '3', '4', '5', '6', '7', '8', '9']);
            let rest = rest.trim_start_matches(['.', ')', ':', ' ', '"']);
            let rest = rest.trim_end_matches([' ', ',', '"']);
            if rest.is_empty() {
                line.to_string()
            } else {
                rest.to_string()
            }
        })
        .collect()
}

fn validate_count(lines: Vec<String>, expected: usize) -> Result<Vec<String>, String> {
    if lines.len() == expected {
        Ok(lines)
    } else {
        Err(format!(
            "Model returned {} translation(s) for {expected} line(s); skipping.",
            lines.len()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_name_is_lowercase_with_auto_suffix() {
        assert_eq!(profile_name("English"), "english(auto)");
        assert_eq!(profile_name("Chinese (Simplified)"), "chinese (simplified)(auto)");
    }

    #[test]
    fn numbered_lines_are_parsed_in_order() {
        let out = parse_numbered(
            "1. Hello\n2. How are you?\n\n3. Goodbye",
        );
        assert_eq!(out, vec!["Hello", "How are you?", "Goodbye"]);
    }

    #[test]
    fn unnumbered_output_is_kept_as_is() {
        let out = parse_numbered("Hello\nHow are you?\nGoodbye");
        assert_eq!(out, vec!["Hello", "How are you?", "Goodbye"]);
    }

    #[test]
    fn count_mismatch_is_rejected() {
        assert!(validate_count(vec!["a".to_string()], 2).is_err());
        assert_eq!(validate_count(vec!["a".to_string()], 1).unwrap(), vec!["a"]);
    }

    #[test]
    fn json_object_is_recovered_from_plain_response() {
        let out = parse_response(
            "{\n  \"translations\": [\n    \"Protect His Majesty.\",\n    \"Sukbin\",\n    \"You\"\n  ]\n}",
        );
        assert_eq!(out, vec!["Protect His Majesty.", "Sukbin", "You"]);
    }

    #[test]
    fn fenced_json_and_bare_arrays_are_recovered() {
        assert_eq!(
            parse_response("```json\n{\"translations\": [\"a\", \"b\"]}\n```"),
            vec!["a", "b"]
        );
        assert_eq!(parse_response("[\"a\", \"b\"]"), vec!["a", "b"]);
    }

    #[test]
    fn free_models_are_never_family_deduped() {
        let listing = ProviderListing {
            api: Some("https://example.test/v1".into()),
            env: vec!["EXAMPLE_API_KEY".into()],
            models: BTreeMap::from([
                (
                    "paid-v1".into(),
                    ModelInfo {
                        status: None,
                        family: Some("paid".into()),
                        release_date: Some("2025-01-01".into()),
                        last_updated: None,
                        cost: Some(ModelCost {
                            input: Some(2.0),
                            output: Some(4.0),
                        }),
                    },
                ),
                (
                    "paid-v2".into(),
                    ModelInfo {
                        status: None,
                        family: Some("paid".into()),
                        release_date: Some("2025-06-01".into()),
                        last_updated: None,
                        cost: Some(ModelCost {
                            input: Some(2.0),
                            output: Some(4.0),
                        }),
                    },
                ),
                (
                    "free-old".into(),
                    ModelInfo {
                        status: None,
                        family: Some("free".into()),
                        release_date: Some("2024-01-01".into()),
                        last_updated: None,
                        cost: Some(ModelCost {
                            input: Some(0.0),
                            output: Some(1.0),
                        }),
                    },
                ),
                (
                    "free-new".into(),
                    ModelInfo {
                        status: None,
                        family: Some("free".into()),
                        release_date: Some("2025-01-01".into()),
                        last_updated: None,
                        cost: Some(ModelCost {
                            input: Some(1.0),
                            output: Some(0.0),
                        }),
                    },
                ),
                (
                    "retired".into(),
                    ModelInfo {
                        status: Some("deprecated".into()),
                        family: Some("retired".into()),
                        release_date: Some("2025-01-01".into()),
                        last_updated: None,
                        cost: Some(ModelCost {
                            input: Some(2.0),
                            output: Some(4.0),
                        }),
                    },
                ),
                (
                    "loner-v1".into(),
                    ModelInfo {
                        status: None,
                        family: None,
                        release_date: Some("2025-01-01".into()),
                        last_updated: None,
                        cost: Some(ModelCost {
                            input: Some(2.0),
                            output: Some(4.0),
                        }),
                    },
                ),
                (
                    "loner-v2".into(),
                    ModelInfo {
                        status: None,
                        family: None,
                        release_date: Some("2025-07-01".into()),
                        last_updated: None,
                        cost: Some(ModelCost {
                            input: Some(2.0),
                            output: Some(4.0),
                        }),
                    },
                ),
            ]),
        };
        let selected = select_models(&listing);
        assert!(selected.contains(&"paid-v2".to_string()));
        assert!(!selected.contains(&"paid-v1".to_string()));
        assert!(selected.contains(&"free-old".to_string()));
        assert!(selected.contains(&"free-new".to_string()));
        assert!(!selected.contains(&"retired".to_string()));
        // Models without a family are their own family: all of them are kept.
        assert!(selected.contains(&"loner-v2".to_string()));
        assert!(selected.contains(&"loner-v1".to_string()));
        assert!(selected.is_sorted());
    }

    #[test]
    fn listing_parse_recovers_api_env_and_family_fields() {
        let json = r#"{
            "id": "opencode",
            "env": ["OPENCODE_API_KEY"],
            "api": "https://opencode.ai/zen/v1",
            "name": "OpenCode Zen",
            "models": {
                "deepseek-v4-flash-free": {
                    "id": "deepseek-v4-flash-free",
                    "name": "DeepSeek V4 Flash Free",
                    "family": "deepseek-flash",
                    "release_date": "2026-07-31",
                    "cost": {"input": 0, "output": 0}
                }
            }
        }"#;
        let listing: ProviderListing = serde_json::from_str(json).unwrap();
        assert_eq!(listing.api.as_deref(), Some("https://opencode.ai/zen/v1"));
        assert_eq!(listing.env, vec!["OPENCODE_API_KEY"]);
        let info = &listing.models["deepseek-v4-flash-free"];
        assert_eq!(info.family.as_deref(), Some("deepseek-flash"));
        assert!(is_free(info));
        assert_eq!(
            select_models(&listing),
            vec!["deepseek-v4-flash-free".to_string()]
        );
    }
}
