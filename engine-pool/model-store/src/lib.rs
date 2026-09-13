//! Model registry and download helpers for EasyScanlate.
//!
//! Models are persisted under the settings directory:
//! `easyscanlate_settings::models_dir()` (e.g. `%APPDATA%\easyscanlate\config\models\`
//! on Windows, `~/.config/easyscanlate/config/models/` on Linux) as a sibling
//! to the `default-config.toml` file. Downloads use `fast-down-api` for
//! resumable, concurrent range requests.

pub mod download;
pub mod registry;

pub use download::{
    download_model, download_model_with_progress, download_model_with_sender, ensure_model,
    ensure_model_with_progress, ensure_model_with_sender, DownloadHandle, DownloadProgress,
};
pub use registry::{ModelSpec, MODELS, get_model, model_path, models_dir, is_available};
