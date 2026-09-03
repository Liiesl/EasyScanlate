// Tab close/queue handlers — view now lives in `easyscanlate-ui::chrome::tabs`.
use iced::Task;

use super::tab::TabId;
use super::{App, Message, TabMessage};

/// 32px titlebar tab strip — `easyscanlate | proj • × | … + | Fill(drag gap)`.
///
/// `+` is inside the scrollable row immediately after last chip, so it sits
/// right on the side of rightmost tab. The scroll viewport grows with tab
/// count until 80% of titlebar width, then becomes fixed and scrollable.
/// Called from `crate::app::view::view` as `Some(titlebar_view(app))` fed to
/// `NativeFrame::view(..., title_content)`. Trailing `Fill` gap falls through
/// to `draggable` in `NeverLiieIcedWidgets/src/title_bar/mod.rs:653`.
pub(crate) fn close_tab_immediate(app: &mut App, id: TabId) -> Task<Message> {
    if let Some(idx) = app.tabs.iter().position(|t| t.id == id) {
        if app.tabs[idx].is_home() {
            return Task::none();
        }
        app.engines.queue.cancel_pending_for_tab(id);
        let freed = !app.engines.queue.cancel_running_for_tab(id).is_empty();
        let promote = if freed {
            crate::app::queue::dispatch_pending(app)
        } else {
            Task::none()
        };
        if freed {
            crate::app::queue::refresh_queued_statuses(app);
        }
        app.tabs.remove(idx);
        if app.active >= app.tabs.len() {
            app.active = app.tabs.len().saturating_sub(1);
        } else if idx < app.active {
            app.active -= 1;
        }
        if app.active >= app.tabs.len() && !app.tabs.is_empty() {
            app.active = app.tabs.len() - 1;
        }
        if app.pending_close == Some(id) {
            app.pending_close = None;
        }
        return promote;
    }
    Task::none()
}

fn cleanup_queue_for_tabs(app: &mut App, ids: &[TabId]) {
    for rid in ids {
        app.engines.queue.cancel_pending_for_tab(*rid);
        app.engines.queue.cancel_running_for_tab(*rid);
    }
}

pub fn handle_close(app: &mut App, raw: u64) -> Task<Message> {
    let id = TabId(raw);
    if let Some(idx) = app.tabs.iter().position(|t| t.id == id) {
        if app.tabs[idx].is_home() {
            return Task::none();
        }
        if app.tabs[idx].dirty {
            app.pending_close = Some(id);
        } else {
            return close_tab_immediate(app, id);
        }
    }
    Task::none()
}

pub fn handle_close_confirmed(app: &mut App, raw: u64, save: bool) -> Task<Message> {
    let id = TabId(raw);
    let Some(idx) = app.tabs.iter().position(|t| t.id == id) else {
        app.pending_close = None;
        return Task::none();
    };
    if app.tabs[idx].is_home() {
        app.pending_close = None;
        return Task::none();
    }
    if save {
        app.pending_close = Some(id);
        let path_opt = app.tabs[idx].mmtl_path.clone();
        if let Some(path) = path_opt {
            let project = app.tabs[idx].project.clone();
            let tid = id;
            let inpaint = {
                let tab = &app.tabs[idx];
                let mut out = Vec::new();
                for loaded in &tab.images {
                    let image_id = loaded.image_id;
                    for layer in &loaded.inpaint {
                        let (width, height, pixels) = match &layer.handle {
                            iced::widget::image::Handle::Rgba { width, height, pixels, .. } => (*width, *height, pixels.to_vec()),
                            iced::widget::image::Handle::Bytes(_id, bytes) => {
                                if let Ok(img) = image::load_from_memory(bytes) {
                                    let rgba = img.to_rgba8();
                                    let (w, h) = (rgba.width(), rgba.height());
                                    (w, h, rgba.into_raw())
                                } else { continue; }
                            }
                            _ => continue,
                        };
                        out.push(easyscanlate_mmtl::InpaintImageData { image_id, bounds: layer.bounds, width, height, rgba: pixels });
                    }
                }
                out
            };
            return Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        easyscanlate_mmtl::save_mmtl(&project, &inpaint, &path).map(|_| path.to_string_lossy().to_string()).map_err(|e| e.to_string())
                    }).await.unwrap_or_else(|e| Err(format!("save task failed: {e}")))
                },
                move |res| Message::Tab(tid, TabMessage::MmtlSaved(res)),
            );
        } else {
            let tid = id;
            return Task::perform(
                async move {
                    let file = rfd::AsyncFileDialog::new()
                        .add_filter("Manga Translation (.mmtl)", &["mmtl"])
                        .set_file_name("project.mmtl")
                        .save_file()
                        .await;
                    file.map(|f| f.path().to_string_lossy().to_string())
                },
                move |picked| Message::Tab(tid, TabMessage::MmtlSavePicked(picked)),
            );
        }
    } else {
        return close_tab_immediate(app, id);
    }
}

pub fn handle_close_cancel(app: &mut App) -> Task<Message> {
    app.pending_close = None;
    Task::none()
}

pub fn handle_close_others(app: &mut App, raw: u64) -> Task<Message> {
    let keep = TabId(raw);
    if let Some(dirty) = app.tabs.iter().find(|t| t.is_project() && t.id != keep && t.dirty).map(|t| t.id) {
        app.pending_close = Some(dirty);
        return Task::none();
    }
    let remove_ids: Vec<TabId> = app.tabs.iter().filter(|t| t.id != keep && t.is_project()).map(|t| t.id).collect();
    cleanup_queue_for_tabs(app, &remove_ids);
    let keep_idx = app.tabs.iter().position(|t| t.id == keep);
    if let Some(kidx) = keep_idx {
        let mut i = app.tabs.len();
        while i > 0 {
            i -= 1;
            if i == 0 { continue; }
            if app.tabs[i].id == keep { continue; }
            app.tabs.remove(i);
            if app.active > i { app.active -= 1; }
            else if app.active == i { app.active = kidx.min(app.tabs.len().saturating_sub(1)); }
        }
        if let Some(new_k) = app.tabs.iter().position(|t| t.id == keep) {
            app.active = new_k;
        }
    }
    let promote = crate::app::queue::dispatch_pending(app);
    crate::app::queue::refresh_queued_statuses(app);
    promote
}

pub fn handle_close_all(app: &mut App) -> Task<Message> {
    if let Some(dirty) = app.tabs.iter().find(|t| t.is_project() && t.dirty).map(|t| t.id) {
        app.pending_close = Some(dirty);
        return Task::none();
    }
    let remove_ids: Vec<TabId> = app.tabs.iter().filter(|t| t.is_project()).map(|t| t.id).collect();
    cleanup_queue_for_tabs(app, &remove_ids);
    app.tabs.retain(|t| t.is_home());
    app.active = 0;
    app.pending_close = None;
    let promote = crate::app::queue::dispatch_pending(app);
    crate::app::queue::refresh_queued_statuses(app);
    promote
}

pub fn handle_selected(app: &mut App, raw: u64) -> Task<Message> {
    if let Some(idx) = app.tabs.iter().position(|t| t.id.0 == raw) {
        app.active = idx;
    }
    Task::none()
}

// Thin shim preserved for call-site stability — canonical view lives in `easyscanlate-ui`.
pub fn titlebar_view(app: &crate::app::App) -> iced::Element<'_, crate::app::Message> {
    easyscanlate_ui::chrome::tabs::view(app).map(crate::app::Message::from)
}
