//! Velopack updater — mirrors NeverLiieStatusBar/src/services/updater.rs
//! GithubSource points at the original EasyScanlate repo so old installers
//! polling `app/utils/update.py:74` (`Liiesl/EasyScanlate` `EasyScanlate-Installer.exe`)
//! discover the new Velopack release via the same tag.
//!
//! Gated behind the `updates` feature so `--no-default-features
//! --features test-ui` skips velopack (and its ureq/zip subtrees) entirely.
//! The stub below mirrors the real API so `app/update.rs` and `app/boot.rs`
//! compile unchanged in both configs.

use std::sync::mpsc;

#[cfg(feature = "updates")]
use velopack::{sources, UpdateCheck, UpdateManager};

/// Update descriptor. Real type when `updates` is on, empty stub otherwise
/// (never constructed — `check_for_updates` always returns `None`).
#[cfg(feature = "updates")]
pub use velopack::UpdateInfo;
#[cfg(not(feature = "updates"))]
#[derive(Debug, Clone, Default)]
pub struct UpdateInfo;

#[cfg(feature = "updates")]
const GITHUB_REPO: &str = "https://github.com/dotliie/EasyScanlate-test";

#[cfg(feature = "updates")]
fn create_manager() -> Option<UpdateManager> {
    let source = sources::GithubSource::new(GITHUB_REPO, None, false);
    UpdateManager::new(source, None, None).ok()
}

#[cfg(feature = "updates")]
pub fn check_for_updates() -> Option<UpdateInfo> {
    let um = create_manager()?;
    match um.check_for_updates().ok()? {
        UpdateCheck::UpdateAvailable(info) => Some(*info),
        _ => None,
    }
}

#[cfg(feature = "updates")]
pub fn get_current_version() -> String {
    create_manager()
        .map(|um| um.get_current_version_as_string())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

#[cfg(feature = "updates")]
pub fn download_updates(info: &UpdateInfo, progress_tx: mpsc::Sender<i16>) -> bool {
    let Some(um) = create_manager() else {
        return false;
    };
    um.download_updates(info, Some(progress_tx)).is_ok()
}

#[cfg(feature = "updates")]
pub fn apply_updates(info: &UpdateInfo) -> bool {
    let Some(um) = create_manager() else {
        return false;
    };
    um.apply_updates_and_restart(info).is_ok()
}

#[cfg(not(feature = "updates"))]
pub fn check_for_updates() -> Option<UpdateInfo> {
    None
}

#[cfg(not(feature = "updates"))]
pub fn get_current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg(not(feature = "updates"))]
pub fn download_updates(_info: &UpdateInfo, _progress_tx: mpsc::Sender<i16>) -> bool {
    false
}

#[cfg(not(feature = "updates"))]
pub fn apply_updates(_info: &UpdateInfo) -> bool {
    false
}
