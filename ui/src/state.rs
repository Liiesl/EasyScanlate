use std::collections::BTreeMap;

use iced::widget::text_editor;
use iced::{Color, Font, Rectangle};

use scanlateit_model::{EntryId, EntryStyle, TextAlign, TextGradientDir};
use crate::translation::Connection;

use crate::connect::ConnectModal;
use crate::event::{EditOrigin, MainAreaMode, SettingsTab, StyleField};
use crate::loaded::LoadedImage;

/// Read-only view of the app state that the widgets render from. Implemented
/// by the app for its own state type; the ui crate never depends on the app.
pub trait UiState {
    fn images(&self) -> &[LoadedImage];
    fn running(&self) -> bool;
    fn translating(&self) -> bool;
    fn status(&self) -> &str;
    /// Every connected translation provider's selectable models, in connected
    /// order: `(provider id, display name, model ids)`. The model ids already
    /// respect the free-only filter. The merged model dropdown of the
    /// translation bar groups these by provider.
    fn translate_model_groups(&self) -> Vec<(String, String, Vec<String>)>;
    /// The currently selected `(provider id, model id)` of the merged model
    /// dropdown; both are always one of `translate_model_groups`.
    fn translate_model_selection(&self) -> (String, String);
    fn translate_lang(&self) -> &str;
    /// The stored translation connections, keyed by provider id; a provider
    /// is connected exactly when it has an entry here.
    fn connections(&self) -> &BTreeMap<String, Connection>;
    /// The connect modal open over the settings modal, if any.
    fn connect_modal(&self) -> Option<&ConnectModal>;
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
    /// Installed system font family names (from the boot fontdb scan),
    /// sorted, as offered by the styling panel's font picker.
    fn installed_fonts(&self) -> &[String];
    /// The working style's font family, if set.
    fn style_font_family(&self) -> Option<&str>;
    /// The working style's text alignment.
    fn style_text_align(&self) -> TextAlign;
    /// The working style's gradient start color.
    fn style_gradient_a(&self) -> Color;
    /// The working style's gradient end color.
    fn style_gradient_b(&self) -> Color;
    /// The working style's gradient direction.
    fn style_gradient_dir(&self) -> TextGradientDir;
    /// Whether automatic style detection for new OCR entries is enabled.
    #[cfg(feature = "styling")]
    fn auto_style_detect(&self) -> bool;
    /// The configured number of parallel OCR detection workers, as typed in
    /// the settings modal (parsed when OCR starts).
    #[cfg(feature = "ocr")]
    fn ocr_workers(&self) -> &str;
    fn editing(&self) -> Option<(usize, EntryId)>;
    fn editing_origin(&self) -> EditOrigin;
    fn editing_rect(&self) -> Option<Rectangle>;
    fn edit_content(&self) -> Option<&text_editor::Content>;
    fn font(&self) -> Option<Font>;
    /// The image whose tile is accepting inpainting range drags; `None`
    /// disables the mode.
    fn inpaint_mode(&self) -> Option<usize>;
    /// Whether the overlay text is drawn over the pages in the main area.
    fn show_overlay_text(&self) -> bool;
    /// Whether applied inpainting patches are drawn over the pages.
    fn show_inpaint(&self) -> bool;
    /// The display mode of the main area (single column or side-by-side
    /// comparison).
    fn view_mode(&self) -> MainAreaMode;
    /// The latest scroll offset published by a main-area viewer, in content
    /// pixels; in Compare mode the panes mirror each other through it.
    fn viewer_scroll(&self) -> f32;
    /// True while the settings modal is open.
    fn settings_open(&self) -> bool;
    /// The settings tab currently shown in the modal.
    fn settings_tab(&self) -> SettingsTab;
}