use iced::widget::text_editor;
use iced::{Color, Font, Rectangle};

use easyscanlate_model::{EntryId, EntryStyle, TextAlign, TextGradientDir};

use crate::connect::ConnectModal;
use crate::event::{EditOrigin, MainAreaMode, ManualMode, SettingsTab, StyleField, TargetProfileSelection, TranslationPanelMode};
use crate::layout::{PaneKind, SidePaneKind, StylingPaneKind};
use crate::loaded::LoadedImage;
use easyscanlate_model::{ProfileId, Project};
use iced::widget::pane_grid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppView {
    Onboarding,
    Home,
    Editor,
}

#[derive(Debug, Clone)]
pub struct NewProjectOverlay {
    pub source_paths: Vec<String>,
    pub original_lang: String,
    pub project_location: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabMeta {
    pub id: u64,
    pub title: String,
    pub dirty: bool,
    pub is_home: bool,
}

/// One selectable model: `(model id, display name)`.
pub type ModelOptionPair = (String, String);
/// One provider group: `(provider id, display name, models)`.
pub type ModelGroup = (String, String, Vec<ModelOptionPair>);

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
    fn translate_model_groups(&self) -> &[ModelGroup];
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
    /// Persistent manual mode (None → no mode, Inpaint/Ocr → multi-select banner).
    fn manual_mode(&self) -> ManualMode;
    /// Pending rubber bands while a manual mode is active, in image pixels.
    fn manual_selections(&self) -> &[(usize, Rectangle)];
    /// True while an inpaint job is running (blocks Start).
    fn is_inpainting(&self) -> bool;
    /// True while a manual OCR job is running.
    fn is_manual_ocring(&self) -> bool;
    /// True while the OCR pipeline (SFX filter / style classify / auto-inpaint staging) is active beyond raw OCR.
    fn is_pipeline_running(&self) -> bool { false }
    /// True while any auto-inpaint jobs are pending (Telea/LaMa/AOT).
    fn is_auto_inpainting(&self) -> bool { false }
    /// True while SFX segmentation filtering is running.
    fn is_segment_filtering(&self) -> bool { false }
    /// True while styling classification jobs are pending/running.
    fn is_styling_busy(&self) -> bool { false }
    /// Weighted overall pipeline progress (`0.0..=1.0`) for the Start OCR
    /// button: OCR x5 + SFX segment x2 + style x1 + auto-inpaint x3,
    /// normalized over the enabled stages. `None` when idle (hide bar).
    fn pipeline_progress(&self) -> Option<f32> { None }
    /// Bulk busy: any job that mutates OCR entries / inpaint patches / translations and conflicts with bulk actions.
    /// Used to disable Start OCR (re-enable), Translate, Retranslate, AutoDetect, InpaintBackground, manual mode etc.
    /// Note: main-area text editing and fine-grained style controls are intentionally *not* gated by this.
    fn is_bulk_busy(&self) -> bool {
        self.running()
            || self.translating()
            || self.is_inpainting()
            || self.is_manual_ocring()
            || self.is_pipeline_running()
            || self.is_auto_inpainting()
            || self.is_segment_filtering()
            || self.is_styling_busy()
    }
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
    /// Blurred snapshot of the window behind the Settings / Manage Models
    /// overlays (`None` = not captured yet: render flat). Captured clean
    /// before the modal opens, downscaled + blurred off-thread.
    fn backdrop_blur(&self) -> Option<iced::widget::image::Handle> { None }
    /// Every connected provider's *all* toggleable models (deprecated already
    /// removed) grouped by provider – shown in the Manage Models overlay.
    /// Each inner pair is `(model id, display name)`; the hidden set is
    /// still keyed by `id`.
    fn all_model_groups(&self) -> Vec<ModelGroup>;
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
    fn recent_projects(&self) -> &[easyscanlate_settings::RecentProject];
    fn new_project_overlay(&self) -> Option<NewProjectOverlay>;
    fn translation_anim_phase(&self) -> f32;
    fn is_loading(&self) -> bool { false }
    fn loading_phase(&self) -> f32 { 0.0 }
    fn loading_title(&self) -> String { String::new() }
    /// True while the active tab is exporting raster images.
    fn is_exporting(&self) -> bool { false }
    /// Export progress as `(done, total, failed)`. `None` when idle.
    fn export_progress(&self) -> Option<(usize, usize, usize)> { None }
    /// Destination folder of the running export, if any.
    fn export_folder(&self) -> Option<String> { None }
    // ——— Tabs (titlebar) ———
    fn tab_metas(&self) -> Vec<TabMeta> { Vec::new() }
    fn active_tab_id(&self) -> u64 { 0 }
    fn pending_close(&self) -> Option<TabMeta> { None }
    fn titlebar_height(&self) -> f32 { 32.0 }
    fn editor_panes(
        &self,
    ) -> Option<(
        &pane_grid::State<PaneKind>,
        &pane_grid::State<SidePaneKind>,
        &pane_grid::State<StylingPaneKind>,
    )> {
        None
    }
    // ——— Updates (Velopack) ———
    fn update_current_version(&self) -> String { String::new() }
    fn update_available_version(&self) -> Option<String> { None }
    fn update_downloading(&self) -> bool { false }
    fn update_progress(&self) -> i16 { 0 }
    fn update_ready(&self) -> bool { false }
    fn update_notes(&self) -> Option<String> { None }
    /// Whether the blocking update-available popup is currently shown.
    fn update_popup_visible(&self) -> bool { false }
    /// Blurred snapshot cropped to the update popup card (`None` = flat).
    fn update_blur(&self) -> Option<iced::widget::image::Handle> { None }
    // ——— Onboarding (first-run, blocking) ———
    fn onboarding_open(&self) -> bool { false }    fn onboarding_step(&self) -> u8 { 0 }
    fn onboarding_models(&self) -> Vec<(String, String, ModelDownloadStatus)> { Vec::new() }
    fn onboarding_overall_progress(&self) -> f32 { 0.0 }
    fn onboarding_downloading(&self) -> bool { false }
    fn onboarding_all_done(&self) -> bool { true }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModelDownloadStatus {
    NotStarted,
    Downloading { percent: f32, downloaded: u64, total: u64 },
    Done,
    Failed(String),
}