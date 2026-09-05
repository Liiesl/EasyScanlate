use iced::Task;
use super::{App, Message};
use super::backdrop::{self, BackdropKind};
use super::translation;

pub fn handle_settings_open(app: &mut App) -> Task<Message> {
    // Capture-then-open: screenshot the clean base first so the blurred
    // backdrop never contains the modal itself. Reuse a cached handle for
    // instant reopen; without a window (tests) open flat immediately.
    if app.backdrop_frame.is_some() {
        backdrop::recrop(app, BackdropKind::Settings);
        app.settings_open = true;
        return Task::none();
    }
    if app.frame.primary_window().is_none() {
        app.settings_open = true;
        return Task::none();
    }
    if app.backdrop_pending.is_some() {
        return Task::none();
    }
    app.backdrop_pending = Some(BackdropKind::Settings);
    backdrop::capture_task(app, BackdropKind::Settings)
}

pub fn handle_settings_open_tab(app: &mut App, tab: easyscanlate_ui::event::SettingsTab) -> Task<Message> {
    app.settings_tab = tab;
    handle_settings_open(app)
}

pub fn handle_settings_close(app: &mut App) -> Task<Message> {
    app.settings_open = false;
    app.manage_models_open = false;
    app.connect_modal = None;
    app.settings_search.clear();
    // Drop the frozen frame so the next open re-captures fresh content.
    app.backdrop_blur = None;
    app.backdrop_frame = None;
    app.backdrop_pending = None;
    Task::none()
}

pub fn handle_settings_tab(app: &mut App, tab: easyscanlate_ui::event::SettingsTab) -> Task<Message> {
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

pub fn handle_setting_edit(app: &mut App, edit: easyscanlate_ui::event::SettingEdit) -> Task<Message> {
    // Compute default hidden sets before mutating the store (needs app.tx models).
    let reset_defaults: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> = match &edit {
        easyscanlate_ui::event::SettingEdit::HiddenModelsReset(provider) => {
            let mut m = std::collections::BTreeMap::new();
            let default = app.tx.default_hidden_for(provider);
            if !default.is_empty() {
                m.insert(provider.clone(), default);
            }
            m
        }
        easyscanlate_ui::event::SettingEdit::HiddenModelsResetAll => {
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
    let _ = easyscanlate_settings::modify(|s| match edit {
        easyscanlate_ui::event::SettingEdit::AuroraDarkMode(v) => s.aurora_is_dark = v,
        easyscanlate_ui::event::SettingEdit::AuroraBlobCount(v) => {
            s.aurora_blob_count = v.clamp(1, 5);
        }
        easyscanlate_ui::event::SettingEdit::AuroraSchema(v) => s.aurora_schema = v % 4,
        easyscanlate_ui::event::SettingEdit::HiddenModelsReset(provider) => {
            if let Some(default) = reset_defaults.get(&provider) {
                s.hidden_models.insert(provider, default.clone());
            } else {
                s.hidden_models.remove(&provider);
            }
        }
        easyscanlate_ui::event::SettingEdit::HiddenModelsResetAll => {
            s.hidden_models = reset_defaults;
        }
        easyscanlate_ui::event::SettingEdit::UiFontSize(v) => {
            s.ui_font_size = v.clamp(8, 30);
        }
        easyscanlate_ui::event::SettingEdit::AutoCheckUpdates(v) => {
            s.auto_check_updates = v;
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
    // Re-crop the existing capture when present (clean base without modals).
    // Otherwise capture first, then open on BackdropReady.
    if app.backdrop_frame.is_some() {
        backdrop::recrop(app, BackdropKind::ManageModels);
        app.manage_models_open = true;
        app.manage_models_search.clear();
        return Task::none();
    }
    if app.frame.primary_window().is_none() {
        app.manage_models_open = true;
        app.manage_models_search.clear();
        return Task::none();
    }
    if app.backdrop_pending.is_some() {
        return Task::none();
    }
    app.backdrop_pending = Some(BackdropKind::ManageModels);
    backdrop::capture_task(app, BackdropKind::ManageModels)
}

pub fn handle_manage_models_close(app: &mut App) -> Task<Message> {
    app.manage_models_open = false;
    app.manage_models_search.clear();
    // Keep the backdrop: Settings is still open underneath.
    app.backdrop_pending = None;
    Task::none()
}

pub fn handle_manage_models_search(app: &mut App, query: String) -> Task<Message> {
    app.manage_models_search = query;
    Task::none()
}
