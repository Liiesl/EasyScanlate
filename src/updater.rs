//! Velopack updater — mirrors NeverLiieStatusBar/src/services/updater.rs
//! GithubSource points at the original EasyScanlate repo so old installers
//! polling `app/utils/update.py:74` (`Liiesl/EasyScanlate` `EasyScanlate-Installer.exe`)
//! discover the new Velopack release via the same tag.

use std::sync::mpsc;

use velopack::{sources, UpdateCheck, UpdateInfo, UpdateManager};

const GITHUB_REPO: &str = "https://github.com/Liiesl/EasyScanlate";

fn create_manager() -> Option<UpdateManager> {
    let source = sources::GithubSource::new(GITHUB_REPO, None, false);
    UpdateManager::new(source, None, None).ok()
}

pub fn check_for_updates() -> Option<UpdateInfo> {
    let um = create_manager()?;
    match um.check_for_updates().ok()? {
        UpdateCheck::UpdateAvailable(info) => Some(*info),
        _ => None,
    }
}

pub fn get_current_version() -> String {
    create_manager()
        .map(|um| um.get_current_version_as_string())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

pub fn download_updates(info: &UpdateInfo, progress_tx: mpsc::Sender<i16>) -> bool {
    let Some(um) = create_manager() else {
        return false;
    };
    um.download_updates(info, Some(progress_tx)).is_ok()
}

pub fn apply_updates(info: &UpdateInfo) -> bool {
    let Some(um) = create_manager() else {
        return false;
    };
    um.apply_updates_and_restart(info).is_ok()
}
