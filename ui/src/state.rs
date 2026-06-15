use iced::widget::text_editor;
use iced::{Color, Font, Rectangle};

use scanlateit_model::{EntryId, EntryStyle};

use crate::event::{EditOrigin, SettingsTab, StyleField};
use crate::loaded::LoadedImage;

/// Read-only view of the app state that the widgets render from. Implemented
/// by the app for its own state type; the ui crate never depends on the app.
pub trait UiState {
    fn images(&self) -> &[LoadedImage];
    fn running(&self) -> bool;
    fn translating(&self) -> bool;
    fn status(&self) -> &str;
    fn translate_provider(&self) -> &String;
    fn translate_providers(&self) -> &[String];
    fn translate_model(&self) -> &String;
    fn translate_models(&self) -> &[String];
    fn translate_lang(&self) -> &str;
    fn translate_api_key(&self) -> &str;
    /// Whether the translation model picker only lists free models.
    fn free_models_only(&self) -> bool;
    fn selected(&self) -> Option<(usize, EntryId)>;
    fn style_working(&self) -> &EntryStyle;
    fn style_text_color(&self) -> Color;
    fn style_stroke_color(&self) -> Color;
    fn style_bg_color(&self) -> Color;
    /// The styling color picker currently open (if any).
    fn style_picker_open(&self) -> Option<StyleField>;
    fn style_stroke_width(&self) -> &str;
    fn style_bg_radius(&self) -> &str;
    /// The saved style presets shown in the styling panel, in memory only:
    /// a fixed set of slots, `None` for an empty slot.
    fn style_presets(&self) -> &[Option<EntryStyle>];
    /// Whether automatic style detection for new OCR entries is enabled.
    fn auto_style_detect(&self) -> bool;
    /// The configured number of parallel OCR detection workers, as typed in
    /// the settings modal (parsed when OCR starts).
    fn ocr_workers(&self) -> &str;
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