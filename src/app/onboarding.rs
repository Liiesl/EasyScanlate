use std::sync::{Arc, Mutex, mpsc};

use iced::Task;

#[cfg(feature = "models")]
use easyscanlate_models::registry::ModelSpec;
/// Stub spec for builds without the `models` feature (test-ui): the
/// mandatory list is always empty so onboarding has nothing to download.
/// Only `id` is read by the shared `handle_model_done` path.
#[cfg(not(feature = "models"))]
#[derive(Debug, Clone, Copy)]
pub struct ModelSpec {
    pub id: &'static str,
    pub description: &'static str,
}
use easyscanlate_ui::state::ModelDownloadStatus;

use super::{App, Message};

/// Runtime state for the blocking onboarding wizard.
#[derive(Debug)]
pub struct OnboardingState {
    pub step: u8, // 0..4
    pub downloading: bool,
    pub models: Vec<(String, String, ModelDownloadStatus)>, // (id, description, status)
    pub error: Option<String>,
}

impl OnboardingState {
    // Dead in builds without `models` (test-ui bypasses onboarding in `App::new`).
    #[cfg_attr(not(feature = "models"), allow(dead_code))]
    pub fn new() -> Self {
        let mut s = Self {
            step: 0,
            downloading: false,
            models: Vec::new(),
            error: None,
        };
        s.refresh_from_disk();
        s
    }

    pub fn refresh_from_disk(&mut self) {
        let specs = mandatory_specs();
        self.models = specs
            .into_iter()
            .map(|spec| {
                let status = if is_present(spec) {
                    ModelDownloadStatus::Done
                } else {
                    ModelDownloadStatus::NotStarted
                };
                (spec.id.to_string(), spec.description.to_string(), status)
            })
            .collect();
    }

    #[cfg(not(feature = "models"))]
    pub fn overall_progress(&self) -> f32 {
        if self.models.is_empty() {
            return 1.0;
        }
        // No byte weights without models-store: count-based only.
        let done = self.models.iter().filter(|(_, _, s)| matches!(s, ModelDownloadStatus::Done)).count() as f32;
        done / self.models.len() as f32
    }

    #[cfg(feature = "models")]
    pub fn overall_progress(&self) -> f32 {
        if self.models.is_empty() {
            return 1.0;
        }
        // Byte-weighted progress: weighted by total bytes when known.
        // Fallback to count-based when no totals are known yet.
        let mut known_totals: Vec<u64> = Vec::new();
        for (id, _, status) in &self.models {
            match status {
                ModelDownloadStatus::Done => {
                    if let Some(spec) = easyscanlate_models::get_model(id) {
                        let path = easyscanlate_settings::model_path(spec.filename);
                        if let Ok(meta) = std::fs::metadata(&path) {
                            let len = meta.len();
                            if len > 0 {
                                known_totals.push(len);
                            }
                        }
                    }
                }
                ModelDownloadStatus::Downloading { total, .. } if *total > 0 => {
                    known_totals.push(*total);
                }
                _ => {}
            }
        }
        let has_known = !known_totals.is_empty();
        if has_known {
            let avg_known = known_totals.iter().sum::<u64>() as f64 / known_totals.len() as f64;
            let mut sum_w: f64 = 0.0;
            let mut sum_pw: f64 = 0.0;
            for (id, _, status) in &self.models {
                let (w, p) = match status {
                    ModelDownloadStatus::Done => {
                        let w = if let Some(spec) = easyscanlate_models::get_model(id) {
                            let path = easyscanlate_settings::model_path(spec.filename);
                            std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) as f64
                        } else {
                            0.0
                        };
                        let w = if w > 0.0 { w } else { avg_known };
                        (w, 1.0)
                    }
                    ModelDownloadStatus::Downloading { percent, total, .. } => {
                        let w = if *total > 0 { *total as f64 } else { avg_known };
                        let p = (*percent as f64 / 100.0).clamp(0.0, 1.0);
                        (w, p)
                    }
                    ModelDownloadStatus::NotStarted | ModelDownloadStatus::Failed(_) => (avg_known, 0.0),
                };
                sum_w += w;
                sum_pw += w * p;
            }
            if sum_w > 0.0 {
                return (sum_pw / sum_w) as f32;
            }
        }
        // fallback: count-based
        let done = self.models.iter().filter(|(_, _, s)| matches!(s, ModelDownloadStatus::Done)).count() as f32;
        done / self.models.len() as f32
    }

    pub fn is_all_done(&self) -> bool {
        self.models.iter().all(|(_, _, s)| matches!(s, ModelDownloadStatus::Done))
    }

    pub fn next_missing(&self) -> Option<ModelSpec> {
        #[cfg(not(feature = "models"))]
        {
            let _ = &self.models;
            return None;
        }
        #[cfg(feature = "models")]
        {
            for (id, _, status) in &self.models {
                if matches!(status, ModelDownloadStatus::Done) {
                    continue;
                }
                if let Some(spec) = easyscanlate_models::get_model(id) {
                    return Some(*spec);
                }
            }
            None
        }
    }
}

#[cfg(feature = "models")]
fn mandatory_specs() -> Vec<&'static ModelSpec> {
    // AOT is optional (lazy download on first AOT use), everything else
    // registered as available is mandatory for first-run.
    easyscanlate_models::MODELS
        .iter()
        .filter(|m| m.available && m.id != "aot-inpaint")
        .collect()
}

/// No models feature: nothing is mandatory, so the list is always empty
/// and the wizard (if ever shown) is immediately complete.
#[cfg(not(feature = "models"))]
fn mandatory_specs() -> Vec<&'static ModelSpec> {
    Vec::new()
}

#[cfg(feature = "models")]
fn is_present(spec: &ModelSpec) -> bool {
    // Only check canonical `models_dir` (where downloads land). Do NOT use
    // `resolve_model_path` fallbacks (workspace `../models`, exe-relative)
    // so that `cargo run` with local legacy models still triggers the
    // onboarding download flow for easier debugging. Runtime engine loading
    // still falls back via `resolve_model_path` — this gate is onboarding-only.
    easyscanlate_models::registry::is_downloaded(spec)
        || easyscanlate_models::registry::is_downloaded_with_legacy(spec)
}

#[cfg(not(feature = "models"))]
fn is_present(_spec: &ModelSpec) -> bool {
    true
}

// ---------------------------------------------------------------------------
// Handlers (called from `src/app.rs::update`)
// ---------------------------------------------------------------------------

pub fn handle_next(app: &mut App) -> Task<Message> {
    let Some(state) = app.onboarding.as_mut() else { return Task::none() };
    match state.step {
        0 => {
            state.step = 1;
            state.refresh_from_disk();
        }
        1 => {
            if !state.is_all_done() {
                // blocking: cannot advance until all mandatory downloaded
                state.error = Some("Download all models to continue".to_string());
                return Task::none();
            }
            state.step = 2;
        }
        2 => state.step = 3,
        3 => state.step = 4,
        4 => return handle_finish(app),
        _ => {}
    }
    Task::none()
}

pub fn handle_back(app: &mut App) -> Task<Message> {
    if let Some(state) = app.onboarding.as_mut()
        && state.step > 0 && state.step < 4 {
            state.step -= 1;
        }
    Task::none()
}

pub fn handle_skip_translation(app: &mut App) -> Task<Message> {
    // Skippable translation step: dismiss any inline connect form and advance
    app.connect_modal = None;
    let Some(state) = app.onboarding.as_mut() else { return Task::none() };
    if state.step == 3 {
        state.step = 4;
    } else {
        return handle_next(app);
    }
    Task::none()
}

pub fn handle_download_all(app: &mut App) -> Task<Message> {
    let Some(state) = app.onboarding.as_mut() else { return Task::none() };
    if state.downloading {
        return Task::none();
    }
    // Collect missing ids first to avoid borrow across Task
    let missing: Vec<String> = state
        .models
        .iter()
        .filter(|(_, _, s)| !matches!(s, ModelDownloadStatus::Done))
        .map(|(id, _, _)| id.clone())
        .collect();
    if missing.is_empty() {
        return Task::none();
    }
    state.downloading = true;
    state.error = None;
    // Mark first missing as downloading 0%
    if let Some(first) = missing.first() {
        for (id, _, st) in &mut state.models {
            if id == first {
                *st = ModelDownloadStatus::Downloading { percent: 0.0, downloaded: 0, total: 0 };
                break;
            }
        }
    }
    // Start sequential download chain: first task, rest will be chained via ModelDone
    let first_id = missing[0].clone();
    start_download(app, first_id)
}

fn start_download(app: &mut App, id: String) -> Task<Message> {
    let (tx, rx) = mpsc::channel();
    app.onboarding_rx = Some(Arc::new(Mutex::new(rx)));
    app.onboarding_active_id = Some(id.clone());
    download_task(id, tx)
}

#[cfg(not(feature = "models"))]
fn download_task(_id: String, _sender: mpsc::Sender<(f32, u64, u64)>) -> Task<Message> {
    // No downloader in this build: callers treat the queue as empty.
    Task::none()
}

#[cfg(feature = "models")]
fn download_task(id: String, sender: mpsc::Sender<(f32, u64, u64)>) -> Task<Message> {
    let spec = match easyscanlate_models::get_model(&id) {
        Some(s) => *s,
        None => return Task::none(),
    };
    Task::perform(
        async move {
            let res = easyscanlate_models::ensure_model_with_sender(&spec, sender).await;
            (id, res.map(|_| ()))
        },
        |(id, res)| Message::OnboardingModelDone { id, result: res.map_err(|e| e.to_string()) },
    )
}

pub fn handle_retry(app: &mut App, id: String) -> Task<Message> {
    let Some(state) = app.onboarding.as_mut() else { return Task::none() };
    // Mark as downloading
    for (mid, _, st) in &mut state.models {
        if *mid == id {
            *st = ModelDownloadStatus::Downloading { percent: 0.0, downloaded: 0, total: 0 };
            break;
        }
    }
    state.downloading = true;
    state.error = None;
    // clear previous channel if any (retry may be called while another download active;
    // sequential chain will serialize, but we reset for this id)
    start_download(app, id)
}

pub fn handle_model_done(app: &mut App, id: String, result: Result<(), String>) -> Task<Message> {
    // Clear active channel for this id
    if app.onboarding_active_id.as_deref() == Some(&id) {
        app.onboarding_rx = None;
        app.onboarding_active_id = None;
    }
    let Some(state) = app.onboarding.as_mut() else { return Task::none() };
    match result {
        Ok(()) => {
            for (mid, _, st) in &mut state.models {
                if *mid == id {
                    *st = ModelDownloadStatus::Done;
                    break;
                }
            }
        }
        Err(e) => {
            for (mid, _, st) in &mut state.models {
                if *mid == id {
                    *st = ModelDownloadStatus::Failed(e.clone());
                    break;
                }
            }
            state.error = Some(format!("{id} failed: {e}"));
            state.downloading = false;
            app.onboarding_rx = None;
            app.onboarding_active_id = None;
            return Task::none();
        }
    }
    // Find next missing and continue sequential chain
    let next = state.next_missing().map(|s| s.id.to_string());
    if let Some(nid) = next {
        for (mid, _, st) in &mut state.models {
            if *mid == nid && matches!(st, ModelDownloadStatus::NotStarted | ModelDownloadStatus::Failed(_)) {
                *st = ModelDownloadStatus::Downloading { percent: 0.0, downloaded: 0, total: 0 };
                break;
            }
        }
        start_download(app, nid)
    } else {
        state.downloading = false;
        state.error = None;
        app.onboarding_rx = None;
        app.onboarding_active_id = None;
        // All done — refresh once more
        state.refresh_from_disk();
        Task::none()
    }
}

pub fn handle_poll(app: &mut App) -> Task<Message> {
    let Some(state) = app.onboarding.as_mut() else { return Task::none() };
    let Some(rx_arc) = app.onboarding_rx.clone() else { return Task::none() };
    let Some(active_id) = app.onboarding_active_id.clone() else { return Task::none() };
    // Drain all pending progress values, keep last
    let mut last: Option<(f32, u64, u64)> = None;
    if let Ok(rx) = rx_arc.lock() {
        while let Ok(v) = rx.try_recv() {
            last = Some(v);
        }
    }
    if let Some((percent, downloaded, total)) = last {
        for (mid, _, st) in &mut state.models {
            if *mid == active_id {
                *st = ModelDownloadStatus::Downloading { percent, downloaded, total };
                break;
            }
        }
    }
    Task::none()
}

pub fn handle_toggle_theme(_app: &mut App) -> Task<Message> {
    let is_dark = easyscanlate_settings::get(|s| s.aurora_is_dark);
    let _ = easyscanlate_settings::modify(|s| s.aurora_is_dark = !is_dark);
    Task::none()
}

pub fn handle_font_size(_app: &mut App, inc: bool) -> Task<Message> {
    let cur = easyscanlate_settings::get(|s| s.ui_font_size);
    let next = if inc { cur.saturating_add(1) } else { cur.saturating_sub(1) };
    let clamped = next.clamp(8, 30);
    let _ = easyscanlate_settings::modify(|s| s.ui_font_size = clamped);
    Task::none()
}

pub fn handle_toggle_auto_style(_app: &mut App) -> Task<Message> {
    let cur = easyscanlate_settings::get(|s| s.auto_style_detect);
    let _ = easyscanlate_settings::modify(|s| s.auto_style_detect = !cur);
    Task::none()
}
pub fn handle_toggle_auto_sfx(_app: &mut App) -> Task<Message> {
    let cur = easyscanlate_settings::get(|s| s.auto_sfx_filter);
    let _ = easyscanlate_settings::modify(|s| s.auto_sfx_filter = !cur);
    Task::none()
}
pub fn handle_toggle_auto_inpaint(_app: &mut App) -> Task<Message> {
    let cur = easyscanlate_settings::get(|s| s.auto_inpaint);
    let _ = easyscanlate_settings::modify(|s| s.auto_inpaint = !cur);
    Task::none()
}

pub fn handle_finish(app: &mut App) -> Task<Message> {
    // Final validation: all mandatory must be present
    let all_done = app.onboarding.as_ref().map(|s| s.is_all_done()).unwrap_or(false);
    if !all_done {
        if let Some(s) = app.onboarding.as_mut() {
            s.error = Some("Cannot finish: models still missing".to_string());
        }
        return Task::none();
    }
    easyscanlate_settings::mark_onboarding_completed();
    app.onboarding = None;
    // Sync translation etc.
    crate::app::translation::sync_tx_from_store(app);
    app.active_tab_mut().status = "Setup complete — welcome!".to_string();
    // Deferred startup update check: show the popup now that the wizard is gone.
    crate::app::update::show_pending_popup(app)
}

pub fn handle_replay(app: &mut App) -> Task<Message> {
    #[cfg(not(feature = "models"))]
    {
        app.active_tab_mut().status =
            "Onboarding unavailable in this build (no models feature).".to_string();
        return Task::none();
    }
    #[cfg(feature = "models")]
    {
        easyscanlate_settings::reset_onboarding();
        app.onboarding = Some(OnboardingState::new());
        app.settings_open = false;
        app.manage_models_open = false;
        app.backdrop_blur = None;
        app.backdrop_frame = None;
        app.backdrop_pending = None;
        app.loading_blur = None;
        app.pending_load = None;
        app.export_blur = None;
        app.pending_export = None;
        app.update_blur = None;
        app.update_popup_visible = false;
        app.update_pending_popup = false;
        Task::none()
    }
}
