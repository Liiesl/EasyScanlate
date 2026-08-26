use std::path::PathBuf;
use std::sync::Arc;

use iced::Task;

use super::{App, Message};

fn extract_inpaint_data(app: &App) -> Vec<scanlateit_mmtl::InpaintImageData> {
    let mut out = Vec::new();
    for loaded in &app.images {
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
            out.push(scanlateit_mmtl::InpaintImageData {
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
    if let Some(path) = app.mmtl_path.clone() {
        return do_save(app.project.clone(), extract_inpaint_data(app), path);
    }
    handle_save_as(app)
}

pub fn handle_save_as(_app: &mut App) -> Task<Message> {
    Task::perform(
        async {
            let file = rfd::AsyncFileDialog::new()
                .add_filter("Manga Translation (.mmtl)", &["mmtl"])
                .set_file_name("project.mmtl")
                .save_file()
                .await;
            file.map(|f| f.path().to_string_lossy().to_string())
        },
        Message::MmtlSavePicked,
    )
}

pub fn handle_open(_app: &mut App) -> Task<Message> {
    Task::perform(
        async {
            let file = rfd::AsyncFileDialog::new()
                .add_filter("Manga Translation (.mmtl)", &["mmtl"])
                .pick_file()
                .await;
            file.map(|f| f.path().to_string_lossy().to_string())
        },
        Message::MmtlOpenPicked,
    )
}

pub fn handle_save_picked(app: &mut App, picked: Option<String>) -> Task<Message> {
    let Some(path_str) = picked else {
        app.status = "Save cancelled.".to_string();
        return Task::none();
    };
    let path = PathBuf::from(&path_str);
    app.mmtl_path = Some(path.clone());
    do_save(app.project.clone(), extract_inpaint_data(app), path)
}

fn build_loaded_images(res: scanlateit_mmtl::LoadResult, display: String) -> Result<(scanlateit_model::Project, Vec<scanlateit_ui::LoadedImage>, String, Option<Arc<tempfile::TempDir>>), String> {
    let project = res.project;
    let mut inpaint_map: std::collections::HashMap<scanlateit_model::ImageId, Vec<scanlateit_ui::loaded::InpaintLayer>> = std::collections::HashMap::new();
    for (img_id, bounds, png_path) in &res.inpaint_files {
        let data = std::fs::read(png_path).map_err(|e| e.to_string())?;
        let img = image::load_from_memory(&data).map_err(|e| e.to_string())?.to_rgba8();
        let (w, h) = (img.width(), img.height());
        let handle = iced::widget::image::Handle::from_rgba(w, h, bytes::Bytes::from(img.into_raw()));
        let quad = project.inpaint_for(*img_id).find(|p| p.bounds == *bounds).and_then(|p| p.quad).or_else(|| {
            project.inpaint_for(*img_id).find(|p| p.bounds[2] as u32 == w && p.bounds[3] as u32 == h).and_then(|p| p.quad)
        });
        inpaint_map.entry(*img_id).or_default().push(scanlateit_ui::loaded::InpaintLayer { bounds: *bounds, quad, handle, width: w, height: h });
    }
    let mut out_images = Vec::new();
    for meta in project.images() {
        let layers = inpaint_map.remove(&meta.id).unwrap_or_default();
        out_images.push(scanlateit_ui::LoadedImage { image_id: meta.id, decode: scanlateit_ui::main_area::decode::PageDecode::default(), inpaint: layers });
    }
    debug_assert_eq!(project.image_count(), out_images.len());
    Ok((project, out_images, display, Some(Arc::new(res.temp_dir))))
}

fn do_save(project: scanlateit_model::Project, inpaint: Vec<scanlateit_mmtl::InpaintImageData>, path: PathBuf) -> Task<Message> {
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                scanlateit_mmtl::save_mmtl(&project, &inpaint, &path).map(|_| path.to_string_lossy().to_string()).map_err(|e| e.to_string())
            })
            .await
            .unwrap_or_else(|e| Err(format!("save task failed: {e}")))
        },
        Message::MmtlSaved,
    )
}

pub fn handle_open_picked(app: &mut App, picked: Option<String>) -> Task<Message> {
    let Some(path_str) = picked else {
        app.status = "Open cancelled.".to_string();
        return Task::none();
    };
    let path = PathBuf::from(path_str);
    app.status = format!("Loading {}...", path.display());
    Task::perform(
        async move {
            let path_clone = path.clone();
            tokio::task::spawn_blocking(move || {
                let res = scanlateit_mmtl::load_mmtl(&path_clone)?;
                let display = path_clone.to_string_lossy().to_string();
                build_loaded_images(res, display)
            })
            .await
            .unwrap_or_else(|e| Err(format!("load task failed: {e}")))
        },
        Message::MmtlLoaded,
    )
}

pub fn handle_saved(app: &mut App, result: Result<String, String>) -> Task<Message> {
    match result {
        Ok(path) => {
            app.status = format!("Saved to {path}");
            app.mmtl_path = Some(PathBuf::from(path.clone()));
            scanlateit_settings::touch_recent(path);
            app.recent_projects = scanlateit_settings::get(|s| s.recent_projects.clone());
        }
        Err(e) => {
            app.status = format!("Save failed: {e}");
        }
    }
    Task::none()
}

pub fn load_created_project(path_str: String) -> Result<(scanlateit_model::Project, Vec<scanlateit_ui::LoadedImage>, String, Option<Arc<tempfile::TempDir>>), String> {
    let path = PathBuf::from(&path_str);
    let res = scanlateit_mmtl::load_mmtl(&path)?;
    let display = path.to_string_lossy().to_string();
    build_loaded_images(res, display)
}

pub fn handle_loaded(
    app: &mut App,
    result: Result<(scanlateit_model::Project, Vec<scanlateit_ui::LoadedImage>, String, Option<Arc<tempfile::TempDir>>), String>,
) -> Task<Message> {
    match result {
        Ok((project, images, display_path, temp_dir)) => {
            debug_assert_eq!(project.image_count(), images.len(), "project/images parity must hold after load");
            app.project = project;
            app.images = images;
            app.mmtl_path = Some(PathBuf::from(display_path.clone()));
            app.mmtl_temp_dir = temp_dir;
            app.selected = None;
            app.selected_inpaint = None;
            app.editing = None;
            app.edit_content = None;
            app.app_view = super::AppView::Editor;
            scanlateit_settings::touch_recent(display_path.clone());
            app.recent_projects = scanlateit_settings::get(|s| s.recent_projects.clone());
            app.status = format!("Loaded {} ({} image(s))", display_path, app.images.len());
            let project = &app.project;
            return app.scheduler.decode_thumbs_with_project(&mut app.images, project, super::Message::ThumbDecoded);
        }
        Err(e) => {
            app.status = format!("Load failed: {e}");
        }
    }
    Task::none()
}
