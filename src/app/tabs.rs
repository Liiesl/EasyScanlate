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
        crate::app::export::cancel_export_for_tab(app, id);
        app.engines.queue.cancel_pending_for_tab(crate::app::queue::owner_of(id));
        let freed = !app.engines.queue.cancel_running_for_tab(crate::app::queue::owner_of(id)).is_empty();
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
        if app.autosave_prompt.as_ref().is_some_and(|p| p.tab_id == id) {
            app.autosave_prompt = None;
        }
        return promote;
    }
    Task::none()
}

fn cleanup_queue_for_tabs(app: &mut App, ids: &[TabId]) {
    for rid in ids {
        app.engines.queue.cancel_pending_for_tab(crate::app::queue::owner_of(*rid));
        app.engines.queue.cancel_running_for_tab(crate::app::queue::owner_of(*rid));
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
            app.tabs[idx].project.ensure_project_id();
            let project = app.tabs[idx].project.clone();
            let tid = id;
            // Raw clone on the UI thread (no decode); `load_from_memory`
            // runs in the blocking task below so close-with-save never stalls.
            let raw: Vec<(easyscanlate_model::ImageId, [f32; 4], Option<(u32, u32, Vec<u8>)>, Option<bytes::Bytes>)> = {
                let tab = &app.tabs[idx];
                let mut out = Vec::new();
                for loaded in &tab.images {
                    let image_id = loaded.image_id;
                    for layer in &loaded.inpaint {
                        match &layer.handle {
                            iced::widget::image::Handle::Rgba { width, height, pixels, .. } => {
                                out.push((image_id, layer.bounds, Some((*width, *height, pixels.to_vec())), None));
                            }
                            iced::widget::image::Handle::Bytes(_id, bytes) => {
                                out.push((image_id, layer.bounds, None, Some(bytes.clone())));
                            }
                            _ => continue,
                        }
                    }
                }
                out
            };
            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        let mut project = project;
                        project.ensure_project_id();
                        let mut inpaint = Vec::with_capacity(raw.len());
                        for (image_id, bounds, rgba_opt, bytes_opt) in raw {
                            if let Some((width, height, rgba)) = rgba_opt {
                                inpaint.push(easyscanlate_mmtl::InpaintImageData { image_id, bounds, width, height, rgba });
                            } else if let Some(b) = bytes_opt
                                && let Ok(img) = image::load_from_memory(&b) {
                                    let rgba = img.to_rgba8();
                                    let (w, h) = (rgba.width(), rgba.height());
                                    inpaint.push(easyscanlate_mmtl::InpaintImageData { image_id, bounds, width: w, height: h, rgba: rgba.into_raw() });
                                }
                        }
                        easyscanlate_mmtl::save_mmtl(&project, &inpaint, &path).map(|_| path.to_string_lossy().to_string()).map_err(|e| e.to_string())
                    }).await.unwrap_or_else(|e| Err(format!("save task failed: {e}")))
                },
                move |res| Message::Tab(tid, TabMessage::MmtlSaved(res)),
            )
        } else {
            let tid = id;
            Task::perform(
                async move {
                    let file = rfd::AsyncFileDialog::new()
                        .add_filter("Manga Translation (.mmtl)", &["mmtl"])
                        .set_file_name("project.mmtl")
                        .save_file()
                        .await;
                    file.map(|f| f.path().to_string_lossy().to_string())
                },
                move |picked| Message::Tab(tid, TabMessage::MmtlSavePicked(picked)),
            )
        }
    } else {
        close_tab_immediate(app, id)
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
    for rid in &remove_ids {
        crate::app::export::cancel_export_for_tab(app, *rid);
    }
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
    for rid in &remove_ids {
        crate::app::export::cancel_export_for_tab(app, *rid);
    }
    cleanup_queue_for_tabs(app, &remove_ids);
    app.tabs.retain(|t| t.is_home());
    app.active = 0;
    app.pending_close = None;
    app.autosave_prompt = None;
    let promote = crate::app::queue::dispatch_pending(app);
    crate::app::queue::refresh_queued_statuses(app);
    promote
}

pub fn handle_selected(app: &mut App, raw: u64) -> Task<Message> {
    if let Some(idx) = app.tabs.iter().position(|t| t.id.0 == raw) {
        app.active = idx;
        // Per-tab scroll restore: the shared `panel-results-list` / `layer-list`
        // widget ids hold ephemeral iced state, so switching tabs (or
        // returning from minimize / focus-lost, which re-resolves the same
        // global ids) must re-apply the newly-active tab's stored anchors.
        // The main viewer needs no task: `build_viewer` feeds
        // `scroll_to(viewer_scroll)` every frame.
        let panel_anchor = app.tabs[idx].panel_scroll;
        let layer_anchor = app.tabs[idx].layer_scroll;
        return Task::batch([
            easyscanlate_ui::panel::results::restore_panel_scroll::<Message>(panel_anchor),
            easyscanlate_ui::panel::inpaint::restore_layer_scroll::<Message>(layer_anchor),
        ]);
    }
    Task::none()
}

// Thin shim preserved for call-site stability — canonical view lives in `easyscanlate-ui`.
pub fn titlebar_view(app: &crate::app::App) -> iced::Element<'_, crate::app::Message> {
    easyscanlate_ui::chrome::tabs::view(app).map(crate::app::Message::from)
}
