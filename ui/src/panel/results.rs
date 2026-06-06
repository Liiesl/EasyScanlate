//! Bottom section: translation controls (model, target language, API key,
//! start button) above the scrollable OCR results list. The list shows one
//! row per entry with two side-by-side inputs: the read-only original OCR
//! text on the left and the selected profile's text on the right. The right
//! side starts the same multi-line inline edit as the main area (fork on
//! first keystroke, Enter = newline, Escape/Ctrl+Enter to commit); clicking
//! a row selects the entry, highlighted with a border.

use iced::widget::text_editor;
use iced::widget::{button, column, container, mouse_area, pick_list, row, scrollable, text, text_input, Column};
use iced::{keyboard, Background, Border, Color, Element, Fill as FillLength, Font, Padding};

use crate::event::{EditOrigin, UiEvent};
use crate::loaded::LoadedImage;
use crate::panel::MUTED_FG;
use crate::state::UiState;
use scanlateit_model::OcrEntry;
use scanlateit_translation as translation;

/// Widget id of the multi-line editor shown in a row while the entry is
/// edited from the panel; must match the app's focus id.
const PANEL_EDIT_INPUT_ID: &'static str = "panel-editor";

const ROW_SPACING: f32 = 8.0;
/// Horizontal gap inside an input box.
const BOX_PADDING: f32 = 6.0;
/// Padding between the row's border/background and its input boxes; the
/// clickable band that selects the row.
const ROW_PADDING: Padding = Padding {
    top: 2.0,
    right: 8.0,
    bottom: 2.0,
    left: 8.0,
};
/// Shaded background of the two input boxes inside a row.
const BOX_BG: Color = Color::from_rgb8(28, 30, 38);
/// Idle row border.
const ROW_BORDER: Color = Color::from_rgba8(255, 255, 255, 0.14);
/// Border of the selected row, matching the overlay's selection handles.
const SELECTED_BORDER: Color = Color::from_rgba8(92, 190, 255, 1.0);
/// Faint fill behind a selected row.
const SELECTED_BG: Color = Color::from_rgba8(92, 190, 255, 0.08);

/// The one input-like box: shaded, rounded, one side of a results row.
fn input_box<'a>(content: impl Into<Element<'a, UiEvent>>) -> Element<'a, UiEvent> {
    container(content)
        .width(FillLength)
        .padding(BOX_PADDING)
        .style(|_theme| container::Style {
            background: Some(BOX_BG.into()),
            border: Border::default().rounded(4.0),
            ..container::Style::default()
        })
        .into()
}

/// The multi-line editor shown in place of the read-only current-profile
/// box while its entry is being edited from the panel. Behaviour matches the
/// main area's floating editor exactly: Enter inserts a newline, Escape or
/// Ctrl+Enter commit, every action replays through the app's `EditAction`.
fn panel_editor<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    let content = state
        .edit_content()
        .expect("editing state always carries a content buffer");
    input_box(
        text_editor::TextEditor::new(content)
            .id(PANEL_EDIT_INPUT_ID)
            .font(state.font().unwrap_or(Font::DEFAULT))
            .size(12)
            .line_height(1.2)
            .padding(0)
            .on_action(UiEvent::EditAction)
            .key_binding(|press| match press.modified_key.as_ref() {
                keyboard::Key::Named(keyboard::key::Named::Escape) => {
                    Some(text_editor::Binding::Custom(UiEvent::EditSubmit))
                }
                keyboard::Key::Named(keyboard::key::Named::Enter) if press.modifiers.command() => {
                    Some(text_editor::Binding::Custom(UiEvent::EditSubmit))
                }
                _ => text_editor::Binding::from_key_press(press),
            })
            .style(|_theme, _status| text_editor::Style {
                background: Background::Color(Color::TRANSPARENT),
                border: Border::default().rounded(0.0),
                placeholder: MUTED_FG,
                value: Color::from_rgb(0.9, 0.9, 0.9),
                selection: Color::from_rgba8(92, 190, 255, 0.35),
            }),
    )
}

/// The read-only original text of an entry, boxed like the other inputs.
fn original_box(entry_text: &str, font: Font) -> Element<'_, UiEvent> {
    input_box(text(entry_text).size(12).font(font))
}

/// The current profile's text of an entry, boxed like the other inputs;
/// read-only until clicked, which starts the inline edit.
fn current_box(value: String, font: Font) -> Element<'static, UiEvent> {
    input_box(text(value).size(12).font(font))
}

/// One results row: the original OCR text on the left, the selected
/// profile's text on the right. Clicking the left box selects the row;
/// clicking the right box starts the inline edit for it. The row being
/// edited from the panel swaps its right box for the live editor (and is
/// not a click target, so the editor keeps the clicks). The selected row
/// is outlined with a highlight border.
fn entry_row<'a, S: UiState + ?Sized>(
    state: &'a S,
    index: usize,
    entry: &'a OcrEntry,
) -> Element<'a, UiEvent> {
    let entry_id = entry.id;
    let selected = state.selected() == Some((index, entry_id));
    let editing_here = state.editing() == Some((index, entry_id));
    let editing_from_panel = editing_here && state.editing_origin() == EditOrigin::Panel;
    let font = state.font().unwrap_or(Font::DEFAULT);

    let original = mouse_area(original_box(&entry.text, font)).on_press(UiEvent::EntryClicked(Some((
        index,
        entry_id,
    ))));

    let current: Element<'_, UiEvent> = if editing_from_panel {
        panel_editor(state)
    } else {
        let shown = if editing_here {
            // Editing via the main area: mirror the live buffer.
            state
                .edit_content()
                .map(|content| content.text().to_string())
                .unwrap_or_else(|| {
                    state.images()[index].project.display_text(entry).to_string()
                })
        } else {
            state.images()[index].project.display_text(entry).to_string()
        };
        mouse_area(current_box(shown, font))
            .on_press(UiEvent::PanelEntryEdit((index, entry_id)))
            .into()
    };

    let row = container(row![original, current].spacing(ROW_SPACING))
        .width(FillLength)
        .padding(ROW_PADDING)
        .style(move |_theme| container::Style {
            background: selected.then_some(SELECTED_BG.into()),
            border: Border::default()
                .width(1.0)
                .color(if selected { SELECTED_BORDER } else { ROW_BORDER })
                .rounded(4.0),
            ..container::Style::default()
        });

    // While the row itself is the active panel editor, clicks must reach the
    // editor, not the row.
    if editing_from_panel {
        row.into()
    } else {
        mouse_area(row).on_press(UiEvent::EntryClicked(Some((index, entry_id)))).into()
    }
}

/// The column labels ("Original" / "Current Profile"), aligned with the two
/// inputs of every row.
fn label_row() -> Element<'static, UiEvent> {
    row![
        text("Original").size(12).color(MUTED_FG).width(FillLength),
        text("Current Profile").size(12).color(MUTED_FG).width(FillLength),
    ]
    .spacing(ROW_SPACING)
    .padding(Padding::from([8.0, 0.0]))
    .into()
}

/// All OCR entry rows of one image in the results list. Images without
/// entries show a placeholder line instead.
fn image_results<'a, S: UiState + ?Sized>(
    state: &'a S,
    image: &'a LoadedImage,
    index: usize,
) -> Vec<Element<'a, UiEvent>> {
    let mut elements = Vec::new();
    if image.project.ocr.visible_count() == 0 {
        elements.push(text("  No results yet.").size(12).color(MUTED_FG).into());
    } else {
        for entry in image.project.ocr.visible() {
            elements.push(entry_row(state, index, entry));
        }
    }
    elements
}

pub fn view<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    let total_results: usize = state.images().iter().map(|i| i.project.ocr.visible_count()).sum();
    let has_entries = total_results > 0;

    let mut results_list: Vec<Element<'_, UiEvent>> = Vec::new();
    if has_entries {
        results_list.push(label_row());
    }
    for (index, image) in state.images().iter().enumerate() {
        results_list.extend(image_results(state, image, index));
    }
    if results_list.is_empty() {
        results_list.push(
            text("No images loaded. Open images to begin.")
                .size(12)
                .color(MUTED_FG)
                .into(),
        );
    }

    column![
        text("Translate").size(16),
        row![
            text("Model:").size(12),
            pick_list(
                translation::MODELS,
                Some(state.translate_model()),
                |m| UiEvent::TranslateModel(m.to_string()),
            )
            .text_size(12)
            .width(FillLength),
        ]
        .spacing(6),
        row![
            text("To:").size(12),
            pick_list(
                translation::LANGUAGES,
                Some(state.translate_lang()),
                |l| UiEvent::TranslateLang(l.to_string()),
            )
            .text_size(12)
            .width(FillLength),
        ]
        .spacing(6),
        text_input("API key (optional, in-memory)", state.translate_api_key())
            .on_input(UiEvent::TranslateApiKey)
            .padding(4)
            .size(12)
            .width(FillLength),
        row![
            button("Translate").on_press_maybe(
                (has_entries && !state.translating() && !state.running()).then_some(UiEvent::Translate)
            ),
            text(format!(
                "{} image(s), {} result(s)",
                state.images().len(),
                total_results
            ))
            .size(12),
        ]
        .spacing(6),
        text(state.status()).size(12),
        scrollable(Column::with_children(results_list).spacing(4))
            .height(FillLength)
            .width(FillLength),
    ]
    .spacing(8)
    .height(FillLength)
    .into()
}