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
    Task::none()
}

pub fn handle_settings_tab(app: &mut App, tab: scanlateit_ui::event::SettingsTab) -> Task<Message> {
    app.settings_tab = tab;
    Task::none()
}

pub fn handle_settings_changed(app: &mut App) -> Task<Message> {
    translation::sync_tx_from_store(app);
    app.status = "Settings saved.".to_string();
    Task::none()
}

pub fn handle_setting_edit(app: &mut App, edit: scanlateit_ui::event::SettingEdit) -> Task<Message> {
    let _ = scanlateit_settings::modify(|s| match edit {
        scanlateit_ui::event::SettingEdit::AuroraDarkMode(v) => s.aurora_is_dark = v,
        scanlateit_ui::event::SettingEdit::AuroraBlobCount(v) => {
            s.aurora_blob_count = v.clamp(1, 5);
        }
        scanlateit_ui::event::SettingEdit::AuroraSchema(v) => s.aurora_schema = v % 4,
        scanlateit_ui::event::SettingEdit::HiddenModelsReset(provider) => {
            s.hidden_models.remove(&provider);
        }
        scanlateit_ui::event::SettingEdit::HiddenModelsResetAll => {
            s.hidden_models.clear();
        }
        scanlateit_ui::event::SettingEdit::UiFontSize(v) => {
            s.ui_font_size = v.clamp(8, 30);
        }
    });
    translation::sync_tx_from_store(app);
    app.status = "Settings saved.".to_string();
    Task::none()
}

pub fn handle_open_url(app: &mut App, url: String) -> Task<Message> {
    if let Err(e) = open::that(&url) {
        eprintln!("[app] failed to open {url}: {e}");
        app.status = format!("Failed to open {url}: {e}");
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
