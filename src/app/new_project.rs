use iced::Task;
use easyscanlate_model::{natural_cmp, Project};
use easyscanlate_ui::main_area::decode::PageDecode;
use easyscanlate_ui::LoadedImage;

use super::layout::IMAGE_FILTERS;
use super::tab::TabId;
use super::{App, Message, TabMessage};

#[derive(Debug, Clone)]
pub struct NewProjectState {
    pub source_files: Vec<(String, u32, u32)>,
    pub original_lang: String,
    pub project_location: Option<String>,
}

fn ensure_new_project_state(app: &mut App) {
    app.new_project = Some(NewProjectState {
        source_files: Vec::new(),
        original_lang: "Korean".to_string(),
        project_location: None,
    });
    app.active_tab_mut().status = "New Project...".to_string();
}

pub fn handle_new(app: &mut App) -> Task<Message> {
    ensure_new_project_state(app);
    Task::none()
}

pub fn handle_close(app: &mut App) -> Task<Message> {
    app.new_project = None;
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
            match files {
                Some(files) => {
                    let mut out = Vec::with_capacity(files.len());
                    for file in files {
                        let path = file.path().to_string_lossy().into_owned();
                        let dims = image::ImageReader::open(&path)
                            .map_err(|e| format!("Failed to open {path}: {e}"))?
                            .into_dimensions()
                            .map_err(|e| format!("Failed to decode {path}: {e}"));
                        match dims {
                            Ok((w, h)) => out.push((path, w, h)),
                            Err(e) => return Err(e),
                        }
                    }
                    Ok(out)
                }
                None => Ok(Vec::new()),
            }
        },
        move |res| Message::Tab(tid, TabMessage::NewProjectSourcePicked(res)),
    )
}

pub fn handle_source_folder(app: &mut App) -> Task<Message> {
    let tid = app.active_tab().id;
    Task::perform(
        async {
            let folder = rfd::AsyncFileDialog::new().pick_folder().await;
            let Some(folder) = folder else { return Ok(Vec::new()) };
            let dir = folder.path().to_path_buf();
            let entries = std::fs::read_dir(&dir).map_err(|e| e.to_string())?;
            let mut out = Vec::new();
            for entry in entries {
                let entry = entry.map_err(|e| e.to_string())?;
                let path = entry.path();
                if !path.is_file() { continue; }
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
                if !IMAGE_FILTERS.contains(&ext.as_str()) { continue; }
                let pstr = path.to_string_lossy().into_owned();
                let dims = image::ImageReader::open(&path)
                    .map_err(|e| format!("Failed to open {pstr}: {e}"))?
                    .into_dimensions()
                    .map_err(|e| format!("Failed to decode {pstr}: {e}"));
                match dims {
                    Ok((w, h)) => out.push((pstr, w, h)),
                    Err(e) => return Err(e),
                }
            }
            out.sort_by(|a, b| natural_cmp(&a.0, &b.0));
            Ok(out)
        },
        move |res| Message::Tab(tid, TabMessage::NewProjectFolderPicked(res)),
    )
}

pub fn handle_location_browse(app: &mut App) -> Task<Message> {
    let default_dir = app
        .new_project
        .as_ref()
        .and_then(|np| np.source_files.first().map(|(p, _, _)| std::path::Path::new(p).parent().map(|par| par.to_path_buf()).unwrap_or_default()))
        .unwrap_or_default();
    let tid = app.active_tab().id;
    Task::perform(
        async move {
            let mut dlg = rfd::AsyncFileDialog::new()
                .add_filter("Manga Translation (.mmtl)", &["mmtl"])
                .set_file_name("project.mmtl");
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

pub fn handle_create(app: &mut App) -> Task<Message> {
    let Some(np) = app.new_project.clone() else { return Task::none() };
    if np.source_files.is_empty() || np.project_location.is_none() {
        app.active_tab_mut().status = "Select source and project location.".to_string();
        return Task::none();
    }
    let dest_str = np.project_location.clone().unwrap();
    let mut dest = std::path::PathBuf::from(&dest_str);
    if dest.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()) != Some("mmtl".to_string()) {
        let mut os = dest.as_os_str().to_owned();
        os.push(".mmtl");
        dest = std::path::PathBuf::from(os);
    }
    let unique_dest = {
        if !dest.exists() {
            dest.clone()
        } else {
            let parent = dest.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let stem = dest.file_stem().and_then(|s| s.to_str()).unwrap_or("project").to_string();
            let ext = dest.extension().and_then(|e| e.to_str()).unwrap_or("mmtl").to_string();
            let mut n = 1;
            let mut cand;
            loop {
                cand = parent.join(format!("{stem} ({n}).{ext}"));
                if !cand.exists() { break; }
                n += 1;
                if n > 999 { break; }
            }
            cand
        }
    };
    if let Some(parent) = unique_dest.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let files = np.source_files.clone();
    let dest_for_task = unique_dest.clone();
    app.new_project = None;
    app.active_tab_mut().status = format!("Creating {}...", unique_dest.display());
    let new_id = TabId(app.next_tab_id);
    app.next_tab_id += 1;
    Task::perform(
        async move {
            let res: Result<String, String> = tokio::task::spawn_blocking(move || -> Result<String, String> {
                let mut project = Project::new();
                let mut metas: Vec<(String, u32, u32)> = files;
                metas.sort_by(|a, b| natural_cmp(&a.0, &b.0));
                let mut loaded: Vec<LoadedImage> = Vec::new();
                for (path, w, h) in metas {
                    let image_id = project.add_image(path.clone(), w as f32, h as f32);
                    loaded.push(LoadedImage { image_id, decode: PageDecode::default(), inpaint: Vec::new() });
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
pub fn handle_source_picked(app: &mut App, tab_id: TabId, result: Result<Vec<(String, u32, u32)>, String>) -> Task<Message> {
    let Some(idx) = app.tabs.iter().position(|t| t.id == tab_id) else { return Task::none() };
    match result {
        Ok(files) if !files.is_empty() => {
            if let Some(np) = app.new_project.as_mut() { np.source_files = files; }
        }
        Ok(_) => {}
        Err(e) => { app.tabs[idx].status = e; }
    }
    Task::none()
}

pub fn handle_folder_picked(app: &mut App, tab_id: TabId, result: Result<Vec<(String, u32, u32)>, String>) -> Task<Message> {
    let Some(idx) = app.tabs.iter().position(|t| t.id == tab_id) else { return Task::none() };
    match result {
        Ok(files) if !files.is_empty() => {
            if let Some(np) = app.new_project.as_mut() { np.source_files = files; }
        }
        Ok(_) => { app.tabs[idx].status = "No images found in folder.".to_string(); }
        Err(e) => { app.tabs[idx].status = e; }
    }
    Task::none()
}

pub fn handle_location_picked(app: &mut App, _tab_id: TabId, picked: Option<String>) -> Task<Message> {
    if let Some(p) = picked { if let Some(np) = app.new_project.as_mut() { np.project_location = Some(p); } }
    Task::none()
}
