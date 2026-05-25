//! iced widgets. Everything here reads the model through [`App`] and emits
//! [`Message`]s; the model itself never touches iced.
//!
//! [`App`]: crate::app::App
//! [`Message`]: crate::app::Message

pub mod decode;
pub mod overlay;
pub mod side_panel;
pub mod tile_view;

pub const KOREAN_FONT_PATH: &str = "C:\\Windows\\Fonts\\malgun.ttf";
pub const KOREAN_FONT_NAME: &str = "Malgun Gothic";