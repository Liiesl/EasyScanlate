//! Machine translation via rig. Only OpenAI-compatible gateways are wired up
//! for now; the rest of the app only ever sees the functions in this module.
//!
//! All OCR lines of all loaded images are translated in a single request.
//! The prompt embeds the lines in an XML-like file structure (grouped per
//! image, each line tagged with its entry id), and the model is asked to
//! return the same file with translations in place. The answer is parsed back
//! by tag, so the order the model emits the lines in does not matter. The
//! result is a `Vec<String>` aligned with the input order, which the app
//! stores into the selected profile named `english(auto)` style.

use std::collections::{BTreeMap, HashMap};

use rig::completion::{AssistantContent, CompletionResponse};
use rig::prelude::*;
use rig::providers::openai;
use serde::Deserialize;

/// Hard cap on lines per request; a single unbounded prompt is guaranteed to
/// blow the model's context window on big projects.
const MAX_LINES: usize = 1000;

/// Model listing mirror. Provider-specific paths (`/opencode`, `/kilo`, ...)
/// keep the payload small instead of the ~6 MB full index.
const MODELS_MIRROR: &str = "https://models.pileofthings.top";

/// Fallback model choices shown in the UI while the listing is unavailable.
pub const MODELS: [&str; 3] = ["big-pickle", "mimo-v2.5-free", "deepseek-v4-flash-free"];

/// The free models of the [`MODELS`] fallback list.
pub const MODELS_FREE: [&str; 2] = ["mimo-v2.5-free", "deepseek-v4-flash-free"];

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

/// One selectable translation model: its id as shown in the UI and whether
/// it is free (input or output cost 0).
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub id: String,
    pub free: bool,
}

/// One translation gateway: where to call, which environment variable holds
/// its API key, and the selectable models (already filtered and sorted, or
/// the fallback list when the mirror is unreachable).
#[derive(Debug, Clone)]
pub struct Provider {
    /// models.dev provider id.
    pub id: String,
    /// OpenAI-compatible chat completions base URL.
    pub api: String,
    /// API key environment variable.
    pub api_key_env: String,
    /// Selectable models.
    pub models: Vec<Model>,
}

/// Profile name convention for machine translations: `english(auto)`.
pub fn profile_name(lang: &str) -> String {
    format!("{}(auto)", lang.to_lowercase())
}

const SYSTEM: &str = "You are a professional scanlation translator for comics, manga and manhwa. \
Translate every line into the requested target language; detect the source language of each line \
yourself (most lines are Korean). Preserve meaning, tone, and the exact structure of the file. \
Do not add, merge, drop or reorder any line. Do not add commentary, explanations, notes or any \
formatting such as markdown or code blocks. Output only the file.";

/// One OCR line to translate: the image it belongs to (used as the file tag
/// in the wire format), its stable entry id (used as the row tag) and the
/// source text.
#[derive(Debug, Clone)]
pub struct TranslateItem {
    pub filename: String,
    pub id: u64,
    pub text: String,
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
        models: MODELS
            .iter()
            .map(|m| Model {
                id: (*m).to_string(),
                free: MODELS_FREE.contains(m),
            })
            .collect(),
    }
}

/// Applies the listing filters: drops deprecated models, keeps only the
/// newest release of each family for paid models, always lists free models
/// (input or output cost 0), and sorts the result by id.
fn select_models(listing: &ProviderListing) -> Vec<Model> {
    let mut ids: Vec<Model> = Vec::new();
    let mut latest: BTreeMap<String, (&str, &ModelInfo)> = BTreeMap::new();
    for (id, info) in &listing.models {
        if info.status.as_deref() == Some("deprecated") {
            continue;
        }
        if is_free(info) {
            ids.push(Model {
                id: id.clone(),
                free: true,
            });
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
    ids.extend(
        latest
            .into_values()
            .map(|(id, _)| Model {
                id: id.to_string(),
                free: false,
            }),
    );
    ids.sort_by(|a, b| a.id.cmp(&b.id));
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

/// Translates every line in `items` into `target` using `model` on the given
/// gateway. `api_key` overrides the provider's environment variable when set
/// (in-memory only; never persisted). On success returns one translation per
/// input line, in the same order.
pub async fn translate_all(
    items: &[TranslateItem],
    target: &str,
    provider: &Provider,
    model: &str,
    api_key: Option<String>,
) -> Result<Vec<String>, String> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    if items.len() > MAX_LINES {
        return Err(format!(
            "Too many lines for a single translation request ({}, max {MAX_LINES}).",
            items.len()
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

    let prompt = build_prompt(items, target);
    eprintln!(
        "[translation] request: provider={} model={model} target={target} lines={}\nprompt:\n{prompt}\n---",
        provider.id,
        items.len()
    );

    let request = completion
        .completion_request(&prompt)
        .preamble(SYSTEM.to_string())
        .temperature(1.0);
    let response = request.send().await.map_err(|e| e.to_string())?;
    let output = choice_text(&response);
    eprintln!("[translation] response:\n{output}\n---");

    let parsed = parse_translation_file(&output);
    let translations = align(items, parsed)?;
    eprintln!("[translation] OK ({} lines)", translations.len());
    Ok(translations)
}

fn build_prompt(items: &[TranslateItem], target: &str) -> String {
    format!(
        "Translate the text to {target}. Keep everything else exactly as it is; \
do not add, merge, drop or reorder any line. Respond only with the file.\n\n{}",
        build_content(items)
    )
}

/// Serializes the lines into the XML-like wire format: a `<translations>`
/// root holding one `<filename>` block per image, each line inside it tagged
/// with its entry id. Mirrors the ManhwaOCR translation file format.
fn build_content(items: &[TranslateItem]) -> String {
    let mut content = String::from("<translations>\n");
    let mut current: Option<&str> = None;
    for item in items {
        if current != Some(item.filename.as_str()) {
            if let Some(filename) = current {
                content.push_str(&format!("</{}>\n", escape(filename)));
            }
            content.push_str(&format!("<{}>\n", escape(&item.filename)));
            current = Some(&item.filename);
        }
        let text = item.text.replace(['\r', '\n'], " ");
        content.push_str(&format!("<{}>{}</{}>\n", item.id, escape(&text), item.id));
    }
    if let Some(filename) = current {
        content.push_str(&format!("</{}>\n", escape(filename)));
    }
    content.push_str("</translations>\n");
    content
}

/// Best-effort parsing of whatever the model actually output, tolerant of
/// missing closing tags and of the optional `<translate>` wrapper inside a
/// row. Returns every recovered `(filename, entry id) -> text` pair; row
/// order in the answer does not matter.
fn parse_translation_file(output: &str) -> HashMap<(String, u64), String> {
    let mut translations = HashMap::new();
    let mut current: Option<String> = None;
    for raw_line in output.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(inner) = line.strip_prefix('<') else {
            continue;
        };
        let Some(gt) = inner.find('>') else {
            continue;
        };
        let tag = &inner[..gt];
        let tail = &inner[gt + 1..];
        if tag.starts_with('/') {
            // Closing tags (`</filename>`, `</1>`, `</translations>`).
            continue;
        }

        // Row tag: `<123>text</123>` or `<123>text` (missing closing tag).
        if let Ok(id) = tag.parse::<u64>() {
            if let Some(filename) = current.as_ref() {
                let closing = format!("</{tag}>");
                let content_end = tail.rfind(&closing).unwrap_or(tail.len());
                let text = translate_wrapped(&tail[..content_end]);
                if !text.is_empty() {
                    translations.insert((filename.clone(), id), text);
                }
            }
            continue;
        }

        // File tag: `<filename>`; structural tags are ignored.
        let name = unescape(tag);
        if matches!(
            name.to_lowercase().as_str(),
            "translations" | "translate" | "context" | "re-translation"
        ) {
            continue;
        }
        if !name.is_empty() {
            current = Some(name);
        }
    }
    translations
}

/// Extracts the translated text from a row's content, honoring the optional
/// `<translate>` wrapper ManhwaOCR-style models sometimes emit.
fn translate_wrapped(content: &str) -> String {
    let lower = content.to_lowercase();
    if let Some(start) = lower.find("<translate>") {
        let inner = &content[start + "<translate>".len()..];
        let end = inner.to_lowercase().find("</translate>");
        let inner = match end {
            Some(i) => &inner[..i],
            None => inner,
        };
        return unescape(inner).trim().to_string();
    }
    unescape(content).trim().to_string()
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Reorders the parsed translations to the input order, failing loudly when
/// the model dropped or renamed any row. Extra rows in the answer are
/// ignored, matching the ManhwaOCR import behaviour.
fn align(items: &[TranslateItem], parsed: HashMap<(String, u64), String>) -> Result<Vec<String>, String> {
    let mut missing = Vec::new();
    let mut translations = Vec::with_capacity(items.len());
    for item in items {
        match parsed.get(&(item.filename.clone(), item.id)) {
            Some(text) => translations.push(text.clone()),
            None => missing.push(item.id),
        }
    }
    if missing.is_empty() {
        Ok(translations)
    } else {
        Err(format!(
            "Model returned {} translation(s) for {} line(s); missing {}; skipping.",
            parsed.len(),
            items.len(),
            missing
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_name_is_lowercase_with_auto_suffix() {
        assert_eq!(profile_name("English"), "english(auto)");
        assert_eq!(profile_name("Chinese (Simplified)"), "chinese (simplified)(auto)");
    }

    fn item(filename: &str, id: u64, text: &str) -> TranslateItem {
        TranslateItem {
            filename: filename.to_string(),
            id,
            text: text.to_string(),
        }
    }

    #[test]
    fn content_groups_lines_by_file_in_input_order() {
        let items = vec![
            item("a.png", 1, "안녕"),
            item("a.png", 2, "하세요"),
            item("b.png", 3, "안녕"),
        ];
        assert_eq!(
            build_content(&items),
            "<translations>\n<a.png>\n<1>안녕</1>\n<2>하세요</2>\n</a.png>\n\
             <b.png>\n<3>안녕</3>\n</b.png>\n</translations>\n"
        );
    }

    #[test]
    fn content_escapes_special_characters() {
        let items = vec![item("a&b.png", 1, "3 < 5 & 7 > 2 \"q\" 's'")];
        assert_eq!(
            build_content(&items),
            "<translations>\n<a&amp;b.png>\n<1>3 &lt; 5 &amp; 7 &gt; 2 &quot;q&quot; \
             &apos;s&apos;</1>\n</a&amp;b.png>\n</translations>\n"
        );
        let parsed = parse_translation_file(&build_content(&items));
        assert_eq!(parsed[&("a&b.png".to_string(), 1)], "3 < 5 & 7 > 2 \"q\" 's'");
    }

    #[test]
    fn parse_recovers_rows_and_files() {
        let out = parse_translation_file(
            "<translations>\n<a.png>\n<1>Hello</1>\n<2>How are you?</2>\n</a.png>\n\
             <b.png>\n<3>Goodbye</3>\n</b.png>\n</translations>",
        );
        assert_eq!(out.len(), 3);
        assert_eq!(out[&("a.png".to_string(), 1)], "Hello");
        assert_eq!(out[&("a.png".to_string(), 2)], "How are you?");
        assert_eq!(out[&("b.png".to_string(), 3)], "Goodbye");
    }

    #[test]
    fn parse_tolerates_missing_closing_tags_and_translate_wrappers() {
        let out = parse_translation_file(
            "<translations>\n<a.png>\n<1>Plain\n<2><translate>Wrapped</translate>\n</a.png>",
        );
        assert_eq!(out[&("a.png".to_string(), 1)], "Plain");
        assert_eq!(out[&("a.png".to_string(), 2)], "Wrapped");
    }

    #[test]
    fn parse_ignores_structural_and_closing_tags() {
        let out = parse_translation_file(
            "<translations>\n</translations>\n</a.png>\n<b.png>\n<1>x</1>\n</b.png>",
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[&("b.png".to_string(), 1)], "x");
    }

    #[test]
    fn rows_outside_any_file_are_dropped() {
        let out = parse_translation_file("<1>orphan</1>\n<a.png>\n<2>kept</2>");
        assert_eq!(out.len(), 1);
        assert_eq!(out[&("a.png".to_string(), 2)], "kept");
    }

    #[test]
    fn align_maps_response_back_to_input_order() {
        let items = vec![item("a.png", 1, "x"), item("a.png", 2, "y"), item("b.png", 3, "z")];
        let parsed = HashMap::from([
            (("b.png".to_string(), 3), "C".to_string()),
            (("a.png".to_string(), 1), "A".to_string()),
            (("a.png".to_string(), 2), "B".to_string()),
        ]);
        assert_eq!(align(&items, parsed).unwrap(), vec!["A", "B", "C"]);
    }

    #[test]
    fn align_rejects_missing_rows() {
        let items = vec![item("a.png", 1, "x")];
        let parsed = HashMap::from([(("a.png".to_string(), 2), "A".to_string())]);
        let err = align(&items, parsed).unwrap_err();
        assert!(err.contains("missing 1"), "{err}");
    }

    #[test]
    fn prompt_embeds_the_file() {
        let items = vec![item("a.png", 1, "안녕")];
        let prompt = build_prompt(&items, "English");
        assert!(prompt.contains("Translate the text to English"));
        assert!(prompt.contains("<translations>"));
        assert!(prompt.contains("<1>안녕</1>"));
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
        let ids: Vec<&str> = selected.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"paid-v2"));
        assert!(!ids.contains(&"paid-v1"));
        assert!(ids.contains(&"free-old"));
        assert!(ids.contains(&"free-new"));
        assert!(!ids.contains(&"retired"));
        // Models without a family are their own family: all of them are kept.
        assert!(ids.contains(&"loner-v2"));
        assert!(ids.contains(&"loner-v1"));
        assert!(ids.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(
            selected
                .iter()
                .filter(|m| m.free)
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            vec!["free-new", "free-old"]
        );
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
            vec![Model {
                id: "deepseek-v4-flash-free".to_string(),
                free: true
            }]
        );
    }
}
