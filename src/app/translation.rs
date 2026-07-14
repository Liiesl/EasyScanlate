use std::collections::HashMap;
use iced::Task;
use scanlateit_model::EntryId;
use scanlateit_ui::translation as translation;
pub use scanlateit_ui::translation::{
    catalog_provider, fetch_local_provider, fetch_local_providers, fetch_providers, file_tag,
    is_custom, is_local, profile_name, provider_name, validate_connection_for, Connection,
    Provider, Session, LANGUAGES,
};
#[cfg(not(feature = "translation"))]
pub use scanlateit_ui::translation::FAKE_PROVIDER;

use super::{App, Message};

/// Re-syncs the translation session's persisted mirrors from the shared
/// settings store: connections, free-only filter and hidden models. The
/// current selection is kept (`sync` falls back when it dropped out); used
/// at boot and on the single [`UiEvent::SettingsChanged`] announcement.
pub fn sync_tx_from_store(app: &mut App) {
    scanlateit_settings::get(|s| {
        app.tx.connections = s.connections.clone();
        app.tx.free_only = s.free_models_only;
        app.tx.hidden_models = s.hidden_models.clone();
    });
    app.tx.sync();
}

pub fn handle_fetch_models(app: &mut App) -> Task<Message> {
    let ids = app.tx.fetch_ids();
    if ids.is_empty() {
        Task::none()
    } else {
        Task::perform(translation::fetch_providers(ids), Message::ModelsFetched)
    }
}

pub fn handle_models_fetched(app: &mut App, providers: HashMap<String, translation::Provider>) -> Task<Message> {
    app.tx.on_fetched(providers);
    Task::none()
}

pub fn handle_translate(app: &mut App) -> Task<Message> {
    if app.translating || app.running {
        return Task::none();
    }
    if !app.tx.is_connected() {
        app.status = "Connect a translation service in Settings first.".to_string();
        return Task::none();
    }
    let jobs: Vec<(usize, EntryId, String, String)> = app
        .images
        .iter()
        .enumerate()
        .flat_map(|(index, image)| {
            let filename = translation::file_tag(&image.path);
            image
                .project
                .ocr
                .visible()
                .map(move |entry| {
                    (
                        index,
                        entry.id,
                        filename.clone(),
                        entry.text.clone(),
                    )
                })
        })
        .collect();
    if jobs.is_empty() {
        app.status = "Run OCR first.".to_string();
        return Task::none();
    }
    app.translating = true;
    let items: Vec<translation::TranslateItem> = jobs
        .iter()
        .map(|(_, id, filename, text)| translation::TranslateItem {
            filename: filename.clone(),
            id: id.0,
            text: text.clone(),
        })
        .collect();
    let target = app.translate_lang.clone();
    let (provider, api_key) = match app.tx.selected_provider() {
        Some(provider) => (provider, app.tx.selected_api_key()),
        None => {
            app.translating = false;
            app.status = "Translation service is not connected.".to_string();
            return Task::none();
        }
    };
    let model = app.tx.selected_model.clone();
    app.status = format!(
        "Translating {} line(s) to {} via {model} ({})...",
        jobs.len(),
        app.translate_lang,
        provider.name
    );
    Task::perform(
        async move {
            let result =
                translation::translate_all(&items, &target, &provider, &model, api_key)
                    .await;
            (jobs, result)
        },
        |(jobs, result)| Message::TranslateFinished(jobs, result),
    )
}

pub fn handle_translate_finished(
    app: &mut App,
    jobs: Vec<(usize, EntryId, String, String)>,
    result: Result<Vec<String>, String>,
) -> Task<Message> {
    app.translating = false;
    match result {
        Ok(translations) => {
            let profile_name = translation::profile_name(&app.translate_lang);
            if translations.len() != jobs.len() {
                let mut saved = 0usize;
                for ((image_index, entry_id, _path, _text), translation) in
                    jobs.iter().zip(translations.iter())
                {
                    if translation.is_empty() {
                        continue;
                    }
                    let image = &mut app.images[*image_index];
                    image
                        .project
                        .store_translation(&profile_name, *entry_id, Some(translation.clone()));
                    saved += 1;
                }
                app.status = format!(
                    "Translated {saved} of {} line(s) into '{profile_name}' (count mismatch, partial).",
                    jobs.len()
                );
            } else {
                let mut saved = 0usize;
                let mut skipped = 0usize;
                for ((image_index, entry_id, _path, _text), translation) in
                    jobs.iter().zip(translations.iter())
                {
                    if translation.is_empty() {
                        skipped += 1;
                        continue;
                    }
                    let image = &mut app.images[*image_index];
                    image
                        .project
                        .store_translation(&profile_name, *entry_id, Some(translation.clone()));
                    saved += 1;
                }
                if skipped > 0 {
                    app.status = format!(
                        "Translated {saved} of {} line(s) into '{profile_name}' ({skipped} still missing after retry, skipped).",
                        jobs.len()
                    );
                } else {
                    app.status = format!(
                        "Translated {saved} line(s) into '{profile_name}'."
                    );
                }
            }
        }
        Err(e) => {
            app.status = e;
        }
    }
    Task::none()
}

pub fn handle_retranslate_finished(
    app: &mut App,
    index: usize,
    entry_id: EntryId,
    result: Result<String, String>,
) -> Task<Message> {
    app.translating = false;
    match result {
        Ok(mut text) => {
            if text.len() >= 2 {
                let quoted = (text.starts_with('"') && text.ends_with('"'))
                    || (text.starts_with('\'') && text.ends_with('\''));
                if quoted {
                    text = text[1..text.len() - 1].to_string();
                }
            }
            let Some(image) = app.images.get_mut(index) else {
                app.status = "Retranslated, but that image is gone.".to_string();
                return Task::none();
            };
            let equals_original = image
                .project
                .ocr
                .get(entry_id)
                .is_some_and(|entry| entry.text == text);
            let stored = if equals_original { None } else { Some(text) };
            let forked_name = image.project.profiles.fork_for_edit();
            image
                .project
                .profiles
                .selected_mut()
                .set_translation(entry_id, stored);
            let label = forked_name
                .unwrap_or_else(|| image.project.profiles.selected().name.clone());
            app.status = format!("Retranslated 1 line into '{label}'.");
        }
        Err(e) => {
            app.status = e;
        }
    }
    Task::none()
}

pub fn handle_retranslate_entry(app: &mut App, index: usize, entry_id: EntryId) -> Task<Message> {
    if app.translating || app.running {
        return Task::none();
    }
    let (text, filename, context_items) = {
        let Some(image) = app.images.get(index) else {
            app.status = "That result no longer exists.".to_string();
            return Task::none();
        };
        let Some(entry) = image.project.ocr.get(entry_id) else {
            app.status = "That result no longer exists.".to_string();
            return Task::none();
        };
        if !app.tx.is_connected() {
            app.status = "Connect a translation service in Settings first.".to_string();
            return Task::none();
        }
        let filename = translation::file_tag(&image.path);
        let context_items: Vec<translation::TranslateItem> = image
            .project
            .ocr
            .visible()
            .map(|e| translation::TranslateItem {
                filename: filename.clone(),
                id: e.id.0,
                text: e.text.clone(),
            })
            .collect();
        (entry.text.clone(), filename, context_items)
    };
    let target = app.translate_lang.clone();
    let (provider, api_key) = match app.tx.selected_provider() {
        Some(provider) => (provider, app.tx.selected_api_key()),
        None => {
            app.status = "Translation service is not connected.".to_string();
            return Task::none();
        }
    };
    let model = app.tx.selected_model.clone();
    app.translating = true;
    app.status = format!(
        "Retranslating 1 line to {} via {model} ({})...",
        app.translate_lang, provider.name
    );
    Task::perform(
        async move {
            let result = translation::translate_one_with_context(
                &text,
                &target,
                &provider,
                &model,
                api_key,
                &context_items,
                entry_id.0,
                &filename,
            )
            .await;
            ((index, entry_id), result)
        },
        |(job, result)| Message::RetranslateFinished(job, result),
    )
}

pub fn handle_model_select(app: &mut App, provider: String, model: String) -> Task<Message> {
    app.tx.select_model(provider.clone(), model);
    let _ = scanlateit_settings::modify(|s| s.last_provider = Some(provider));
    Task::none()
}

pub fn handle_connect(app: &mut App, provider_id: String) -> Task<Message> {
    use scanlateit_ui::ConnectModal;
    let is_custom = translation::is_custom(&provider_id);
    let existing = app.tx.connections.get(&provider_id);
    app.connect_modal = Some(ConnectModal {
        provider_id,
        is_custom,
        api_key: existing.map(|c| c.api_key.clone()).unwrap_or_default(),
        base_url: existing
            .and_then(|c| c.base_url.clone())
            .unwrap_or_default(),
        model: existing.and_then(|c| c.model.clone()).unwrap_or_default(),
        error: None,
    });
    Task::none()
}

pub fn handle_disconnect(app: &mut App, provider_id: String) -> Task<Message> {
    let _ = scanlateit_settings::modify(|s| {
        s.connections.remove(&provider_id);
        if s.last_provider.as_deref() == Some(provider_id.as_str()) {
            s.last_provider = None;
        }
    });
    app.tx.disconnect(&provider_id);
    app.status = format!(
        "Disconnected {}. Its API key was removed.",
        translation::provider_name(&provider_id)
    );
    Task::none()
}

pub fn handle_connect_modal_key(app: &mut App, key: String) -> Task<Message> {
    if let Some(modal) = &mut app.connect_modal {
        modal.api_key = key;
        modal.error = None;
    }
    Task::none()
}

pub fn handle_connect_modal_base_url(app: &mut App, url: String) -> Task<Message> {
    if let Some(modal) = &mut app.connect_modal {
        modal.base_url = url;
        modal.error = None;
    }
    Task::none()
}

pub fn handle_connect_modal_model(app: &mut App, model: String) -> Task<Message> {
    if let Some(modal) = &mut app.connect_modal {
        modal.model = model;
        modal.error = None;
    }
    Task::none()
}

pub fn handle_connect_modal_submit(app: &mut App) -> Task<Message> {
    let Some(modal) = app.connect_modal.take() else {
        return Task::none();
    };
    if let Some(error) = translation::validate_connection_for(
        &modal.provider_id,
        &modal.api_key,
        &modal.base_url,
        &modal.model,
    ) {
        app.connect_modal = Some(scanlateit_ui::ConnectModal {
            error: Some(error),
            ..modal
        });
        return Task::none();
    }
    let id = modal.provider_id.clone();
    let is_local = translation::is_local(&id);
    let is_custom = translation::is_custom(&id);
    let base_url = modal.base_url.trim().to_string();
    let connection = translation::Connection {
        api_key: if is_local {
            id.clone()
        } else {
            modal.api_key.trim().to_string()
        },
        base_url: if is_local || is_custom {
            Some(base_url.clone())
        } else {
            None
        },
        model: if is_custom {
            Some(modal.model.trim().to_string())
        } else {
            None
        },
    };
    let _ = scanlateit_settings::modify(|s| {
        s.connections.insert(id.clone(), connection.clone());
        s.last_provider = Some(id.clone());
    });
    app.tx.connect(id.clone(), connection);
    app.status = format!("Connected {}.", translation::provider_name(&id));
    if is_custom {
        Task::none()
    } else if is_local {
        let base = base_url.clone();
        let fetch_id = id.clone();
        Task::perform(
            async move {
                let provider =
                    translation::fetch_local_provider(&fetch_id, &base).await;
                let mut map = HashMap::new();
                map.insert(fetch_id, provider);
                map
            },
            Message::ModelsFetched,
        )
    } else {
        Task::perform(
            translation::fetch_providers(vec![id]),
            Message::ModelsFetched,
        )
    }
}

pub fn handle_connect_modal_cancel(app: &mut App) -> Task<Message> {
    app.connect_modal = None;
    Task::none()
}
