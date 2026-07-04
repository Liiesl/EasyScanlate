//! Right column: a pinned header (the "TRANSLATION" label and the profile
//! dropdown), the tall scrollable OCR results list below it and the short
//! translation controls (the merged model dropdown, the language picker and
//! the translate button) at the bottom. The list shows one row per entry with
//! two side-by-side inputs: the read-only original OCR text on the left and
//! the selected profile's text on the right. The right side starts the same
//! multi-line inline edit as the main area (fork on first keystroke, Enter =
//! newline, Escape/Ctrl+Enter to commit); clicking a row selects the entry,
//! highlighted with a border. The API key is configured in the settings
//! modal, not here.

use iced::advanced::widget::operation::{self as widget_op, Operation, Outcome, Scrollable};
use iced::advanced::widget::{operate, Id as WidgetId};
use iced::widget::operation::AbsoluteOffset;
use iced::widget::text_editor;
use iced::widget::{
    button, column, container, mouse_area, pick_list, row, scrollable, space, text, Column, Id,
};
use iced::{keyboard, Background, Border, Color, Element, Fill as FillLength, Font, Padding,
    Rectangle, Vector};
use neverliie_iced_widgets::advanced_dropdown::{advanced_dropdown, Footer, Item, MenuItem};

use crate::event::{EditOrigin, SettingsTab, ToolbarAction, UiEvent};
use crate::loaded::LoadedImage;
use crate::panel::{MUTED_FG, PANEL_BG};
use crate::scale;
use crate::state::UiState;
use crate::translation;
use scanlateit_model::{EntryId, OcrEntry, ProfileId};

/// Widget id of the multi-line editor shown in a row while the entry is
/// edited from the panel; must match the app's focus id.
const PANEL_EDIT_INPUT_ID: &'static str = "panel-editor";

/// Widget id of the scrollable results list; used by the app to scroll a
/// selected entry's row into view.
pub const PANEL_LIST_ID: &'static str = "panel-results-list";

/// The widget id of the results row for `(index, id)`; must match the
/// container id set in `entry_row`.
pub fn panel_row_id(index: usize, id: EntryId) -> Id {
    format!("panel-row-{index}-{}", id.0).into()
}

const ROW_SPACING: f32 = 8.0;
/// Horizontal gap inside an input box.
const BOX_PADDING: f32 = 6.0;
/// Large padding between the row's border/background and its content; the
/// whitespace around each item in the list.
const ROW_PADDING: Padding = Padding {
    top: 14.0,
    right: 16.0,
    bottom: 14.0,
    left: 16.0,
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
        .padding(scale::s(BOX_PADDING))
        .style(|_theme| container::Style {
            background: Some(BOX_BG.into()),
            border: Border::default().rounded(scale::s(4.0)),
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
            .size(scale::s(12.0))
            .line_height(1.2)
            .padding(scale::s(0.0))
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
                border: Border::default().rounded(scale::s(0.0)),
                placeholder: MUTED_FG,
                value: Color::from_rgb(0.9, 0.9, 0.9),
                selection: Color::from_rgba8(92, 190, 255, 0.35),
            }),
    )
}

/// The read-only original text of an entry, boxed like the other inputs.
fn original_box(entry_text: &str, font: Font) -> Element<'_, UiEvent> {
    input_box(text(entry_text).size(scale::s(12.0)).font(font))
}

/// The current profile's text of an entry, boxed like the other inputs;
/// read-only until clicked, which starts the inline edit.
fn current_box(value: String, font: Font) -> Element<'static, UiEvent> {
    input_box(text(value).size(scale::s(12.0)).font(font))
}

/// One results row: the original OCR text on the left, the selected
/// profile's text on the right, and the delete/retranslate buttons on the
/// far right. Each box carries a small label above it: "kor" for the OCR
/// source language, the selected profile's name for the right side.
/// Clicking the left box selects the row; clicking the right box starts the
/// inline edit for it. The row being edited from the panel swaps its right
/// box for the live editor (and is not a click target, so the editor keeps
/// the clicks). The selected row is outlined with a highlight border.
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
    let profile_name = state.images()[index].project.profiles.selected().name.clone();

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

    let mut buttons: Vec<Element<'_, UiEvent>> = vec![
        button(text("Delete").size(scale::s(10.0)))
            .padding([scale::s(2.0), scale::s(6.0)])
            .on_press(UiEvent::EntryToolbar((index, entry_id, ToolbarAction::Delete)))
            .into(),
    ];
    buttons.push(
        button(text("Retranslate").size(scale::s(10.0)))
            .padding([scale::s(2.0), scale::s(6.0)])
            .on_press_maybe(
                (!state.translating()
                    && !state.running()
                    && !scanlateit_settings::get(|s| s.connections.is_empty()))
                .then_some(UiEvent::RetranslateEntry((index, entry_id))),
            )
            .into(),
    );

    let row = container(
        row![
            column![text("kor").size(scale::s(10.0)).color(MUTED_FG), original]
                .spacing(scale::s(2.0))
                .width(FillLength),
            column![text(profile_name).size(scale::s(10.0)).color(MUTED_FG), current]
                .spacing(scale::s(2.0))
                .width(FillLength),
            column(buttons).spacing(scale::s(4.0)),
        ]
        .spacing(scale::s(ROW_SPACING))
        .align_y(iced::Alignment::Center),
    )
    .id(panel_row_id(index, entry_id))
    .width(FillLength)
    .padding(Padding {
        top: scale::s(ROW_PADDING.top),
        right: scale::s(ROW_PADDING.right),
        bottom: scale::s(ROW_PADDING.bottom),
        left: scale::s(ROW_PADDING.left),
    })
    .style(move |_theme| container::Style {
        background: Some(PANEL_BG.into()),
        border: Border::default()
            .width(scale::s(1.0))
            .color(if selected { SELECTED_BORDER } else { ROW_BORDER })
            .rounded(scale::s(12.0)),
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

/// All OCR entry rows of one image in the results list. Images without
/// entries contribute nothing: the list is simply empty for them.
fn image_results<'a, S: UiState + ?Sized>(
    state: &'a S,
    image: &'a LoadedImage,
    index: usize,
) -> Vec<Element<'a, UiEvent>> {
    let mut elements = Vec::new();
    if image.project.ocr.visible_count() > 0 {
        for entry in image.project.ocr.visible() {
            elements.push(entry_row(state, index, entry));
        }
    }
    elements
}

/// One entry of the merged model dropdown of the translation bar: the model
/// id plus its provider's id and display name. The closed dropdown renders
/// this as `{provider}:{model}`.
#[derive(Debug, Clone, PartialEq)]
struct ModelOption {
    provider_id: String,
    provider_name: String,
    model: String,
}

impl ToString for ModelOption {
    fn to_string(&self) -> String {
        format!("{}:{}", self.provider_name, self.model)
    }
}

/// One entry of the profile dropdown in the results header: the profile's id
/// and display name. Wraps [`ProfileId`] because the dropdown requires a
/// `ToString` value and the id alone has no display rendering.
#[derive(Debug, Clone, PartialEq)]
struct ProfileOption {
    id: ProfileId,
    name: String,
}

impl ToString for ProfileOption {
    fn to_string(&self) -> String {
        self.name.clone()
    }
}

/// The pinned header of the results column: the "TRANSLATION" label on the
/// left and the profile dropdown on the right. The dropdown lists every
/// profile of the first image's project (all projects share the same
/// profiles) with a "+ New Profile" footer row that creates a fresh one.
fn profile_header<'a, S: UiState + ?Sized>(state: &'a S) -> Element<'a, UiEvent> {
    let mut entries: Vec<MenuItem<'a, ProfileOption, UiEvent, iced::Theme, iced::Renderer>> =
        Vec::new();
    let mut selected = None;
    if let Some(project) = state.images().first().map(|i| &i.project) {
        for profile in project.profiles.iter() {
            let option = ProfileOption {
                id: profile.id,
                name: profile.name.clone(),
            };
            if profile.id == project.profiles.selected_id() {
                selected = Some(option.clone());
            }
            entries.push(MenuItem::Item(Item::new(option, profile.name.clone())));
        }
    }

    row![
        text("TRANSLATION").size(scale::s(11.0)).color(MUTED_FG),
        space::horizontal(),
        text("profile").size(scale::s(11.0)).color(MUTED_FG),
        advanced_dropdown(entries, selected, |option| UiEvent::ProfileSelect(option.id))
            .placeholder("Profile…")
            .text_size(scale::s(12.0))
            .width(scale::s(150.0))
            .footer(Footer::new("+ New Profile", UiEvent::ProfileCreate)),
    ]
    .spacing(scale::s(6.0))
    .align_y(iced::Alignment::Center)
    .width(FillLength)
    .into()
}

/// The bottom translation bar: one row with the merged provider/model
/// dropdown, the target-language picker and the translate button. When no
/// connection is configured the bar collapses to a status row with a
/// "Configure…" button that opens the settings modal on the Translation tab;
/// the configure button is never shown otherwise.
fn translate_bar<'a, S: UiState + ?Sized>(
    state: &'a S,
    has_entries: bool,
) -> Element<'a, UiEvent> {
    let connected = scanlateit_settings::get(|s| !s.connections.is_empty());
    let body: Element<'_, UiEvent> = if connected {
        let (sel_provider, sel_model) = state.translate_model_selection();
        let mut entries: Vec<MenuItem<'a, ModelOption, UiEvent, iced::Theme, iced::Renderer>> =
            Vec::new();
        let mut selected = None;
        for (provider_id, provider_name, models) in state.translate_model_groups() {
            entries.push(MenuItem::Label(provider_name.as_str()));
            for model in models {
                let option = ModelOption {
                    provider_id: provider_id.clone(),
                    provider_name: provider_name.clone(),
                    model: model.clone(),
                };
                if provider_id.as_str() == sel_provider && model.as_str() == sel_model {
                    selected = Some(option.clone());
                }
                entries.push(MenuItem::Item(Item::new(option, model.as_str())));
            }
        }

        row![
            advanced_dropdown(entries, selected, |option| UiEvent::TranslateModelSelect {
                provider: option.provider_id.clone(),
                model: option.model.clone(),
            })
            .placeholder("Select a model…")
            .searchable(true)
            .text_size(scale::s(12.0))
            .width(FillLength)
            .menu_max_height(280.0)
            .footer(Footer::new("Manage models…", UiEvent::ManageModelsOpen)),
            text("To:").size(scale::s(12.0)),
            pick_list(
                translation::LANGUAGES,
                Some(state.translate_lang()),
                |l| UiEvent::TranslateLang(l.to_string()),
            )
            .text_size(scale::s(12.0)),
            button("Translate").on_press_maybe(
                (has_entries && !state.translating() && !state.running())
                    .then_some(UiEvent::Translate)
            ),
        ]
        .spacing(scale::s(6.0))
        .align_y(iced::Alignment::Center)
        .into()
    } else {
        row![
            text("Translation service: Not connected").size(scale::s(12.0)),
            space::horizontal(),
            button(text("Configure…").size(scale::s(11.0)))
                .padding([scale::s(2.0), scale::s(6.0)])
                .on_press(UiEvent::SettingsOpenTab(SettingsTab::Translation)),
        ]
        .spacing(scale::s(6.0))
        .align_y(iced::Alignment::Center)
        .into()
    };

    container(body)
        .width(FillLength)
        .padding(scale::s(6.0))
        .style(|_theme| container::Style {
            background: Some(BOX_BG.into()),
            border: Border::default().rounded(scale::s(4.0)),
            ..container::Style::default()
        })
        .into()
}

pub fn view<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    let total_results: usize = state.images().iter().map(|i| i.project.ocr.visible_count()).sum();
    let has_entries = total_results > 0;

    let mut results_list: Vec<Element<'_, UiEvent>> = Vec::new();
    for (index, image) in state.images().iter().enumerate() {
        results_list.extend(image_results(state, image, index));
    }
    if state.images().is_empty() {
        results_list.push(
            text("No images loaded. Open images to begin.")
                .size(scale::s(12.0))
                .color(MUTED_FG)
                .into(),
        );
    }

    let bar = translate_bar(state, has_entries);

    column![
        profile_header(state),
        scrollable(Column::with_children(results_list).spacing(scale::s(8.0)))
            .id(PANEL_LIST_ID)
            .height(FillLength)
            .width(FillLength),
        bar,
    ]
    .spacing(scale::s(8.0))
    .height(FillLength)
    .into()
}

/// Widget operation: finds the results panel's scrollable and the row
/// container of `(index, id)` during one traversal, then — if that row is
/// not fully visible — chains a `scroll_to` on the panel list (second
/// traversal pass, see the runtime's `Outcome::Chain` handling) that centers
/// the row. All bounds are absolute (iced layouts use absolute coordinates).
struct MeasurePanelRow {
    panel: WidgetId,
    row: WidgetId,
    panel_bounds: Option<Rectangle>,
    panel_offset: f32,
    row_y: Option<f32>,
    row_h: Option<f32>,
}

impl<T: 'static> Operation<T> for MeasurePanelRow {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&WidgetId>,
        bounds: Rectangle,
        _content_bounds: Rectangle,
        translation: Vector,
        _state: &mut dyn Scrollable,
    ) {
        if id == Some(&self.panel) {
            self.panel_bounds = Some(bounds);
            self.panel_offset = translation.y;
        }
    }

    fn container(&mut self, id: Option<&WidgetId>, bounds: Rectangle) {
        if id == Some(&self.row) {
            self.row_y = Some(bounds.y);
            self.row_h = Some(bounds.height);
        }
    }

    fn finish(&self) -> Outcome<T> {
        let (Some(panel), Some(row_y), Some(row_h)) =
            (self.panel_bounds, self.row_y, self.row_h)
        else {
            return Outcome::None;
        };
        let top = row_y - self.panel_offset; // row's window-space top
        let bottom = top + row_h; // row's window-space bottom
        let visible = top >= panel.y && bottom <= panel.y + panel.height;
        if visible {
            return Outcome::None; // already visible: no jump
        }
        let target = (row_y - panel.y - (panel.height - row_h) / 2.0).max(0.0);
        Outcome::Chain(Box::new(widget_op::scrollable::scroll_to(
            self.panel.clone(),
            AbsoluteOffset { x: Some(0.0), y: Some(target) },
        )))
    }
}

/// Scrolls the results list so the row of `(index, id)` is fully visible
/// (centered when out of view); no-op when already visible. Generic over the
/// message type so the app can return it directly.
pub fn scroll_to_row<T>(index: usize, id: EntryId) -> iced::Task<T>
where
    T: Send + 'static,
{
    operate(MeasurePanelRow {
        panel: WidgetId::new(PANEL_LIST_ID),
        row: panel_row_id(index, id),
        panel_bounds: None,
        panel_offset: 0.0,
        row_y: None,
        row_h: None,
    })
}