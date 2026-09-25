use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use iced::Task;

use easyscanlate_model::ProfileId;
use easyscanlate_mmtl::translation::TranslationLine;

use super::tab::{Tab, TabId};
use super::{App, Message, TabMessage};

/// Async result of reading + parsing a translation XML file:
/// `(file_profile_name, lines)`.
pub type TranslationImportResult = Result<(String, Vec<TranslationLine>), String>;

fn sanitize_filename(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ') {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    let t = out.trim().replace(' ', "_");
    if t.is_empty() {
        "profile".to_string()
    } else if t.len() > 80 {
        t[..80].to_string()
    } else {
        t
    }
}

/// Every visible line as the AI receives it: image order, `visible_for` per
/// image, OCR `<source>` plus the profile-resolved `<text>`.
fn collect_lines(tab: &Tab, pid: ProfileId) -> Option<(String, Vec<TranslationLine>)> {
    let profile = tab.project.profiles.iter().find(|p| p.id == pid)?;
    let pname = profile.name.clone();
    let mut lines = Vec::new();
    for image in &tab.images {
        let image_id = image.image_id;
        let filename = tab
            .project
            .image(image_id)
            .map(|m| super::translation::file_tag(&m.path))
            .unwrap_or_default();
        for entry in tab.project.visible_for(image_id).collect::<Vec<_>>() {
            let text = tab
                .project
                .resolved_text_for(pid, entry.id)
                .unwrap_or(&entry.text)
                .to_string();
            lines.push(TranslationLine {
                id: entry.id.0,
                file: filename.clone(),
                source: entry.text.clone(),
                text,
            });
        }
    }
    Some((pname, lines))
}

fn resolve_export(app: &App) -> Option<(ProfileId, String, usize, usize)> {
    let tab = app.active_tab();
    if !tab.is_project() {
        return None;
    }
    let pid = app
        .adv_export_profile
        .filter(|id| tab.project.profiles.iter().any(|p| p.id == *id))
        .unwrap_or_else(|| tab.project.profiles.selected_id());
    let profile = tab.project.profiles.iter().find(|p| p.id == pid)?;
    let name = profile.name.clone();
    let total = tab.project.visible_entries().count();
    let translated = tab
        .project
        .visible_entries()
        .filter(|e| profile.translation_of(e.id).is_some())
        .count();
    Some((pid, name, total, translated))
}

pub fn handle_export_profile(app: &mut App, id: ProfileId) -> Task<Message> {
    let exists = app.active_tab().project.profiles.iter().any(|p| p.id == id);
    if exists {
        app.adv_export_profile = Some(id);
    }
    Task::none()
}

pub fn handle_import_target(app: &mut App, id: ProfileId) -> Task<Message> {
    let exists = app.active_tab().project.profiles.iter().any(|p| p.id == id);
    if exists {
        app.adv_import_target = Some(id);
    }
    Task::none()
}

pub fn handle_import_name(app: &mut App, name: String) -> Task<Message> {
    app.adv_import_name = name;
    Task::none()
}

pub fn handle_export(app: &mut App) -> Task<Message> {
    if !app.active_tab().is_project() {
        app.active_tab_mut().status = "Open a project first.".to_string();
        return Task::none();
    }
    let Some((pid, pname, total, _translated)) = resolve_export(app) else {
        app.active_tab_mut().status = "Open a project first.".to_string();
        return Task::none();
    };
    if total == 0 {
        app.active_tab_mut().status = "Run OCR first.".to_string();
        return Task::none();
    }
    let _ = pname;
    let tid = app.active_tab().id;
    let title = sanitize_filename(&app.active_tab().title);
    let profile_part = sanitize_filename(
        &app.active_tab()
            .project
            .profiles
            .iter()
            .find(|p| p.id == pid)
            .map(|p| p.name.clone())
            .unwrap_or_default(),
    );
    let default_name = format!("{title}_{profile_part}.xml");
    Task::perform(
        async move {
            let file = rfd::AsyncFileDialog::new()
                .add_filter("Translation XML (.xml)", &["xml"])
                .set_file_name(&default_name)
                .save_file()
                .await;
            file.map(|f| f.path().to_string_lossy().to_string())
        },
        move |picked| Message::Tab(tid, TabMessage::TranslationExportPicked { profile: pid, path: picked }),
    )
}

pub fn handle_export_picked(
    app: &mut App,
    tab_id: TabId,
    profile: ProfileId,
    picked: Option<String>,
) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) {
        Some(i) => i,
        None => return Task::none(),
    };
    let Some(path_str) = picked else {
        app.tabs[idx].status = "Export cancelled.".to_string();
        return Task::none();
    };
    if !app.tabs[idx].is_project() {
        app.tabs[idx].status = "Open a project first.".to_string();
        return Task::none();
    }
    let pid = if app.tabs[idx].project.profiles.iter().any(|p| p.id == profile) {
        profile
    } else {
        app.tabs[idx].project.profiles.selected_id()
    };
    let Some((pname, lines)) = collect_lines(&app.tabs[idx], pid) else {
        app.tabs[idx].status = "Export failed: profile is gone.".to_string();
        return Task::none();
    };
    if lines.is_empty() {
        app.tabs[idx].status = "Run OCR first.".to_string();
        return Task::none();
    }
    let path = PathBuf::from(path_str);
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                let xml = easyscanlate_mmtl::translation::to_xml_string(&pname, &lines)?;
                std::fs::write(&path, xml).map_err(|e| e.to_string())?;
                Ok(format!(
                    "Exported {} line(s) of “{pname}” to {}",
                    lines.len(),
                    path.display()
                ))
            })
            .await
            .unwrap_or_else(|e| Err(format!("export task failed: {e}")))
        },
        move |res| Message::Tab(tab_id, TabMessage::TranslationExported(res)),
    )
}

pub fn handle_exported(app: &mut App, tab_id: TabId, result: Result<String, String>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) {
        Some(i) => i,
        None => return Task::none(),
    };
    match result {
        Ok(msg) => app.tabs[idx].status = msg,
        Err(e) => app.tabs[idx].status = format!("Export failed: {e}"),
    }
    Task::none()
}

pub fn handle_import(app: &mut App) -> Task<Message> {
    if !app.active_tab().is_project() {
        app.active_tab_mut().status = "Open a project first.".to_string();
        return Task::none();
    }
    let tid = app.active_tab().id;
    let target = app.adv_import_target;
    let new_name = app.adv_import_name.clone();
    Task::perform(
        async move {
            let file = rfd::AsyncFileDialog::new()
                .add_filter("Translation XML (.xml)", &["xml"])
                .pick_file()
                .await;
            file.map(|f| f.path().to_string_lossy().to_string())
        },
        move |picked| {
            Message::Tab(
                tid,
                TabMessage::TranslationImportPicked { target, new_name: new_name.clone(), path: picked },
            )
        },
    )
}

pub fn handle_import_picked(
    app: &mut App,
    tab_id: TabId,
    target: Option<ProfileId>,
    new_name: String,
    picked: Option<String>,
) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) {
        Some(i) => i,
        None => return Task::none(),
    };
    let Some(path_str) = picked else {
        app.tabs[idx].status = "Import cancelled.".to_string();
        return Task::none();
    };
    let path = PathBuf::from(path_str);
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                easyscanlate_mmtl::translation::from_xml_str(&text)
            })
            .await
            .unwrap_or_else(|e| Err(format!("import task failed: {e}")))
        },
        move |result| {
            Message::Tab(
                tab_id,
                TabMessage::TranslationImportLoaded { target, new_name: new_name.clone(), result },
            )
        },
    )
}

pub fn handle_import_loaded(
    app: &mut App,
    tab_id: TabId,
    target: Option<ProfileId>,
    new_name: String,
    result: TranslationImportResult,
) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) {
        Some(i) => i,
        None => return Task::none(),
    };
    if !app.tabs[idx].is_project() {
        app.tabs[idx].status = "Open a project first.".to_string();
        return Task::none();
    }
    let (file_profile, lines) = match result {
        Ok(v) => v,
        Err(e) => {
            app.tabs[idx].status = format!("Import failed: {e}");
            return Task::none();
        }
    };
    let trimmed = new_name.trim().to_string();
    let mut target_name = if !trimmed.is_empty() {
        trimmed
    } else if let Some(tid) = target
        && let Some(p) = app.tabs[idx].project.profiles.iter().find(|p| p.id == tid)
    {
        p.name.clone()
    } else {
        file_profile.clone()
    };
    if target_name.trim().is_empty() {
        app.tabs[idx].status = "Import failed: pick a profile or enter a name.".to_string();
        return Task::none();
    }

    let mut map: HashMap<u64, String> = HashMap::with_capacity(lines.len());
    for line in lines {
        map.insert(line.id, line.text);
    }
    let known: Vec<(easyscanlate_model::EntryId, String)> = app.tabs[idx]
        .project
        .all_entries()
        .iter()
        .map(|e| (e.id, e.text.clone()))
        .collect();
    let known_ids: HashSet<u64> = known.iter().map(|(id, _)| id.0).collect();
    let matched = map.keys().filter(|id| known_ids.contains(id)).count();
    let unknown = map.len().saturating_sub(matched);

    let mut target_id = match app.tabs[idx].project.profiles.find_by_name(&target_name) {
        Some(id) => {
            if app.tabs[idx].project.profiles.selected_id() != id {
                app.tabs[idx].project.profiles.select(id);
            }
            id
        }
        None => {
            let (nid, ev) = app.tabs[idx].project.create_profile_with_event(target_name.clone());
            crate::app::handle_model_event(&mut app.tabs[idx], ev);
            if let Some(ev2) = app.tabs[idx].project.select_profile_with_event(nid) {
                crate::app::handle_model_event(&mut app.tabs[idx], ev2);
            }
            nid
        }
    };

    // The original ("Default") profile never holds deltas: inline edits fork
    // off it, so an import targeting it forks the same way instead of writing
    // into the OCR source of truth.
    let mut fork_note = String::new();
    if target_id == app.tabs[idx].project.profiles.original_id() {
        if let Some((fork_name, evs)) = app.tabs[idx].project.fork_for_edit_with_event() {
            for ev in evs {
                crate::app::handle_model_event(&mut app.tabs[idx], ev);
            }
            target_id = app.tabs[idx].project.profiles.selected_id();
            fork_note = format!(" (forked from “{target_name}” into “{fork_name}”)");
            target_name = fork_name;
        } else {
            let name = app.tabs[idx].project.profiles.next_available_name();
            let (nid, ev) = app.tabs[idx].project.create_profile_with_event(name.clone());
            crate::app::handle_model_event(&mut app.tabs[idx], ev);
            if let Some(ev2) = app.tabs[idx].project.select_profile_with_event(nid) {
                crate::app::handle_model_event(&mut app.tabs[idx], ev2);
            }
            target_id = nid;
            fork_note = format!(" (forked from “{target_name}” into “{name}”)");
            target_name = name;
        }
    }

    let mut applied = 0usize;
    {
        let tab = &mut app.tabs[idx];
        for (eid, ocr_text) in &known {
            if let Some(t) = map.get(&eid.0) {
                let stored = if t == ocr_text { None } else { Some(t.clone()) };
                tab.project
                    .profiles
                    .selected_mut()
                    .set_translation(*eid, stored);
                applied += 1;
            } else {
                tab.project
                    .profiles
                    .selected_mut()
                    .set_translation(*eid, None);
            }
        }
        tab.dirty = true;
    }
    debug_assert_eq!(
        app.tabs[idx].project.profiles.selected_id(),
        target_id,
        "overwrite must land on the resolved target profile"
    );
    app.adv_import_target = Some(target_id);
    app.adv_import_name.clear();
    let cleared = known.len().saturating_sub(applied);
    app.tabs[idx].status = format!(
        "Imported {applied} line(s) into “{target_name}”{fork_note} ({cleared} cleared, {unknown} unknown skipped). Save to persist."
    );
    Task::none()
}
