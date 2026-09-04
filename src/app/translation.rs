use std::collections::HashMap;
use iced::Task;
use easyscanlate_model::EntryId;
use easyscanlate_ui::UiState;
use easyscanlate_ui::translation as translation;
#[allow(unused_imports)]
pub use easyscanlate_ui::translation::{
    catalog_provider, default_hidden_ids, default_hidden_ids_for_models, fetch_local_provider,
    fetch_local_providers, fetch_providers, file_tag, is_custom, is_local, profile_name,
    provider_name, usable_models, validate_connection_for, Connection, Model, Provider,
    Session, LANGUAGES,
};
#[cfg(not(feature = "translation"))]
pub use easyscanlate_ui::translation::FAKE_PROVIDER;

use super::{App, Message};

/// Re-syncs the translation session's persisted mirrors from the shared
/// settings store: connections, free-only filter and hidden models. The
/// current selection is kept (`sync` falls back when it dropped out); used
/// at boot and on the single [`UiEvent::SettingsChanged`] announcement.
pub fn sync_tx_from_store(app: &mut App) {
    easyscanlate_settings::get(|s| {
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
    // Seed default hidden (older family members) for newly fetched providers
    // where the user has no entry yet: hidden via Manage Models instead of
    // being filtered out. Free and `*-latest` stay visible.
    let mut to_seed: Vec<(String, std::collections::BTreeSet<String>)> = Vec::new();
    for (id, provider) in app.tx.fetched.iter() {
        let has_entry = easyscanlate_settings::get(|s| s.hidden_models.contains_key(id));
        if !has_entry {
            let default = translation::default_hidden_ids_for_models(&provider.models);
            if !default.is_empty() {
                to_seed.push((id.clone(), default));
            }
        }
    }
    if !to_seed.is_empty() {
        let _ = easyscanlate_settings::modify(|s| {
            for (id, default) in to_seed {
                s.hidden_models.entry(id).or_insert(default);
            }
        });
        // Pull seeded hidden back into session and sync models visibility
        easyscanlate_settings::get(|s| {
            app.tx.hidden_models = s.hidden_models.clone();
        });
        app.tx.sync_models();
    }
    Task::none()
}

pub(crate) fn placeholder_name(lang: &str) -> String {
    format!("{lang}(auto)")
}

fn capitalized_profile_name(lang: &str) -> String {
    placeholder_name(lang)
}

fn resolve_base_id(app: &App) -> Option<easyscanlate_model::ProfileId> {
    let tab = app.active_tab();
    if let Some(id) = tab.translate_base
        && tab.project.profiles.iter().any(|p| p.id == id) {
            return Some(id);
        }
    if tab.images.is_empty() {
        return None;
    }
    Some(tab.project.profiles.selected_id())
}

pub fn handle_translate(app: &mut App) -> Task<Message> {
    if app.active_state().is_bulk_busy() {
        app.active_tab_mut().status = "Wait for current task to finish.".to_string();
        return Task::none();
    }
    if !app.tx.is_connected() {
        app.active_tab_mut().status = "Connect a translation service in Settings first.".to_string();
        return Task::none();
    }
    // Ensure base/target initialized when entering translate without prior selection
    if app.active_tab_mut().translation_panel_mode == easyscanlate_ui::event::TranslationPanelMode::Translate && !app.active_tab_mut().images.is_empty() {
        if app.active_tab_mut().translate_base.is_none() {
            app.active_tab_mut().translate_base = Some(app.active_tab_mut().project.profiles.selected_id());
        }
        if let easyscanlate_ui::event::TargetProfileSelection::AutoPlaceholder(name) = app.active_tab_mut().translate_target.clone()
            && name != capitalized_profile_name(&app.active_tab_mut().translate_lang) {
                // keep as is; lang change handler already syncs
            }
    }
    let is_translate_mode = app.active_tab().translation_panel_mode == easyscanlate_ui::event::TranslationPanelMode::Translate;
    let base_id = if is_translate_mode { resolve_base_id(app) } else { None };
    let mut jobs: Vec<(usize, EntryId, String, String)> = Vec::new();
    {
        let tab = app.active_tab();
        for (index, image) in tab.images.iter().enumerate() {
            let image_id = image.image_id;
            let filename = tab
                .project
                .image(image_id)
                .map(|m| translation::file_tag(&m.path))
                .unwrap_or_default();
            for entry in tab.project.visible_for(image_id).collect::<Vec<_>>() {
                let text = if let Some(pid) = base_id {
                    tab.project
                        .resolved_text_for(pid, entry.id)
                        .unwrap_or(&entry.text)
                        .to_string()
                } else {
                    entry.text.clone()
                };
                jobs.push((index, entry.id, filename.clone(), text));
            }
        }
    }
    if jobs.is_empty() {
        app.active_tab_mut().status = "Run OCR first.".to_string();
        return Task::none();
    }
    app.active_tab_mut().translating = true;
    app.active_tab_mut().translate_anim_phase = 0.0;
    let items: Vec<translation::TranslateItem> = jobs
        .iter()
        .map(|(_, id, filename, text)| translation::TranslateItem {
            filename: filename.clone(),
            id: id.0,
            text: text.clone(),
        })
        .collect();
    let target = app.active_tab_mut().translate_lang.clone();
    let (provider, api_key) = match app.tx.selected_provider() {
        Some(provider) => (provider, app.tx.selected_api_key()),
        None => {
            app.active_tab_mut().translating = false;
            app.active_tab_mut().status = "Translation service is not connected.".to_string();
            return Task::none();
        }
    };
    let model = app.tx.selected_model.clone();
    app.active_tab_mut().status = format!(
        "Translating {} line(s) to {} via {model} ({})...",
        jobs.len(),
        app.active_tab_mut().translate_lang,
        provider.name
    );
    let tid = app.active_tab().id;
    Task::perform(
        async move {
            let result =
                translation::translate_all(&items, &target, &provider, &model, api_key)
                    .await;
            (jobs, result)
        },
        move |(jobs, result)| Message::Tab(tid, crate::app::TabMessage::TranslateFinished(jobs, result)),
    )
}

pub fn handle_retranslate_entry(app: &mut App, index: usize, entry_id: EntryId) -> Task<Message> {
    if app.active_state().is_bulk_busy() {
        app.active_tab_mut().status = "Wait for current task to finish.".to_string();
        return Task::none();
    }
    let (text, filename, context_items) = {
        // Validate image/entry existence via immutable tab
        let (image_id, entry_text, entry_image_id) = {
            let tab = app.active_tab();
            let Some(image) = tab.images.get(index) else {
                let _ = tab;
                app.active_tab_mut().status = "That result no longer exists.".to_string();
                return Task::none();
            };
            let image_id = image.image_id;
            let Some(entry) = tab.project.entry(entry_id) else {
                let _ = tab;
                app.active_tab_mut().status = "That result no longer exists.".to_string();
                return Task::none();
            };
            (image_id, entry.text.clone(), entry.image_id)
        };
        if entry_image_id != image_id {
            app.active_tab_mut().status = "That result no longer exists.".to_string();
            return Task::none();
        }
        if !app.tx.is_connected() {
            app.active_tab_mut().status = "Connect a translation service in Settings first.".to_string();
            return Task::none();
        }
        let filename = {
            let tab = app.active_tab();
            tab.project
            .image(image_id)
            .map(|m| translation::file_tag(&m.path))
            .unwrap_or_default()
        };
        // In Translate mode, base profile's text is the source, with context from base as well
        let (text, context_items) = if app.active_tab().translation_panel_mode == easyscanlate_ui::event::TranslationPanelMode::Translate {
            let base_id = resolve_base_id(app);
            let (txt, ctx) = {
                let tab = app.active_tab();
                let txt = base_id
                    .and_then(|pid| tab.project.resolved_text_for(pid, entry_id))
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| entry_text.clone());
                let ctx: Vec<translation::TranslateItem> = tab
                    .project
                    .visible_for(image_id)
                    .map(|e| {
                        let t = base_id
                            .and_then(|pid| tab.project.resolved_text_for(pid, e.id))
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| e.text.clone());
                        translation::TranslateItem { filename: filename.clone(), id: e.id.0, text: t }
                    })
                    .collect();
                (txt, ctx)
            };
            (txt, ctx)
        } else {
            let ctx: Vec<translation::TranslateItem> = {
                let tab = app.active_tab();
                tab
                .project
                .visible_for(image_id)
                .map(|e| translation::TranslateItem {
                    filename: filename.clone(),
                    id: e.id.0,
                    text: e.text.clone(),
                })
                .collect()
            };
            (entry_text.clone(), ctx)
        };
        (text, filename, context_items)
    };
    let target = app.active_tab_mut().translate_lang.clone();
    let (provider, api_key) = match app.tx.selected_provider() {
        Some(provider) => (provider, app.tx.selected_api_key()),
        None => {
            app.active_tab_mut().status = "Translation service is not connected.".to_string();
            return Task::none();
        }
    };
    let model = app.tx.selected_model.clone();
    app.active_tab_mut().translating = true;
    app.active_tab_mut().translate_anim_phase = 0.0;
    app.active_tab_mut().status = format!(
        "Retranslating 1 line to {} via {model} ({})...",
        app.active_tab_mut().translate_lang, provider.name
    );
    let tid = app.active_tab().id;
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
        move |(job, result)| Message::Tab(tid, crate::app::TabMessage::RetranslateFinished(job, result)),
    )
}

pub fn handle_model_select(app: &mut App, provider: String, model: String) -> Task<Message> {
    app.tx.select_model(provider.clone(), model);
    let _ = easyscanlate_settings::modify(|s| s.last_provider = Some(provider));
    Task::none()
}

pub fn handle_connect(app: &mut App, provider_id: String) -> Task<Message> {
    use easyscanlate_ui::ConnectModal;
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
    let _ = easyscanlate_settings::modify(|s| {
        s.connections.remove(&provider_id);
        if s.last_provider.as_deref() == Some(provider_id.as_str()) {
            s.last_provider = None;
        }
    });
    app.tx.disconnect(&provider_id);
    app.active_tab_mut().status = format!(
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
        app.connect_modal = Some(easyscanlate_ui::ConnectModal {
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
    let _ = easyscanlate_settings::modify(|s| {
        s.connections.insert(id.clone(), connection.clone());
        s.last_provider = Some(id.clone());
    });
    app.tx.connect(id.clone(), connection);
    app.active_tab_mut().status = format!("Connected {}.", translation::provider_name(&id));
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

pub fn handle_panel_mode(app: &mut App, mode: easyscanlate_ui::event::TranslationPanelMode) -> Task<Message> {
    if mode == easyscanlate_ui::event::TranslationPanelMode::Translate && app.active_tab_mut().translation_panel_mode != easyscanlate_ui::event::TranslationPanelMode::Translate {
        if app.active_tab_mut().translate_base.is_none() && !app.active_tab_mut().images.is_empty() {
            app.active_tab_mut().translate_base = Some(app.active_tab_mut().project.profiles.selected_id());
        }
        if let easyscanlate_ui::event::TargetProfileSelection::AutoPlaceholder(_) = app.active_tab_mut().translate_target.clone() {
            app.active_tab_mut().translate_target = easyscanlate_ui::event::TargetProfileSelection::AutoPlaceholder(placeholder_name(&app.active_tab_mut().translate_lang));
        }
        if let easyscanlate_ui::event::TargetProfileSelection::AutoPlaceholder(name) = app.active_tab_mut().translate_target.clone()
            && let Some(id) = app.active_tab_mut().project.profiles.find_by_name(&name) {
                let base = app.active_tab_mut().translate_base.or_else(|| Some(app.active_tab_mut().project.profiles.selected_id()));
                if Some(id) != base {
                    app.active_tab_mut().translate_target = easyscanlate_ui::event::TargetProfileSelection::Existing(id);
                }
            }
        if let (Some(base), easyscanlate_ui::event::TargetProfileSelection::Existing(tid)) = (app.active_tab_mut().translate_base, app.active_tab_mut().translate_target.clone())
            && base == tid {
                app.active_tab_mut().translate_target = easyscanlate_ui::event::TargetProfileSelection::AutoPlaceholder(placeholder_name(&app.active_tab_mut().translate_lang));
            }
    }
    app.active_tab_mut().translation_panel_mode = mode;
    app.active_tab_mut().status = match mode {
        easyscanlate_ui::event::TranslationPanelMode::Edit => "Edit mode: single profile.".to_string(),
        easyscanlate_ui::event::TranslationPanelMode::Translate => "Translate mode: base → target.".to_string(),
    };
    if app.active_tab_mut().editing.is_some() && app.active_tab_mut().editing_origin == easyscanlate_ui::event::EditOrigin::Panel {
        crate::app::edit::clear_editing(app);
    }
    Task::none()
}

pub fn handle_base_select(app: &mut App, id: easyscanlate_model::ProfileId) -> Task<Message> {
    if app.active_tab_mut().images.is_empty() {
        return Task::none();
    }
    let exists = app.active_tab_mut().project.profiles.iter().any(|p| p.id == id);
    if !exists {
        return Task::none();
    }
    if let easyscanlate_ui::event::TargetProfileSelection::Existing(tid) = app.active_tab_mut().translate_target.clone()
        && tid == id {
            app.active_tab_mut().status = "Base and target must differ.".to_string();
            return Task::none();
        }
    if let easyscanlate_ui::event::TargetProfileSelection::AutoPlaceholder(name) = app.active_tab().translate_target.clone() {
        let bprof_name = app.active_tab().project.profiles.iter().find(|p| p.id == id).map(|p| p.name.clone());
        if let Some(bname) = bprof_name && bname == name { app.active_tab_mut().status = "Base and target must differ.".to_string(); return Task::none(); }
    }
    app.active_tab_mut().translate_base = Some(id);
    let name = app.active_tab_mut().project.profiles.iter().find(|p| p.id == id).map(|p| p.name.clone()).unwrap_or_default();
    app.active_tab_mut().status = format!("Base: {name}");
    Task::none()
}

pub fn handle_target_select(app: &mut App, sel: easyscanlate_ui::event::TargetProfileSelection) -> Task<Message> {
    if app.active_tab().images.is_empty() {
        return Task::none();
    }
    let base = {
        let tab = app.active_tab();
        tab.translate_base.or_else(|| Some(tab.project.profiles.selected_id()))
    };
    match &sel {
        easyscanlate_ui::event::TargetProfileSelection::Existing(id) => {
            if Some(*id) == base {
                app.active_tab_mut().status = "Base and target must differ.".to_string();
                return Task::none();
            }
            let exists = app.active_tab().project.profiles.iter().any(|p| p.id == *id);
            if !exists { return Task::none(); }
        }
        easyscanlate_ui::event::TargetProfileSelection::AutoPlaceholder(name) => {
            if let Some(b) = base {
                let bprof_name = app.active_tab().project.profiles.iter().find(|p| p.id == b).map(|p| p.name.clone());
                if let Some(bname) = bprof_name && &bname == name { app.active_tab_mut().status = "Base and target must differ.".to_string(); return Task::none(); }
            }
        }
    }
    let resolved = match sel.clone() {
        easyscanlate_ui::event::TargetProfileSelection::AutoPlaceholder(name) => {
            let found = app.active_tab().project.profiles.find_by_name(&name);
            if let Some(id) = found { if Some(id) != base { easyscanlate_ui::event::TargetProfileSelection::Existing(id) } else { easyscanlate_ui::event::TargetProfileSelection::AutoPlaceholder(name) } } else { easyscanlate_ui::event::TargetProfileSelection::AutoPlaceholder(name) }
        }
        other => other,
    };
    app.active_tab_mut().translate_target = resolved.clone();
    let label = match resolved {
        easyscanlate_ui::event::TargetProfileSelection::Existing(id) => app.active_tab().project.profiles.iter().find(|p| p.id == id).map(|p| p.name.clone()).unwrap_or_default(),
        easyscanlate_ui::event::TargetProfileSelection::AutoPlaceholder(n) => n,
    };
    app.active_tab_mut().status = format!("Target: {label}");
    Task::none()
}

pub fn handle_lang(app: &mut App, lang: String) -> Task<Message> {
    app.active_tab_mut().translate_lang = lang.clone();
    if let easyscanlate_ui::event::TargetProfileSelection::AutoPlaceholder(_) = app.active_tab_mut().translate_target.clone() {
        let new_name = placeholder_name(&lang);
        if !app.active_tab_mut().images.is_empty() {
            if let Some(id) = app.active_tab_mut().project.profiles.find_by_name(&new_name) {
                let base = app.active_tab_mut().translate_base.or_else(|| Some(app.active_tab_mut().project.profiles.selected_id()));
                if Some(id) != base {
                    app.active_tab_mut().translate_target = easyscanlate_ui::event::TargetProfileSelection::Existing(id);
                } else {
                    app.active_tab_mut().translate_target = easyscanlate_ui::event::TargetProfileSelection::AutoPlaceholder(new_name);
                }
            } else {
                app.active_tab_mut().translate_target = easyscanlate_ui::event::TargetProfileSelection::AutoPlaceholder(new_name);
            }
        } else {
            app.active_tab_mut().translate_target = easyscanlate_ui::event::TargetProfileSelection::AutoPlaceholder(new_name);
        }
    }
    Task::none()
}

pub fn handle_translate_finished(app: &mut App, tab_id: crate::app::tab::TabId, jobs: Vec<(usize, EntryId, String, String)>, result: Result<Vec<String>, String>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    app.tabs[idx].translating = false;
    app.tabs[idx].translate_anim_phase = 0.0;
    match result {
        Ok(translations) => {
            let is_translate_mode = app.tabs[idx].translation_panel_mode == easyscanlate_ui::event::TranslationPanelMode::Translate;
            let profile_name = if is_translate_mode {
                let tab = &app.tabs[idx];
                match &tab.translate_target {
                    easyscanlate_ui::event::TargetProfileSelection::Existing(id) => tab.project.profiles.iter().find(|p| &p.id == id).map(|p| p.name.clone()).unwrap_or_else(|| format!("{}(auto)", tab.translate_lang)),
                    easyscanlate_ui::event::TargetProfileSelection::AutoPlaceholder(name) => name.clone(),
                }
            } else {
                let lang = app.tabs[idx].translate_lang.clone();
                format!("{lang}(auto)")
            };
            if translations.len() != jobs.len() {
                let mut saved = 0usize;
                for ((_, entry_id, _path, _text), translation) in jobs.iter().zip(translations.iter()) {
                    if translation.is_empty() { continue; }
                    let evs = {
                        let tab = &mut app.tabs[idx];
                        let (_, evs) = tab.project.store_translation_with_event(&profile_name, *entry_id, Some(translation.clone()));
                        evs
                    };
                    for ev in evs { crate::app::handle_model_event(&mut app.tabs[idx], ev); }
                    saved += 1;
                }
                app.tabs[idx].status = format!("Translated {saved} of {} line(s) into '{profile_name}' (count mismatch, partial).", jobs.len());
            } else {
                let mut saved = 0usize;
                let mut skipped = 0usize;
                for ((_, entry_id, _path, _text), translation) in jobs.iter().zip(translations.iter()) {
                    if translation.is_empty() { skipped += 1; continue; }
                    let evs = {
                        let tab = &mut app.tabs[idx];
                        let (_, evs) = tab.project.store_translation_with_event(&profile_name, *entry_id, Some(translation.clone()));
                        evs
                    };
                    for ev in evs { crate::app::handle_model_event(&mut app.tabs[idx], ev); }
                    saved += 1;
                }
                if skipped > 0 {
                    app.tabs[idx].status = format!("Translated {saved} of {} line(s) into '{profile_name}' ({skipped} still missing after retry, skipped).", jobs.len());
                } else {
                    app.tabs[idx].status = format!("Translated {saved} line(s) into '{profile_name}'.");
                }
            }
            if is_translate_mode {
                let target_name = profile_name.clone();
                if let easyscanlate_ui::event::TargetProfileSelection::AutoPlaceholder(name) = app.tabs[idx].translate_target.clone()
                    && name == target_name
                        && let Some(id) = app.tabs[idx].project.profiles.find_by_name(&name) {
                            let base = app.tabs[idx].translate_base.or_else(|| Some(app.tabs[idx].project.profiles.selected_id()));
                            if Some(id) != base {
                                app.tabs[idx].translate_target = easyscanlate_ui::event::TargetProfileSelection::Existing(id);
                            }
                        }
            }
        }
        Err(e) => { app.tabs[idx].status = e; }
    }
    Task::none()
}
pub fn handle_retranslate_finished(app: &mut App, tab_id: crate::app::tab::TabId, index: usize, entry_id: EntryId, result: Result<String, String>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    app.tabs[idx].translating = false;
    app.tabs[idx].translate_anim_phase = 0.0;
    match result {
        Ok(mut text) => {
            if text.len() >= 2 {
                let quoted = (text.starts_with('"') && text.ends_with('"')) || (text.starts_with('\'') && text.ends_with('\''));
                if quoted { text = text[1..text.len()-1].to_string(); }
            }
            if app.tabs[idx].translation_panel_mode == easyscanlate_ui::event::TranslationPanelMode::Translate {
                let target_name = {
                    let tab = &app.tabs[idx];
                    match &tab.translate_target {
                        easyscanlate_ui::event::TargetProfileSelection::Existing(id) => tab.project.profiles.iter().find(|p| &p.id == id).map(|p| p.name.clone()).unwrap_or_else(|| format!("{}(auto)", tab.translate_lang)),
                        easyscanlate_ui::event::TargetProfileSelection::AutoPlaceholder(name) => name.clone(),
                    }
                };
                if index >= app.tabs[idx].images.len() {
                    app.tabs[idx].status = "Retranslated, but that image is gone.".to_string();
                    return Task::none();
                }
                let equals_original = app.tabs[idx].project.entry_including_deleted(entry_id).is_some_and(|entry| entry.text == text);
                let stored = if equals_original { None } else { Some(text) };
                let evs = {
                    let tab = &mut app.tabs[idx];
                    let (_target_id, evs) = tab.project.store_translation_with_event(&target_name, entry_id, stored.clone());
                    evs
                };
                for ev in evs { crate::app::handle_model_event(&mut app.tabs[idx], ev); }
                if let easyscanlate_ui::event::TargetProfileSelection::AutoPlaceholder(name) = app.tabs[idx].translate_target.clone()
                    && name == target_name
                        && let Some(id) = app.tabs[idx].project.profiles.find_by_name(&name) {
                            let base = app.tabs[idx].translate_base.or_else(|| Some(app.tabs[idx].project.profiles.selected_id()));
                            if Some(id) != base {
                                app.tabs[idx].translate_target = easyscanlate_ui::event::TargetProfileSelection::Existing(id);
                            }
                        }
                app.tabs[idx].status = format!("Retranslated 1 line into '{target_name}'.");
                return Task::none();
            }
            if index >= app.tabs[idx].images.len() {
                app.tabs[idx].status = "Retranslated, but that image is gone.".to_string();
                return Task::none();
            }
            let equals_original = app.tabs[idx].project.entry_including_deleted(entry_id).is_some_and(|entry| entry.text == text);
            let stored = if equals_original { None } else { Some(text) };
            let forked = {
                let tab = &mut app.tabs[idx];
                let (name_opt, evs) = match tab.project.fork_for_edit_with_event() { Some((n, evs)) => (Some(n), evs), None => (None, Vec::new()) };
                for ev in evs { crate::app::handle_model_event(tab, ev); }
                name_opt
            };
            let ev = {
                let tab = &mut app.tabs[idx];
                tab.project.set_translation_with_event(entry_id, stored.clone())
            };
            crate::app::handle_model_event(&mut app.tabs[idx], ev);
            let label = forked.unwrap_or_else(|| app.tabs[idx].project.profiles.selected().name.clone());
            app.tabs[idx].status = format!("Retranslated 1 line into '{label}'.");
        }
        Err(e) => { app.tabs[idx].status = e; }
    }
    Task::none()
}
