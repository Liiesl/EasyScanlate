use std::ops::Range;

use iced::widget::pane_grid;
use iced::widget::text_editor;
use iced::{Color, Rectangle};

use scanlateit_model::{EntryId, Quad};

/// The two actions offered by the selection toolbar drawn under the
/// selected overlay box in the main area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarAction {
    /// Start the inline text edit for the entry (same as double-click).
    Rename,
    /// Soft-delete the entry and clear the selection.
    Delete,
}

/// The tabs shown inside the settings modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    /// Placeholder tab; nothing configurable yet.
    General,
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
}

/// Widget-level events produced by the ui crate. The app maps these into its
/// own `Message` and owns all state changes; widgets never see the app.
#[derive(Debug, Clone)]
pub enum UiEvent {
    OpenImages,
    StartOcr,
    StopOcr,
    CycleProfile,
    TilesVisible(Range<usize>),
    /// The user finished a scrollbar drag or touch pan: the viewport will
    /// not move again until a new input, so the app can settle immediately
    /// without waiting out the debounce.
    TileScrollEnded,
    Translate,
    TranslateProvider(String),
    TranslateModel(String),
    TranslateLang(String),
    TranslateApiKey(String),
    /// The user toggled the "only free models" filter in the settings modal.
    FreeModelsOnlyToggle(bool),
    EntryClicked(Option<(usize, EntryId)>),
    EntryDoubleClicked((usize, EntryId)),
    /// Start the inline text edit from the panel's results list instead of
    /// the overlay: the row's current-profile side becomes the live editor.
    PanelEntryEdit((usize, EntryId)),
    EntryMoved((usize, EntryId, Quad)),
    /// A button of the selection toolbar under the selected entry.
    EntryToolbar((usize, EntryId, ToolbarAction)),
    /// Toggle inpainting mode from the panel: the next drag on the page
    /// selects the range to clean.
    Inpaint,
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
    /// Toggle automatic style detection for new OCR entries.
    StyleAutoDetectToggle(bool),
    /// The user typed in the OCR detection workers field of the settings
    /// modal; the string is parsed (with a fallback) when OCR starts.
    OcrWorkers(String),
    /// The user dragged the divider between the main area and the side panel.
    PanelResized(pane_grid::ResizeEvent),
    /// Open the settings modal from the toolbar.
    SettingsOpen,
    /// Close the settings modal; the app persists the settings.
    SettingsClose,
    /// Switch the visible settings tab.
    SettingsTab(SettingsTab),
}