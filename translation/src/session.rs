//! The connected-provider session: which connections exist, which one is
//! selected, the model picker lists and the free-only filter. The app stores
//! one of these and forwards `UiEvent`s to it; the crate owns all rules
//! (catalog ordering, selection fallback, model list sync).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::{
    is_custom, is_local, provider_for_connection, provider_name, Connection, Provider,
    CUSTOM_ANTHROPIC, CUSTOM_OPENAI, SUPPORTED_PROVIDERS,
};

/// The connected-provider session state of the translation bar.
#[derive(Debug, Clone, Default)]
pub struct Session {
    /// Stored connections, keyed by provider id; connected == has entry.
    pub connections: BTreeMap<String, Connection>,
    /// The selected provider id; always one of `connected_ids` when non-empty.
    pub selected_id: String,
    /// Connected ids in catalog order, then the custom slots.
    pub connected_ids: Vec<String>,
    /// Fetched gateway configs from the models mirror, keyed by id.
    pub fetched: HashMap<String, Provider>,
    /// The model picker entries of the selected provider.
    pub models: Vec<String>,
    /// The selected model id; always one of `models` when non-empty.
    pub selected_model: String,
    /// Free-only filter for the model picker.
    pub free_only: bool,
    /// Per-provider set of model ids explicitly hidden by the user via the
    /// Manage Models overlay. The overlay lists all usable models (deprecated
    /// already removed) and this set hides older-family models by default.
    pub hidden_models: BTreeMap<String, BTreeSet<String>>,
    /// Cached output of [`Self::model_groups`]; rebuilt at the top of every
    /// [`Self::sync_models`] call so callers can borrow it for the frame.
    groups: Vec<(String, String, Vec<String>)>,
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
    /// provider, or empty). Calls `sync_models`. Local providers are part of
    /// `SUPPORTED_PROVIDERS` so they appear in catalog order automatically.
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
            // If free filter + hidden emptied the list, fall back to provider's
            // full list minus hidden (mirrors Provider::selectable_models fallback).
            if ids.is_empty() && !provider.models.is_empty() {
                let mut fallback: Vec<String> =
                    provider.models.iter().map(|m| m.id.clone()).collect();
                fallback.retain(|id| !hidden.contains(id));
                if !fallback.is_empty() {
                    ids = fallback;
                } else {
                    // All hidden – keep empty so UI shows nothing rather than
                    // unexpectedly resurrecting hidden models.
                }
            }
        }
        ids
    }

    /// Rebuilds `models`/`selected_model` for the current provider. Also
    /// refreshes the [`Self::model_groups`] cache: every mutating path
    /// (`connect`, `disconnect`, `select`, `set_free_only`,
    /// `set_model_visible`, `on_fetched`, …) funnels through here, so the
    /// cache can never go stale.
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
        // Prune stale hidden entries that no longer exist in provider.
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
    /// `(provider id, display name, model ids)`. The model ids respect the
    /// free-only filter and the Manage Models hidden set; providers without
    /// selectable models are skipped. This is what the merged model dropdown
    /// renders, grouped by provider.
    ///
    /// Returns a borrow of the internal cache (refreshed by
    /// [`Self::sync_models`]) so view code can hold the `&str`s for a frame
    /// without cloning.
    pub fn model_groups(&self) -> &[(String, String, Vec<String>)] {
        &self.groups
    }

    /// Recomputes [`Self::model_groups`] from scratch.
    fn compute_model_groups(&self) -> Vec<(String, String, Vec<String>)> {
        self.connected_ids
            .iter()
            .filter_map(|id| {
                let provider = self.fetched.get(id).cloned().or_else(|| {
                    self.connections
                        .get(id)
                        .map(|connection| provider_for_connection(id, connection))
                })?;
                let models = self.visible_models(&provider);
                (!models.is_empty())
                    .then(|| (id.clone(), provider_name(id), models))
            })
            .collect()
    }

    /// All usable models per connected provider, without `free_only` or
    /// hidden filtering – deprecated already removed. Used by the Manage
    /// Models overlay to list every toggleable model.
    pub fn all_model_groups(&self) -> Vec<(String, String, Vec<String>)> {
        self.connected_ids
            .iter()
            .filter_map(|id| {
                let provider = self.fetched.get(id).cloned().or_else(|| {
                    self.connections
                        .get(id)
                        .map(|connection| provider_for_connection(id, connection))
                })?;
                let mut ids: Vec<String> =
                    provider.models.iter().map(|m| m.id.clone()).collect();
                ids.sort();
                (!ids.is_empty())
                    .then(|| (id.clone(), provider_name(id), ids))
            })
            .collect()
    }

    /// Whether `model` of `provider` is currently hidden.
    pub fn is_hidden(&self, provider: &str, model: &str) -> bool {
        self.hidden_models
            .get(provider)
            .is_some_and(|set| set.contains(model))
    }

    /// Toggle visibility of `model` for `provider`. `visible=true` means the
    /// model should be shown in the dropdown.
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

    /// Reset hidden set for `provider` to empty (all models visible).
    pub fn clear_hidden(&mut self, provider: &str) {
        self.hidden_models.remove(provider);
        self.sync_models();
    }

    /// Reset all hidden models (show everything).
    pub fn clear_all_hidden(&mut self) {
        self.hidden_models.clear();
        self.sync_models();
    }

    /// Selects a provider and pins a specific model for it in one step, the
    /// way the merged model dropdown selects. `sync_models` keeps the model
    /// choice valid: it rebuilds the selected provider's list first, then the
    /// requested model wins when it is on that list.
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
    /// Local providers are fetched via [`local_fetch_endpoints`] instead.
    pub fn fetch_ids(&self) -> Vec<String> {
        self.connected_ids
            .iter()
            .filter(|id| !is_custom(id) && !is_local(id))
            .cloned()
            .collect()
    }

    /// The local providers that need discovery, keyed by `id -> base_url`.
    /// Uses the stored `base_url` when present, otherwise the catalog default.
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
                if let Some(catalog) = super::catalog_provider(id) {
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
    fn sync_orders_catalog_then_custom() {
        let session = Session::new(
            BTreeMap::from([
                (CUSTOM_OPENAI.to_string(), connection("sk-c")),
                ("deepseek".to_string(), connection("sk-d")),
                ("openai".to_string(), connection("sk-o")),
            ]),
            None,
        );
        assert_eq!(
            session.connected_ids,
            vec!["openai".to_string(), "deepseek".to_string(), CUSTOM_OPENAI.to_string()]
        );
    }

    #[test]
    fn selection_falls_back_to_the_first_connected() {
        let session = Session::new(
            BTreeMap::from([
                ("openai".to_string(), connection("sk-o")),
                ("deepseek".to_string(), connection("sk-d")),
            ]),
            None,
        );
        assert_eq!(session.selected_id, "openai");
    }

    #[test]
    fn selection_picks_last_provider_when_still_connected() {
        let session = Session::new(
            BTreeMap::from([
                ("openai".to_string(), connection("sk-o")),
                ("deepseek".to_string(), connection("sk-d")),
            ]),
            Some("deepseek".to_string()),
        );
        assert_eq!(session.selected_id, "deepseek");
    }

    #[test]
    fn selection_ignores_a_last_provider_that_dropped_out() {
        let session = Session::new(
            BTreeMap::from([("openai".to_string(), connection("sk-o"))]),
            Some("deepseek".to_string()),
        );
        assert_eq!(session.selected_id, "openai");
    }

    #[test]
    fn selection_falls_back_to_empty_without_connections() {
        let session = Session::new(BTreeMap::new(), None);
        assert_eq!(session.selected_id, "");
        assert!(session.connected_ids.is_empty());
        assert!(session.models.is_empty());
        assert!(session.selected_model.is_empty());
    }

    #[test]
    fn disconnect_removes_the_connection_and_reselects() {
        let mut session = Session::new(
            BTreeMap::from([
                ("openai".to_string(), connection("sk-o")),
                ("deepseek".to_string(), connection("sk-d")),
            ]),
            Some("deepseek".to_string()),
        );
        session.disconnect("deepseek");
        assert_eq!(session.selected_id, "openai");
        assert!(!session.connected_ids.contains(&"deepseek".to_string()));
        assert!(!session.fetched.contains_key("deepseek"));
    }

    #[test]
    fn connect_stores_and_selects_the_new_connection() {
        let mut session = Session::new(
            BTreeMap::from([("openai".to_string(), connection("sk-o"))]),
            Some("openai".to_string()),
        );
        session.connect("deepseek".to_string(), connection("sk-d"));
        assert_eq!(session.selected_id, "deepseek");
        assert!(session.connected_ids.contains(&"deepseek".to_string()));
        assert_eq!(
            session.connections["deepseek"].api_key,
            "sk-d",
            "the API key is kept in memory only"
        );
    }

    #[test]
    fn select_ignores_unknown_or_already_selected_ids() {
        let mut session = Session::new(
            BTreeMap::from([("openai".to_string(), connection("sk-o"))]),
            Some("openai".to_string()),
        );
        session.select("not-connected".to_string());
        assert_eq!(session.selected_id, "openai");
        session.select("openai".to_string());
        assert_eq!(session.selected_id, "openai");
    }

    fn fetched_deepseek() -> Provider {
        Provider {
            id: "deepseek".to_string(),
            name: "DeepSeek".to_string(),
            api: "https://api.deepseek.com".to_string(),
            kind: super::super::CompatKind::OpenAI,
            api_key_env: "DEEPSEEK_API_KEY".to_string(),
            models: vec![
                super::super::Model { id: "free-1".to_string(), free: true, family: None },
                super::super::Model { id: "paid-1".to_string(), free: false, family: None },
            ],
        }
    }

    #[test]
    fn models_rebuild_on_fetch_connect_select_and_free_only_toggle() {
        let mut session = Session::new(
            BTreeMap::from([("deepseek".to_string(), connection("sk-d"))]),
            Some("deepseek".to_string()),
        );
        assert_eq!(
            session.models,
            vec!["deepseek-chat".to_string(), "deepseek-reasoner".to_string()],
            "before the fetch the catalog fallback list is shown"
        );

        session.on_fetched(HashMap::from([("deepseek".to_string(), fetched_deepseek())]));
        assert_eq!(session.models, vec!["free-1".to_string(), "paid-1".to_string()]);
        assert_eq!(session.selected_model, "free-1");

        session.set_free_only(true);
        assert_eq!(session.models, vec!["free-1".to_string()]);
        session.set_free_only(false);
        assert_eq!(session.models, vec!["free-1".to_string(), "paid-1".to_string()]);

        session.connect("openai".to_string(), connection("sk-o"));
        assert_eq!(session.selected_id, "openai");
        assert_eq!(
            session.models,
            vec!["gpt-4o-mini".to_string(), "gpt-5-nano".to_string()],
            "switching providers rebuilds the model list from the catalog"
        );

        session.select("deepseek".to_string());
        assert_eq!(session.models, vec!["free-1".to_string(), "paid-1".to_string()]);
    }

    #[test]
    fn fetch_ids_exclude_custom_slots() {
        let session = Session::new(
            BTreeMap::from([
                (CUSTOM_OPENAI.to_string(), connection("sk-c")),
                (CUSTOM_ANTHROPIC.to_string(), connection("sk-c2")),
                ("openai".to_string(), connection("sk-o")),
                ("deepseek".to_string(), connection("sk-d")),
            ]),
            None,
        );
        assert_eq!(session.fetch_ids(), vec!["openai".to_string(), "deepseek".to_string()]);
    }

    #[test]
    fn fetch_ids_are_empty_without_connections() {
        let session = Session::new(BTreeMap::new(), None);
        assert!(session.fetch_ids().is_empty());
    }

    #[test]
    fn selected_provider_resolves_custom_api_and_model() {
        let session = Session::new(
            BTreeMap::from([(
                CUSTOM_OPENAI.to_string(),
                Connection {
                    api_key: "sk-custom".to_string(),
                    base_url: Some("http://localhost:11434/v1".to_string()),
                    model: Some("llama-3.1-8b".to_string()),
                },
            )]),
            Some(CUSTOM_OPENAI.to_string()),
        );
        let provider = session.selected_provider().expect("custom selected");
        assert_eq!(provider.api, "http://localhost:11434/v1");
        assert_eq!(provider.models.len(), 1);
        assert_eq!(provider.models[0].id, "llama-3.1-8b");
        assert_eq!(session.selected_api_key().as_deref(), Some("sk-custom"));
    }

    #[test]
    fn selected_provider_uses_the_catalog_for_builtins() {
        let session = Session::new(
            BTreeMap::from([("deepseek".to_string(), connection("sk-d"))]),
            Some("deepseek".to_string()),
        );
        let provider = session.selected_provider().expect("builtin selected");
        assert_eq!(provider.api, "https://api.deepseek.com");
        assert_eq!(session.selected_api_key().as_deref(), Some("sk-d"));
    }

    #[test]
    fn selected_provider_is_none_without_selection() {
        let session = Session::new(BTreeMap::new(), None);
        assert!(session.selected_provider().is_none());
        assert!(session.selected_api_key().is_none());
    }

    #[test]
    fn is_connected_reflects_the_selection() {
        assert!(!Session::new(BTreeMap::new(), None).is_connected());
        assert!(
            Session::new(
                BTreeMap::from([("openai".to_string(), connection("sk-o"))]),
                None,
            )
            .is_connected()
        );
    }

    #[test]
    fn model_groups_lists_every_connected_provider() {
        let session = Session::new(
            BTreeMap::from([
                ("deepseek".to_string(), connection("sk-d")),
                ("openai".to_string(), connection("sk-o")),
            ]),
            Some("deepseek".to_string()),
        );
        assert_eq!(
            session.model_groups(),
            vec![
                (
                    "openai".to_string(),
                    "OpenAI".to_string(),
                    vec!["gpt-4o-mini".to_string(), "gpt-5-nano".to_string()],
                ),
                (
                    "deepseek".to_string(),
                    "DeepSeek".to_string(),
                    vec!["deepseek-chat".to_string(), "deepseek-reasoner".to_string()],
                ),
            ]
        );
    }

    #[test]
    fn model_groups_use_fetched_models_and_the_free_filter() {
        let mut session = Session::new(
            BTreeMap::from([("deepseek".to_string(), connection("sk-d"))]),
            Some("deepseek".to_string()),
        );
        session.on_fetched(HashMap::from([("deepseek".to_string(), fetched_deepseek())]));
        assert_eq!(
            session.model_groups(),
            vec![(
                "deepseek".to_string(),
                "DeepSeek".to_string(),
                vec!["free-1".to_string(), "paid-1".to_string()],
            )]
        );
        session.set_free_only(true);
        assert_eq!(
            session.model_groups(),
            vec![(
                "deepseek".to_string(),
                "DeepSeek".to_string(),
                vec!["free-1".to_string()],
            )]
        );
    }

    #[test]
    fn model_groups_include_custom_connections() {
        let session = Session::new(
            BTreeMap::from([(
                CUSTOM_OPENAI.to_string(),
                Connection {
                    api_key: "sk-custom".to_string(),
                    base_url: Some("http://localhost:11434/v1".to_string()),
                    model: Some("llama-3.1-8b".to_string()),
                },
            )]),
            Some(CUSTOM_OPENAI.to_string()),
        );
        assert_eq!(
            session.model_groups(),
            vec![(
                CUSTOM_OPENAI.to_string(),
                "Custom (OpenAI-compatible)".to_string(),
                vec!["llama-3.1-8b".to_string()],
            )]
        );
    }

    #[test]
    fn model_groups_are_empty_without_connections() {
        let session = Session::new(BTreeMap::new(), None);
        assert!(session.model_groups().is_empty());
    }

    #[test]
    fn select_model_sets_provider_and_model_in_one_step() {
        let mut session = Session::new(
            BTreeMap::from([
                ("openai".to_string(), connection("sk-o")),
                ("deepseek".to_string(), connection("sk-d")),
            ]),
            Some("openai".to_string()),
        );
        session.select_model("deepseek".to_string(), "deepseek-reasoner".to_string());
        assert_eq!(session.selected_id, "deepseek");
        assert_eq!(session.selected_model, "deepseek-reasoner");
        // An unknown model falls back to the provider's first model.
        session.select_model("openai".to_string(), "does-not-exist".to_string());
        assert_eq!(session.selected_id, "openai");
        assert_eq!(session.selected_model, "gpt-4o-mini");
        // Unknown providers are ignored.
        session.select_model("nope".to_string(), "gpt-4o-mini".to_string());
        assert_eq!(session.selected_id, "openai");
    }
}