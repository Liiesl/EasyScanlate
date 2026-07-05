//! Fake translation for UI-only builds (compiled whenever the real
//! `translation` feature is off, i.e. no rig dependency). Mirrors the public
//! surface of `scanlateit_translation` so the translation UI stays live
//! instead of showing a "not available in this build" placeholder: the
//! pickers, settings list and translate flow are all exercisable with mock
//! providers/models, exactly like the fake OCR entries of the TEST-UI build.
//!
//! The UI and the app refer to [`crate::translation`], which re-exports this
//! module (or the real one when the `translation` feature is enabled); both
//! expose the same API surface used by the UI and the app.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

/// Context window for retranslate, mirrors real crate.
const RETRANSLATE_CONTEXT: usize = 3;

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

/// How the provider speaks: OpenAI chat-completions style or the Anthropic
/// Messages API. Mirrors the real module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatKind {
    OpenAI,
    Anthropic,
}

/// One stored connection: the API key plus (for custom endpoints) the base
/// URL and the single model id. Owned by the settings crate (it is persisted
/// data) and re-exported here, mirroring the real module.
pub use scanlateit_settings::Connection;

/// The id of the custom OpenAI-compatible connection.
pub const CUSTOM_OPENAI: &str = "custom-openai";
/// The id of the custom Anthropic-compatible connection.
pub const CUSTOM_ANTHROPIC: &str = "custom-anthropic";

/// Local providers — same ids as the real module, no API key needed.
pub const LOCAL_OLLAMA: &str = "ollama";
pub const LOCAL_VLLM: &str = "vllm";
pub const LOCAL_LLAMA_CPP: &str = "llama cpp";
pub const LOCAL_PROVIDERS: [&str; 3] = [LOCAL_OLLAMA, LOCAL_VLLM, LOCAL_LLAMA_CPP];

/// Hardcoded model choices for a custom connection when the user did not
/// enter one; the first entry is the default.
pub const CUSTOM_OPENAI_MODELS: [&str; 2] = ["fake-openai-custom", "fake-deepseek-custom"];
/// See [`CUSTOM_OPENAI_MODELS`].
pub const CUSTOM_ANTHROPIC_MODELS: [&str; 1] = ["fake-claude-custom"];

/// Target languages offered in the UI. Same list as the real module.
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

/// One selectable translation model: wire `id` and display `name`.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub free: bool,
    pub family: Option<String>,
}

impl Model {
    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            &self.id
        } else {
            &self.name
        }
    }
}

/// One translation gateway: where to call, which environment variable holds
/// its API key, and the selectable models. Mirrors the real module.
#[derive(Debug, Clone)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub api: String,
    pub kind: CompatKind,
    pub api_key_env: String,
    pub models: Vec<Model>,
}

impl Provider {
    /// The model picker entries of this provider, respecting the free-only
    /// filter; the full list when filtering would drop everything. Returns wire `id`s.
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

    pub fn model_display_name(&self, id: &str) -> Option<&str> {
        self.models.iter().find(|m| m.id == id).map(|m| m.display_name())
    }
}

/// The fake gateways offered in the settings UI. The ids are clearly fake so
/// nobody mistakes them for real endpoints.
pub static SUPPORTED_PROVIDERS: LazyLock<Vec<Provider>> = LazyLock::new(|| {
    vec![
        entry(
            "fake-llm",
            "Fake LLM",
            "https://fake.example/v1",
            CompatKind::OpenAI,
            "FAKE_LLM_API_KEY",
            &["fake-gpt-4o", "fake-claude-sonnet", "fake-deepseek-v4"],
        ),
        entry(
            "fake-lite",
            "Fake Lite",
            "https://fake.example/v1",
            CompatKind::OpenAI,
            "FAKE_LITE_API_KEY",
            &["fake-lite-mini", "fake-lite-flash"],
        ),
    ]
});

/// The id of the primary fake provider; connected at TEST-UI boot so the
/// translation bar shows a provider with models immediately.
pub const FAKE_PROVIDER: &str = "fake-llm";

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

fn fallback_models(ids: &[&str]) -> Vec<Model> {
    ids.iter()
        .map(|id| {
            let display = match *id {
                "fake-gpt-4o" => "Fake GPT-4o",
                "fake-claude-sonnet" => "Fake Claude Sonnet",
                "fake-deepseek-v4" => "Fake DeepSeek V4",
                "fake-lite-mini" => "Fake Lite Mini",
                "fake-lite-flash" => "Fake Lite Flash",
                "fake-openai-custom" => "Fake OpenAI Custom",
                "fake-deepseek-custom" => "Fake DeepSeek Custom",
                "fake-claude-custom" => "Fake Claude Custom",
                _ => *id,
            };
            Model {
                id: (*id).to_string(),
                name: display.to_string(),
                free: false,
                family: None,
            }
        })
        .collect()
}

/// Looks up a fake gateway by its provider id.
pub fn catalog_provider(id: &str) -> Option<&'static Provider> {
    SUPPORTED_PROVIDERS.iter().find(|p| p.id == id)
}

/// The connection id of a custom endpoint matching `kind`.
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

/// Display name for a connection id.
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

/// First validation error of the connect modal form, if any.
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

/// One OCR line to translate.
#[derive(Debug, Clone)]
pub struct TranslateItem {
    pub filename: String,
    pub id: u64,
    pub text: String,
}

/// Fetches the fake provider configs for the requested ids. No network;
/// returns the catalog entries.
pub async fn fetch_providers(ids: Vec<String>) -> HashMap<String, Provider> {
    let mut providers = HashMap::new();
    for id in &ids {
        providers.insert(id.clone(), fetch_provider(id).await);
    }
    providers
}

/// Returns the fake provider for `id` (the catalog entry or the custom-*
/// defaults).
pub async fn fetch_provider(id: &str) -> Provider {
    match catalog_provider(id) {
        Some(catalog) => catalog.clone(),
        None => custom_fallback_provider(id),
    }
}

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
        models: fallback_models(models),
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
        api: base_url.trim().trim_end_matches('/').to_string(),
        kind: CompatKind::OpenAI,
        api_key_env: String::new(),
        models: Vec::new(),
    }
}

/// Fake local discovery — in fake builds just returns the fallback.
pub async fn fetch_local_models(_base_url: &str, _id: &str) -> Result<Vec<Model>, String> {
    Err("fake discovery has no endpoint".to_string())
}
pub async fn fetch_local_provider(id: &str, base_url: &str) -> Provider {
    local_fallback_provider(id, base_url)
}
pub async fn fetch_local_providers(
    endpoints: HashMap<String, String>,
) -> HashMap<String, Provider> {
    let mut out = HashMap::new();
    for (id, base) in endpoints {
        out.insert(id.clone(), fetch_local_provider(&id, &base).await);
    }
    out
}

/// The `Provider` to send requests to for a connection: the catalog gateway
/// for built-ins, or one built from the connection's own base URL and model
/// for custom endpoints. Local providers use `base_url` as `api`.
pub fn provider_for_connection(id: &str, connection: &Connection) -> Provider {
    let mut provider = match catalog_provider(id) {
        Some(catalog) => catalog.clone(),
        None => custom_fallback_provider(id),
    };
    if is_local(id) {
        provider.api = connection
            .base_url
            .clone()
            .map(|u| u.trim().trim_end_matches('/').to_string())
            .unwrap_or_else(|| provider.api.clone());
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

/// Builds retranslate wire format with context, mirroring real crate and
/// `ManhwaOCR/app/core/translations.py:generate_retranslate_content`.
pub fn build_retranslate_content(
    items: &[TranslateItem],
    selected: &[(String, u64)],
    context_size: usize,
) -> String {
    if selected.is_empty() {
        return String::new();
    }
    let mut all_by_file: HashMap<String, Vec<&TranslateItem>> = HashMap::new();
    for it in items {
        all_by_file.entry(it.filename.clone()).or_default().push(it);
    }
    for v in all_by_file.values_mut() {
        v.sort_by_key(|it| it.id);
    }
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

/// Translates every line with a fake, deterministic answer: the target
/// language name bracketed in front of the source text. Mirrors the real
/// module's signature so the app flow is identical. Handles missing via
/// context-aware retry to mirror real crate.
pub async fn translate_all(
    items: &[TranslateItem],
    target: &str,
    provider: &Provider,
    model: &str,
    api_key: Option<String>,
) -> Result<Vec<String>, String> {
    // In fake builds we always succeed, but we implement missing detection +
    // retry via `translate_one_with_context` to keep parity with real crate.
    // Since fake never drops rows, this is a no-op path.
    let translations: Vec<String> = items
        .iter()
        .map(|item| format!("[{target}] {}", item.text))
        .collect();
    // Simulate parsing roundtrip (no missing in fake)
    let mut parsed: HashMap<(String, u64), String> = HashMap::new();
    for (item, tr) in items.iter().zip(translations.iter()) {
        parsed.insert((item.filename.clone(), item.id), tr.clone());
    }
    let missing: Vec<&TranslateItem> = items
        .iter()
        .filter(|it| !parsed.contains_key(&(it.filename.clone(), it.id)))
        .collect();
    if !missing.is_empty() {
        // Retry each missing with context (fake just returns same format)
        for miss in missing {
            let same_file: Vec<TranslateItem> = items
                .iter()
                .filter(|i| i.filename == miss.filename)
                .cloned()
                .collect();
            let t = translate_one_with_context(
                &miss.text, target, provider, model, api_key.clone(), &same_file, miss.id, &miss.filename,
            )
            .await?;
            parsed.insert((miss.filename.clone(), miss.id), t);
        }
    }
    // Build aligned result with empty placeholders for still-missing (none in fake)
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(
            parsed
                .get(&(item.filename.clone(), item.id))
                .cloned()
                .unwrap_or_default(),
        );
    }
    Ok(out)
}

/// Translates one line with context-aware retranslate logic (mirrors real crate).
pub async fn translate_one_with_context(
    text: &str,
    target: &str,
    _provider: &Provider,
    _model: &str,
    _api_key: Option<String>,
    context_items: &[TranslateItem],
    selected_id: u64,
    filename: &str,
) -> Result<String, String> {
    // In fake builds context is ignored beyond building the retranslate content for parity.
    if !context_items.is_empty() && selected_id != 0 && !filename.is_empty() {
        let selected = vec![(filename.to_string(), selected_id)];
        let _ = build_retranslate_content(context_items, &selected, RETRANSLATE_CONTEXT);
    }
    Ok(format!("[{target}] {text}"))
}

/// Translates one line with the same fake answer.
pub async fn translate_one(
    text: &str,
    target: &str,
    provider: &Provider,
    model: &str,
    api_key: Option<String>,
) -> Result<String, String> {
    translate_one_with_context(text, target, provider, model, api_key, &[], 0, "").await
}

/// The connected-provider session. Mirrors the real module's session so the
/// app and the UI can use it identically.
#[derive(Debug, Clone, Default)]
pub struct Session {
    /// Stored connections, keyed by provider id; connected == has entry.
    pub connections: BTreeMap<String, Connection>,
    /// The selected provider id; always one of `connected_ids` when non-empty.
    pub selected_id: String,
    /// Connected ids in catalog order, then the custom slots.
    pub connected_ids: Vec<String>,
    /// Fetched gateway configs (the fake catalog), keyed by id.
    pub fetched: HashMap<String, Provider>,
    /// The model picker entries of the selected provider.
    pub models: Vec<String>,
    /// The selected model id; always one of `models` when non-empty.
    pub selected_model: String,
    /// Free-only filter for the model picker.
    pub free_only: bool,
    pub hidden_models: BTreeMap<String, BTreeSet<String>>,
    /// Cached output of [`Self::model_groups`]; rebuilt at the top of every
    /// [`Self::sync_models`] call so callers can borrow it for the frame.
    /// Each inner pair is `(model id, display name)`.
    groups: Vec<(String, String, Vec<(String, String)>)>,
}

impl Session {
    /// Restores the stored connections, then picks `last_provider` when it is
    /// still connected (or falls back to the first connected provider).
    pub fn new(
        connections: BTreeMap<String, Connection>,
        last_provider: Option<String>,
    ) -> Self {
        let mut session = Session {
            connections,
            ..Session::default()
        };
        session.sync();
        if let Some(id) = last_provider {
            if session.connections.contains_key(&id) {
                session.selected_id = id;
                session.sync_models();
            }
        }
        session
    }

    /// Rebuilds `connected_ids` (catalog order + custom slots) and fixes
    /// `selected_id` when it dropped out (falls back to the first connected
    /// provider, or empty). Calls `sync_models`.
    pub fn sync(&mut self) {
        let mut ids: Vec<String> = Vec::new();
        for provider in SUPPORTED_PROVIDERS.iter() {
            if self.connections.contains_key(&provider.id) {
                ids.push(provider.id.clone());
            }
        }
        for custom in [CUSTOM_OPENAI, CUSTOM_ANTHROPIC] {
            if self.connections.contains_key(custom) {
                ids.push(custom.to_string());
            }
        }
        self.connected_ids = ids;
        if !self.connected_ids.contains(&self.selected_id) {
            self.selected_id = self.connected_ids.first().cloned().unwrap_or_default();
        }
        self.sync_models();
    }

    fn visible_models(&self, provider: &Provider) -> Vec<String> {
        let mut ids = provider.selectable_models(self.free_only);
        if let Some(hidden) = self.hidden_models.get(&provider.id) {
            ids.retain(|id| !hidden.contains(id));
            if ids.is_empty() && !provider.models.is_empty() {
                let mut fallback: Vec<String> =
                    provider.models.iter().map(|m| m.id.clone()).collect();
                fallback.retain(|id| !hidden.contains(id));
                if !fallback.is_empty() {
                    ids = fallback;
                }
            }
        }
        ids
    }

    fn visible_model_pairs(&self, provider: &Provider) -> Vec<(String, String)> {
        let ids = self.visible_models(provider);
        ids.into_iter()
            .map(|id| {
                let display = provider.model_display_name(&id).unwrap_or(&id).to_string();
                (id, display)
            })
            .collect()
    }

    /// Rebuilds `models`/`selected_model` for the current provider. Also
    /// refreshes the [`Self::model_groups`] cache, mirroring the real
    /// session.
    pub fn sync_models(&mut self) {
        self.groups = self.compute_model_groups();
        if self.selected_id.is_empty() {
            self.models = Vec::new();
            self.selected_model = String::new();
            return;
        }
        let provider = self
            .fetched
            .get(&self.selected_id)
            .cloned()
            .or_else(|| {
                self.connections.get(&self.selected_id).map(|connection| {
                    provider_for_connection(&self.selected_id, connection)
                })
            });
        let models = provider
            .as_ref()
            .map(|p| self.visible_models(p))
            .unwrap_or_default();
        if models.is_empty() {
            self.models = Vec::new();
            self.selected_model = String::new();
            return;
        }
        self.models = models;
        if !self.models.contains(&self.selected_model) {
            self.selected_model = self.models[0].clone();
        }
        if let Some(provider) = provider {
            if let Some(hidden) = self.hidden_models.get_mut(&provider.id) {
                let valid: BTreeSet<String> =
                    provider.models.iter().map(|m| m.id.clone()).collect();
                hidden.retain(|id| valid.contains(id));
            }
        }
    }

    /// Stores a connection and selects it; `sync`s.
    pub fn connect(&mut self, id: String, connection: Connection) {
        self.connections.insert(id.clone(), connection);
        self.sync();
        self.selected_id = id;
        self.sync_models();
    }

    /// Removes a connection; `sync`s.
    pub fn disconnect(&mut self, id: &str) {
        self.connections.remove(id);
        self.fetched.remove(id);
        self.sync();
    }

    /// Selects `id` (only when connected); `sync_models`s.
    pub fn select(&mut self, id: String) {
        if id.is_empty() || !self.connected_ids.contains(&id) {
            return;
        }
        if self.selected_id != id {
            self.selected_id = id;
            self.sync_models();
        }
    }

    /// Every connected provider's selectable models, in connected order:
    /// `(provider id, display name, model pairs)`. Each pair is `(model id,
    /// display name)`. The pairs respect the free-only filter and hidden set.
    ///
    /// Returns a borrow of the internal cache (refreshed by
    /// [`Self::sync_models`]) so view code can hold the `&str`s for a frame
    /// without cloning. Request still uses `id`.
    pub fn model_groups(&self) -> &[(String, String, Vec<(String, String)>)] {
        &self.groups
    }

    /// Recomputes [`Self::model_groups`] from scratch.
    fn compute_model_groups(&self) -> Vec<(String, String, Vec<(String, String)>)> {
        self.connected_ids
            .iter()
            .filter_map(|id| {
                let provider = self.fetched.get(id).cloned().or_else(|| {
                    self.connections
                        .get(id)
                        .map(|connection| provider_for_connection(id, connection))
                })?;
                let models = self.visible_model_pairs(&provider);
                (!models.is_empty())
                    .then(|| (id.clone(), provider_name(id), models))
            })
            .collect()
    }

    pub fn all_model_groups(&self) -> Vec<(String, String, Vec<(String, String)>)> {
        self.connected_ids
            .iter()
            .filter_map(|id| {
                let provider = self.fetched.get(id).cloned().or_else(|| {
                    self.connections
                        .get(id)
                        .map(|connection| provider_for_connection(id, connection))
                })?;
                let mut pairs: Vec<(String, String)> = provider
                    .models
                    .iter()
                    .map(|m| (m.id.clone(), m.display_name().to_string()))
                    .collect();
                pairs.sort_by(|a, b| a.0.cmp(&b.0));
                (!pairs.is_empty()).then(|| (id.clone(), provider_name(id), pairs))
            })
            .collect()
    }

    pub fn is_hidden(&self, provider: &str, model: &str) -> bool {
        self.hidden_models
            .get(provider)
            .is_some_and(|set| set.contains(model))
    }

    pub fn set_model_visible(&mut self, provider: String, model: String, visible: bool) {
        if visible {
            if let Some(set) = self.hidden_models.get_mut(&provider) {
                set.remove(&model);
                if set.is_empty() {
                    self.hidden_models.remove(&provider);
                }
            }
        } else {
            self.hidden_models
                .entry(provider.clone())
                .or_default()
                .insert(model);
        }
        self.sync_models();
    }

    pub fn clear_hidden(&mut self, provider: &str) {
        self.hidden_models.remove(provider);
        self.sync_models();
    }

    pub fn clear_all_hidden(&mut self) {
        self.hidden_models.clear();
        self.sync_models();
    }

    /// Selects a provider and pins a specific model for it in one step.
    /// Mirrors the real module's session.
    pub fn select_model(&mut self, id: String, model: String) {
        if id.is_empty() || !self.connected_ids.contains(&id) {
            return;
        }
        self.selected_id = id;
        self.sync_models();
        if self.models.contains(&model) {
            self.selected_model = model;
        }
    }

    /// Sets the free-only filter; `sync_models`s.
    pub fn set_free_only(&mut self, free_only: bool) {
        if self.free_only != free_only {
            self.free_only = free_only;
            self.sync_models();
        }
    }

    /// Merges fetched listings; `sync_models`s.
    pub fn on_fetched(&mut self, providers: HashMap<String, Provider>) {
        self.fetched.extend(providers);
        self.sync_models();
    }

    /// The ids that need a models fetch (connected, non-custom, non-local).
    pub fn fetch_ids(&self) -> Vec<String> {
        self.connected_ids
            .iter()
            .filter(|id| !is_custom(id) && !is_local(id))
            .cloned()
            .collect()
    }

    pub fn local_fetch_endpoints(&self) -> HashMap<String, String> {
        let mut endpoints = HashMap::new();
        for id in &self.connected_ids {
            if is_local(id) {
                if let Some(conn) = self.connections.get(id) {
                    if let Some(url) = &conn.base_url {
                        if !url.trim().is_empty() {
                            endpoints.insert(id.clone(), url.clone());
                            continue;
                        }
                    }
                }
                if let Some(catalog) = catalog_provider(id) {
                    endpoints.insert(id.clone(), catalog.api.clone());
                }
            }
        }
        endpoints
    }

    /// The requestable [`Provider`] for the selected connection (catalog or
    /// custom, with the connection's api/kind/model baked in).
    pub fn selected_provider(&self) -> Option<Provider> {
        self.connections.get(&self.selected_id).map(|connection| {
            provider_for_connection(&self.selected_id, connection)
        })
    }

    /// The stored API key of the selected connection, if any.
    pub fn selected_api_key(&self) -> Option<String> {
        self.connections
            .get(&self.selected_id)
            .map(|connection| connection.api_key.clone())
    }

    pub fn is_connected(&self) -> bool {
        !self.selected_id.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection(api_key: &str) -> Connection {
        Connection {
            api_key: api_key.to_string(),
            base_url: None,
            model: None,
        }
    }

    #[test]
    fn session_orders_catalog_then_custom() {
        let session = Session::new(
            BTreeMap::from([
                (CUSTOM_OPENAI.to_string(), connection("sk-c")),
                ("fake-lite".to_string(), connection("sk-f")),
                ("fake-llm".to_string(), connection("sk-f")),
            ]),
            None,
        );
        assert_eq!(
            session.connected_ids,
            vec![
                "fake-llm".to_string(),
                "fake-lite".to_string(),
                CUSTOM_OPENAI.to_string()
            ]
        );
    }

    #[test]
    fn fake_providers_expose_models() {
        let provider = catalog_provider(FAKE_PROVIDER).unwrap();
        assert_eq!(provider.selectable_models(false).len(), 3);
        assert_eq!(provider.name, "Fake LLM");
        assert_eq!(provider_name(FAKE_PROVIDER), "Fake LLM");
        assert!(!is_custom(FAKE_PROVIDER));
    }

    #[test]
    fn fake_translate_marks_the_target_language() {
        let items = vec![
            TranslateItem { filename: "a.png".into(), id: 1, text: "안녕".into() },
            TranslateItem { filename: "a.png".into(), id: 2, text: "하세요".into() },
        ];
        let provider = catalog_provider(FAKE_PROVIDER).unwrap().clone();
        let out = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(translate_all(&items, "English", &provider, "fake-gpt-4o", None))
            .unwrap();
        assert_eq!(out, vec!["[English] 안녕", "[English] 하세요"]);
    }

    #[test]
    fn model_groups_lists_every_connected_provider() {
        let session = Session::new(
            BTreeMap::from([
                (FAKE_PROVIDER.to_string(), connection("sk-f")),
                ("fake-lite".to_string(), connection("sk-f")),
            ]),
            Some(FAKE_PROVIDER.to_string()),
        );
        assert_eq!(
            session.model_groups(),
            vec![
                (
                    "fake-llm".to_string(),
                    "Fake LLM".to_string(),
                    vec![
                        ("fake-gpt-4o".to_string(), "Fake GPT-4o".to_string()),
                        ("fake-claude-sonnet".to_string(), "Fake Claude Sonnet".to_string()),
                        ("fake-deepseek-v4".to_string(), "Fake DeepSeek V4".to_string()),
                    ],
                ),
                (
                    "fake-lite".to_string(),
                    "Fake Lite".to_string(),
                    vec![
                        ("fake-lite-mini".to_string(), "Fake Lite Mini".to_string()),
                        ("fake-lite-flash".to_string(), "Fake Lite Flash".to_string()),
                    ],
                ),
            ]
        );
    }

    #[test]
    fn select_model_sets_provider_and_model_in_one_step() {
        let mut session = Session::new(
            BTreeMap::from([
                (FAKE_PROVIDER.to_string(), connection("sk-f")),
                ("fake-lite".to_string(), connection("sk-f")),
            ]),
            Some(FAKE_PROVIDER.to_string()),
        );
        session.select_model("fake-lite".to_string(), "fake-lite-flash".to_string());
        assert_eq!(session.selected_id, "fake-lite");
        assert_eq!(session.selected_model, "fake-lite-flash");
        // An unknown model falls back to the provider's first model.
        session.select_model(FAKE_PROVIDER.to_string(), "nope".to_string());
        assert_eq!(session.selected_id, FAKE_PROVIDER);
        assert_eq!(session.selected_model, "fake-gpt-4o");
    }
}