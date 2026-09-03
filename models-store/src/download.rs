//! Download helpers built on `fast-down-api`.
//!
//! All models are persisted under `easyscanlate_settings::models_dir()`.
//! `fast-down-api` handles resumable, concurrent range downloads via `.part`
//! and `.fd` sidecars next to the target file (`save_dir` + `filename`).

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use fast_down_api::{
    create_cancellation_token, create_channel, download, Event, PartialConfig,
};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::registry::ModelSpec;

/// Snapshot of download progress emitted by `fast-down-api`.
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    /// Model id that is being downloaded.
    pub id: String,
    /// Human filename.
    pub filename: String,
    /// Bytes downloaded so far.
    pub downloaded: u64,
    /// Total bytes (0 if unknown yet).
    pub total: u64,
    /// Percent 0.0..=100.0.
    pub percent: f64,
    /// Recent bytes/sec.
    pub bps: u64,
    /// Average bytes/sec.
    pub avg_bps: u64,
    /// Elapsed download time.
    pub elapsed: Duration,
    /// Estimated time remaining.
    pub eta: Option<Duration>,
}

/// Handle returned by `download_model_with_progress`: channel receiver plus cancellation token.
pub struct DownloadHandle {
    /// Receive `Event`s (including `Event::Progress` with `DownloadProgress`).
    pub rx: fast_down_api::Rx,
    /// Cancel the download cooperatively. Leaves `.part`/`.fd` for resume.
    pub token: CancellationToken,
    /// Expected final path after `Renamed`.
    pub final_path: PathBuf,
}

/// Build a `PartialConfig` for a given model spec, always targeting `models_dir()`.
///
/// `overwrite=false` so existing files are kept; set to `true` to force re-download.
fn config_for(spec: &ModelSpec, overwrite: bool) -> PartialConfig {
    PartialConfig {
        save_dir: Some(easyscanlate_settings::models_dir()),
        // `filename` is `String` in Config, `Option<String>` in PartialConfig
        filename: Some(spec.filename.to_string()),
        overwrite: Some(overwrite),
        // Reasonable defaults: 16 threads, resume enabled, 20 redirects for ModelScope/HF
        threads: Some(16),
        resume: Some(true),
        max_redirects: Some(20),
        ..Default::default()
    }
}

/// Ensure `models_dir()` exists.
fn ensure_dir() -> Result<PathBuf, String> {
    easyscanlate_settings::ensure_models_dir().map_err(|e| format!("failed to create models dir: {e}"))
}

/// Check preconditions for a download. Returns the Url on success.
fn prepare_download(spec: &ModelSpec) -> Result<Url, String> {
    if !spec.available {
        return Err(format!("model '{}' is not yet available (deferred)", spec.id));
    }
    if spec.url.is_empty() {
        return Err(format!("model '{}' has no URL", spec.id));
    }
    Url::parse(spec.url).map_err(|e| format!("invalid URL for {}: {e}", spec.id))
}

/// Download a model and drive the `fast-down-api` channel to completion,
/// returning the final file path on success.
///
/// This is a convenience blocking-on-channel helper: it spawns the `fast-down-api`
/// background task and `await`s `Event::Renamed`. Requires a Tokio runtime
/// (`#[tokio::main]` or `iced` with `tokio` feature). Resumable: if a `.part`
/// exists, `download` will resume automatically.
///
/// `overwrite=false` — if the file already exists, returns immediately without downloading.
///
/// Use `download_model_with_progress` if you need live progress updates in the UI.
pub async fn ensure_model(spec: &ModelSpec) -> Result<PathBuf, String> {
    let path = easyscanlate_settings::model_path(spec.filename);
    if path.exists() {
        return Ok(path);
    }
    download_model(spec).await
}

/// Download a model unconditionally (even if it already exists, it will be skipped
/// unless `overwrite=true`). Returns the final path after `Renamed`.
pub async fn download_model(spec: &ModelSpec) -> Result<PathBuf, String> {
    download_model_with_overwrite(spec, false).await
}

/// Like `download_model` but with explicit `overwrite` control.
pub async fn download_model_with_overwrite(spec: &ModelSpec, overwrite: bool) -> Result<PathBuf, String> {
    let url = prepare_download(spec)?;
    let _dir = ensure_dir()?;

    // If not overwriting and file already exists, succeed fast
    if !overwrite {
        let path = easyscanlate_settings::model_path(spec.filename);
        if path.exists() {
            return Ok(path);
        }
    }

    let (tx, rx) = create_channel();
    let token = create_cancellation_token();
    let cfg = config_for(spec, overwrite);

    download(url, cfg, tx, token.clone());

    loop {
        let event = rx.recv().await.map_err(|_| "download channel closed without completion".to_string())?;
        match event {
            Event::Renamed(p) => return Ok(p),
            Event::RenameFailed(e) => return Err(format!("rename failed for {}: {e}", spec.id)),
            Event::PrefetchError(e) => return Err(format!("prefetch failed for {}: {e}", spec.id)),
            Event::GenPathError(e) => return Err(format!("gen path failed for {}: {e}", spec.id)),
            Event::BuildClientError(e) => return Err(format!("client build failed for {}: {e}", spec.id)),
            Event::BuildPusherError(e) => return Err(format!("pusher build failed for {}: {e}", spec.id)),
            Event::ResumeError(e) => return Err(format!("resume failed for {}: {e:?}", spec.id)),
            // Progress and worker events are ignored in the simple helper; use the
            // `*_with_progress` variant to observe them.
            _ => continue,
        }
    }
}

/// Spawn a `fast-down-api` download and return a handle for progress observation.
///
/// The download runs in the background (detached `tokio::spawn` inside `fast-down-api`).
/// Drain `handle.rx.recv().await` until `Event::Renamed` or `Event::RenameFailed`.
/// Call `handle.token.cancel()` to cancel cooperatively — `.part`/`.fd` are kept
/// for a later resume.
///
/// This does **not** await completion; the caller drives the channel. For a
/// simple await, use `download_model` / `ensure_model`.
pub fn download_model_with_progress(spec: &ModelSpec) -> Result<DownloadHandle, String> {
    download_model_with_progress_overwrite(spec, false)
}

/// Like `download_model_with_progress` with explicit `overwrite`.
pub fn download_model_with_progress_overwrite(spec: &ModelSpec, overwrite: bool) -> Result<DownloadHandle, String> {
    let url = prepare_download(spec)?;
    let _dir = ensure_dir()?;

    let (tx, rx) = create_channel();
    let token = create_cancellation_token();
    let cfg = config_for(spec, overwrite);
    let final_path = easyscanlate_settings::model_path(spec.filename);

    download(url, cfg, tx, token.clone());

    Ok(DownloadHandle {
        rx,
        token,
        final_path,
    })
}

/// Ensure a model and report progress via the returned channel. The caller
/// awaits `Renamed` but can also read `Event::Progress` in between.
///
/// Returns `Ok(handle)` immediately; the download is already running.
/// If the file already exists and `overwrite=false`, no download is spawned and
/// the returned handle's channel is immediately closed — the caller should
/// check `model_path().exists()` first or use `ensure_model_with_progress`.
pub fn ensure_model_with_progress(spec: &ModelSpec) -> Result<Option<DownloadHandle>, String> {
    let path = easyscanlate_settings::model_path(spec.filename);
    if path.exists() {
        return Ok(None);
    }
    Ok(Some(download_model_with_progress(spec)?))
}

/// Download with live progress forwarded through a `std::mpsc::Sender`.
///
/// The sender receives `(percent 0..100, downloaded bytes, total bytes)` on every
/// `Event::Progress`. It is polled by the UI via `try_recv` on a timer
/// (`iced::time::every`). Completion is still awaited via `Event::Renamed`.
pub async fn download_model_with_sender(
    spec: &ModelSpec,
    sender: mpsc::Sender<(f32, u64, u64)>,
    overwrite: bool,
) -> Result<PathBuf, String> {
    let url = prepare_download(spec)?;
    let _dir = ensure_dir()?;
    if !overwrite {
        let path = easyscanlate_settings::model_path(spec.filename);
        if path.exists() {
            return Ok(path);
        }
    }
    let (tx, rx) = create_channel();
    let token = create_cancellation_token();
    let cfg = config_for(spec, overwrite);
    download(url, cfg, tx, token.clone());
    loop {
        let event = rx
            .recv()
            .await
            .map_err(|_| "download channel closed without completion".to_string())?;
        match event {
            Event::Progress(sample) => {
                let _ = sender.send((sample.percent as f32, sample.downloaded, sample.total));
            }
            Event::Renamed(p) => {
                // ensure final 100% is emitted (in case progress cadence missed it)
                // last sample's total may already be 100; send once more if not yet
                // we don't have sample here, so try to infer from file size if possible
                // send 100 with total from metadata if available
                if let Ok(meta) = std::fs::metadata(&p) {
                    let total = meta.len();
                    let _ = sender.send((100.0, total, total));
                } else {
                    let _ = sender.send((100.0, 0, 0));
                }
                return Ok(p);
            }
            Event::RenameFailed(e) => return Err(format!("rename failed for {}: {e}", spec.id)),
            Event::PrefetchError(e) => return Err(format!("prefetch failed for {}: {e}", spec.id)),
            Event::GenPathError(e) => return Err(format!("gen path failed for {}: {e}", spec.id)),
            Event::BuildClientError(e) => return Err(format!("client build failed for {}: {e}", spec.id)),
            Event::BuildPusherError(e) => return Err(format!("pusher build failed for {}: {e}", spec.id)),
            Event::ResumeError(e) => return Err(format!("resume failed for {}: {e:?}", spec.id)),
            _ => continue,
        }
    }
}

/// Like `download_model_with_sender` but skips download if the file already exists.
pub async fn ensure_model_with_sender(
    spec: &ModelSpec,
    sender: mpsc::Sender<(f32, u64, u64)>,
) -> Result<PathBuf, String> {
    let path = easyscanlate_settings::model_path(spec.filename);
    if path.exists() {
        return Ok(path);
    }
    download_model_with_sender(spec, sender, false).await
}

/// Helper to map a `fast_down_api::event::ProgressSample` + spec into our `DownloadProgress`.
pub fn progress_from_sample(id: &str, filename: &str, sample: &fast_down_api::ProgressSample) -> DownloadProgress {
    DownloadProgress {
        id: id.to_string(),
        filename: filename.to_string(),
        downloaded: sample.downloaded,
        total: sample.total,
        percent: sample.percent,
        bps: sample.bps,
        avg_bps: sample.avg_bps,
        elapsed: sample.elapsed,
        eta: sample.eta,
    }
}

/// Cancel-safe wrapper: cancel token after `handle` is dropped is a no-op (token is already detached).
impl Drop for DownloadHandle {
    fn drop(&mut self) {
        // Do not auto-cancel on drop; the background task is detached and will keep running
        // until the channel is drained. If caller wants cancel, they must call `token.cancel()`.
    }
}
