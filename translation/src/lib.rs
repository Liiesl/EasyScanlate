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

use std::collections::{BTreeMap, HashMap, HashSet};
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

/// Context window for retranslate: number of neighboring lines included as
/// `<context>` on each side of a selected row. Mirrors
/// `ManhwaOCR/app/core/translations.py:generate_retranslate_content`.
const RETRANSLATE_CONTEXT: usize = 3;

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
/// URL and the single model id. Owned by the settings crate (it is persisted
/// data) and re-exported here; one entry per provider id.
pub use scanlateit_settings::Connection;

/// The id of the custom OpenAI-compatible connection.
pub const CUSTOM_OPENAI: &str = "custom-openai";
/// The id of the custom Anthropic-compatible connection.
pub const CUSTOM_ANTHROPIC: &str = "custom-anthropic";

/// Local providers that do not need an API key. Their `Connection.api_key`
/// is the literal id (`"ollama"`, `"vllm"`, `"llama cpp"`), and `base_url`
/// is required. Model lists are discovered from the endpoint itself.
pub const LOCAL_OLLAMA: &str = "ollama";
pub const LOCAL_VLLM: &str = "vllm";
pub const LOCAL_LLAMA_CPP: &str = "llama cpp";
/// All local provider ids in UI order.
pub const LOCAL_PROVIDERS: [&str; 3] = [LOCAL_OLLAMA, LOCAL_VLLM, LOCAL_LLAMA_CPP];

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

/// One selectable translation model: its wire id (sent to the API) and its
/// display name (shown in the UI), plus whether it is free (input or output
/// cost 0). `family` is the models.dev family (if any), used to seed the
/// default hidden set (older family members hidden until the user enables them
/// in Manage Models). The request always uses `id`; the UI always shows `name`.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub free: bool,
    pub family: Option<String>,
}

impl Model {
    /// Display name for the UI: `name` when present, otherwise `id`.
    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            &self.id
        } else {
            &self.name
        }
    }
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
    /// filter; the offline fallback list when there are none. Returns the
    /// wire `id`s; the UI maps them to `display_name` separately.
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

    /// Maps a wire `id` to its display name, if known for this provider.
    pub fn model_display_name(&self, id: &str) -> Option<&str> {
        self.models
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.display_name())
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
        // Local providers — no API key, models discovered from endpoint
        entry(LOCAL_OLLAMA, "Ollama", "http://localhost:11434/v1", CompatKind::OpenAI, "", &[]),
        entry(LOCAL_VLLM, "vLLM", "http://localhost:8000/v1", CompatKind::OpenAI, "", &[]),
        entry(LOCAL_LLAMA_CPP, "llama.cpp", "http://localhost:8080/v1", CompatKind::OpenAI, "", &[]),
    ]
});

fn fallback_models(ids: &[&str]) -> Vec<Model> {
    ids.iter()
        .map(|id| Model {
            id: (*id).to_string(),
            name: String::new(),
            free: false,
            family: None,
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

/// Whether `id` is a local provider that needs an endpoint but no API key.
pub fn is_local(id: &str) -> bool {
    id == LOCAL_OLLAMA || id == LOCAL_VLLM || id == LOCAL_LLAMA_CPP
}

/// Display name for a connection id: the catalog name, the custom labels,
/// the local labels, or the id itself as a last resort.
pub fn provider_name(id: &str) -> String {
    if id == CUSTOM_OPENAI {
        return "Custom (OpenAI-compatible)".to_string();
    }
    if id == CUSTOM_ANTHROPIC {
        return "Custom (Anthropic-compatible)".to_string();
    }
    if id == LOCAL_OLLAMA {
        return "Ollama".to_string();
    }
    if id == LOCAL_VLLM {
        return "vLLM".to_string();
    }
    if id == LOCAL_LLAMA_CPP {
        return "llama.cpp".to_string();
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
/// - api_key must be non-blank (except for local providers `ollama`/`vllm`/`llama cpp`);
/// - custom connections also need a base URL and a model id;
/// - local providers need a base URL.
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

/// Validates a connection by provider id: local providers (`ollama`/`vllm`/
/// `llama cpp`) need only a base URL and no API key; customs need base URL
/// + model; cloud needs an API key.
pub fn validate_connection_for(
    id: &str,
    api_key: &str,
    base_url: &str,
    model: &str,
) -> Option<String> {
    if is_local(id) {
        if base_url.trim().is_empty() {
            return Some("Enter a base URL.".to_string());
        }
        return None;
    }
    validate_connection(is_custom(id), api_key, base_url, model)
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
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
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
    #[serde(default)]
    modalities: Option<ModelModalities>,
}

/// Model input/output modality arrays (models.dev schema). Translation only
/// works with models that emit plain text.
#[derive(Debug, Deserialize, Default)]
struct ModelModalities {
    #[serde(default)]
    output: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ModelCost {
    #[serde(default)]
    input: Option<f64>,
    #[serde(default)]
    output: Option<f64>,
}

/// OpenAI-compatible `/v1/models` response shape.
#[derive(Debug, Deserialize)]
struct OpenAiModelsResponse {
    #[serde(default)]
    data: Vec<OpenAiModelEntry>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelEntry {
    #[serde(default)]
    id: String,
}

/// Ollama native `/api/tags` response shape.
#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    #[serde(default)]
    models: Vec<OllamaModelEntry>,
}

#[derive(Debug, Deserialize)]
struct OllamaModelEntry {
    #[serde(default)]
    name: String,
    #[serde(default)]
    model: String,
}

fn normalize_base_url(base: &str) -> String {
    base.trim().trim_end_matches('/').to_string()
}

fn openai_models_urls(base: &str) -> Vec<String> {
    let base = normalize_base_url(base);
    if base.is_empty() {
        return Vec::new();
    }
    let mut urls = Vec::new();
    if base.ends_with("/v1") {
        urls.push(format!("{base}/models"));
        let root = base.trim_end_matches("/v1").trim_end_matches('/').to_string();
        if !root.is_empty() {
            urls.push(format!("{root}/v1/models"));
            urls.push(format!("{root}/models"));
        }
    } else {
        urls.push(format!("{base}/v1/models"));
        urls.push(format!("{base}/models"));
    }
    urls
}

/// Tries OpenAI-compatible model discovery then, for `ollama`, falls back to
/// Ollama's native `/api/tags`. Returns sorted deduped models.
pub async fn fetch_local_models(base_url: &str, id: &str) -> Result<Vec<Model>, String> {
    let base = normalize_base_url(base_url);
    if base.is_empty() {
        return Err("Base URL is empty.".to_string());
    }
    // Try OpenAI-compatible endpoints first
    for url in openai_models_urls(&base) {
        match reqwest::get(&url).await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(parsed) = resp.json::<OpenAiModelsResponse>().await {
                    let mut ids: Vec<Model> = parsed
                        .data
                        .into_iter()
                        .filter(|m| !m.id.trim().is_empty())
                        .map(|m| Model { id: m.id.clone(), name: m.id, free: false, family: None })
                        .collect();
                    if !ids.is_empty() {
                        ids.sort_by(|a, b| a.id.cmp(&b.id));
                        ids.dedup_by(|a, b| a.id == b.id);
                        eprintln!("[translation] {} model(s) discovered from {url}", ids.len());
                        return Ok(ids);
                    }
                }
            }
            Ok(resp) => {
                eprintln!("[translation] {url} returned {}", resp.status());
            }
            Err(e) => {
                eprintln!("[translation] {url} fetch failed: {e}");
            }
        }
    }
    // Ollama native fallback
    if id == LOCAL_OLLAMA {
        let root = if base.ends_with("/v1") {
            base.trim_end_matches("/v1").trim_end_matches('/').to_string()
        } else {
            base.clone()
        };
        let url = format!("{root}/api/tags");
        match reqwest::get(&url).await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(parsed) = resp.json::<OllamaTagsResponse>().await {
                    let mut ids: Vec<Model> = parsed
                        .models
                        .into_iter()
                        .map(|m| {
                            let name = if !m.name.trim().is_empty() { m.name } else { m.model };
                            Model { id: name.clone(), name, free: false, family: None }
                        })
                        .filter(|m| !m.id.trim().is_empty())
                        .collect();
                    if !ids.is_empty() {
                        ids.sort_by(|a, b| a.id.cmp(&b.id));
                        ids.dedup_by(|a, b| a.id == b.id);
                        eprintln!("[translation] {} model(s) discovered from {url}", ids.len());
                        return Ok(ids);
                    }
                }
            }
            Ok(resp) => {
                eprintln!("[translation] {url} returned {}", resp.status());
            }
            Err(e) => {
                eprintln!("[translation] {url} fetch failed: {e}");
            }
        }
    }
    Err(format!("No models discovered from {base_url}"))
}

/// Fetches one local provider's model list from `base_url`. Falls back to
/// the catalog on error so the provider remains usable.
pub async fn fetch_local_provider(id: &str, base_url: &str) -> Provider {
    let catalog = match catalog_provider(id) {
        Some(c) => c.clone(),
        None => local_fallback_provider(id, base_url),
    };
    match fetch_local_models(base_url, id).await {
        Ok(models) => Provider {
            id: catalog.id.clone(),
            name: catalog.name.clone(),
            api: normalize_base_url(base_url),
            kind: catalog.kind,
            api_key_env: catalog.api_key_env.clone(),
            models,
        },
        Err(e) => {
            eprintln!("[translation] {id} local fetch failed: {e}; using fallback");
            let mut fallback = catalog;
            if !base_url.trim().is_empty() {
                fallback.api = normalize_base_url(base_url);
            }
            fallback
        }
    }
}

/// Fetches all requested local providers keyed by id, each using its own `base_url`.
pub async fn fetch_local_providers(endpoints: HashMap<String, String>) -> HashMap<String, Provider> {
    let mut out = HashMap::new();
    for (id, base) in endpoints {
        out.insert(id.clone(), fetch_local_provider(&id, &base).await);
    }
    out
}

/// Fetches and filters the model list of every requested gateway (by
/// provider id), keyed by id. Local ids (`ollama`/`vllm`/`llama cpp`) are
/// discovered from their catalog default endpoint; pass a connection-aware
/// fetch via `fetch_local_providers` when the stored `base_url` should win.
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
/// so the app always has something to show. For local ids the default
/// endpoint is probed instead of the mirror.
pub async fn fetch_provider(id: &str) -> Provider {
    if is_local(id) {
        if let Some(catalog) = catalog_provider(id) {
            return fetch_local_provider(id, &catalog.api).await;
        }
        return local_fallback_provider(id, "");
    }
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
                name: (*m).to_string(),
                free: false,
                family: None,
            })
            .collect(),
    }
}

fn local_fallback_provider(id: &str, base_url: &str) -> Provider {
    let name = match id {
        LOCAL_OLLAMA => "Ollama",
        LOCAL_VLLM => "vLLM",
        LOCAL_LLAMA_CPP => "llama.cpp",
        _ => id,
    };
    Provider {
        id: id.to_string(),
        name: name.to_string(),
        api: normalize_base_url(base_url),
        kind: CompatKind::OpenAI,
        api_key_env: String::new(),
        models: Vec::new(),
    }
}

/// The `Provider` to send requests to for a connection: the fetched/catalog
/// gateway for built-ins, or one built from the connection's own base URL
/// and model for custom endpoints. The model list is a single entry per
/// connection, replaced by the connection's model or the kind's defaults.
/// Local providers use the stored `base_url` as `api` and keep discovered
/// models (fallback is the catalog default with overridden `api`).
pub fn provider_for_connection(id: &str, connection: &Connection) -> Provider {
    let mut provider = match catalog_provider(id) {
        Some(catalog) => catalog.clone(),
        None => custom_fallback_provider(id),
    };
    if is_local(id) {
        provider.api = connection
            .base_url
            .clone()
            .map(|u| normalize_base_url(&u))
            .unwrap_or_else(|| provider.api.clone());
        // If the connection carries a pinned model (legacy manual entry),
        // surface it as a single model so translation still works before
        // discovery finishes. Otherwise keep whatever models the catalog/
        // fetched provider already had (empty until fetch completes).
        if let Some(model) = connection.model.clone().filter(|m| !m.trim().is_empty()) {
            if provider.models.is_empty() {
                provider.models = vec![Model { id: model.clone(), name: model, free: false, family: None }];
            }
        }
        return provider;
    }
    if is_custom(id) {
        provider.api = connection.base_url.clone().unwrap_or_default();
        let model = connection
            .model
            .clone()
            .unwrap_or_else(|| provider.models.first().map(|m| m.id.clone()).unwrap_or_default());
        provider.models = vec![Model {
            id: model.clone(),
            name: model,
            free: false,
            family: None,
        }];
    }
    provider
}

/// Returns every usable model from `listing`: drops deprecated models and
/// any model that does not output plain text. Free flag preserved, no family
/// de-duplication – this is the full list shown in the Manage Models overlay.
pub fn usable_models(listing: &ProviderListing) -> Vec<Model> {
    let mut out = Vec::new();
    for (id, info) in &listing.models {
        if info.status.as_deref() == Some("deprecated") {
            continue;
        }
        if !outputs_text_only(info) {
            continue;
        }
        let display = info
            .name
            .clone()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| id.clone());
        out.push(Model {
            id: id.clone(),
            name: display,
            free: is_free(info),
            family: info.family.clone(),
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// The ids of usable models that are hidden by the default family filter
/// (older releases of each paid family). `usable` should be the result of
/// `usable_models(listing)`; this recomputes the family latest selection to
/// find the hidden tail. Free models and deprecated (already removed) are
/// never hidden.
pub fn default_hidden_ids(listing: &ProviderListing) -> std::collections::BTreeSet<String> {
    let usable = usable_models(listing);
    let visible = select_models(listing);
    let visible_set: std::collections::BTreeSet<String> =
        visible.into_iter().map(|m| m.id).collect();
    usable
        .into_iter()
        .filter(|m| !m.free && !visible_set.contains(&m.id))
        .map(|m| m.id)
        .collect()
}

/// Applies the listing filters: drops deprecated models and any model that
/// outputs something other than text (image/audio/video generators are not
/// usable for translation), keeps only the newest release of each family for
/// paid models, always lists free models (input or output cost 0), and sorts
/// the result by id.
fn select_models(listing: &ProviderListing) -> Vec<Model> {
    let mut ids: Vec<Model> = Vec::new();
    let mut latest: BTreeMap<String, (&str, &ModelInfo)> = BTreeMap::new();
    for (id, info) in &listing.models {
        if info.status.as_deref() == Some("deprecated") {
            continue;
        }
        if !outputs_text_only(info) {
            continue;
        }
        if is_free(info) {
            let display = info
                .name
                .clone()
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| id.clone());
            ids.push(Model {
                id: id.clone(),
                name: display,
                free: true,
                family: info.family.clone(),
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
            .map(|(id, info)| {
                let display = info
                    .name
                    .clone()
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| id.to_string());
                Model {
                    id: id.to_string(),
                    name: display,
                    free: false,
                    family: info.family.clone(),
                }
            }),
    );
    ids.sort_by(|a, b| a.id.cmp(&b.id));
    ids
}

/// A model whose input or output cost is zero is free and always listed.
fn is_free(info: &ModelInfo) -> bool {
    matches!(&info.cost, Some(cost) if cost.input == Some(0.0) || cost.output == Some(0.0))
}

/// Whether the model emits plain text only. Models whose output modalities
/// include anything else (image, audio, video) or that declare no output at
/// all cannot produce the text translation and are filtered out. Models
/// without modality info are kept (the mirror always provides it, but a
/// listing change must not silently drop every model).
fn outputs_text_only(info: &ModelInfo) -> bool {
    match &info.modalities {
        Some(modalities) => {
            !modalities.output.is_empty() && modalities.output.iter().all(|m| m == "text")
        }
        None => true,
    }
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

    let key = resolve_credentials(api_key.clone(), provider)?;

    let prompt = build_prompt(items, target);
    let output = complete(&prompt, provider, model, &key).await?;
    eprintln!("[translation] response:\n{output}\n---");

    let mut parsed = parse_translation_file(&output);

    // Find missing entries
    let missing: Vec<&TranslateItem> = items
        .iter()
        .filter(|it| !parsed.contains_key(&(it.filename.clone(), it.id)))
        .collect();

    if !missing.is_empty() {
        let missing_ids: Vec<u64> = missing.iter().map(|m| m.id).collect();
        eprintln!(
            "[translation] missing {} entries {:?}, retrying with context (retranslate logic)",
            missing_ids.len(),
            missing_ids
        );
        // Build retranslate content for all missing, grouped by proximity like ManhwaOCR
        let selected: Vec<(String, u64)> = missing
            .iter()
            .map(|m| (m.filename.clone(), m.id))
            .collect();
        let retry_content = build_retranslate_content(items, &selected, RETRANSLATE_CONTEXT);
        if !retry_content.trim().is_empty() {
            let retry_prompt = format!(
                "Translate the text to {target}. Keep everything else exactly as it is; \
do not add, merge, drop or reorder any line. Respond only with the file.\n\n{}",
                retry_content
            );
            match complete(&retry_prompt, provider, model, &key).await {
                Ok(retry_output) => {
                    eprintln!("[translation] retry response:\n{retry_output}\n---");
                    let retry_parsed = parse_translation_file(&retry_output);
                    let mut recovered = 0usize;
                    for miss in &missing {
                        if let Some(t) = retry_parsed.get(&(miss.filename.clone(), miss.id)) {
                            if !t.is_empty() {
                                parsed.insert((miss.filename.clone(), miss.id), t.clone());
                                recovered += 1;
                            }
                        }
                    }
                    eprintln!(
                        "[translation] retry recovered {}/{} missing",
                        recovered,
                        missing.len()
                    );
                    // Fallback per-item isolated retry for any still missing (using translate_one logic)
                    let still_missing: Vec<&TranslateItem> = items
                        .iter()
                        .filter(|it| !parsed.contains_key(&(it.filename.clone(), it.id)))
                        .collect();
                    if !still_missing.is_empty() {
                        for miss in still_missing {
                            // Isolated single-line retry as last resort
                            let single_prompt = format!(
                                "Translate the following text to {target}. Respond ONLY with the \
translation, no explanation.\n\nText: {}",
                                miss.text
                            );
                            match complete(&single_prompt, provider, model, &key).await {
                                Ok(single_out) => {
                                    let t = single_out.trim().to_string();
                                    if !t.is_empty() {
                                        parsed.insert((miss.filename.clone(), miss.id), t);
                                        eprintln!("[translation] single retry recovered {}", miss.id);
                                    }
                                }
                                Err(e) => {
                                    eprintln!("[translation] single retry for {} failed: {}", miss.id, e);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[translation] retry request failed: {e}");
                }
            }
        }
    }

    // After retries, build aligned result but don't fail the whole batch if some still missing.
    // We return Ok with placeholders for still-missing so the caller can store successes.
    let mut translations = Vec::with_capacity(items.len());
    let mut still_missing_ids = Vec::new();
    for item in items {
        if let Some(t) = parsed.get(&(item.filename.clone(), item.id)) {
            translations.push(t.clone());
        } else {
            still_missing_ids.push(item.id);
            translations.push(String::new()); // placeholder for missing, caller skips empty
        }
    }
    if still_missing_ids.is_empty() {
        eprintln!("[translation] OK ({} lines, after retry if any)", translations.len());
        Ok(translations)
    } else {
        eprintln!(
            "[translation] partial OK: {} of {} lines; still missing after retry: {}; returning partial (empty placeholders skipped by caller)",
            translations.len() - still_missing_ids.len(),
            items.len(),
            still_missing_ids
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        // Return Ok with partial (caller skips empty). This implements "don't skip entire translations".
        Ok(translations)
    }
}

/// Translates one line with context-aware retranslate logic (ManhwaOCR
/// `generate_retranslate_content`): the target row is wrapped with
/// `RETRANSLATE_CONTEXT` neighbors as `<context>` inside a
/// `<re-translation>` block. When `context_items` is empty it falls back
/// to the simple isolated prompt.
pub async fn translate_one_with_context(
    text: &str,
    target: &str,
    provider: &Provider,
    model: &str,
    api_key: Option<String>,
    context_items: &[TranslateItem],
    selected_id: u64,
    filename: &str,
) -> Result<String, String> {
    let key = resolve_credentials(api_key, provider)?;

    // Build context-aware prompt if we have neighbors
    let prompt = if context_items.is_empty() {
        format!(
            "Translate the following text to {target}. Respond ONLY with the \
translation, no explanation.\n\nText: {text}"
        )
    } else {
        // Build a temporary items list that includes context + selected, deduplicated
        // `context_items` are the window (including the selected if present). Ensure selected is present.
        let mut all_for_file: Vec<TranslateItem> = context_items.to_vec();
        if !all_for_file.iter().any(|it| it.id == selected_id) {
            all_for_file.push(TranslateItem {
                filename: filename.to_string(),
                id: selected_id,
                text: text.to_string(),
            });
        }
        // For build_retranslate_content we need the full items list; we use `all_for_file` sorted by id
        // and selected single.
        let selected = vec![(filename.to_string(), selected_id)];
        // Sort all_for_file by id to mimic ManhwaOCR sorted order
        all_for_file.sort_by_key(|it| it.id);
        let content = build_retranslate_content(&all_for_file, &selected, RETRANSLATE_CONTEXT);
        let inner = if content.trim().is_empty() {
            // fallback to simple
            format!("<{selected_id}>{}</{selected_id}>", escape(text))
        } else {
            content
        };
        format!(
            "Translate the text to {target}. Keep everything else exactly as it is; \
do not add, merge, drop or reorder any line. Respond only with the file.\n\n{}",
            inner
        )
    };

    let output = complete(&prompt, provider, model, &key).await?;
    // Try to parse as file format first; if we got context format, extract the row.
    let parsed = parse_translation_file(&output);
    if let Some(t) = parsed.get(&(filename.to_string(), selected_id)) {
        return Ok(t.trim().to_string());
    }
    // Fallback to raw trimmed output (isolated prompt case)
    Ok(output.trim().to_string())
}

/// Translates one line. When called without explicit context it uses the
/// simple isolated prompt for backward compatibility; callers that have
/// context should use `translate_one_with_context`.
pub async fn translate_one(
    text: &str,
    target: &str,
    provider: &Provider,
    model: &str,
    api_key: Option<String>,
) -> Result<String, String> {
    translate_one_with_context(text, target, provider, model, api_key, &[], 0, "").await
}

/// Resolves the API key for a request: `api_key` overrides the provider's
/// environment variable when set (in-memory only; never persisted).
/// Local providers (`ollama`/`vllm`/`llama cpp`) do not need an API key; a
/// dummy `provider.id` is used when none is supplied.
fn resolve_credentials(
    api_key: Option<String>,
    provider: &Provider,
) -> Result<String, String> {
    if is_local(&provider.id) {
        let key = api_key
            .filter(|key| !key.is_empty())
            .unwrap_or_else(|| provider.id.clone());
        if provider.api.is_empty() {
            return Err(format!(
                "Translation init failed: no base URL for {}; enter one in Settings.",
                provider.name
            ));
        }
        return Ok(key);
    }
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

/// Builds retranslate wire format with context, mirroring
/// `ManhwaOCR/app/core/translations.py:generate_retranslate_content`.
/// Groups selected rows by proximity (`context_size` overlap) into
/// `<re-translation>` blocks and wraps non-selected neighbors as `<context>`.
pub fn build_retranslate_content(
    items: &[TranslateItem],
    selected: &[(String, u64)],
    context_size: usize,
) -> String {
    use std::collections::HashMap;

    if selected.is_empty() {
        return String::new();
    }

    // Organize all valid results by filename, sorted by id
    let mut all_by_file: HashMap<String, Vec<&TranslateItem>> = HashMap::new();
    for it in items {
        all_by_file.entry(it.filename.clone()).or_default().push(it);
    }
    for v in all_by_file.values_mut() {
        v.sort_by_key(|it| it.id);
    }

    // Organize selected by filename
    let mut selected_by_file: HashMap<String, Vec<u64>> = HashMap::new();
    for (filename, id) in selected {
        selected_by_file.entry(filename.clone()).or_default().push(*id);
    }

    let mut filenames: Vec<String> = selected_by_file.keys().cloned().collect();
    filenames.sort();

    let mut content = String::new();
    for filename in filenames {
        let file_results = match all_by_file.get(&filename) {
            Some(v) => v,
            None => continue,
        };
        if file_results.is_empty() {
            continue;
        }
        let mut row_to_idx: HashMap<String, usize> = HashMap::new();
        for (idx, it) in file_results.iter().enumerate() {
            row_to_idx.insert(it.id.to_string(), idx);
        }
        let mut selected_indices: Vec<usize> = selected_by_file[&filename]
            .iter()
            .filter_map(|id| row_to_idx.get(&id.to_string()).copied())
            .collect();
        selected_indices.sort_unstable();
        if selected_indices.is_empty() {
            continue;
        }
        content.push_str(&format!("<{}>\n", escape(&filename)));

        // Group selected indices by proximity (overlapping context)
        let mut groups: Vec<Vec<usize>> = Vec::new();
        let mut current_group = vec![selected_indices[0]];
        for &idx in &selected_indices[1..] {
            let prev = *current_group.last().unwrap();
            if idx.saturating_sub(context_size) <= prev + context_size {
                current_group.push(idx);
            } else {
                groups.push(current_group);
                current_group = vec![idx];
            }
        }
        groups.push(current_group);

        for group in groups {
            content.push_str("<re-translation>\n");
            let min_idx = group[0].saturating_sub(context_size);
            let max_idx = (group[group.len() - 1] + context_size).min(file_results.len() - 1);
            let selected_set: HashSet<usize> = group.into_iter().collect();
            for idx in min_idx..=max_idx {
                let it = file_results[idx];
                let text = it.text.replace(['\r', '\n'], " ");
                if selected_set.contains(&idx) {
                    content.push_str(&format!(
                        "<{}>{}</{}>\n",
                        it.id,
                        escape(&text),
                        it.id
                    ));
                } else {
                    content.push_str(&format!("<context>{}</context>\n", escape(&text)));
                }
            }
            content.push_str("</re-translation>\n");
        }
        content.push_str(&format!("</{}>\n", escape(&filename)));
    }
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
#[allow(dead_code)]
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
                        id: None,
                        name: None,
                        status: None,
                        family: Some("paid".into()),
                        release_date: Some("2025-01-01".into()),
                        last_updated: None,
                        modalities: None,
                        cost: Some(ModelCost {
                            input: Some(2.0),
                            output: Some(4.0),
                        }),
                    },
                ),
                (
                    "paid-v2".into(),
                    ModelInfo {
                        id: None,
                        name: None,
                        status: None,
                        family: Some("paid".into()),
                        release_date: Some("2025-06-01".into()),
                        last_updated: None,
                        modalities: None,
                        cost: Some(ModelCost {
                            input: Some(2.0),
                            output: Some(4.0),
                        }),
                    },
                ),
                (
                    "free-old".into(),
                    ModelInfo {
                        id: None,
                        name: None,
                        status: None,
                        family: Some("free".into()),
                        release_date: Some("2024-01-01".into()),
                        last_updated: None,
                        modalities: None,
                        cost: Some(ModelCost {
                            input: Some(0.0),
                            output: Some(1.0),
                        }),
                    },
                ),
                (
                    "free-new".into(),
                    ModelInfo {
                        id: None,
                        name: None,
                        status: None,
                        family: Some("free".into()),
                        release_date: Some("2025-01-01".into()),
                        last_updated: None,
                        modalities: None,
                        cost: Some(ModelCost {
                            input: Some(1.0),
                            output: Some(0.0),
                        }),
                    },
                ),
                (
                    "retired".into(),
                    ModelInfo {
                        id: None,
                        name: None,
                        status: Some("deprecated".into()),
                        family: Some("retired".into()),
                        release_date: Some("2025-01-01".into()),
                        last_updated: None,
                        modalities: None,
                        cost: Some(ModelCost {
                            input: Some(2.0),
                            output: Some(4.0),
                        }),
                    },
                ),
                (
                    "loner-v1".into(),
                    ModelInfo {
                        id: None,
                        name: None,
                        status: None,
                        family: None,
                        release_date: Some("2025-01-01".into()),
                        last_updated: None,
                        modalities: None,
                        cost: Some(ModelCost {
                            input: Some(2.0),
                            output: Some(4.0),
                        }),
                    },
                ),
                (
                    "loner-v2".into(),
                    ModelInfo {
                        id: None,
                        name: None,
                        status: None,
                        family: None,
                        release_date: Some("2025-07-01".into()),
                        last_updated: None,
                        modalities: None,
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
    fn models_with_non_text_output_are_filtered_out() {
        fn info(output: &[&str]) -> ModelInfo {
            ModelInfo {
                id: None,
                name: None,
                status: None,
                family: None,
                release_date: None,
                last_updated: None,
                modalities: Some(ModelModalities {
                    output: output.iter().map(|m| m.to_string()).collect(),
                }),
                cost: None,
            }
        }
        let listing = ProviderListing {
            api: None,
            env: vec![],
            models: BTreeMap::from([
                ("text-only".into(), info(&["text"])),
                ("image-gen".into(), info(&["text", "image"])),
                ("audio-only".into(), info(&["audio"])),
                ("video-only".into(), info(&["video"])),
                ("no-output".into(), info(&[])),
            ]),
        };
        let selected = select_models(&listing);
        let ids: Vec<&str> = selected.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["text-only"]);
    }

    #[test]
    fn models_without_modality_info_are_kept() {
        let listing = ProviderListing {
            api: None,
            env: vec![],
            models: BTreeMap::from([(
                "no-modalities".into(),
                ModelInfo {
                    id: None,
                    name: None,
                    status: None,
                    family: None,
                    release_date: None,
                    last_updated: None,
                    modalities: None,
                    cost: None,
                },
            )]),
        };
        let selected = select_models(&listing);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id, "no-modalities");
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
                    "modalities": {"input": ["text"], "output": ["text"]},
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
        assert!(outputs_text_only(info));
        assert_eq!(
            select_models(&listing),
            vec![Model {
                id: "deepseek-v4-flash-free".to_string(),
                name: "DeepSeek V4 Flash Free".to_string(),
                free: true,
                family: Some("deepseek-flash".to_string())
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
                Model { id: "free-1".to_string(), name: "Free 1".to_string(), free: true, family: None },
                Model { id: "paid-1".to_string(), name: "Paid 1".to_string(), free: false, family: None },
            ],
        };
        assert_eq!(provider.selectable_models(true), vec!["free-1"]);
        assert_eq!(
            provider.selectable_models(false),
            vec!["free-1", "paid-1"]
        );
    }
}
