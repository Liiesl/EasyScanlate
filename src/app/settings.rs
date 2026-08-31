use iced::Task;
use super::{App, Message};
use super::translation;

pub fn handle_settings_open(app: &mut App) -> Task<Message> {
    app.settings_open = true;
    Task::none()
}

pub fn handle_settings_open_tab(app: &mut App, tab: scanlateit_ui::event::SettingsTab) -> Task<Message> {
    app.settings_open = true;
    app.settings_tab = tab;
    Task::none()
}

pub fn handle_settings_close(app: &mut App) -> Task<Message> {
    app.settings_open = false;
    app.manage_models_open = false;
    app.connect_modal = None;
    app.settings_search.clear();
    Task::none()
}

pub fn handle_settings_tab(app: &mut App, tab: scanlateit_ui::event::SettingsTab) -> Task<Message> {
    app.settings_tab = tab;
    Task::none()
}

pub fn handle_settings_search(app: &mut App, query: String) -> Task<Message> {
    app.settings_search = query;
    Task::none()
}

pub fn handle_settings_changed(app: &mut App) -> Task<Message> {
    translation::sync_tx_from_store(app);
    app.active_tab_mut().status = "Settings saved.".to_string();
    Task::none()
}

pub fn handle_setting_edit(app: &mut App, edit: scanlateit_ui::event::SettingEdit) -> Task<Message> {
    // Compute default hidden sets before mutating the store (needs app.tx models).
    let reset_defaults: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> = match &edit {
        scanlateit_ui::event::SettingEdit::HiddenModelsReset(provider) => {
            let mut m = std::collections::BTreeMap::new();
            let default = app.tx.default_hidden_for(provider);
            if !default.is_empty() {
                m.insert(provider.clone(), default);
            }
            m
        }
        scanlateit_ui::event::SettingEdit::HiddenModelsResetAll => {
            let mut m = std::collections::BTreeMap::new();
            for id in app.tx.connected_ids.clone() {
                let default = app.tx.default_hidden_for(&id);
                if !default.is_empty() {
                    m.insert(id, default);
                }
            }
            m
        }
        _ => std::collections::BTreeMap::new(),
    };
    let _ = scanlateit_settings::modify(|s| match edit {
        scanlateit_ui::event::SettingEdit::AuroraDarkMode(v) => s.aurora_is_dark = v,
        scanlateit_ui::event::SettingEdit::AuroraBlobCount(v) => {
            s.aurora_blob_count = v.clamp(1, 5);
        }
        scanlateit_ui::event::SettingEdit::AuroraSchema(v) => s.aurora_schema = v % 4,
        scanlateit_ui::event::SettingEdit::HiddenModelsReset(provider) => {
            if let Some(default) = reset_defaults.get(&provider) {
                s.hidden_models.insert(provider, default.clone());
            } else {
                s.hidden_models.remove(&provider);
            }
        }
        scanlateit_ui::event::SettingEdit::HiddenModelsResetAll => {
            s.hidden_models = reset_defaults;
        }
        scanlateit_ui::event::SettingEdit::UiFontSize(v) => {
            s.ui_font_size = v.clamp(8, 30);
        }
    });
    translation::sync_tx_from_store(app);
    app.active_tab_mut().status = "Settings saved.".to_string();
    Task::none()
}

pub fn handle_open_url(app: &mut App, url: String) -> Task<Message> {
    if let Err(e) = open::that(&url) {
        eprintln!("[app] failed to open {url}: {e}");
        app.active_tab_mut().status = format!("Failed to open {url}: {e}");
    }
    Task::none()
}

pub fn handle_manage_models_open(app: &mut App) -> Task<Message> {
    app.manage_models_open = true;
    app.manage_models_search.clear();
    Task::none()
}

pub fn handle_manage_models_close(app: &mut App) -> Task<Message> {
    app.manage_models_open = false;
    app.manage_models_search.clear();
    Task::none()
}

pub fn handle_manage_models_search(app: &mut App, query: String) -> Task<Message> {
    app.manage_models_search = query;
    Task::none()
}
