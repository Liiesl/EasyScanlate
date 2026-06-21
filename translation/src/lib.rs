//! Machine translation via rig. A fixed catalog of well-known gateways is
//! offered as "supported" providers, plus two free-form slots for any
//! OpenAI-compatible or Anthropic-compatible endpoint. A provider only
//! becomes usable ("connected") when the user stores an API key for it; the
//! rest of the app only ever sees the functions in this module.
//!
//! All OCR lines of all loaded images are translated in a single request.
//! The prompt embeds the lines in an XML-like file structure (grouped per
//! image, each line tagged with its entry id), and the model is asked to
//! return the same file with translations in place. The answer is parsed back
//! by tag, so the order the model emits the lines in does not matter. The
//! result is a `Vec<String>` aligned with the input order, which the app
//! stores into the selected profile named `english(auto)` style.

use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

use rig::completion::{AssistantContent, CompletionResponse};
use rig::prelude::*;
use rig::providers::openai;
use serde::Deserialize;

pub mod session;
pub use session::Session;

/// Hard cap on lines per request; a single unbounded prompt is guaranteed to
/// blow the model's context window on big projects.
const MAX_LINES: usize = 1000;

/// Model listing mirror. Provider-specific paths (`/openai`, `/deepseek`, ...)
/// keep the payload small instead of the ~6 MB full index.
const MODELS_MIRROR: &str = "https://models.pileofthings.top";

/// How the provider speaks: OpenAI chat-completions style or the Anthropic
/// Messages API. Custom connections are built around one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatKind {
    OpenAI,
    Anthropic,
}

/// One stored connection: the API key plus (for custom endpoints) the base
/// URL and the single model id. Persisted in settings.json, one entry per
/// provider id.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Connection {
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: Option<String>,
}

/// The id of the custom OpenAI-compatible connection.
pub const CUSTOM_OPENAI: &str = "custom-openai";
/// The id of the custom Anthropic-compatible connection.
pub const CUSTOM_ANTHROPIC: &str = "custom-anthropic";

/// Hardcoded model choices for a custom connection when the user did not
/// enter one; the first entry is the default.
pub const CUSTOM_OPENAI_MODELS: [&str; 2] = ["gpt-4o-mini", "deepseek-chat"];
/// See [`CUSTOM_OPENAI_MODELS`].
pub const CUSTOM_ANTHROPIC_MODELS: [&str; 1] = ["claude-sonnet-4-5"];

/// Maximum output tokens for Anthropic-style requests (rig requires
/// `max_tokens` to be set; a single request may translate up to
/// [`MAX_LINES`] lines).
const ANTHROPIC_MAX_TOKENS: u64 = 16_384;

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
    /// models.dev provider id (or a custom-* id for free-form endpoints).
    pub id: String,
    /// Display name shown in the UI.
    pub name: String,
    /// Chat completions base URL (OpenAI style) or the Anthropic API root.
    pub api: String,
    /// How requests are formed against `api`.
    pub kind: CompatKind,
    /// API key environment variable.
    pub api_key_env: String,
    /// Selectable models.
    pub models: Vec<Model>,
}

impl Provider {
    /// The model picker entries of this provider, respecting the free-only
    /// filter; the offline fallback list when there are none.
    pub fn selectable_models(&self, free_only: bool) -> Vec<String> {
        let mut ids: Vec<String> = self
            .models
            .iter()
            .filter(|model| !free_only || model.free)
            .map(|model| model.id.clone())
            .collect();
        if ids.is_empty() && !self.models.is_empty() {
            ids = self.models.iter().map(|model| model.id.clone()).collect();
        }
        ids
    }
}

/// One entry of the built-in gateway catalog, as a ready-to-use [`Provider`]
/// (its `models` are the offline fallbacks used while the mirror is
/// unreachable).
fn entry(
    id: &str,
    name: &str,
    api: &str,
    kind: CompatKind,
    api_key_env: &str,
    fallback: &[&str],
) -> Provider {
    Provider {
        id: id.to_string(),
        name: name.to_string(),
        api: api.to_string(),
        kind,
        api_key_env: api_key_env.to_string(),
        models: fallback_models(fallback),
    }
}

/// The supported gateways offered in the settings UI ("connect" buttons).
/// Data verified against the models.dev mirror (`{MODELS_MIRROR}/{id}`):
/// the `api` and `env` fields come from the API when present, otherwise from
/// the provider's public defaults. `google` speaks through Gemini's
/// OpenAI-compatibility endpoint; `minimax` is Anthropic-compatible.
pub static SUPPORTED_PROVIDERS: LazyLock<Vec<Provider>> = LazyLock::new(|| {
    vec![
        entry("openai", "OpenAI", "https://api.openai.com/v1", CompatKind::OpenAI, "OPENAI_API_KEY", &["gpt-4o-mini", "gpt-5-nano"]),
        entry("anthropic", "Anthropic", "https://api.anthropic.com", CompatKind::Anthropic, "ANTHROPIC_API_KEY", &["claude-sonnet-4-5", "claude-haiku-4-5"]),
        entry("google", "Google (Gemini)", "https://generativelanguage.googleapis.com/v1beta/openai/", CompatKind::OpenAI, "GOOGLE_API_KEY", &["gemini-flash-lite-latest", "gemini-3.5-flash"]),
        entry("xai", "xAI (Grok)", "https://api.x.ai/v1", CompatKind::OpenAI, "XAI_API_KEY", &["grok-4.3", "grok-4.5"]),
        entry("openrouter", "OpenRouter", "https://openrouter.ai/api/v1", CompatKind::OpenAI, "OPENROUTER_API_KEY", &["openai/gpt-4o-mini"]),
        entry("nvidia", "NVIDIA", "https://integrate.api.nvidia.com/v1", CompatKind::OpenAI, "NVIDIA_API_KEY", &["nvidia/llama-3.1-nemotron-nano-8b-v1"]),
        entry("deepseek", "DeepSeek", "https://api.deepseek.com", CompatKind::OpenAI, "DEEPSEEK_API_KEY", &["deepseek-chat", "deepseek-reasoner"]),
        entry("kilo", "Kilo", "https://api.kilo.ai/api/gateway", CompatKind::OpenAI, "KILO_API_KEY", &["deepseek-v4-flash", "mimo-v2.5"]),
        entry("moonshotai", "Moonshot AI", "https://api.moonshot.ai/v1", CompatKind::OpenAI, "MOONSHOT_API_KEY", &["kimi-k2.5", "kimi-k3"]),
        entry("zai", "Z.AI", "https://api.z.ai/api/paas/v4", CompatKind::OpenAI, "ZHIPU_API_KEY", &["glm-4.5-flash", "glm-4.6"]),
        entry("minimax", "MiniMax", "https://api.minimax.io/anthropic/v1", CompatKind::Anthropic, "MINIMAX_API_KEY", &["MiniMax-M2.1", "MiniMax-M2.5"]),
        entry("opencode", "OpenCode Zen", "https://opencode.ai/zen/v1", CompatKind::OpenAI, "OPENCODE_API_KEY", &["deepseek-v4-flash", "mimo-v2.5-free"]),
        entry("opencode-go", "OpenCode Go", "https://opencode.ai/zen/go/v1", CompatKind::OpenAI, "OPENCODE_API_KEY", &["deepseek-v4-flash", "mimo-v2.5"]),
        entry("mistral", "Mistral", "https://api.mistral.ai/v1", CompatKind::OpenAI, "MISTRAL_API_KEY", &["mistral-small-latest", "mistral-medium-2508"]),
        entry("ollama-cloud", "Ollama Cloud", "https://ollama.com/v1", CompatKind::OpenAI, "OLLAMA_API_KEY", &["deepseek-v4-flash", "kimi-k3"]),
    ]
});

fn fallback_models(ids: &[&str]) -> Vec<Model> {
    ids.iter()
        .map(|id| Model {
            id: (*id).to_string(),
            free: false,
        })
        .collect()
}

/// Looks up a supported gateway by its provider id.
pub fn catalog_provider(id: &str) -> Option<&'static Provider> {
    SUPPORTED_PROVIDERS.iter().find(|p| p.id == id)
}

/// The connection id of a custom endpoint matching `kind`, or `None` when
/// `id` is a built-in gateway.
pub fn custom_id(kind: CompatKind) -> &'static str {
    match kind {
        CompatKind::OpenAI => CUSTOM_OPENAI,
        CompatKind::Anthropic => CUSTOM_ANTHROPIC,
    }
}

/// Whether `id` is one of the two custom connection slots.
pub fn is_custom(id: &str) -> bool {
    id == CUSTOM_OPENAI || id == CUSTOM_ANTHROPIC
}

/// Display name for a connection id: the catalog name, the custom labels,
/// or the id itself as a last resort.
pub fn provider_name(id: &str) -> String {
    if id == CUSTOM_OPENAI {
        return "Custom (OpenAI-compatible)".to_string();
    }
    if id == CUSTOM_ANTHROPIC {
        return "Custom (Anthropic-compatible)".to_string();
    }
    catalog_provider(id)
        .map(|p| p.name.to_string())
        .unwrap_or_else(|| id.to_string())
}

/// Profile name convention for machine translations: `english(auto)`.
pub fn profile_name(lang: &str) -> String {
    format!("{}(auto)", lang.to_lowercase())
}

/// The last path component of an image path; the file tag of the wire format.
pub fn file_tag(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

/// First validation error of the connect modal form, if any:
/// - api_key must be non-blank;
/// - custom connections also need a base URL and a model id.
pub fn validate_connection(
    is_custom: bool,
    api_key: &str,
    base_url: &str,
    model: &str,
) -> Option<String> {
    if api_key.trim().is_empty() {
        return Some("Enter an API key.".to_string());
    }
    if is_custom {
        if base_url.trim().is_empty() {
            return Some("Enter a base URL.".to_string());
        }
        if model.trim().is_empty() {
            return Some("Enter a model id.".to_string());
        }
    }
    None
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

/// Fetches and filters the model list of every requested gateway (by
/// provider id), keyed by id. Each entry falls back to the catalog's
/// offline defaults when its listing is unreachable.
pub async fn fetch_providers(ids: Vec<String>) -> HashMap<String, Provider> {
    let mut providers = HashMap::new();
    for id in &ids {
        providers.insert(id.clone(), fetch_provider(id).await);
    }
    providers
}

/// Fetches one gateway's listing from the models mirror and returns the
/// usable [`Provider`]: the API base URL and key environment variable come
/// from the catalog. On any failure the catalog's fallback models are used,
/// so the app always has something to show.
pub async fn fetch_provider(id: &str) -> Provider {
    let Some(catalog) = catalog_provider(id) else {
        return custom_fallback_provider(id);
    };
    let url = format!("{MODELS_MIRROR}/{id}");
    let response = match reqwest::get(&url).await {
        Ok(response) => response,
        Err(e) => {
            eprintln!("[translation] {id} models fetch failed: {e}; using fallback list");
            return catalog.clone();
        }
    };
    let listing: ProviderListing = match response.json().await {
        Ok(listing) => listing,
        Err(e) => {
            eprintln!("[translation] {id} models listing parse failed: {e}; using fallback list");
            return catalog.clone();
        }
    };
    let models = select_models(&listing);
    eprintln!("[translation] {} model(s) loaded from {url}", models.len());
    Provider {
        id: catalog.id.to_string(),
        name: catalog.name.to_string(),
        api: listing
            .api
            .filter(|api| !api.is_empty())
            .unwrap_or_else(|| catalog.api.to_string()),
        kind: catalog.kind,
        api_key_env: listing
            .env
            .first()
            .cloned()
            .unwrap_or_else(|| catalog.api_key_env.to_string()),
        models,
    }
}

/// The offline fallback for a custom connection id: its catalog entry when
/// known, otherwise the plain custom-* defaults.
fn custom_fallback_provider(id: &str) -> Provider {
    let (kind, name, models) = match id {
        CUSTOM_OPENAI => (
            CompatKind::OpenAI,
            "Custom (OpenAI-compatible)",
            &CUSTOM_OPENAI_MODELS[..],
        ),
        CUSTOM_ANTHROPIC => (
            CompatKind::Anthropic,
            "Custom (Anthropic-compatible)",
            &CUSTOM_ANTHROPIC_MODELS[..],
        ),
        _ => (CompatKind::OpenAI, id, &[][..]),
    };
    Provider {
        id: id.to_string(),
        name: name.to_string(),
        api: String::new(),
        kind,
        api_key_env: String::new(),
        models: models
            .iter()
            .map(|m| Model {
                id: (*m).to_string(),
                free: false,
            })
            .collect(),
    }
}

/// The `Provider` to send requests to for a connection: the fetched/catalog
/// gateway for built-ins, or one built from the connection's own base URL
/// and model for custom endpoints. The model list is a single entry per
/// connection, replaced by the connection's model or the kind's defaults.
pub fn provider_for_connection(id: &str, connection: &Connection) -> Provider {
    let mut provider = match catalog_provider(id) {
        Some(catalog) => catalog.clone(),
        None => custom_fallback_provider(id),
    };
    if is_custom(id) {
        provider.api = connection.base_url.clone().unwrap_or_default();
        let model = connection
            .model
            .clone()
            .unwrap_or_else(|| provider.models.first().map(|m| m.id.clone()).unwrap_or_default());
        provider.models = vec![Model {
            id: model,
            free: false,
        }];
    }
    provider
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
/// gateway (dispatched by its [`CompatKind`]). `api_key` overrides the
/// provider's environment variable when set (in-memory only; never
/// persisted). On success returns one translation per input line, in the
/// same order.
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

    let key = resolve_credentials(api_key, provider)?;

    let prompt = build_prompt(items, target);
    let output = complete(&prompt, provider, model, &key).await?;
    eprintln!(
        "[translation] response:\n{output}\n---"
    );

    let parsed = parse_translation_file(&output);
    let translations = align(items, parsed)?;
    eprintln!("[translation] OK ({} lines)", translations.len());
    Ok(translations)
}

/// Translates one line with the simple single-line prompt used by the
/// per-row "Retranslate" action (ManhwaOCR mechanics): the model replies
/// with only the translated text, no wire format. The result is returned
/// as-is; the caller strips quotes the model may wrap the answer in.
pub async fn translate_one(
    text: &str,
    target: &str,
    provider: &Provider,
    model: &str,
    api_key: Option<String>,
) -> Result<String, String> {
    let key = resolve_credentials(api_key, provider)?;
    let prompt = format!(
        "Translate the following text to {target}. Respond ONLY with the \
translation, no explanation.\n\nText: {text}"
    );
    let output = complete(&prompt, provider, model, &key).await?;
    Ok(output.trim().to_string())
}

/// Resolves the API key for a request: `api_key` overrides the provider's
/// environment variable when set (in-memory only; never persisted).
fn resolve_credentials(
    api_key: Option<String>,
    provider: &Provider,
) -> Result<String, String> {
    let key = api_key
        .filter(|key| !key.is_empty())
        .or_else(|| std::env::var(&provider.api_key_env).ok())
        .unwrap_or_default();
    if key.is_empty() {
        return Err(format!(
            "Translation init failed: no API key for {} (set {} or connect it in Settings)",
            provider.name, provider.api_key_env
        ));
    }
    if provider.api.is_empty() {
        return Err(format!(
            "Translation init failed: no base URL for {}; enter one in Settings.",
            provider.name
        ));
    }
    Ok(key)
}

/// Sends `prompt` to `model` on the given gateway (dispatched by its
/// [`CompatKind`]) and returns the raw completion text.
async fn complete(
    prompt: &str,
    provider: &Provider,
    model: &str,
    key: &str,
) -> Result<String, String> {
    match provider.kind {
        CompatKind::OpenAI => {
            let client = openai::CompletionsClient::builder()
                .api_key(key)
                .base_url(&provider.api)
                .build()
                .map_err(|e| format!("Translation init failed: {e}"))?;
            let completion = client.completion_model(model);
            let response = completion
                .completion_request(prompt)
                .preamble(SYSTEM.to_string())
                .temperature(1.0)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            Ok(choice_text(&response))
        }
        CompatKind::Anthropic => {
            let client = rig::providers::anthropic::Client::builder()
                .api_key(key)
                .base_url(&provider.api)
                .build()
                .map_err(|e| format!("Translation init failed: {e}"))?;
            let completion = client.completion_model(model);
            let response = completion
                .completion_request(prompt)
                .preamble(SYSTEM.to_string())
                .max_tokens(ANTHROPIC_MAX_TOKENS)
                .temperature(1.0)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            Ok(choice_text(&response))
        }
    }
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
fn choice_text<R>(response: &CompletionResponse<R>) -> String {
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

    #[test]
    fn file_tag_strips_directory_components() {
        assert_eq!(file_tag(r"C:\a\b\c.png"), "c.png");
        assert_eq!(file_tag("/a/b/d.png"), "d.png");
    }

    #[test]
    fn validate_connection_returns_the_first_error() {
        assert_eq!(validate_connection(false, "sk", "", ""), None);
        assert_eq!(
            validate_connection(false, "  ", "", ""),
            Some("Enter an API key.".to_string())
        );
        assert_eq!(
            validate_connection(true, "sk", "", ""),
            Some("Enter a base URL.".to_string())
        );
        assert_eq!(
            validate_connection(true, "sk", "https://x", ""),
            Some("Enter a model id.".to_string())
        );
        assert_eq!(validate_connection(true, "sk", "https://x", "m"), None);
        // First error wins: a blank key beats the missing custom fields.
        assert_eq!(
            validate_connection(true, "", "", ""),
            Some("Enter an API key.".to_string())
        );
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

    #[test]
    fn catalog_covers_the_supported_providers() {
        for id in [
            "openai",
            "anthropic",
            "google",
            "xai",
            "openrouter",
            "nvidia",
            "deepseek",
            "kilo",
            "moonshotai",
            "zai",
            "minimax",
            "opencode",
            "opencode-go",
            "mistral",
            "ollama-cloud",
        ] {
            let provider = catalog_provider(id).expect("supported provider in catalog");
            assert!(!provider.name.is_empty());
            assert!(!provider.api.is_empty());
            assert!(!provider.api_key_env.is_empty());
            assert!(!provider.models.is_empty(), "{id} needs fallback models");
        }
    }

    #[test]
    fn catalog_metadata_matches_the_models_dev_mirror() {
        assert_eq!(catalog_provider("minimax").unwrap().kind, CompatKind::Anthropic);
        assert_eq!(catalog_provider("anthropic").unwrap().kind, CompatKind::Anthropic);
        assert_eq!(
            catalog_provider("minimax").unwrap().api,
            "https://api.minimax.io/anthropic/v1"
        );
        assert_eq!(
            catalog_provider("google").unwrap().api,
            "https://generativelanguage.googleapis.com/v1beta/openai/"
        );
        assert_eq!(catalog_provider("ollama-cloud").unwrap().api_key_env, "OLLAMA_API_KEY");
        assert_eq!(catalog_provider("moonshotai").unwrap().api_key_env, "MOONSHOT_API_KEY");
    }

    #[test]
    fn custom_ids_resolve_kinds_and_names() {
        assert_eq!(custom_id(CompatKind::OpenAI), CUSTOM_OPENAI);
        assert_eq!(custom_id(CompatKind::Anthropic), CUSTOM_ANTHROPIC);
        assert!(is_custom(CUSTOM_OPENAI));
        assert!(is_custom(CUSTOM_ANTHROPIC));
        assert!(!is_custom("openai"));
        assert_eq!(provider_name(CUSTOM_OPENAI), "Custom (OpenAI-compatible)");
        assert_eq!(provider_name(CUSTOM_ANTHROPIC), "Custom (Anthropic-compatible)");
        assert_eq!(provider_name("openai"), "OpenAI");
    }

    #[test]
    fn provider_for_connection_overrides_custom_api_and_models() {
        let connection = Connection {
            api_key: "sk-test".to_string(),
            base_url: Some("http://localhost:11434/v1".to_string()),
            model: Some("llama-3.1-8b".to_string()),
        };
        let provider = provider_for_connection(CUSTOM_OPENAI, &connection);
        assert_eq!(provider.api, "http://localhost:11434/v1");
        assert_eq!(provider.kind, CompatKind::OpenAI);
        assert_eq!(provider.models.len(), 1);
        assert_eq!(provider.models[0].id, "llama-3.1-8b");

        let empty = Connection {
            api_key: String::new(),
            base_url: None,
            model: None,
        };
        let provider = provider_for_connection(CUSTOM_OPENAI, &empty);
        assert_eq!(provider.models[0].id, CUSTOM_OPENAI_MODELS[0]);

        // A built-in connection keeps the catalog api/kind.
        let provider = provider_for_connection("deepseek", &empty);
        assert_eq!(provider.api, "https://api.deepseek.com");
        assert_eq!(provider.kind, CompatKind::OpenAI);
    }

    #[test]
    fn selectable_models_honors_the_free_only_filter() {
        let provider = Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            api: "https://example.test/v1".to_string(),
            kind: CompatKind::OpenAI,
            api_key_env: "TEST_API_KEY".to_string(),
            models: vec![
                Model { id: "free-1".to_string(), free: true },
                Model { id: "paid-1".to_string(), free: false },
            ],
        };
        assert_eq!(provider.selectable_models(true), vec!["free-1"]);
        assert_eq!(
            provider.selectable_models(false),
            vec!["free-1", "paid-1"]
        );
    }
}
