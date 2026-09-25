//! Periodic autosave of unsaved work (`project.xml` + inpaint delta).
//!
//! Lightweight delta stored centrally in `config_dir()/autosave/<stem>__<hash>/`
//! (sibling to `default-config.toml`), never next to the `.mmtl`:
//! - `project.xml` — current in-memory `Project` serialization
//! - `inpaint/<file>.png` + `inpaint_manifest.json` — current in-memory
//!   inpaint layers (the "not saved yet" delta; source images are NOT copied)
//! - `meta.toml` — `{ mmtl_path, saved_at_unix, source_mtime_unix }`, written last
//!
//! Recovery: on project open, if an autosave exists and is fresher than the
//! `.mmtl` mtime, the app shows an in-app modal offering
//! `Load autosave from <date time>`. `Load` merges the autosaved XML/delta
//! onto the just-loaded `.mmtl` images. Manual save or Discard deletes the
//! autosave dir.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use iced::Task;

use super::tab::{Tab, TabId};
use super::{App, Message, TabMessage};

/// Manifest entry mapping one autosaved PNG back to its patch.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct InpaintManifestEntry {
    file: String,
    image_id: u64,
    bounds: [f32; 4],
}

/// On-disk `meta.toml` for one project's autosave dir.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AutosaveMeta {
    pub mmtl_path: String,
    pub saved_at_unix: i64,
    pub source_mtime_unix: Option<i64>,
    pub app_version: String,
    /// Stable project identity; `None` for backups written before ids existed.
    #[serde(default)]
    pub project_id: Option<String>,
}

/// What the recovery check found (passed to the prompt modal).
#[derive(Debug, Clone)]
pub struct AutosaveFound {
    pub dir: PathBuf,
    pub saved_at_unix: i64,
    pub saved_at_display: String,
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn source_mtime_unix(mmtl_path: &Path) -> Option<i64> {
    std::fs::metadata(mmtl_path)
        .and_then(|m| m.modified())
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

/// Format unix seconds as `YYYY-MM-DD HH:MM:SS UTC` without extra deps.
pub fn format_saved_at(unix: i64) -> String {
    if unix <= 0 {
        return "unknown time".to_string();
    }
    let secs = unix.max(0) as u64;
    let days = (secs / 86400) as i64;
    let rem = (secs % 86400) as i64;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Howard Hinnant's civil_from_days.
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 }.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02} UTC")
}

/// Write the lightweight delta for `mmtl_path`. Called off-thread via
/// `spawn_blocking`. `meta.toml` is written last so a torn write is ignored.
pub fn write_autosave(
    project: &easyscanlate_model::Project,
    inpaint: &[easyscanlate_mmtl::InpaintImageData],
    mmtl_path: &Path,
) -> Result<PathBuf, String> {
    let dir = easyscanlate_settings::autosave_dir_for(mmtl_path);
    std::fs::create_dir_all(dir.join("inpaint")).map_err(|e| e.to_string())?;
    let xml = easyscanlate_mmtl::to_xml_string(project).map_err(|e| e.to_string())?;
    // project.xml atomically.
    let tmp_xml = dir.join("project.xml.tmp");
    std::fs::write(&tmp_xml, xml.as_bytes()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp_xml, dir.join("project.xml")).map_err(|e| e.to_string())?;

    // Remove stale patches, then write current set + manifest.
    if let Ok(entries) = std::fs::read_dir(dir.join("inpaint")) {
        for e in entries.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let mut manifest: Vec<InpaintManifestEntry> = Vec::with_capacity(inpaint.len());
    for (idx, patch) in inpaint.iter().enumerate() {
        let file = format!("{}_{idx}.png", patch.image_id.0);
        let rgba = image::RgbaImage::from_raw(patch.width, patch.height, patch.rgba.clone())
            .ok_or_else(|| "invalid rgba size".to_string())?;
        let mut buf = Vec::new();
        {
            use image::ImageEncoder;
            let enc = image::codecs::png::PngEncoder::new(&mut buf);
            enc.write_image(
                rgba.as_raw(),
                patch.width,
                patch.height,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| e.to_string())?;
        }
        let tmp = dir.join("inpaint").join(format!("{file}.tmp"));
        std::fs::write(&tmp, &buf).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, dir.join("inpaint").join(&file)).map_err(|e| e.to_string())?;
        manifest.push(InpaintManifestEntry {
            file,
            image_id: patch.image_id.0,
            bounds: patch.bounds,
        });
    }
    let manifest_str = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    let tmp_man = dir.join("inpaint_manifest.json.tmp");
    std::fs::write(&tmp_man, manifest_str.as_bytes()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp_man, dir.join("inpaint_manifest.json")).map_err(|e| e.to_string())?;

    let meta = AutosaveMeta {
        mmtl_path: mmtl_path.to_string_lossy().to_string(),
        saved_at_unix: now_unix(),
        source_mtime_unix: source_mtime_unix(mmtl_path),
        app_version: crate::updater::get_current_version(),
        project_id: project.project_id().map(str::to_owned),
    };
    let meta_str = toml::to_string(&meta).map_err(|e| e.to_string())?;
    let tmp_meta = dir.join("meta.toml.tmp");
    std::fs::write(&tmp_meta, meta_str.as_bytes()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp_meta, dir.join("meta.toml")).map_err(|e| e.to_string())?;
    // The project may have moved since the last autosave: drop any stale dir
    // holding the same project id so orphans don't accumulate.
    if let Some(pid) = project.project_id() {
        remove_stale_id_dirs(&dir, pid);
    }
    Ok(dir)
}

/// Delete autosave dirs (other than `keep_dir`) whose `meta.toml` carries the
/// same project id — leftovers from a move/rename. Best-effort.
fn remove_stale_id_dirs(keep_dir: &Path, project_id: &str) {
    if project_id.is_empty() {
        return;
    }
    let root = easyscanlate_settings::autosave_root();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    for e in entries.flatten() {
        let dir = e.path();
        if dir == keep_dir {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(dir.join("meta.toml")) else {
            continue;
        };
        let Ok(meta): Result<AutosaveMeta, _> = toml::from_str(&text) else {
            continue;
        };
        if meta.project_id.as_deref() == Some(project_id) {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

/// Locate the autosave dir + meta for `mmtl_path`, if complete.
///
/// Fast path is the hash dir for the current path; fallback scans
/// `autosave_root()` for a `meta.toml` with the same `project_id` so a
/// moved/renamed project still finds its backup. Duplicate ids (Explorer
/// copy) resolve newest-wins.
pub fn find_autosave(mmtl_path: &Path, project_id: Option<&str>) -> Option<(PathBuf, AutosaveMeta)> {
    let dir = easyscanlate_settings::autosave_dir_for(mmtl_path);
    if let Ok(meta_str) = std::fs::read_to_string(dir.join("meta.toml"))
        && let Ok(meta) = toml::from_str::<AutosaveMeta>(&meta_str)
        && dir.join("project.xml").exists()
    {
        // Fast path hits when the path (hence hash) is unchanged, or for
        // legacy backups without ids.
        if project_id.is_none_or(|pid| meta.project_id.as_deref().is_none_or(|m| m == pid)) {
            return Some((dir, meta));
        }
    }
    let wanted = project_id.filter(|s| !s.is_empty())?;
    find_autosave_by_id(&dir, wanted)
}

/// Scan `autosave_root()` for the freshest complete backup with `project_id`.
fn find_autosave_by_id(except_dir: &Path, project_id: &str) -> Option<(PathBuf, AutosaveMeta)> {
    let root = easyscanlate_settings::autosave_root();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return None;
    };
    let mut best: Option<(PathBuf, AutosaveMeta)> = None;
    for e in entries.flatten() {
        let dir = e.path();
        if dir == except_dir {
            continue;
        }
        let Ok(meta_str) = std::fs::read_to_string(dir.join("meta.toml")) else {
            continue;
        };
        let Ok(meta): Result<AutosaveMeta, _> = toml::from_str(&meta_str) else {
            continue;
        };
        if meta.project_id.as_deref() != Some(project_id) {
            continue;
        }
        if !dir.join("project.xml").exists() {
            continue;
        }
        let fresher = best
            .as_ref()
            .is_none_or(|(_, cur)| meta.saved_at_unix > cur.saved_at_unix);
        if fresher {
            best = Some((dir, meta));
        }
    }
    best
}

/// True when the autosave should prompt: it exists and is newer than the
/// last manual save (or the `.mmtl` is gone but unsaved work survived).
pub fn is_fresher(meta: &AutosaveMeta, mmtl_path: &Path) -> bool {
    let Ok(md) = std::fs::metadata(mmtl_path) else {
        return true;
    };
    let Ok(modified) = md.modified() else {
        return true;
    };
    let mtime = modified
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // Prompt when the autosave was written after the .mmtl mtime.
    meta.saved_at_unix > mtime
}

/// Delete the autosave dir for `mmtl_path` (manual save / discard path),
/// plus any orphan dir holding the same project id (moved project).
pub fn clear_autosave(mmtl_path: &Path, project_id: Option<&str>) {
    let dir = easyscanlate_settings::autosave_dir_for(mmtl_path);
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    if let Some(pid) = project_id.filter(|s| !s.is_empty()) {
        remove_stale_id_dirs(&dir, pid);
    }
}

fn clear_autosave_dir(dir: &Path) {
    if dir.exists() {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// Prune autosave dirs older than `max_age_secs` (best-effort, boot only).
pub fn prune_old_autosaves(max_age_secs: i64) {
    let root = easyscanlate_settings::autosave_root();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    let now = now_unix();
    for e in entries.flatten() {
        let meta_path = e.path().join("meta.toml");
        let Ok(text) = std::fs::read_to_string(&meta_path) else {
            continue;
        };
        let Ok(meta): Result<AutosaveMeta, _> = toml::from_str(&text) else {
            continue;
        };
        if now - meta.saved_at_unix > max_age_secs {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Raw inpaint payload cloned on the UI thread without decoding (mirrors
/// `mmtl::RawInpaint`): `Bytes` layers decode in the `spawn_blocking` write.
#[derive(Clone)]
enum RawInpaint {
    Rgba {
        image_id: easyscanlate_model::ImageId,
        bounds: [f32; 4],
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    },
    Bytes {
        image_id: easyscanlate_model::ImageId,
        bounds: [f32; 4],
        bytes: bytes::Bytes,
    },
}

/// Extract inpaint pixel payloads from a tab without decoding on the UI thread.
fn extract_inpaint_raw(tab: &Tab) -> Vec<RawInpaint> {
    let mut out = Vec::new();
    for loaded in &tab.images {
        let image_id = loaded.image_id;
        for layer in &loaded.inpaint {
            match &layer.handle {
                iced::widget::image::Handle::Rgba {
                    width,
                    height,
                    pixels,
                    ..
                } => out.push(RawInpaint::Rgba {
                    image_id,
                    bounds: layer.bounds,
                    width: *width,
                    height: *height,
                    pixels: pixels.to_vec(),
                }),
                iced::widget::image::Handle::Bytes(_id, bytes) => {
                    out.push(RawInpaint::Bytes {
                        image_id,
                        bounds: layer.bounds,
                        bytes: bytes.clone(),
                    });
                }
                _ => continue,
            }
        }
    }
    out
}

fn decode_raw_inpaint(raw: Vec<RawInpaint>) -> Vec<easyscanlate_mmtl::InpaintImageData> {
    let mut out = Vec::with_capacity(raw.len());
    for r in raw {
        match r {
            RawInpaint::Rgba { image_id, bounds, width, height, pixels } => {
                out.push(easyscanlate_mmtl::InpaintImageData {
                    image_id,
                    bounds,
                    width,
                    height,
                    rgba: pixels,
                });
            }
            RawInpaint::Bytes { image_id, bounds, bytes } => {
                if let Ok(img) = image::load_from_memory(&bytes) {
                    let rgba = img.to_rgba8();
                    let (w, h) = (rgba.width(), rgba.height());
                    out.push(easyscanlate_mmtl::InpaintImageData {
                        image_id,
                        bounds,
                        width: w,
                        height: h,
                        rgba: rgba.into_raw(),
                    });
                }
            }
        }
    }
    out
}

/// Global tick (from `subscription`): autosave every dirty project tab whose
/// throttle expired. Skips loading/exporting/busy tabs and untitled tabs.
pub fn handle_tick(app: &mut App) -> Task<Message> {
    if !easyscanlate_settings::autosave_enabled() {
        return Task::none();
    }
    let interval = easyscanlate_settings::autosave_interval_secs();
    let now = now_unix();
    let mut tasks = Vec::new();
    for tab in &mut app.tabs {
        if !tab.is_project() || !tab.dirty || tab.loading || tab.exporting || tab.autosave_busy {
            continue;
        }
        let Some(path) = tab.mmtl_path.clone() else {
            continue;
        };
        if now - tab.last_autosave_unix < interval as i64 {
            continue;
        }
        // Skip while heavy bulk work mutates the model mid-frame; the next
        // tick will catch the settled state.
        if tab.running || tab.inpainting {
            continue;
        }
        tab.autosave_busy = true;
        tab.last_autosave_unix = now;
        let tid = tab.id;
        // Stamp legacy projects so the backup carries a stable id.
        tab.project.ensure_project_id();
        let project = tab.project.clone();
        let raw = extract_inpaint_raw(tab);
        tasks.push(Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    // Decode + XML + PNG + FS all off the UI thread.
                    let inpaint = decode_raw_inpaint(raw);
                    write_autosave(&project, &inpaint, &path)
                        .map(|p| p.to_string_lossy().to_string())
                        .map_err(|e| e.to_string())
                })
                .await
                .unwrap_or_else(|e| Err(format!("autosave task failed: {e}")))
            },
            move |res| Message::Tab(tid, TabMessage::AutosaveDone(res)),
        ));
    }
    if tasks.is_empty() {
        Task::none()
    } else {
        Task::batch(tasks)
    }
}

pub fn handle_done(
    app: &mut App,
    tab_id: TabId,
    result: Result<String, String>,
) -> Task<Message> {
    let Some(idx) = app.tabs.iter().position(|t| t.id == tab_id) else {
        return Task::none();
    };
    app.tabs[idx].autosave_busy = false;
    match result {
        Ok(_) => {
            // Keep `dirty=true`: autosave is crash recovery, not a save.
            // A short status confirms the backup without implying safety.
            app.tabs[idx].last_autosave_unix = now_unix();
            if app.tabs[idx].status.starts_with("Autosaved") || app.tabs[idx].status.is_empty() {
                app.tabs[idx].status =
                    format!("Autosaved {}", format_saved_at(app.tabs[idx].last_autosave_unix));
            }
        }
        Err(e) => {
            // Never surface autosave failures modally; log to status only
            // when the tab is otherwise idle.
            eprintln!("[autosave] write failed: {e}");
        }
    }
    Task::none()
}

/// After a project finishes loading, check for a fresher autosave off-thread.
/// Shows the in-app modal when one is found.
pub fn check_after_load(
    _app: &App,
    tab_id: TabId,
    mmtl_path: PathBuf,
    project_id: Option<String>,
) -> Task<Message> {
    if !easyscanlate_settings::autosave_enabled() {
        return Task::none();
    }
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                let found = find_autosave(&mmtl_path, project_id.as_deref())
                    .filter(|(_, meta)| is_fresher(meta, &mmtl_path));
                found.map(|(dir, meta)| AutosaveFound {
                    dir,
                    saved_at_unix: meta.saved_at_unix,
                    saved_at_display: format_saved_at(meta.saved_at_unix),
                })
            })
            .await
            .unwrap_or(None)
        },
        move |found| Message::Tab(tab_id, TabMessage::AutosaveCheckDone(found)),
    )
}

pub fn handle_check_done(
    app: &mut App,
    tab_id: TabId,
    found: Option<AutosaveFound>,
) -> Task<Message> {
    let Some(found) = found else {
        return Task::none();
    };
    // Only prompt when the tab still exists and is clean (fresh load).
    let Some(idx) = app.tabs.iter().position(|t| t.id == tab_id) else {
        return Task::none();
    };
    if app.tabs[idx].loading || app.tabs[idx].dirty {
        return Task::none();
    }
    // One prompt at a time; a pending close takes precedence.
    if app.autosave_prompt.is_some() || app.pending_close.is_some() {
        return Task::none();
    }
    app.autosave_prompt = Some(super::AutosavePrompt {
        tab_id,
        saved_at_display: found.saved_at_display,
        dir: found.dir,
    });
    app.tabs[idx].status = format!(
        "Autosave found from {} — choose Load or Discard.",
        app.autosave_prompt.as_ref().map(|p| p.saved_at_display.as_str()).unwrap_or("")
    );
    Task::none()
}

/// Load the autosaved `project.xml` + inpaint delta onto the tab's current
/// images (source of absolute temp paths). Marks the tab dirty.
pub fn handle_load(app: &mut App, raw: u64) -> Task<Message> {
    let id = TabId(raw);
    let prompt = match app.autosave_prompt.clone() {
        Some(p) if p.tab_id == id => p,
        _ => return Task::none(),
    };
    let Some(idx) = app.tabs.iter().position(|t| t.id == id) else {
        app.autosave_prompt = None;
        return Task::none();
    };
    // Absolute image paths of the just-loaded .mmtl (temp dir).
    let mut id_to_path: HashMap<easyscanlate_model::ImageId, String> = HashMap::new();
    for m in app.tabs[idx].project.images() {
        id_to_path.insert(m.id, m.path.clone());
    }
    match load_autosave_into_tab(&mut app.tabs[idx], &prompt.dir, &id_to_path) {
        Ok(count) => {
            app.tabs[idx].dirty = true;
            app.tabs[idx].status = format!(
                "Loaded autosave from {} ({count} inpaint layer(s)). Save to keep it.",
                prompt.saved_at_display
            );
        }
        Err(e) => {
            app.tabs[idx].status = format!("Autosave load failed: {e}");
        }
    }
    // Keep the autosave files until the next manual save (crash during
    // review still recovers); only dismiss the prompt.
    app.autosave_prompt = None;
    Task::none()
}

pub fn handle_discard(app: &mut App, raw: u64) -> Task<Message> {
    let id = TabId(raw);
    let prompt = match app.autosave_prompt.clone() {
        Some(p) if p.tab_id == id => p,
        _ => return Task::none(),
    };
    let dir = prompt.dir.clone();
    app.autosave_prompt = None;
    if let Some(idx) = app.tabs.iter().position(|t| t.id == id) {
        app.tabs[idx].status = "Autosave discarded.".to_string();
    }
    // Directory removal off the UI thread (was sync `remove_dir_all`).
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || clear_autosave_dir(&dir))
                .await
                .unwrap_or(())
        },
        |_| Message::AutosaveCleared,
    )
}

pub fn handle_later(app: &mut App) -> Task<Message> {
    app.autosave_prompt = None;
    Task::none()
}

/// Called on successful manual save: the backup is now redundant.
pub fn clear_for_mmtl_path(mmtl_path: &Path, project_id: Option<String>) -> Task<Message> {
    let path = mmtl_path.to_path_buf();
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || clear_autosave(&path, project_id.as_deref()))
                .await
                .unwrap_or(())
        },
        |_| Message::AutosaveCleared,
    )
}

fn load_autosave_into_tab(
    tab: &mut Tab,
    dir: &Path,
    id_to_path: &HashMap<easyscanlate_model::ImageId, String>,
) -> Result<usize, String> {
    let xml = std::fs::read_to_string(dir.join("project.xml")).map_err(|e| e.to_string())?;
    let mut project: easyscanlate_model::Project =
        easyscanlate_mmtl::from_xml_str(&xml).map_err(|e| e.to_string())?;
    // Remap autosaved image paths onto the just-loaded temp absolute paths.
    // Images are immutable: ids must match 1:1 or the backup is stale.
    if project.image_count() != id_to_path.len() {
        return Err(format!(
            "image count mismatch (autosave {} vs open {})",
            project.image_count(),
            id_to_path.len()
        ));
    }
    let images: Vec<easyscanlate_model::ImageMeta> = project
        .images()
        .iter()
        .map(|m| {
            let path = id_to_path.get(&m.id).cloned().unwrap_or_else(|| m.path.clone());
            easyscanlate_model::ImageMeta {
                id: m.id,
                path,
                width: m.width,
                height: m.height,
            }
        })
        .collect();
    for m in &images {
        if !id_to_path.contains_key(&m.id) {
            return Err(format!("unknown image id {} in autosave", m.id.0));
        }
    }
    let ocr = std::mem::replace(
        &mut project.ocr,
        easyscanlate_model::OcrResult::from_raw(Vec::new(), 0),
    );
    let profiles = std::mem::take(&mut project.profiles);
    let styles = project.styles().clone();
    let view_quads = project.view_quads().clone();
    let extras = std::mem::take(&mut project.extras);
    let pid = project.project_id().map(str::to_owned);
    // Keep the tab's live id when the autosave predates ids (legacy).
    let pid = pid.or_else(|| tab.project.project_id().map(str::to_owned));
    let series = project.series().map(str::to_owned)
        .or_else(|| tab.project.series().map(str::to_owned));
    let next_image_id = images.iter().map(|m| m.id.0 + 1).max().unwrap_or(0);
    project = easyscanlate_model::Project::from_raw(
        pid,
        series,
        images,
        next_image_id,
        ocr,
        profiles,
        styles,
        view_quads,
        extras,
    );
    // Rebuild inpaint GPU layers from the delta manifest.
    let manifest_str =
        std::fs::read_to_string(dir.join("inpaint_manifest.json")).map_err(|e| e.to_string())?;
    let manifest: Vec<InpaintManifestEntry> =
        serde_json::from_str(&manifest_str).map_err(|e| e.to_string())?;
    let mut per_image: HashMap<easyscanlate_model::ImageId, Vec<easyscanlate_ui::loaded::InpaintLayer>> =
        HashMap::new();
    for entry in &manifest {
        let data = std::fs::read(dir.join("inpaint").join(&entry.file)).map_err(|e| e.to_string())?;
        let img = image::load_from_memory(&data).map_err(|e| e.to_string())?.to_rgba8();
        let (w, h) = (img.width(), img.height());
        let handle =
            iced::widget::image::Handle::from_rgba(w, h, bytes::Bytes::from(img.into_raw()));
        let image_id = easyscanlate_model::ImageId(entry.image_id);
        let quad = project
            .inpaint_for(image_id)
            .find(|p| p.bounds == entry.bounds)
            .and_then(|p| p.quad);
        per_image.entry(image_id).or_default().push(
            easyscanlate_ui::loaded::InpaintLayer {
                bounds: entry.bounds,
                quad,
                handle,
                width: w,
                height: h,
            },
        );
    }
    let count: usize = per_image.values().map(|v| v.len()).sum();
    tab.project = project;
    for loaded in &mut tab.images {
        loaded.inpaint = per_image.remove(&loaded.image_id).unwrap_or_default();
    }
    Ok(count)
}
