use std::ops::Range;

use iced::widget::pane_grid;
use iced::widget::text_editor;
use iced::Rectangle;
use iced::window::screenshot::Screenshot;

use easyscanlate_model::{CapsMode, EntryId, ProfileId, Quad, TextAlign};
use easyscanlate_settings::InpaintBackend;

/// The actions offered by the floating inpaint toolbar under the selected patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InpaintToolbarAction {
    /// Remove the selected inpaint patch.
    Delete,
    /// Re-run inpainting on the exact same bounds.
    Repaint,
}

/// The actions offered by the selection decorations around the selected
/// overlay box in the main area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarAction {
    /// Start the inline text edit for the entry (same as double-click).
    Rename,
    /// Soft-delete the entry and clear the selection.
    Delete,
    /// Reset the box's transform (move, resize, rotation, free-transform
    /// distortion) back to the OCR quad.
    RevertTransform,
}

/// The tabs shown inside the settings modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    /// General tunables.
    General,
    /// Aurora background appearance (color, blobs, schema, light/dark) + font size.
    Appearance,
    /// OCR engine tuning.
    Ocr,
    /// Inpaint (manual + auto) tuning.
    Inpaint,
    /// Machine-translation settings (API key).
    Translation,
    /// App updates (Velopack, GitHub releases).
    Updates,
}

/// Where an inline text edit was started; decides which editor widget
/// renders and receives the focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditOrigin {
    /// The floating editor pinned over the entry's box in the main area.
    Overlay,
    /// The multi-line editor in the panel's results list row.
    Panel,
}

/// The display mode of the main area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MainAreaMode {
    /// The single scrollable column with overlays (the default).
    #[default]
    View,
    /// Original (no inpaint/overlay) vs current (with both), side by side.
    Compare,
}

/// Persistent manual tool mode (multi-select). When active the main area is
/// forced to `View` and the `View|Compare` pill is replaced by a mode banner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ManualMode {
    #[default]
    None,
    Inpaint,
    Ocr,
}

/// The mode of the translation/results panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TranslationPanelMode {
    /// Single input per row (current profile's text), no original, no translate bar.
    #[default]
    Edit,
    /// Two inputs per row (base vs target profile), translate bar visible.
    Translate,
}

/// The virtual vs real target profile choice in translate mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetProfileSelection {
    /// An existing profile by id.
    Existing(ProfileId),
    /// Placeholder `"{Lang}(auto)"` for the current `translate_lang`; not yet created.
    AutoPlaceholder(String),
}

/// The color field a styling [`HexColorInput`](neverliie_iced_widgets::hex_color_input::HexColorInput)
/// edits: the text fill, the stroke, or the background of the selected
/// entry. Each field holds a unified solid-or-gradient value, so no
/// per-stop sub-fields are needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StyleField {
    /// The entry's text fill (solid color or gradient).
    Fill,
    /// The entry's stroke (solid color or gradient).
    Stroke,
    /// The entry's background (solid color or gradient).
    Background,
}

/// A deferred settings edit for widget builders that take a *message value*
/// (iced buttons evaluate their builder eagerly during every view build):
/// the payload names the change, and the app applies it to the settings
/// store in `update`. Field-level widgets (text inputs, checkboxes, pick
/// lists, the color wheel) keep writing the store directly instead.
#[derive(Debug, Clone)]
pub enum SettingEdit {
    /// Aurora dark (`true`) vs light (`false`).
    AuroraDarkMode(bool),
    /// Aurora blob count target; clamped to 1..=5 when applied.
    AuroraBlobCount(u8),
    /// Aurora color-theory schema index; taken modulo 4 when applied.
    AuroraSchema(u8),
    /// Clear one provider's hidden-model set ("Show all").
    HiddenModelsReset(String),
    /// Clear every provider's hidden-model set ("Reset all").
    HiddenModelsResetAll,
    /// UI base font size, like VS Code's `editor.fontSize`. Integer only.
    UiFontSize(u32),
    /// Auto-check GitHub releases for updates at startup.
    AutoCheckUpdates(bool),
    /// Periodic autosave of dirty projects into the central delta dir.
    AutosaveEnabled(bool),
    /// Autosave period in seconds (clamped 15..=600 when applied).
    AutosaveInterval(u64),
}

/// Widget-level events produced by the ui crate. The app maps these into its
/// own `Message` and owns all state changes; widgets never see the app.
#[derive(Debug, Clone)]
pub enum UiEvent {
    // -- Homepage / New Project (single-window overlay) --
    HomeNewProject,
    HomeOpenProject,
    HomeRecentClicked(String),
    HomeSettings,
    NewProjectClose,
    NewProjectSourceImage,
    NewProjectSourceFolder,
    NewProjectLocationBrowse,
    NewProjectOriginalLang(String),
    NewProjectCreate,
    StartOcr,
    StopOcr,
    /// The user selected profile `id` in the results panel's profile
    /// dropdown; the app switches every image's project to it.
    ProfileSelect(ProfileId),
    /// The user pressed the "+ New Profile" row of the profile dropdown:
    /// create and select a fresh profile in every project.
    ProfileCreate,
    /// The user toggled the translation panel between Edit and Translate.
    TranslationPanelMode(TranslationPanelMode),
    /// The user picked the base profile for translate mode.
    BaseProfileSelect(ProfileId),
    /// The user picked the target profile for translate mode (existing or auto placeholder).
    TargetProfileSelect(TargetProfileSelection),
    TilesVisible(Range<usize>),
    /// The user finished a scrollbar drag or touch pan: the viewport will
    /// not move again until a new input, so the app can settle immediately
    /// without waiting out the debounce.
    TileScrollEnded,
    Translate,
    /// The user picked a (provider, model) pair in the merged model dropdown
    /// of the translation bar; both are selected together.
    TranslateModelSelect { provider: String, model: String },
    TranslateLang(String),
    /// The user pressed "Connect" for the translation provider id; the app
    /// opens the API-key entry modal.
    TranslateConnect(String),
    /// The user pressed "Disconnect" for the translation provider id; the
    /// app drops its stored API key.
    TranslateDisconnect(String),
    /// The user typed in the API-key field of the connect modal.
    ConnectModalKey(String),
    /// The user typed in the base-URL field of the connect modal (custom
    /// endpoints only).
    ConnectModalBaseUrl(String),
    /// The user typed in the model field of the connect modal (custom
    /// endpoints only).
    ConnectModalModel(String),
    /// The user confirmed the connect modal; the app validates and stores
    /// the connection.
    ConnectModalSubmit,
    /// The user cancelled the connect modal; nothing is stored.
    ConnectModalCancel,
    /// Open the Manage Models overlay (over the settings modal).
    ManageModelsOpen,
    /// Close the Manage Models overlay.
    ManageModelsClose,
    /// The user typed in the Manage Models search field.
    ManageModelsSearch(String),
    EntryClicked(Option<(usize, EntryId)>),
    EntryDoubleClicked((usize, EntryId)),
    /// Start the inline text edit from the panel's results list instead of
    /// the overlay: the row's current-profile side becomes the live editor.
    PanelEntryEdit((usize, EntryId)),
    /// The user pressed "Retranslate" on a results row: re-run machine
    /// translation for that entry. The result replaces the entry's text in
    /// the selected profile.
    RetranslateEntry((usize, EntryId)),
    /// Request to reorder OCR entries by image file then visual Y→X (top
    /// first, left→right). Model operation — future OCR-entry modal will emit
    /// this; handler loops every image's `Project::reorder_entries_for_image`.
    ReorderEntries,
    EntryMoved((usize, EntryId, Quad)),
    /// A button of the selection toolbar under the selected entry.
    EntryToolbar((usize, EntryId, ToolbarAction)),
    /// The user clicked an inpaint layer row in the Layers panel. `None` deselects.
    InpaintClicked(Option<(usize, usize)>),
    /// Delete an inpaint patch: `(image index, patch index)`.
    InpaintDelete((usize, usize)),
    /// Re-run inpainting on the exact same bounds as `(image index, patch index)`.
    InpaintRepaint((usize, usize)),
    /// A button of the floating inpaint toolbar under the selected patch.
    InpaintToolbar((usize, usize, InpaintToolbarAction)),
    // ——— Persistent manual multi-select mode ———
    /// Enter the persistent manual mode (`Inpaint` or `Ocr`). The next drags
    /// accumulate rubber bands until `ManualModeStart` / `Reset` / `Cancel`.
    ManualModeEnter(ManualMode),
    /// Exit (clear) the persistent manual mode without running.
    ManualModeCancel,
    /// Clear all pending rubber bands but stay in the mode.
    ManualModeReset,
    /// Run the pending selections (inpaint → multi-canvas stitch, OCR → multi-crop).
    ManualModeStart,
    /// A single drag has finished while a manual mode is active. The viewer
    /// publishes every drag as a rect in image pixels; the app accumulates it.
    ManualSelectionAdded((usize, Rectangle)),
    /// Span variant of `ManualSelectionAdded` (drag crossed the seam).
    ManualSelectionSpan(Vec<(usize, Rectangle)>),
    /// Toggle hiding the overlay text drawn over the pages in the main area.
    ToggleOverlayText,
    /// Toggle showing the applied inpainting patches over the pages.
    ToggleInpaintLayer,
    /// The user clicked a main-area mode button (View or Compare).
    MainAreaMode(MainAreaMode),
    /// A main-area viewer's scroll changed; payload is the normalized center
    /// anchor `(offset + viewport/2)/content_height` in `0..1`. The app mirrors
    /// it into the peer pane in Compare mode and restores it on resize /
    /// `View↔Compare` so the same row stays centered instead of the same
    /// absolute pixel offset.
    ViewerScroll(f32),
    /// The results panel's scroll changed; payload is the normalized relative
    /// offset `0..1` (`offset/max_offset`). The app restores it on
    /// `Edit↔Translate` so the same fraction stays visible instead of the
    /// same absolute pixel offset (row heights differ between modes).
    PanelScroll(f32),
    EditAction(text_editor::Action),
    EditRect(Rectangle),
    EditSubmit,
    StyleBold(bool),
    StyleItalic(bool),
    /// The user requested the color picker for `field` to open.
    StyleColorOpen(StyleField),
    /// The user cancelled the color picker for `field`; discard any change.
    StyleColorCancel(StyleField),
    /// The unified hex input for `field` changed (live: hex typing,
    /// alpha/angle edits, picker drags).
    StyleColorChanged(StyleField, neverliie_iced_widgets::hex_color_input::HexColorValue),
    /// The user confirmed the value for `field` in its picker (OK button).
    StyleColorSubmit(StyleField, neverliie_iced_widgets::hex_color_input::HexColorValue),
    /// The picker's top-level tab (`Color | Gradient | Library`) changed for
    /// `field`. Used to show the gradient angle handle immediately when the
    /// Gradient tab is selected, even before any gradient value edit.
    StyleColorTabChanged(StyleField, neverliie_iced_widgets::color_picker::PickerTab),
    /// The user dragged the Figma-like gradient angle handle on the selected
    /// entry in the main area: `(image index, entry id, field, angle degrees)`.
    /// Live-published per mouse move while the picker's gradient is open.
    StyleGradientAngle((usize, EntryId, StyleField, f32)),
    /// The eye dropper button requested a fresh window snapshot: the app
    /// should screenshot the window and store it in the shared
    /// `DropperBuffer` (see `UiState::dropper_buffer`).
    StyleDropperCapture,
    /// A window screenshot for the eye dropper, boxed to keep the event
    /// small (mirrors `BackdropCaptured`). The app stores it in the shared
    /// `DropperBuffer`; the widget consumes it on the next update pass.
    StyleDropperShot(Box<Screenshot>),
    StyleStrokeWidth(f32),
    StyleBgRadius(f32),
    /// The user typed a fixed font size (image px) for the selected entry;
    /// applied only when auto-size is off.
    StyleFontSize(f32),
    /// The user typed a relative line-height multiplier for the selected entry.
    StyleLineHeight(f32),
    /// The user typed letter spacing (image px) for the selected entry.
    StyleLetterSpacing(f32),
    /// The user toggled auto-size for the selected entry (default true).
    StyleAutoSize(bool),
    /// The user picked the caps transform for the selected entry.
    StyleCaps(CapsMode),
    /// The user picked an installed font family name for the selected entry.
    StyleFont(String),
    /// The font dropdown was opened: load the first visible families so
    /// their preview rows render in the real face (visible-only lazy load).
    StyleFontPreviewOpen,
    /// The user hovered a font row: load that family for its preview
    /// without changing the selection.
    StyleFontPreviewHover(String),
    /// The user picked the text alignment mode for the selected entry.
    StyleTextAlign(TextAlign),
    /// The user clicked preset swatch `usize`: apply that style to the
    /// selected entry.
    StylePresetApply(usize),
    /// The user clicked the "+" swatch: save the current working style in
    /// the first empty preset slot.
    StylePresetAdd,
    /// The user chose "Replace with current style" in a preset's context
    /// menu: overwrite that slot (empty or filled) with the working style.
    StylePresetReplace(usize),
    /// The user chose "Remove preset" in a preset's context menu: empty
    /// that slot.
    StylePresetRemove(usize),
    /// The user dismissed a preset's context menu; nothing to do.
    StylePresetMenuDismiss,
    /// Run the ONNX style classifier on the selected entry and apply the
    /// result. Works regardless of the auto-detect setting.
    StyleAutoDetect,
    /// Make the selected entry's background transparent and inpaint its
    /// *current* view quad (not the original OCR quad) — the box's present
    /// position/size after moves/resizes/rotations. Mirrors the auto pipeline's
    /// use of `view_quad` for the `rect` + `quads` fed to the inpaint engine.
    StyleInpaintBackground,
    /// The user picked a default inpaint backend from the styling panel's
    /// "Inpaint Background" split button; select-only (persists the backend,
    /// does not start inpainting — the main area runs via
    /// `StyleInpaintBackground`).
    StyleInpaintBackendSelected(InpaintBackend),
    /// The user dragged the divider between the main area and the styling panel.
    PanelResized(pane_grid::ResizeEvent),
    /// The user dragged the divider between the translation list and the inpaint/layers panel.
    ResultsPaneResized(pane_grid::ResizeEvent),
    /// The user dragged the divider between the left Translation/Inpaint
    /// column and everything right of the toolbar.
    EditorResized(pane_grid::ResizeEvent),
    /// Open the settings modal from the toolbar.
    SettingsOpen,
    /// Open the settings modal directly on the given tab (used by the
    /// translation bar's configure button).
    SettingsOpenTab(SettingsTab),
    /// Close the settings modal.
    SettingsClose,
    /// Switch the visible settings tab.
    SettingsTab(SettingsTab),
    /// The user typed in the settings sidebar search field.
    SettingsSearch(String),
    /// Some setting was changed: the ui crate already wrote it into the
    /// shared settings store; the app re-syncs its runtime mirrors from
    /// there. This is the single message for every settings edit.
    SettingsChanged,
    /// A deferred button-driven settings edit (see [`SettingEdit`]): the
    /// app applies it to the store, then re-syncs like `SettingsChanged`.
    SettingEdit(SettingEdit),
    /// Open an external URL in the system browser (used for recommended
    /// provider docs links).
    OpenUrl(String),
    /// Save the current project to its .mmtl path (or Save As if none).
    SaveProject,
    /// Export every page as a baked raster image (original + inpaint + overlay)
    /// to a chosen folder. One click exports the whole chapter.
    ExportAll,
    /// Cancel the running raster export. Remaining chunks are skipped; the
    /// overlay dismisses with an "Export cancelled" status.
    ExportCancel,
    // -- Tab management (multi-project, Phase 2+) --
    /// Select the tab with `id` (titlebar chip or Ctrl+1..9 / Ctrl+Tab).
    TabSelected(u64),
    /// Close the tab with `id` (× button or Ctrl+W). Home (0) is pinned.
    TabClose(u64),
    /// User confirmed the dirty-close modal for `id`: `bool` = save?
    TabCloseConfirmed(u64, bool),
    /// Close all project tabs except `id` (future use). None = all.
    TabCloseOthers(u64),
    /// Close all project tabs.
    TabCloseAll,
    /// Create a new tab (Home → New Project overlay or Ctrl+T).
    TabNew,
    /// Dismiss the dirty-close confirmation modal (Cancel / backdrop / Esc).
    TabCloseCancel,
    /// Load the autosaved delta for tab `id` (recovery modal).
    AutosaveLoad(u64),
    /// Discard the autosaved delta for tab `id` (deletes its autosave dir).
    AutosaveDiscard(u64),
    /// Dismiss the autosave recovery modal without loading or deleting.
    AutosaveLater,
    // ——— Updates (Velopack) ———
    UpdateCheck,
    UpdateDownload,
    UpdateApply,
    UpdateDismiss,
    // ——— Onboarding (first-run, blocking) ———
    OnboardingNext,
    OnboardingBack,
    OnboardingDownloadAll,
    OnboardingRetry(String),
    OnboardingToggleTheme,
    OnboardingFontSize(bool), // true = inc, false = dec
    OnboardingToggleAutoStyle,
    OnboardingToggleAutoSfx,
    OnboardingToggleAutoInpaint,
    OnboardingOpenTranslationSettings,
    OnboardingSkipTranslation,
    OnboardingFinish,
    OnboardingReplay,
}
