//! The connected-provider session: which connections exist, which one is
//! selected, the model picker lists and the free-only filter. The app stores
//! one of these and forwards `UiEvent`s to it; the crate owns all rules
//! (catalog ordering, selection fallback, model list sync).

use std::collections::{BTreeMap, HashMap};

use super::{
    Connection, Provider, is_custom, provider_for_connection, CUSTOM_ANTHROPIC, CUSTOM_OPENAI,
    SUPPORTED_PROVIDERS,
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

    /// Rebuilds `models`/`selected_model` for the current provider.
    pub fn sync_models(&mut self) {
        if self.selected_id.is_empty() {
            self.models = Vec::new();
            self.selected_model = String::new();
            return;
        }
        let free_only = self.free_only;
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
            .map(|provider| provider.selectable_models(free_only))
            .unwrap_or_default();
        if models.is_empty() {
            return;
        }
        self.models = models;
        if !self.models.contains(&self.selected_model) {
            self.selected_model = self.models[0].clone();
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

    /// The ids that need a models fetch (connected, non-custom).
    pub fn fetch_ids(&self) -> Vec<String> {
        self.connected_ids
            .iter()
            .filter(|id| !is_custom(id))
            .cloned()
            .collect()
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
                super::super::Model { id: "free-1".to_string(), free: true },
                super::super::Model { id: "paid-1".to_string(), free: false },
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
}