use std::sync::{Arc, Mutex, mpsc};
use iced::Task;

use super::{App, Message};

pub fn handle_check(app: &mut App) -> Task<Message> {
    app.update_error = None;
    Task::perform(
        async { tokio::task::spawn_blocking(crate::updater::check_for_updates).await.unwrap_or(None) },
        |info| Message::UpdateCheckResult(Box::new(info)),
    )
}

pub fn handle_download(app: &mut App) -> Task<Message> {
    if app.update_info.is_none() || app.update_downloading || app.update_ready {
        return Task::none();
    }
    let info = app.update_info.clone().unwrap();
    let (tx, rx) = mpsc::channel::<i16>();
    app.update_downloading = true;
    app.update_progress = 0;
    app.update_ready = false;
    app.update_rx = Some(Arc::new(Mutex::new(rx)));
    app.update_error = None;
    Task::perform(
        async move { tokio::task::spawn_blocking(move || crate::updater::download_updates(&info, tx)).await.unwrap_or(false) },
        |ok| if ok { Message::UpdatePoll } else { Message::UpdateDismiss },
    )
}

pub fn handle_apply(app: &mut App) -> Task<Message> {
    if let Some(info) = app.update_info.clone() {
        let _ = crate::updater::apply_updates(&info);
    }
    Task::none()
}

pub fn handle_dismiss(app: &mut App) -> Task<Message> {
    // Popup "Later" only hides the popup but keeps `update_info` so
    // Settings → Updates still offers Download. Dismissing from Settings
    // (popup already hidden) clears the available update entirely.
    if app.update_popup_visible {
        app.update_popup_visible = false;
        app.update_blur = None;
        app.update_pending_popup = false;
        return Task::none();
    }
    app.update_info = None;
    app.update_ready = false;
    app.update_downloading = false;
    app.update_progress = 0;
    app.update_rx = None;
    app.update_error = None;
    app.update_popup_visible = false;
    app.update_blur = None;
    app.update_pending_popup = false;
    Task::none()
}

pub fn handle_check_again(app: &mut App) -> Task<Message> {
    handle_check(app)
}

pub fn handle_check_result(app: &mut App, info: Option<crate::updater::UpdateInfo>) -> Task<Message> {
    app.update_info = info;
    if app.update_info.is_none() {
        app.update_error = None;
        app.update_popup_visible = false;
        app.update_blur = None;
        app.update_pending_popup = false;
        return Task::none();
    }
    // Don't interrupt an in-progress download or a ready-to-restart state,
    // and don't pop twice.
    if app.update_downloading || app.update_ready || app.update_popup_visible {
        return Task::none();
    }
    // While another backdrop capture is in flight, defer: the pending flag
    // makes the next BackdropReady-equivalent path re-evaluate. Simplest is
    // to stash and let the onboarding-finish / next check show it; but for
    // the common boot case nothing else is capturing, so show immediately.
    super::backdrop::begin_update(app)
}

/// Show a previously deferred update popup (e.g. after onboarding finish).
pub fn show_pending_popup(app: &mut App) -> Task<Message> {
    if !app.update_pending_popup {
        return Task::none();
    }
    app.update_pending_popup = false;
    if app.update_info.is_none() || app.update_popup_visible {
        return Task::none();
    }
    super::backdrop::begin_update(app)
}

pub fn handle_poll(app: &mut App) -> Task<Message> {
    if let Some(rx_arc) = app.update_rx.clone() {
        if let Ok(rx) = rx_arc.lock() {
            let mut last: Option<i16> = None;
            while let Ok(v) = rx.try_recv() {
                last = Some(v);
            }
            if let Some(p) = last {
                app.update_progress = p.clamp(0, 100);
                if p >= 100 {
                    app.update_downloading = false;
                    app.update_ready = true;
                }
            }
        }
        if app.update_downloading
            && let Ok(rx) = rx_arc.lock()
                && let Err(mpsc::TryRecvError::Disconnected) = rx.try_recv() {
                    app.update_downloading = false;
                    app.update_ready = true;
                    app.update_progress = 100;
                    app.update_rx = None;
                }
    }
    Task::none()
}
