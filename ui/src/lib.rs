//! iced widgets. Everything here reads the app through [`UiState`] and emits
//! [`UiEvent`]s; the ui crate never touches the app crate.
//!
//! [`UiState`]: crate::state::UiState
//! [`UiEvent`]: crate::event::UiEvent

pub mod color;
pub mod connect;
pub mod event;
pub mod loaded;
pub mod main_area;
pub mod panel;
pub mod settings;
pub mod state;
pub mod toolbar;

pub use connect::ConnectModal;
pub use loaded::LoadedImage;
pub use state::UiState;

pub const KOREAN_FONT_PATH: &str = "C:\\Windows\\Fonts\\malgun.ttf";
pub const KOREAN_FONT_NAME: &str = "Malgun Gothic";