//! Blurred backdrop confined to the Settings / Manage Models panel rect.
//!
//! Real blur of the iced widgets behind the panel, without forking `iced`:
//! capture a window screenshot *before* the modal opens (clean base frame),
//! nearest-decimate to 0.125x on the CPU (the output is blurred anyway, so
//! high frequencies are invisible — the downscale itself is the first blur
//! stage and makes the blur ~64x cheaper), fast multi-pass box-blur at low
//! res (≈ gaussian), then row-slice crop to the modal window rect. Only the
//! cropped blur is shown, directly behind the translucent panel; everything
//! outside stays live base + plain dim.
//!
//! Timing matters: the screenshot re-renders the *current* view, so it must
//! be dispatched while the modal is still closed (pending flag) and the
//! modal opens only when the blurred frame is ready (or immediately without
//! blur when there is no window, e.g. tests).

use iced::Task;
use iced::widget::image::Handle as ImageHandle;
use iced::window::Screenshot;

use super::{App, Message};

/// Downscale factor for the snapshot: 0.125x in each axis (64x fewer pixels).
pub const DOWNSCALE: f32 = 0.125;
/// Blur sigma applied *at low res*; ~3.5px here ≈ ~28px at native scale
/// (same look as the old 7.0 @ 0.25x, at a quarter of the pixels).
pub const BLUR_SIGMA: f32 = 3.5;
/// Separable box-blur passes; 3 passes closely approximate a gaussian.
const BLUR_PASSES: u32 = 3;
/// Manage Models modal design size (mirrors `ui/src/manage_models.rs`, which
/// renders `scale::s(MODAL_WIDTH) × scale::s(MODAL_HEIGHT)`).
const MANAGE_W: f32 = 540.0;
const MANAGE_H: f32 = 500.0;
/// Loading splash card size (mirrors `src/app/view.rs`, which renders
/// `scale::s(520.0) × scale::s(280.0)` with `rounded(scale::s(16.0))`).
const LOADING_W: f32 = 520.0;
const LOADING_H: f32 = 280.0;
/// Export progress card size (mirrors `src/app/view.rs::export_overlay`,
/// same card geometry as the loading splash so the crop aligns).
const EXPORT_W: f32 = 520.0;
const EXPORT_H: f32 = 280.0;
/// Update-available popup card size (mirrors `ui/src/dialog/update.rs`, which
/// renders `scale::s(UPDATE_W) × scale::s(UPDATE_H)` with
/// `rounded(scale::s(16.0))`).
const UPDATE_W: f32 = 480.0;
const UPDATE_H: f32 = 320.0;

/// Which modal the pending/ready backdrop belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackdropKind {
    Settings,
    ManageModels,
    Loading,
    Export,
    Update,
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
///
/// `rgba` is refcounted so `backdrop_frame.clone()` in `begin_load`/`recrop`
/// is a pointer bump, not a multi-MB copy.
#[derive(Debug, Clone)]
pub struct CapturedBackdrop {
    width: u32,
    height: u32,
    rgba: bytes::Bytes,
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
    if kind == BackdropKind::Export {
        if let Some(f) = frame {
            app.export_blur = crop_for(&f, kind);
            app.backdrop_frame = Some(f);
        } else {
            app.export_blur = None;
        }
        let task = app
            .pending_export
            .take()
            .map(|op| super::export::start_pending_export(app, op))
            .unwrap_or(Task::none());
        if !is_exporting_now(app) {
            app.export_blur = None;
            // Keep `backdrop_frame` for settings reuse; export blur is single-use.
        }
        return task;
    }
    if kind == BackdropKind::Update {
        if frame.is_some() {
            if let Some(f) = frame {
                app.update_blur = crop_for(&f, kind);
                app.backdrop_frame = Some(f);
            }
        } else {
            app.update_blur = None;
        }
        // Only show when there is still an update to offer and nothing
        // blocking (onboarding defers via `update_pending_popup`).
        if app.update_info.is_some() && app.onboarding.is_none() {
            app.update_popup_visible = true;
        } else {
            app.update_blur = None;
            if app.update_info.is_some() && app.onboarding.is_some() {
                app.update_pending_popup = true;
            }
        }
        return Task::none();
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
        BackdropKind::Loading | BackdropKind::Export | BackdropKind::Update => unreachable!("handled above"),
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

/// Mirrors the export overlay condition in `src/app/view.rs`: active tab
/// currently exporting raster images.
fn is_exporting_now(app: &App) -> bool {
    !app.active_is_home() && app.tabs.get(app.active).is_some_and(|t| t.exporting)
}

/// Entry point for raster-export triggers that show the progress overlay:
/// captures the clean base first (stashing `op`), then starts the export on
/// `BackdropReady` so the screenshot never contains the overlay. Falls back
/// to an immediate start (flat, no blur) when headless/in tests or while
/// another capture is in flight.
pub fn begin_export(app: &mut App, op: super::export::PendingExport) -> Task<Message> {
    if app.frame.primary_window().is_none() || app.backdrop_pending.is_some() {
        app.export_blur = None;
        return super::export::start_pending_export(app, op);
    }
    // Reuse a fresh-enough capture? Export happens long after project load,
    // so the stored frame is stale — always recapture for a matching backdrop.
    app.pending_export = Some(op);
    app.export_blur = None;
    app.backdrop_pending = Some(BackdropKind::Export);
    capture_task(app, BackdropKind::Export)
}

/// Entry point for the update-available popup: captures the clean base
/// first so the screenshot never contains the popup, then shows it on
/// `BackdropReady`. Falls back to a flat (no-blur) popup when headless/in
/// tests or while another capture is in flight.
pub fn begin_update(app: &mut App) -> Task<Message> {
    if app.update_info.is_none() || app.update_popup_visible {
        return Task::none();
    }
    if app.onboarding.is_some() {
        app.update_pending_popup = true;
        return Task::none();
    }
    if let Some(frame) = app.backdrop_frame.clone() {
        app.update_blur = crop_for(&frame, BackdropKind::Update);
        app.update_popup_visible = true;
        app.update_pending_popup = false;
        return Task::none();
    }
    if app.frame.primary_window().is_none() || app.backdrop_pending.is_some() {
        app.update_blur = None;
        app.update_popup_visible = true;
        app.update_pending_popup = false;
        return Task::none();
    }
    app.update_blur = None;
    app.update_pending_popup = false;
    app.backdrop_pending = Some(BackdropKind::Update);
    capture_task(app, BackdropKind::Update)
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

/// Downscale + fast box-blur the whole screenshot at low res.
/// Returns `None` on empty/degenerate input so callers open flat.
///
/// Pipeline (all single-pass, no full-size copies):
/// 1. nearest-decimate straight from the screenshot bytes (no `RgbaImage`
///    copy of the full frame, no interpolation — invisible under blur),
/// 2. `BLUR_PASSES` separable box-blur passes (≈ gaussian, O(1) per px wrt
///    radius via sliding window).
fn blur_fullscreen(shot: &Screenshot, titlebar_h: f32) -> Option<CapturedBackdrop> {
    let w = shot.size.width;
    let h = shot.size.height;
    let sf = shot.scale_factor;
    if w == 0 || h == 0 || sf <= 0.0 || shot.rgba.is_empty() {
        return None;
    }
    let src: &[u8] = &shot.rgba;
    if src.len() < (w as usize) * (h as usize) * 4 {
        return None;
    }
    let dw = ((w as f32 * DOWNSCALE).round() as u32).max(1);
    let dh = ((h as f32 * DOWNSCALE).round() as u32).max(1);
    let mut small = downscale_nearest_rgba(src, w, h, dw, dh);
    let radius = box_radius_for_sigma(BLUR_SIGMA, BLUR_PASSES);
    if radius > 0 {
        box_blur_rgba_inplace(&mut small, dw, dh, radius, BLUR_PASSES);
    }
    Some(CapturedBackdrop {
        width: dw,
        height: dh,
        rgba: bytes::Bytes::from(small),
        scale_factor: sf,
        win_w: w as f32 / sf,
        win_h: h as f32 / sf,
        titlebar_h,
    })
}

/// Nearest-decimate `src` (`sw`×`sh` RGBA) to `dw`×`dh` by top-left sampling.
/// One pass over the destination; no interpolation.
fn downscale_nearest_rgba(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let (sw64, sh64, dw64, dh64) = (sw as u64, sh as u64, dw as u64, dh as u64);
    let mut out = vec![0u8; (dw as usize) * (dh as usize) * 4];
    let src_stride = sw as usize * 4;
    for dy in 0..dh as usize {
        let sy = (((dy as u64) * sh64) / dh64) as usize;
        let src_row = sy * src_stride;
        let dst_row = dy * dw as usize * 4;
        for dx in 0..dw as usize {
            let sx = (((dx as u64) * sw64) / dw64) as usize;
            let s = src_row + sx * 4;
            let d = dst_row + dx * 4;
            out[d] = src[s];
            out[d + 1] = src[s + 1];
            out[d + 2] = src[s + 2];
            out[d + 3] = src[s + 3];
        }
    }
    out
}

/// Box-blur radius per pass approximating `sigma` with `passes` passes.
/// Variance of `passes` box passes of width `w` ≈ `passes*(w²-1)/12`.
fn box_radius_for_sigma(sigma: f32, passes: u32) -> u32 {
    if sigma <= 0.0 || passes == 0 {
        return 0;
    }
    let n = passes as f32;
    let width = ((12.0 * sigma * sigma) / n + 1.0).sqrt().round().max(3.0) as u32;
    // Width is odd by construction below; radius = (width-1)/2, clamped.
    let radius = width.saturating_sub(1) / 2;
    radius.clamp(1, 32)
}

/// In-place multi-pass separable box blur on an RGBA buffer.
/// Each pass = horizontal slide into scratch + vertical slide back.
fn box_blur_rgba_inplace(buf: &mut Vec<u8>, w: u32, h: u32, radius: u32, passes: u32) {
    let (w, h) = (w as usize, h as usize);
    if buf.len() < w * h * 4 || w == 0 || h == 0 || radius == 0 || passes == 0 {
        return;
    }
    let r = radius as usize;
    let win = 2 * r + 1;
    let inv = 1.0 / win as f32;
    let mut scratch = vec![0u8; w * h * 4];
    for _ in 0..passes {
        // Horizontal: buf -> scratch.
        for y in 0..h {
            let row = y * w * 4;
            let mut acc = [0u32; 4];
            for k in 0..win {
                let xi = k.saturating_sub(r).min(w - 1);
                let o = row + xi * 4;
                acc[0] += buf[o] as u32;
                acc[1] += buf[o + 1] as u32;
                acc[2] += buf[o + 2] as u32;
                acc[3] += buf[o + 3] as u32;
            }
            for x in 0..w {
                let d = row + x * 4;
                scratch[d] = (acc[0] as f32 * inv).round() as u8;
                scratch[d + 1] = (acc[1] as f32 * inv).round() as u8;
                scratch[d + 2] = (acc[2] as f32 * inv).round() as u8;
                scratch[d + 3] = (acc[3] as f32 * inv).round() as u8;
                let x_out = x.saturating_sub(r).min(w - 1);
                let x_in = (x + r + 1).min(w - 1);
                let o = row + x_out * 4;
                let i = row + x_in * 4;
                acc[0] += buf[i] as u32 - buf[o] as u32;
                acc[1] += buf[i + 1] as u32 - buf[o + 1] as u32;
                acc[2] += buf[i + 2] as u32 - buf[o + 2] as u32;
                acc[3] += buf[i + 3] as u32 - buf[o + 3] as u32;
            }
        }
        // Vertical: scratch -> buf.
        for x in 0..w {
            let mut acc = [0u32; 4];
            for k in 0..win {
                let yi = k.saturating_sub(r).min(h - 1);
                let o = (yi * w + x) * 4;
                acc[0] += scratch[o] as u32;
                acc[1] += scratch[o + 1] as u32;
                acc[2] += scratch[o + 2] as u32;
                acc[3] += scratch[o + 3] as u32;
            }
            for y in 0..h {
                let d = (y * w + x) * 4;
                buf[d] = (acc[0] as f32 * inv).round() as u8;
                buf[d + 1] = (acc[1] as f32 * inv).round() as u8;
                buf[d + 2] = (acc[2] as f32 * inv).round() as u8;
                buf[d + 3] = (acc[3] as f32 * inv).round() as u8;
                let y_out = y.saturating_sub(r).min(h - 1);
                let y_in = (y + r + 1).min(h - 1);
                let o = (y_out * w + x) * 4;
                let i = (y_in * w + x) * 4;
                acc[0] += scratch[i] as u32 - scratch[o] as u32;
                acc[1] += scratch[i + 1] as u32 - scratch[o + 1] as u32;
                acc[2] += scratch[i + 2] as u32 - scratch[o + 2] as u32;
                acc[3] += scratch[i + 3] as u32 - scratch[o + 3] as u32;
            }
        }
    }
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
        BackdropKind::Export => centered_fixed(
            cx,
            cy,
            cw,
            ch,
            easyscanlate_ui::scale::s(EXPORT_W),
            easyscanlate_ui::scale::s(EXPORT_H),
        ),
        BackdropKind::Update => centered_fixed(
            cx,
            cy,
            cw,
            ch,
            easyscanlate_ui::scale::s(UPDATE_W),
            easyscanlate_ui::scale::s(UPDATE_H),
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
///
/// Row-slice copy only (no full-frame clone, no intermediate image).
pub fn crop_for(frame: &CapturedBackdrop, kind: BackdropKind) -> Option<ImageHandle> {
    let (x, y, w, h) = panel_rect_lowres(frame, kind)?;
    let (x, y, w, h) = (x as usize, y as usize, w as usize, h as usize);
    let fw = frame.width as usize;
    let fh = frame.height as usize;
    if x + w > fw || y + h > fh || w == 0 || h == 0 {
        return None;
    }
    let src: &[u8] = &frame.rgba;
    if src.len() < fw * fh * 4 {
        return None;
    }
    let w_bytes = w * 4;
    let mut dst = Vec::with_capacity(w * h * 4);
    for row in 0..h {
        let start = ((y + row) * fw + x) * 4;
        dst.extend_from_slice(&src[start..start + w_bytes]);
    }
    Some(ImageHandle::from_rgba(
        w as u32,
        h as u32,
        bytes::Bytes::from(dst),
    ))
}
