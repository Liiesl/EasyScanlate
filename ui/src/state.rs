use iced::widget::text_editor;
use iced::{Color, Font, Rectangle};

use scanlateit_model::{EntryId, EntryStyle, TextAlign, TextGradientDir};

use crate::connect::ConnectModal;
use crate::event::{EditOrigin, MainAreaMode, SettingsTab, StyleField, TargetProfileSelection, TranslationPanelMode};
use crate::loaded::LoadedImage;
use scanlateit_model::{ProfileId, Project};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppView {
    Home,
    Editor,
}

#[derive(Debug, Clone)]
pub struct NewProjectOverlay {
    pub source_paths: Vec<String>,
    pub original_lang: String,
    pub project_location: Option<String>,
}

/// Read-only view of the app state that the widgets render from. Implemented
/// by the app for its own state type; the ui crate never depends on the app.
pub trait UiState {
    fn images(&self) -> &[LoadedImage];
    fn project(&self) -> &Project;
    fn running(&self) -> bool;
    fn translating(&self) -> bool;
    fn status(&self) -> &str;
    /// Every connected translation provider's selectable models, in connected
    /// order: `(provider id, display name, model pairs)`. Each pair is
    /// `(model id, display name)`. The pairs already respect the free-only
    /// filter. The merged model dropdown groups these by provider and shows
    /// the display name while the request still uses the `id`. Borrows from
    /// `&self` (the session's cache), so the result is valid for as long as
    /// the state borrow — enough for a frame.
    fn translate_model_groups(&self) -> &[(String, String, Vec<(String, String)>)];
    /// The currently selected `(provider id, model id)` of the merged model
    /// dropdown; both are always one of `translate_model_groups` (matched by
    /// `id`; display is the `name`).
    fn translate_model_selection(&self) -> (String, String);
    fn translate_lang(&self) -> &str;
    /// The connect modal open over the settings modal, if any.
    fn connect_modal(&self) -> Option<&ConnectModal>;
    fn selected(&self) -> Option<(usize, EntryId)>;
    /// The currently selected inpaint patch as `(image index, patch index within that image)`; `None` when no inpaint is selected.
    fn selected_inpaint(&self) -> Option<(usize, usize)>;
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
    /// The raw hex text buffer for `field`, if the user is currently typing
    /// (valid or intermediate). `None` means show the canonical `hex_label`.
    fn style_hex_override(&self, field: StyleField) -> Option<&str>;
    fn editing(&self) -> Option<(usize, EntryId)>;
    fn editing_origin(&self) -> EditOrigin;
    fn editing_rect(&self) -> Option<Rectangle>;
    fn edit_content(&self) -> Option<&text_editor::Content>;
    fn font(&self) -> Option<Font>;
    /// Whether inpainting range drags are enabled; when `true` a drag on
    /// any tile selects the range to clean.
    fn inpaint_mode(&self) -> bool;
    /// Whether manual OCR range drags are enabled; when `true` a drag on
    /// any tile selects the region to OCR (same UX as inpaint, but without padding).
    fn ocr_mode(&self) -> bool;
    /// Whether the overlay text is drawn over the pages in the main area.
    fn show_overlay_text(&self) -> bool;
    /// Whether applied inpainting patches are drawn over the pages.
    fn show_inpaint(&self) -> bool;
    /// The display mode of the main area (single column or side-by-side
    /// comparison).
    fn view_mode(&self) -> MainAreaMode;
    /// The latest scroll *center anchor* published by a main-area viewer:
    /// `(offset + viewport/2)/content_height` in `0..1`. In Compare mode the
    /// panes mirror each other through it, and on resize / `View↔Compare`
    /// the same centered row stays visible instead of the same absolute
    /// offset.
    fn viewer_scroll(&self) -> f32;
    /// True while the settings modal is open.
    fn settings_open(&self) -> bool;
    /// The settings tab currently shown in the modal.
    fn settings_tab(&self) -> SettingsTab;
    /// Current filter text of the settings sidebar search field.
    fn settings_search(&self) -> &str;
    /// Whether the Manage Models overlay is open (over the settings modal).
    fn manage_models_open(&self) -> bool;
    /// Current filter text of the Manage Models search field.
    fn manage_models_search(&self) -> &str;
    /// Every connected provider's *all* toggleable models (deprecated already
    /// removed) grouped by provider – shown in the Manage Models overlay.
    /// Each inner pair is `(model id, display name)`; the hidden set is
    /// still keyed by `id`.
    fn all_model_groups(&self) -> Vec<(String, String, Vec<(String, String)>)>;
    /// The mode of the translation/results panel (Edit vs Translate).
    fn translation_panel_mode(&self) -> TranslationPanelMode;
    /// The base profile for translate-mode left column (None when no images).
    fn base_profile(&self) -> Option<ProfileId>;
    /// The target profile for translate-mode right column. When `AutoPlaceholder`
    /// the profile may not exist yet and the right inputs are blank.
    fn target_profile(&self) -> TargetProfileSelection;
    /// Placeholder name for the current language in translate mode: `"{Lang}(auto)"`.
    fn target_placeholder_name(&self) -> String;
    fn app_view(&self) -> AppView;
    fn recent_projects(&self) -> &[scanlateit_settings::RecentProject];
    fn new_project_overlay(&self) -> Option<NewProjectOverlay>;
    fn translation_anim_phase(&self) -> f32;
}