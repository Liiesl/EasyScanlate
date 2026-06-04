use std::ops::Range;

use iced::widget::text_editor;
use iced::Rectangle;

use scanlateit_model::EntryId;

/// The two actions offered by the selection toolbar drawn under the selected
/// overlay box in the main area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarAction {
    /// Start the inline text edit for the entry (same as double-click).
    Rename,
    /// Soft-delete the entry and clear the selection.
    Delete,
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
    Translate,
    TranslateModel(String),
    TranslateLang(String),
    TranslateApiKey(String),
    EntryClicked(Option<(usize, EntryId)>),
    EntryDoubleClicked((usize, EntryId)),
    EntryMoved((usize, EntryId, [f32; 4])),
    /// A button of the selection toolbar under the selected entry.
    EntryToolbar((usize, EntryId, ToolbarAction)),
    EditAction(text_editor::Action),
    EditRect(Rectangle),
    EditSubmit,
    StyleBold(bool),
    StyleItalic(bool),
    StyleTextHex(String),
    StyleStrokeHex(String),
    StyleStrokeWidth(String),
    StyleBgHex(String),
    StyleBgRadius(String),
}