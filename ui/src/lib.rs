//! iced widgets. Everything here reads the app through [`UiState`] and emits
//! [`UiEvent`]s; the ui crate never touches the app crate.
//!
//! [`UiState`]: crate::state::UiState
//! [`UiEvent`]: crate::event::UiEvent

pub mod background;
pub mod chrome;
pub mod color;
pub mod connect;
pub mod dialog;
pub mod event;
pub mod layout;
pub mod loading_bar;
pub mod home;
pub mod loaded;
pub mod main_area;
pub mod manage_models;
pub mod new_project;
pub mod onboarding;
pub mod panel;
pub mod icon;
pub mod scale;
pub mod segmented;
pub mod shell;
pub mod settings;
pub mod state;
pub mod toggler_style;
pub mod toolbar;

pub use connect::ConnectModal;
pub use loaded::LoadedImage;
pub use state::UiState;

/// The translation API used by the UI and the app: the real
/// `easyscanlate_translation` module when the `translation` feature is enabled,
/// otherwise the local [`fake_translation`] mock. Both expose the same
/// surface, so the translation UI is always live — never a disabled
/// placeholder.
#[cfg(feature = "translation")]
pub use easyscanlate_translation as translation;
#[cfg(not(feature = "translation"))]
pub mod fake_translation;
#[cfg(not(feature = "translation"))]
pub use fake_translation as translation;

pub const KOREAN_FONT_PATH: &str = "C:\\Windows\\Fonts\\malgun.ttf";
pub const KOREAN_FONT_NAME: &str = "Malgun Gothic";