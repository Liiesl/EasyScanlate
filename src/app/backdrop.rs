//! Blurred backdrop confined to the Settings / Manage Models panel rect.
//!
//! Real blur of the iced widgets behind the panel, without forking `iced`:
//! capture a window screenshot *before* the modal opens (clean base frame),
//! downscale to 0.25x on the CPU (the output is blurred anyway, so high
//! frequencies are invisible — the downscale itself is the first blur stage
//! and makes the gaussian ~16x cheaper), gaussian-blur at low res, then crop
//! to the modal window rect. Only the cropped blur is shown, directly behind
//! the translucent panel; everything outside stays live base + plain dim.
//!
//! Timing matters: the screenshot re-renders the *current* view, so it must
//! be dispatched while the modal is still closed (pending flag) and the
//! modal opens only when the blurred frame is ready (or immediately without
//! blur when there is no window, e.g. tests).

use iced::Task;
use iced::widget::image::Handle as ImageHandle;
use iced::window::Screenshot;

use super::{App, Message};

/// Downscale factor for the snapshot: 0.25x in each axis (16x fewer pixels).
pub const DOWNSCALE: f32 = 0.25;
/// Gaussian sigma applied *at low res*; ~7px here ≈ ~28px at native scale.
pub const BLUR_SIGMA: f32 = 7.0;
/// Manage Models modal design size (mirrors `ui/src/manage_models.rs`, which
/// renders `scale::s(MODAL_WIDTH) × scale::s(MODAL_HEIGHT)`).
const MANAGE_W: f32 = 540.0;
const MANAGE_H: f32 = 500.0;
/// Loading splash card size (mirrors `src/app/view.rs`, which renders
/// `scale::s(520.0) × scale::s(280.0)` with `rounded(scale::s(16.0))`).
const LOADING_W: f32 = 520.0;
const LOADING_H: f32 = 280.0;

/// Which modal the pending/ready backdrop belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackdropKind {
    Settings,
    ManageModels,
    Loading,
}

/// A deferred project-load trigger, stashed while the clean-base screenshot
/// is captured and replayed on `BackdropReady`. Payloads are plain data so
/// the original handlers run unchanged (dedup guards keep replays safe).
#[derive(Debug, Clone)]
pub(crate) enum PendingLoad {
    /// File-picker result: `mmtl::handle_open_picked(tab_id, Some(path))`.
    OpenPicked {
        tab_id: super::tab::TabId,
        path: String,
    },
    /// Recent-project click.
    Recent(String),
    /// IPC / drag-drop paths.
    External(Vec<String>),
    /// New-project Create button.
    Create,
}

impl PendingLoad {
    fn is_empty(&self) -> bool {
        matches!(self, PendingLoad::External(paths) if paths.is_empty())
    }
}

/// Fullscreen low-res blurred frame plus the geometry needed to crop the
/// panel rect out of it later (re-crop is microseconds, so Manage Models can
/// reuse Settings' capture with its own rect).
#[derive(Debug, Clone)]
pub struct CapturedBackdrop {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    scale_factor: f32,
    win_w: f32,
    win_h: f32,
    titlebar_h: f32,
}

/// Dispatch a window screenshot; the result comes back as
/// `Message::BackdropCaptured(shot, kind)` while the modal stays closed.
pub fn capture_task(app: &App, kind: BackdropKind) -> Task<Message> {
    let Some(id) = app.frame.primary_window() else {
        return Task::none();
    };
    iced::window::screenshot(id).map(move |shot| Message::BackdropCaptured(Box::new(shot), kind))
}

/// `BackdropCaptured` handler: blur off-thread, then open the modal.
/// Keeps the modal closed until `BackdropReady` so the capture stays clean.
pub fn handle_captured(app: &mut App, shot: Screenshot, kind: BackdropKind) -> Task<Message> {
    app.backdrop_pending = Some(kind);
    let titlebar_h = app.frame.config().title_bar_height;
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || blur_fullscreen(&shot, titlebar_h))
                .await
                .unwrap_or(None)
        },
        move |frame| Message::BackdropReady(frame, kind),
    )
}

/// `BackdropReady` handler: store the fullscreen frame, crop the panel rect
/// for display, and open the modal. For `Loading`, replays the stashed
/// trigger (which creates the loading tab) and drops the capture when no
/// loading overlay resulted (dedup / failure / cancelled).
pub fn handle_ready(
    app: &mut App,
    frame: Option<CapturedBackdrop>,
    kind: BackdropKind,
) -> Task<Message> {
    app.backdrop_pending = None;
    if kind == BackdropKind::Loading {
        if let Some(f) = frame {
            app.loading_blur = crop_for(&f, kind);
            app.backdrop_frame = Some(f);
        } else {
            app.loading_blur = None;
        }
        let task = app
            .pending_load
            .take()
            .map(|op| run_pending(app, op))
            .unwrap_or(Task::none());
        if !is_loading_now(app) {
            app.loading_blur = None;
            app.backdrop_frame = None;
        }
        return task;
    }
    if let Some(f) = frame {
        app.backdrop_blur = crop_for(&f, kind);
        app.backdrop_frame = Some(f);
    }
    match kind {
        BackdropKind::Settings => {
            app.settings_open = true;
        }
        BackdropKind::ManageModels => {
            app.manage_models_open = true;
            app.manage_models_search.clear();
        }
        BackdropKind::Loading => unreachable!("handled above"),
    }
    Task::none()
}

/// Entry point for every project-load trigger that shows the loading splash:
/// captures the clean base first (stashing `op`), then replays it on
/// `BackdropReady` so the screenshot never contains the overlay. When a
/// capture is already available (or impossible/pending), runs `op`
/// immediately — flat when there is nothing to blur from.
pub fn begin_load(app: &mut App, op: PendingLoad) -> Task<Message> {
    if op.is_empty() {
        return run_pending(app, op);
    }
    if let Some(frame) = app.backdrop_frame.clone() {
        app.loading_blur = crop_for(&frame, BackdropKind::Loading);
        let task = run_pending(app, op);
        if !is_loading_now(app) {
            app.loading_blur = None;
        }
        return task;
    }
    if app.frame.primary_window().is_none() || app.backdrop_pending.is_some() {
        app.loading_blur = None;
        return run_pending(app, op);
    }
    app.pending_load = Some(op);
    app.loading_blur = None;
    app.backdrop_pending = Some(BackdropKind::Loading);
    capture_task(app, BackdropKind::Loading)
}

/// Runs a stashed load trigger (original handlers, unchanged).
fn run_pending(app: &mut App, op: PendingLoad) -> Task<Message> {
    match op {
        PendingLoad::OpenPicked { tab_id, path } => {
            super::mmtl::handle_open_picked(app, tab_id, Some(path))
        }
        PendingLoad::Recent(path) => super::mmtl::handle_recent_open(app, path),
        PendingLoad::External(paths) => super::mmtl::handle_external_opens(app, paths),
        PendingLoad::Create => super::new_project::handle_create(app),
    }
}

/// Mirrors the overlay condition in `src/app/view.rs`: an active non-home tab
/// still in its loading placeholder.
fn is_loading_now(app: &App) -> bool {
    !app.active_is_home() && app.tabs.get(app.active).is_some_and(|t| t.loading)
}

/// Re-crop the stored fullscreen frame for `kind` (microseconds; no
/// re-capture). Used when Manage Models opens over an already-captured
/// Settings backdrop.
pub fn recrop(app: &mut App, kind: BackdropKind) {
    if let Some(frame) = app.backdrop_frame.clone() {
        app.backdrop_blur = crop_for(&frame, kind);
    } else {
        app.backdrop_blur = None;
    }
}

/// Downscale + gaussian-blur the whole screenshot at low res.
/// Returns `None` on empty/degenerate input so callers open flat.
fn blur_fullscreen(shot: &Screenshot, titlebar_h: f32) -> Option<CapturedBackdrop> {
    let w = shot.size.width;
    let h = shot.size.height;
    let sf = shot.scale_factor;
    if w == 0 || h == 0 || sf <= 0.0 || shot.rgba.is_empty() {
        return None;
    }
    let img = image::RgbaImage::from_raw(w, h, shot.rgba.to_vec())?;
    let dw = ((w as f32 * DOWNSCALE).round() as u32).max(1);
    let dh = ((h as f32 * DOWNSCALE).round() as u32).max(1);
    let small = image::imageops::resize(&img, dw, dh, image::imageops::FilterType::Triangle);
    let blurred = image::imageops::blur(&small, BLUR_SIGMA);
    let (bw, bh) = blurred.dimensions();
    Some(CapturedBackdrop {
        width: bw,
        height: bh,
        rgba: blurred.into_raw(),
        scale_factor: sf,
        win_w: w as f32 / sf,
        win_h: h as f32 / sf,
        titlebar_h,
    })
}

/// Panel rect in low-res pixels: the modal window occupies the content area
/// (full window minus titlebar strip minus `OUTER_PADDING` frame). Settings
/// is the centered 80% cell (`FillPortion` 1-8-1 split both axes);
/// Manage Models is the fixed size centered via `center()`.
fn panel_rect_lowres(frame: &CapturedBackdrop, kind: BackdropKind) -> Option<(u32, u32, u32, u32)> {
    let pad = easyscanlate_ui::layout::OUTER_PADDING;
    let cx = pad;
    let cy = frame.titlebar_h + pad;
    let cw = frame.win_w - 2.0 * pad;
    let ch = frame.win_h - frame.titlebar_h - 2.0 * pad;
    if cw <= 0.0 || ch <= 0.0 {
        return None;
    }
    let (x, y, w, h) = match kind {
        BackdropKind::Settings => (cx + cw * 0.1, cy + ch * 0.1, cw * 0.8, ch * 0.8),
        BackdropKind::ManageModels => centered_fixed(
            cx,
            cy,
            cw,
            ch,
            easyscanlate_ui::scale::s(MANAGE_W),
            easyscanlate_ui::scale::s(MANAGE_H),
        ),
        BackdropKind::Loading => centered_fixed(
            cx,
            cy,
            cw,
            ch,
            easyscanlate_ui::scale::s(LOADING_W),
            easyscanlate_ui::scale::s(LOADING_H),
        ),
    };
    // Logical → physical → low-res, clamped into the frame.
    let k = frame.scale_factor * DOWNSCALE;
    let fw = frame.width as f32;
    let fh = frame.height as f32;
    let x0 = (x * k).round().clamp(0.0, fw - 1.0);
    let y0 = (y * k).round().clamp(0.0, fh - 1.0);
    let mut w0 = (w * k).round().clamp(1.0, fw - x0);
    let mut h0 = (h * k).round().clamp(1.0, fh - y0);
    if x0 + w0 > fw {
        w0 = fw - x0;
    }
    if y0 + h0 > fh {
        h0 = fh - y0;
    }
    if w0 < 1.0 || h0 < 1.0 {
        return None;
    }
    Some((x0 as u32, y0 as u32, w0 as u32, h0 as u32))
}

/// Centers a fixed-size modal rect inside the content area.
fn centered_fixed(cx: f32, cy: f32, cw: f32, ch: f32, w: f32, h: f32) -> (f32, f32, f32, f32) {
    (cx + (cw - w) / 2.0, cy + (ch - h) / 2.0, w, h)
}

/// Crop the panel rect out of the blurred fullscreen frame for display.
pub fn crop_for(frame: &CapturedBackdrop, kind: BackdropKind) -> Option<ImageHandle> {
    let (x, y, w, h) = panel_rect_lowres(frame, kind)?;
    let img = image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone())?;
    let cropped = image::imageops::crop_imm(&img, x, y, w, h).to_image();
    let (bw, bh) = cropped.dimensions();
    Some(ImageHandle::from_rgba(
        bw,
        bh,
        bytes::Bytes::from(cropped.into_raw()),
    ))
}
