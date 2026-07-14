use std::ops::Range;

use iced::widget::pane_grid;
use iced::widget::text_editor;
use iced::{Color, Rectangle};

use scanlateit_model::{EntryId, ProfileId, Quad, TextAlign, TextGradientDir};

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
    /// General tunables (OCR workers, inpaint backend, ...).
    General,
    /// Aurora background appearance (color, blobs, schema, light/dark).
    Appearance,
    /// Machine-translation settings (API key).
    Translation,
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

/// The color field a styling [`ColorPicker`] edits: the text color, the
/// stroke (outline) color, or the background color of the selected entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleField {
    /// The entry's text color.
    Text,
    /// The entry's stroke (outline) color.
    Stroke,
    /// The entry's background color.
    Background,
    /// The gradient start color of the selected entry.
    GradientA,
    /// The gradient end color of the selected entry.
    GradientB,
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
}

/// Widget-level events produced by the ui crate. The app maps these into its
/// own `Message` and owns all state changes; widgets never see the app.
#[derive(Debug, Clone)]
pub enum UiEvent {
    OpenImages,
    StartOcr,
    StopOcr,
    /// The user selected profile `id` in the results panel's profile
    /// dropdown; the app switches every image's project to it.
    ProfileSelect(ProfileId),
    /// The user pressed the "+ New Profile" row of the profile dropdown:
    /// create and select a fresh profile in every project.
    ProfileCreate,
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
    /// Toggle inpainting mode from the panel: the next drag on the page
    /// selects the range to clean.
    Inpaint,
    /// Toggle hiding the overlay text drawn over the pages in the main area.
    ToggleOverlayText,
    /// Toggle showing the applied inpainting patches over the pages.
    ToggleInpaintLayer,
    /// The user clicked a main-area mode button (View or Compare).
    MainAreaMode(MainAreaMode),
    /// A main-area viewer's scroll offset changed; the app mirrors it into
    /// the peer pane in Compare mode.
    ViewerScroll(f32),
    /// The user finished dragging an inpainting range on `index`'s tile;
    /// `Rectangle` is `(x, y, w, h)` in image pixels.
    InpaintSelection((usize, Rectangle)),
    EditAction(text_editor::Action),
    EditRect(Rectangle),
    EditSubmit,
    StyleBold(bool),
    StyleItalic(bool),
    /// The user requested the color picker for `field` to open.
    StyleColorOpen(StyleField),
    /// The user cancelled the color picker for `field`; discard any change.
    StyleColorCancel(StyleField),
    /// The user confirmed a color for `field` in its color picker.
    StyleColorSubmit(StyleField, Color),
    StyleStrokeWidth(String),
    StyleBgRadius(String),
    /// The user picked an installed font family name for the selected entry.
    StyleFont(String),
    /// The user picked the text alignment mode for the selected entry.
    StyleTextAlign(TextAlign),
    /// The user toggled the two-color text gradient for the selected entry.
    StyleGradientToggle(bool),
    /// The user picked the gradient direction for the selected entry.
    StyleGradientDir(TextGradientDir),
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
    /// The user dragged the divider between the main area and the side panel.
    PanelResized(pane_grid::ResizeEvent),
    /// The user dragged the divider between the styling and the translation/results panels.
    SidePanelResized(pane_grid::ResizeEvent),
    /// The user dragged the divider between the styling inspector and the inpaint/layers panel.
    StylingPaneResized(pane_grid::ResizeEvent),
    /// Open the settings modal from the toolbar.
    SettingsOpen,
    /// Open the settings modal directly on the given tab (used by the
    /// translation bar's configure button).
    SettingsOpenTab(SettingsTab),
    /// Close the settings modal.
    SettingsClose,
    /// Switch the visible settings tab.
    SettingsTab(SettingsTab),
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
}