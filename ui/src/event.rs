use std::ops::Range;

use iced::widget::text_editor;
use iced::Rectangle;

use scanlateit_model::EntryId;

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