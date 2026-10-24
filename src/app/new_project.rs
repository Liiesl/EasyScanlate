use easyscanlate_model::{Project, natural_cmp};
use easyscanlate_ui::LoadedImage;
use easyscanlate_ui::main_area::decode::PageDecode;
use iced::Task;

use super::backdrop::{self, BackdropKind};
use super::layout::IMAGE_FILTERS;
use super::tab::TabId;
use super::{App, Message, TabMessage};

#[derive(Debug, Clone)]
pub struct NewProjectState {
    pub source_files: Vec<String>,
    pub project_name: String,
    pub total_bytes: u64,
    pub source_dir: Option<String>,
    pub thumbs: Vec<iced::widget::image::Handle>,
    pub original_lang: String,
    pub project_location: Option<String>,
    /// Selected series (`None` = standalone). Defaults to `None`.
    pub series: Option<String>,
    /// Whether the "+ Add to a series" section is expanded.
    pub series_open: bool,
    /// Whether the "+ New series" inline input is shown.
    pub creating_series: bool,
    /// Current text of the "+ New series" input.
    pub new_series_name: String,
    /// Paste-a-path input in the empty drop zone (image or folder).
    pub path_input: String,
    /// Whether a file drag is hovering the window (highlights drop zone).
    pub drag_hovered: bool,
}

fn normalize_series_name(name: &str) -> Option<String> {
    let t = name.trim().to_string();
    if t.is_empty() { None } else { Some(t) }
}

pub(crate) fn ensure_new_project_state(app: &mut App) {
    app.new_project = Some(NewProjectState {
        source_files: Vec::new(),
        project_name: String::new(),
        total_bytes: 0,
        source_dir: None,
        thumbs: Vec::new(),
        original_lang: "Korean".to_string(),
        project_location: None,
        series: None,
        series_open: false,
        creating_series: false,
        new_series_name: String::new(),
        path_input: String::new(),
        drag_hovered: false,
    });
    app.active_tab_mut().status = "New Project...".to_string();
}

pub fn handle_new(app: &mut App) -> Task<Message> {
    // Capture-then-open so the blurred backdrop never contains the modal
    // itself (same pattern as Settings). Reuse a cached frame for instant
    // reopen; without a window (tests) open flat immediately. A modal
    // already on top keeps its own crop — the new overlay sits underneath.
    if app.settings_open || app.manage_models_open || app.connect_modal.is_some() {
        ensure_new_project_state(app);
        return Task::none();
    }
    if app.backdrop_frame.is_some() {
        backdrop::recrop(app, BackdropKind::NewProject);
        ensure_new_project_state(app);
        return Task::none();
    }
    if app.frame.primary_window().is_none() {
        ensure_new_project_state(app);
        return Task::none();
    }
    if app.backdrop_pending.is_some() {
        return Task::none();
    }
    app.backdrop_pending = Some(BackdropKind::NewProject);
    backdrop::capture_task(app, BackdropKind::NewProject)
}

pub fn handle_close(app: &mut App) -> Task<Message> {
    app.new_project = None;
    // Another modal on top owns the shared capture — keep it for them.
    if app.settings_open || app.manage_models_open || app.connect_modal.is_some() {
        if app.backdrop_pending == Some(BackdropKind::NewProject) {
            app.backdrop_pending = None;
        }
        return Task::none();
    }
    // Bottommost overlay closed: drop the frozen frame so the next open
    // re-captures fresh content.
    app.backdrop_blur = None;
    app.backdrop_frame = None;
    if app.backdrop_pending == Some(BackdropKind::NewProject) {
        app.backdrop_pending = None;
    }
    Task::none()
}

pub fn handle_source_image(app: &mut App) -> Task<Message> {
    let tid = app.active_tab().id;
    Task::perform(
        async {
            let files = rfd::AsyncFileDialog::new()
                .add_filter("Images", IMAGE_FILTERS)
                .pick_files()
                .await;
            let Some(files) = files else {
                return Ok(Vec::new());
            };
            // Paths only — dimensions are probed in parallel at Create time.
            let mut out: Vec<String> = files
                .iter()
                .map(|f| f.path().to_string_lossy().into_owned())
                .collect();
            out.sort_by(|a, b| natural_cmp(a, b));
            Ok(out)
        },
        move |res| Message::Tab(tid, TabMessage::NewProjectSourcePicked(res)),
    )
}

pub fn handle_source_folder(app: &mut App) -> Task<Message> {
    let tid = app.active_tab().id;
    Task::perform(
        async {
            let folder = rfd::AsyncFileDialog::new().pick_folder().await;
            let Some(folder) = folder else {
                return Ok(Vec::new());
            };
            let dir = folder.path().to_path_buf();
            // `read_dir` on the blocking pool. Paths only — dimensions are
            // probed in parallel at Create time.
            tokio::task::spawn_blocking(move || {
                let entries = std::fs::read_dir(&dir).map_err(|e| e.to_string())?;
                let mut out: Vec<String> = Vec::new();
                for entry in entries {
                    let entry = entry.map_err(|e| e.to_string())?;
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }
                    let ext = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if !IMAGE_FILTERS.contains(&ext.as_str()) {
                        continue;
                    }
                    out.push(path.to_string_lossy().into_owned());
                }
                out.sort_by(|a, b| natural_cmp(a, b));
                Ok(out)
            })
            .await
            .unwrap_or_else(|e| Err(format!("folder scan task failed: {e}")))
        },
        move |res| Message::Tab(tid, TabMessage::NewProjectFolderPicked(res)),
    )
}

/// Filename-safe version of a project name for the save dialog suggestion.
fn sanitize_file_stem(name: &str) -> Option<String> {
    let t = name.trim().trim_matches(['.', ' ']).to_string();
    if t.is_empty() {
        return None;
    }
    let cleaned: String = t
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches(['.', ' ']).to_string();
    if cleaned.is_empty() {
        return None;
    }
    // Cap length so the suggestion never blows the dialog width.
    if cleaned.chars().count() > 100 {
        Some(cleaned.chars().take(100).collect())
    } else {
        Some(cleaned)
    }
}

pub fn handle_location_browse(app: &mut App) -> Task<Message> {
    let default_dir = app
        .new_project
        .as_ref()
        .and_then(|np| {
            np.source_files.first().map(|p| {
                std::path::Path::new(p)
                    .parent()
                    .map(|par| par.to_path_buf())
                    .unwrap_or_default()
            })
        })
        .unwrap_or_default();
    let file_name = app
        .new_project
        .as_ref()
        .and_then(|np| sanitize_file_stem(&np.project_name))
        .map(|stem| format!("{stem}.mmtl"))
        .unwrap_or_else(|| "project.mmtl".to_string());
    let tid = app.active_tab().id;
    Task::perform(
        async move {
            let mut dlg = rfd::AsyncFileDialog::new()
                .add_filter("Manga Translation (.mmtl)", &["mmtl"])
                .set_file_name(&file_name);
            if default_dir.exists() {
                dlg = dlg.set_directory(&default_dir);
            }
            let file = dlg.save_file().await;
            file.map(|f| f.path().to_string_lossy().to_string())
        },
        move |picked| Message::Tab(tid, TabMessage::NewProjectLocationPicked(picked)),
    )
}

pub fn handle_original_lang(app: &mut App, lang: String) -> Task<Message> {
    if let Some(np) = app.new_project.as_mut() {
        np.original_lang = lang;
    }
    Task::none()
}

pub fn handle_name(app: &mut App, name: String) -> Task<Message> {
    if let Some(np) = app.new_project.as_mut() {
        np.project_name = name;
    }
    Task::none()
}

pub fn handle_series_toggle(app: &mut App) -> Task<Message> {
    if let Some(np) = app.new_project.as_mut() {
        np.series_open = !np.series_open;
    }
    Task::none()
}

fn refresh_source_meta(app: &mut App) {
    let Some(np) = app.new_project.as_mut() else {
        return;
    };
    let mut total: u64 = 0;
    for p in &np.source_files {
        if let Ok(m) = std::fs::metadata(p) {
            total = total.saturating_add(m.len());
        }
    }
    np.total_bytes = total;
    np.source_dir = np
        .source_files
        .first()
        .and_then(|p| std::path::Path::new(p).parent())
        .map(|par| par.to_string_lossy().into_owned());
}

fn spawn_thumbs(app: &App) -> Task<Message> {
    let Some(np) = app.new_project.clone() else {
        return Task::none();
    };
    let tab_id = app.active_tab().id;
    let paths: Vec<String> = np.source_files.iter().take(8).cloned().collect();
    if paths.is_empty() {
        return Task::none();
    }
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                let mut out = Vec::new();
                for path in paths {
                    match easyscanlate_ui::main_area::decode::decode_cover(&path, 96, 128) {
                        Ok(page) => out.push(page.handle),
                        Err(_) => continue,
                    }
                }
                out
            })
            .await
            .unwrap_or_default()
        },
        move |handles| Message::Tab(tab_id, TabMessage::NewProjectThumbs(handles)),
    )
}

pub fn handle_thumbs(
    app: &mut App,
    _tab_id: TabId,
    handles: Vec<iced::widget::image::Handle>,
) -> Task<Message> {
    if let Some(np) = app.new_project.as_mut() {
        np.thumbs = handles;
    }
    Task::none()
}

/// Route drag-dropped image paths / folders into the open modal (replaces source).
pub fn handle_dropped(app: &mut App, paths: Vec<String>) -> Task<Message> {
    if app.new_project.is_none() {
        return Task::none();
    }
    // Paths only — dimensions are probed in parallel at Create time.
    let mut out: Vec<String> = Vec::new();
    for raw in paths {
        let pb = std::path::PathBuf::from(&raw);
        if pb.is_dir() {
            let Ok(entries) = std::fs::read_dir(&pb) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if !IMAGE_FILTERS.contains(&ext.as_str()) {
                    continue;
                }
                out.push(path.to_string_lossy().into_owned());
            }
        } else {
            let ext = pb
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !IMAGE_FILTERS.contains(&ext.as_str()) {
                continue;
            }
            out.push(raw);
        }
    }
    if out.is_empty() {
        app.active_tab_mut().status = "No images found in dropped files.".to_string();
        return Task::none();
    }
    out.sort_by(|a, b| natural_cmp(a, b));
    if let Some(np) = app.new_project.as_mut() {
        np.source_files = out;
        np.thumbs.clear();
        np.drag_hovered = false;
    }
    refresh_source_meta(app);
    spawn_thumbs(app)
}

pub fn handle_series_select(app: &mut App, series: Option<String>) -> Task<Message> {
    if let Some(np) = app.new_project.as_mut() {
        np.series = normalize_series_name(series.as_deref().unwrap_or_default());
        np.creating_series = false;
        np.new_series_name.clear();
    }
    Task::none()
}

pub fn handle_series_create_start(app: &mut App) -> Task<Message> {
    if let Some(np) = app.new_project.as_mut() {
        np.creating_series = true;
        np.new_series_name.clear();
    }
    Task::none()
}

pub fn handle_series_name(app: &mut App, name: String) -> Task<Message> {
    if let Some(np) = app.new_project.as_mut() {
        np.new_series_name = name;
    }
    Task::none()
}

pub fn handle_series_create_confirm(app: &mut App) -> Task<Message> {
    if let Some(np) = app.new_project.as_mut() {
        if let Some(name) = normalize_series_name(&np.new_series_name.clone()) {
            np.series = Some(name);
        }
        np.creating_series = false;
        np.new_series_name.clear();
    }
    Task::none()
}

pub fn handle_series_cancel(app: &mut App) -> Task<Message> {
    if let Some(np) = app.new_project.as_mut() {
        np.creating_series = false;
        np.new_series_name.clear();
    }
    Task::none()
}

pub fn handle_path_input(app: &mut App, input: String) -> Task<Message> {
    if let Some(np) = app.new_project.as_mut() {
        np.path_input = input;
    }
    Task::none()
}

/// Parse the paste-a-path input as an image file or a folder of images.
/// Paths only — dimensions are probed in parallel at Create time.
pub fn handle_path_submit(app: &mut App) -> Task<Message> {
    let raw = app
        .new_project
        .as_ref()
        .map(|np| np.path_input.trim().trim_matches(['"', '\'']).to_string())
        .unwrap_or_default();
    if raw.is_empty() {
        return Task::none();
    }
    let tid = app.active_tab().id;
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                let pb = std::path::PathBuf::from(&raw);
                if pb.is_dir() {
                    let entries = std::fs::read_dir(&pb).map_err(|e| e.to_string())?;
                    let mut out: Vec<String> = Vec::new();
                    for entry in entries {
                        let entry = entry.map_err(|e| e.to_string())?;
                        let path = entry.path();
                        if !path.is_file() {
                            continue;
                        }
                        let ext = path
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("")
                            .to_ascii_lowercase();
                        if !IMAGE_FILTERS.contains(&ext.as_str()) {
                            continue;
                        }
                        out.push(path.to_string_lossy().into_owned());
                    }
                    out.sort_by(|a, b| natural_cmp(a, b));
                    if out.is_empty() {
                        return Err("No images found in that folder.".to_string());
                    }
                    Ok(out)
                } else if pb.is_file() {
                    let ext = pb
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if !IMAGE_FILTERS.contains(&ext.as_str()) {
                        return Err("Not an image file (JPG, PNG, WebP).".to_string());
                    }
                    Ok(vec![raw])
                } else {
                    Err("Path doesn't exist.".to_string())
                }
            })
            .await
            .unwrap_or_else(|e| Err(format!("path scan task failed: {e}")))
        },
        move |res| Message::Tab(tid, TabMessage::NewProjectFolderPicked(res)),
    )
}

pub fn handle_drag_hover(app: &mut App, hovered: bool) -> Task<Message> {
    if let Some(np) = app.new_project.as_mut() {
        np.drag_hovered = hovered;
    }
    Task::none()
}

pub fn handle_create(app: &mut App) -> Task<Message> {
    let Some(np) = app.new_project.clone() else {
        return Task::none();
    };
    if np.source_files.is_empty() || np.project_location.is_none() {
        app.active_tab_mut().status = "Select source and project location.".to_string();
        return Task::none();
    }
    let dest_str = np.project_location.clone().unwrap();
    let mut dest = std::path::PathBuf::from(&dest_str);
    if dest
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        != Some("mmtl".to_string())
    {
        let mut os = dest.as_os_str().to_owned();
        os.push(".mmtl");
        dest = std::path::PathBuf::from(os);
    }
    let unique_dest = {
        if !dest.exists() {
            dest.clone()
        } else {
            let parent = dest.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let stem = dest
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("project")
                .to_string();
            let ext = dest
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("mmtl")
                .to_string();
            let mut n = 1;
            let mut cand;
            loop {
                cand = parent.join(format!("{stem} ({n}).{ext}"));
                if !cand.exists() {
                    break;
                }
                n += 1;
                if n > 999 {
                    break;
                }
            }
            cand
        }
    };
    let files = np.source_files.clone();
    let series = np.series.clone();
    app.pending_create_series = Some(series.clone());
    let dest_for_task = unique_dest.clone();
    app.new_project = None;
    // Create instant loading placeholder tab so the user sees feedback right away.
    let Some(new_id) = crate::app::mmtl::create_loading_tab(app, unique_dest.clone()) else {
        return Task::none();
    };
    // Override status to "Creating ..." instead of "Loading ..."
    if let Some(idx) = app.tabs.iter().position(|t| t.id == new_id) {
        app.tabs[idx].status = format!("Creating {}...", unique_dest.display());
    }
    Task::perform(
        async move {
            let res: Result<String, String> =
                tokio::task::spawn_blocking(move || -> Result<String, String> {
                    // Directory creation off the UI thread (was sync `create_dir_all`).
                    if let Some(parent) = dest_for_task.parent()
                        && !parent.as_os_str().is_empty()
                    {
                        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    let mut project = Project::new();
                    project.set_series(series);
                    let mut paths: Vec<String> = files;
                    paths.sort_by(|a, b| natural_cmp(a, b));
                    // Header-only dimension probes in parallel; unreadable files
                    // are skipped.
                    use rayon::prelude::*;
                    let metas: Vec<(String, u32, u32)> = paths
                        .into_par_iter()
                        .filter_map(|path| {
                            image::ImageReader::open(&path)
                                .ok()?
                                .into_dimensions()
                                .ok()
                                .map(|(w, h)| (path, w, h))
                        })
                        .collect();
                    if metas.is_empty() {
                        return Err("No readable images found.".to_string());
                    }
                    let mut loaded: Vec<LoadedImage> = Vec::new();
                    for (path, w, h) in metas {
                        let image_id = project.add_image(path.clone(), w as f32, h as f32);
                        loaded.push(LoadedImage {
                            image_id,
                            decode: PageDecode::default(),
                            inpaint: Vec::new(),
                        });
                    }
                    debug_assert_eq!(project.image_count(), loaded.len());
                    easyscanlate_mmtl::save_mmtl(&project, &[], &dest_for_task)
                        .map_err(|e| e.to_string())?;
                    Ok(dest_for_task.to_string_lossy().to_string())
                })
                .await
                .unwrap_or_else(|e| Err(format!("create task failed: {e}")));
            res
        },
        move |res| Message::Tab(new_id, TabMessage::CreateProjectPicked(res)),
    )
}

// TabMessage handlers
pub fn handle_source_picked(
    app: &mut App,
    tab_id: TabId,
    result: Result<Vec<String>, String>,
) -> Task<Message> {
    let Some(idx) = app.tabs.iter().position(|t| t.id == tab_id) else {
        return Task::none();
    };
    match result {
        Ok(files) if !files.is_empty() => {
            if let Some(np) = app.new_project.as_mut() {
                np.source_files = files;
                np.thumbs.clear();
            }
            refresh_source_meta(app);
            spawn_thumbs(app)
        }
        Ok(_) => Task::none(),
        Err(e) => {
            app.tabs[idx].status = e.clone();
            super::error::show("Couldn't read the selected images", e)
        }
    }
}

pub fn handle_folder_picked(
    app: &mut App,
    tab_id: TabId,
    result: Result<Vec<String>, String>,
) -> Task<Message> {
    let Some(idx) = app.tabs.iter().position(|t| t.id == tab_id) else {
        return Task::none();
    };
    match result {
        Ok(files) if !files.is_empty() => {
            if let Some(np) = app.new_project.as_mut() {
                np.source_files = files;
                np.thumbs.clear();
            }
            refresh_source_meta(app);
            spawn_thumbs(app)
        }
        Ok(_) => {
            app.tabs[idx].status = "No images found in folder.".to_string();
            Task::none()
        }
        Err(e) => {
            app.tabs[idx].status = e.clone();
            super::error::show("Couldn't read that folder", e)
        }
    }
}

pub fn handle_location_picked(
    app: &mut App,
    _tab_id: TabId,
    picked: Option<String>,
) -> Task<Message> {
    if let Some(p) = picked
        && let Some(np) = app.new_project.as_mut()
    {
        // Auto-fill an untouched project name from the picked filename.
        if np.project_name.trim().is_empty() {
            let stem = std::path::Path::new(&p)
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && *s != "project");
            if let Some(stem) = stem {
                np.project_name = stem;
            }
        }
        np.project_location = Some(p);
    }
    Task::none()
}
