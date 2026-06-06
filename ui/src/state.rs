use iced::widget::text_editor;
use iced::{Font, Rectangle};

use scanlateit_model::{EntryId, EntryStyle};

use crate::event::{EditOrigin, SettingsTab};
use crate::loaded::LoadedImage;

/// Read-only view of the app state that the widgets render from. Implemented
/// by the app for its own state type; the ui crate never depends on the app.
pub trait UiState {
    fn images(&self) -> &[LoadedImage];
    fn running(&self) -> bool;
    fn translating(&self) -> bool;
    fn status(&self) -> &str;
    fn translate_model(&self) -> &str;
    fn translate_lang(&self) -> &str;
    fn translate_api_key(&self) -> &str;
    fn selected(&self) -> Option<(usize, EntryId)>;
    fn style_working(&self) -> &EntryStyle;
    fn style_text_hex(&self) -> &str;
    fn style_stroke_hex(&self) -> &str;
    fn style_bg_hex(&self) -> &str;
    fn style_stroke_width(&self) -> &str;
    fn style_bg_radius(&self) -> &str;
    fn editing(&self) -> Option<(usize, EntryId)>;
    fn editing_origin(&self) -> EditOrigin;
    fn editing_rect(&self) -> Option<Rectangle>;
    fn edit_content(&self) -> Option<&text_editor::Content>;
    fn font(&self) -> Option<Font>;
    /// The image whose tile is accepting inpainting range drags; `None`
    /// disables the mode.
    fn inpaint_mode(&self) -> Option<usize>;
    /// True while the settings modal is open.
    fn settings_open(&self) -> bool;
    /// The settings tab currently shown in the modal.
    fn settings_tab(&self) -> SettingsTab;
}