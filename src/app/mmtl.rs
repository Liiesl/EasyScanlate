use std::path::PathBuf;
use std::sync::Arc;

use iced::Task;

use super::tab::Tab;
use super::{App, Message};

fn extract_inpaint_data(tab: &Tab) -> Vec<easyscanlate_mmtl::InpaintImageData> {
    let mut out = Vec::new();
    for loaded in &tab.images {
        let image_id = loaded.image_id;
        for layer in &loaded.inpaint {
            let (width, height, pixels) = match &layer.handle {
                iced::widget::image::Handle::Rgba { width, height, pixels, .. } => {
                    (*width, *height, pixels.to_vec())
                }
                iced::widget::image::Handle::Bytes(_id, bytes) => {
                    if let Ok(img) = image::load_from_memory(bytes) {
                        let rgba = img.to_rgba8();
                        let (w, h) = (rgba.width(), rgba.height());
                        (w, h, rgba.into_raw())
                    } else {
                        continue;
                    }
                }
                _ => continue,
            };
            out.push(easyscanlate_mmtl::InpaintImageData {
                image_id,
                bounds: layer.bounds,
                width,
                height,
                rgba: pixels,
            });
        }
    }
    out
}

pub fn handle_save(app: &mut App) -> Task<Message> {
    let tab = app.active_tab();
    let tab_id = tab.id;
    if let Some(path) = tab.mmtl_path.clone() {
        return do_save(tab_id, tab.project.clone(), extract_inpaint_data(tab), path);
    }
    handle_save_as(app)
}

pub fn handle_save_as(_app: &mut App) -> Task<Message> {
    let tab_id = _app.active_tab().id;
    Task::perform(
        async {
            let file = rfd::AsyncFileDialog::new()
                .add_filter("Manga Translation (.mmtl)", &["mmtl"])
                .set_file_name("project.mmtl")
                .save_file()
                .await;
            file.map(|f| f.path().to_string_lossy().to_string())
        },
        move |picked| Message::Tab(tab_id, crate::app::TabMessage::MmtlSavePicked(picked)),
    )
}

pub fn handle_open(_app: &mut App) -> Task<Message> {
    let tab_id = _app.active_tab().id;
    Task::perform(
        async {
            let file = rfd::AsyncFileDialog::new()
                .add_filter("Manga Translation (.mmtl)", &["mmtl"])
                .pick_file()
                .await;
            file.map(|f| f.path().to_string_lossy().to_string())
        },
        move |picked| Message::Tab(tab_id, crate::app::TabMessage::MmtlOpenPicked(picked)),
    )
}

pub fn handle_save_picked(app: &mut App, tab_id: crate::app::tab::TabId, picked: Option<String>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    let Some(path_str) = picked else {
        app.tabs[idx].status = "Save cancelled.".to_string();
        return Task::none();
    };
    let path = PathBuf::from(&path_str);
    {
        let tab = &mut app.tabs[idx];
        tab.mmtl_path = Some(path.clone());
    }
    let tab = &app.tabs[idx];
    do_save(tab_id, tab.project.clone(), extract_inpaint_data(tab), path)
}

fn build_loaded_images(res: easyscanlate_mmtl::LoadResult, display: String) -> Result<(easyscanlate_model::Project, Vec<easyscanlate_ui::LoadedImage>, String, Option<Arc<tempfile::TempDir>>), String> {
    let project = res.project;
    let mut inpaint_map: std::collections::HashMap<easyscanlate_model::ImageId, Vec<easyscanlate_ui::loaded::InpaintLayer>> = std::collections::HashMap::new();
    for (img_id, bounds, png_path) in &res.inpaint_files {
        let data = std::fs::read(png_path).map_err(|e| e.to_string())?;
        let img = image::load_from_memory(&data).map_err(|e| e.to_string())?.to_rgba8();
        let (w, h) = (img.width(), img.height());
        let handle = iced::widget::image::Handle::from_rgba(w, h, bytes::Bytes::from(img.into_raw()));
        let quad = project.inpaint_for(*img_id).find(|p| p.bounds == *bounds).and_then(|p| p.quad).or_else(|| {
            project.inpaint_for(*img_id).find(|p| p.bounds[2] as u32 == w && p.bounds[3] as u32 == h).and_then(|p| p.quad)
        });
        inpaint_map.entry(*img_id).or_default().push(easyscanlate_ui::loaded::InpaintLayer { bounds: *bounds, quad, handle, width: w, height: h });
    }
    let mut out_images = Vec::new();
    for meta in project.images() {
        let layers = inpaint_map.remove(&meta.id).unwrap_or_default();
        out_images.push(easyscanlate_ui::LoadedImage { image_id: meta.id, decode: easyscanlate_ui::main_area::decode::PageDecode::default(), inpaint: layers });
    }
    debug_assert_eq!(project.image_count(), out_images.len());
    Ok((project, out_images, display, Some(Arc::new(res.temp_dir))))
}

fn do_save(tab_id: crate::app::tab::TabId, project: easyscanlate_model::Project, inpaint: Vec<easyscanlate_mmtl::InpaintImageData>, path: PathBuf) -> Task<Message> {
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                easyscanlate_mmtl::save_mmtl(&project, &inpaint, &path).map(|_| path.to_string_lossy().to_string()).map_err(|e| e.to_string())
            })
            .await
            .unwrap_or_else(|e| Err(format!("save task failed: {e}")))
        },
        move |res| Message::Tab(tab_id, crate::app::TabMessage::MmtlSaved(res)),
    )
}

fn find_tab_by_path(app: &App, path: &PathBuf) -> Option<usize> {
    app.tabs.iter().position(|t| {
        t.mmtl_path.as_deref() == Some(path.as_path())
            || t.loading_path.as_deref() == Some(path.as_path())
    })
}

fn dedup_activate(app: &mut App, path: &PathBuf) -> bool {
    if let Some(idx) = find_tab_by_path(app, path) {
        app.active = idx;
        // If it's a loading placeholder, keep its "Loading..." status; otherwise show Already open.
        if !app.tabs[idx].loading {
            app.tabs[idx].status = format!("Already open: {}", app.tabs[idx].title);
        }
        return true;
    }
    false
}

/// Try to create an instant loading placeholder tab for `path`. Returns `Some(id)` if a new
/// placeholder was created and activated, `None` if dedup activated an existing tab (no new tab).
pub(crate) fn create_loading_tab(app: &mut App, path: PathBuf) -> Option<crate::app::tab::TabId> {
    if dedup_activate(app, &path) {
        return None;
    }
    let id = crate::app::tab::TabId(app.next_tab_id);
    app.next_tab_id += 1;
    let title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("project")
        .to_string();
    let tab = crate::app::tab::Tab::loading_placeholder(id, title, path);
    app.tabs.push(tab);
    app.active = app.tabs.len() - 1;
    Some(id)
}

pub(crate) fn push_project_tab(
    app: &mut App,
    id: crate::app::tab::TabId,
    project: easyscanlate_model::Project,
    images: Vec<easyscanlate_ui::LoadedImage>,
    display_path: String,
    temp_dir: Option<Arc<tempfile::TempDir>>,
) -> Task<Message> {
    let path = PathBuf::from(&display_path);

    // Hydration path: `id` is a loading placeholder that was created optimistically.
    if let Some(idx) = app.tabs.iter().position(|t| t.id == id) {
        if app.tabs[idx].loading {
            // Another non-placeholder tab already has this path (race: opened elsewhere while loading).
            // Remove the placeholder and activate the existing one.
            if let Some(other_idx) = app
                .tabs
                .iter()
                .position(|t| t.id != id && t.mmtl_path.as_deref() == Some(path.as_path()))
            {
                // Remove placeholder
                let ph_pos = app.tabs.iter().position(|t| t.id == id).unwrap();
                app.tabs.remove(ph_pos);
                // Adjust active if needed
                if app.active >= app.tabs.len() {
                    app.active = app.tabs.len().saturating_sub(1);
                } else if ph_pos < app.active {
                    app.active -= 1;
                } else if ph_pos == app.active {
                    // active was placeholder, switch to existing (re-find)
                    if let Some(new_other) = app.tabs.iter().position(|t| t.mmtl_path.as_deref() == Some(path.as_path())) {
                        app.active = new_other;
                    }
                }
                // dedup_activate semantics
                if let Some(nidx) = find_tab_by_path(app, &path) {
                    app.active = nidx;
                    app.tabs[nidx].status = format!("Already open: {}", app.tabs[nidx].title);
                } else {
                    // fallback: activate other_idx adjusted
                    let _ = other_idx;
                }
                easyscanlate_settings::touch_recent(display_path.clone());
                app.recent_projects = easyscanlate_settings::get(|s| s.recent_projects.clone());
                return Task::none();
            }
            // Normal hydration: fill placeholder in place.
            let len = images.len();
            let project_clone = project.clone();
            {
                let tab = &mut app.tabs[idx];
                tab.hydrate_from_loaded(project, images, path.clone(), temp_dir);
            }
            app.active = idx;
            easyscanlate_settings::touch_recent(display_path.clone());
            app.recent_projects = easyscanlate_settings::get(|s| s.recent_projects.clone());
            if len > 0 {
                let tid = app.tabs[idx].id;
                let new_tab = &mut app.tabs[idx];
                return new_tab.scheduler.decode_thumbs_with_project(
                    &mut new_tab.images,
                    &project_clone,
                    move |i, r| Message::Tab(tid, crate::app::TabMessage::ThumbDecoded(i, r)),
                );
            }
            return Task::none();
        }
    }

    // Non-hydration (legacy or placeholder was closed before load finished).
    // Dedup: if already open, just activate it and discard this load.
    if let Some(idx) = find_tab_by_path(app, &path) {
        app.active = idx;
        if !app.tabs[idx].loading {
            app.tabs[idx].status = format!("Already open: {}", app.tabs[idx].title);
        }
        // If there was a stray loading placeholder with `id` that is not the dedup target,
        // remove it (it was created optimistically but now deduped).
        if let Some(ph_idx) = app.tabs.iter().position(|t| t.id == id && t.loading) {
            // Only remove if it's not the dedup target
            if ph_idx != idx {
                let remove_pos = ph_idx;
                app.tabs.remove(remove_pos);
                if app.active > remove_pos {
                    app.active -= 1;
                } else if app.active == remove_pos {
                    app.active = idx.min(app.tabs.len().saturating_sub(1));
                    if ph_idx < idx {
                        // idx shifted
                        if let Some(new_idx) = find_tab_by_path(app, &path) {
                            app.active = new_idx;
                        }
                    }
                }
            }
        }
        easyscanlate_settings::touch_recent(display_path.clone());
        app.recent_projects = easyscanlate_settings::get(|s| s.recent_projects.clone());
        return Task::none();
    }
    let title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("project")
        .to_string();
    let len = images.len();
    let project_clone = project.clone();
    // If `id` collides with an existing non-loading tab, mint a fresh one.
    let use_id = if app.tabs.iter().any(|t| t.id == id) {
        let nid = crate::app::tab::TabId(app.next_tab_id);
        app.next_tab_id += 1;
        nid
    } else {
        id
    };
    let tab = crate::app::tab::Tab::project_from_loaded(use_id, title, project, images, path.clone(), temp_dir);
    app.tabs.push(tab);
    app.active = app.tabs.len() - 1;
    easyscanlate_settings::touch_recent(display_path.clone());
    app.recent_projects = easyscanlate_settings::get(|s| s.recent_projects.clone());
    // Update status on the newly created tab (project_from_loaded already sets Loaded, keep it)
    // but ensure recent already touched; no extra status override needed.
    if len > 0 {
        let new_tab = &mut app.tabs[app.active];
        let tid = new_tab.id;
        return new_tab.scheduler.decode_thumbs_with_project(
            &mut new_tab.images,
            &project_clone,
            move |i, r| Message::Tab(tid, crate::app::TabMessage::ThumbDecoded(i, r)),
        );
    }
    Task::none()
}

pub fn handle_open_picked(app: &mut App, tab_id: crate::app::tab::TabId, picked: Option<String>) -> Task<Message> {
    let idx_opt = app.tabs.iter().position(|t| t.id == tab_id);
    let Some(path_str) = picked else {
        if let Some(idx) = idx_opt {
            app.tabs[idx].status = "Open cancelled.".to_string();
        } else {
            app.active_tab_mut().status = "Open cancelled.".to_string();
        }
        return Task::none();
    };
    let path = PathBuf::from(path_str);
    let Some(new_id) = create_loading_tab(app, path.clone()) else {
        return Task::none();
    };
    Task::perform(
        async move {
            let path_clone = path.clone();
            tokio::task::spawn_blocking(move || {
                let res = easyscanlate_mmtl::load_mmtl(&path_clone)?;
                let display = path_clone.to_string_lossy().to_string();
                build_loaded_images(res, display)
            })
            .await
            .unwrap_or_else(|e| Err(format!("load task failed: {e}")))
        },
        move |res| Message::Tab(new_id, crate::app::TabMessage::MmtlLoaded(res)),
    )
}

pub fn handle_saved(app: &mut App, tab_id: crate::app::tab::TabId, result: Result<String, String>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    match result {
        Ok(path) => {
            {
                let tab = &mut app.tabs[idx];
                tab.status = format!("Saved to {path}");
                tab.mmtl_path = Some(PathBuf::from(path.clone()));
                tab.dirty = false;
                tab.title = PathBuf::from(&path).file_stem().and_then(|s| s.to_str()).unwrap_or("project").to_string();
            }
            easyscanlate_settings::touch_recent(path.clone());
            app.recent_projects = easyscanlate_settings::get(|s| s.recent_projects.clone());
            // If there was a pending close for this tab, now close it
            if app.pending_close == Some(tab_id) {
                if let Some(pos) = app.tabs.iter().position(|t| t.id == tab_id) {
                    if app.tabs[pos].is_project() {
                        app.tabs.remove(pos);
                        if app.active >= app.tabs.len() { app.active = app.tabs.len().saturating_sub(1); }
                        else if pos < app.active { app.active -= 1; }
                    }
                }
                app.pending_close = None;
            }
        }
        Err(e) => {
            app.tabs[idx].status = format!("Save failed: {e}");
        }
    }
    Task::none()
}

pub fn load_created_project(path_str: String) -> Result<(easyscanlate_model::Project, Vec<easyscanlate_ui::LoadedImage>, String, Option<Arc<tempfile::TempDir>>), String> {
    let path = PathBuf::from(&path_str);
    let res = easyscanlate_mmtl::load_mmtl(&path)?;
    let display = path.to_string_lossy().to_string();
    build_loaded_images(res, display)
}

pub fn handle_external_opens(app: &mut App, paths: Vec<String>) -> Task<Message> {
    if paths.is_empty() { return Task::none(); }
    let mut tasks = Vec::new();
    for raw in paths {
        let trimmed = raw.trim().trim_matches('"').trim().to_string();
        if trimmed.is_empty() { continue; }
        if !trimmed.to_ascii_lowercase().ends_with(".mmtl") {
            app.active_tab_mut().status = format!("Not a .mmtl: {trimmed}");
            continue;
        }
        let path = std::path::PathBuf::from(&trimmed);
        if !path.exists() {
            app.active_tab_mut().status = format!("Missing: {}", path.display());
            continue;
        }
        let Some(new_id) = create_loading_tab(app, path.clone()) else {
            continue;
        };
        let path_clone = path.clone();
        tasks.push(Task::perform(
            async move {
                tokio::task::spawn_blocking(move || load_created_project(path_clone.to_string_lossy().to_string()))
                    .await
                    .unwrap_or_else(|e| Err(format!("load task failed: {e}")))
            },
            move |res| Message::Tab(new_id, crate::app::TabMessage::RecentPickedToLoad(res)),
        ));
    }
    if tasks.is_empty() { Task::none() } else { Task::batch(tasks) }
}

pub fn handle_loaded(
    app: &mut App,
    tab_id: crate::app::tab::TabId,
    result: Result<(easyscanlate_model::Project, Vec<easyscanlate_ui::LoadedImage>, String, Option<Arc<tempfile::TempDir>>), String>,
) -> Task<Message> {
    match result {
        Ok((project, images, display_path, temp_dir)) => {
            debug_assert_eq!(project.image_count(), images.len(), "project/images parity must hold after load");
            return push_project_tab(app, tab_id, project, images, display_path, temp_dir);
        }
        Err(e) => {
            // If this was a loading placeholder, keep the tab but mark it failed so the
            // user sees the error in the tab's status/overlay and can close it.
            if let Some(idx) = app.tabs.iter().position(|t| t.id == tab_id) {
                let tab = &mut app.tabs[idx];
                if tab.loading {
                    tab.loading = false;
                    tab.loading_path = None;
                    tab.loading_phase = 0.0;
                }
                tab.status = format!("Load failed: {e}");
            } else {
                app.active_tab_mut().status = format!("Load failed: {e}");
            }
        }
    }
    Task::none()
}
