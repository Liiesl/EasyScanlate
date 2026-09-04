//! Right column: header, dual profile pickers (translate mode), scrollable OCR results list and translation bar.
//! In Edit mode the list shows one input per row (current profile's text, OCR fallback); in Translate mode two inputs (base vs target).

use iced::advanced::widget::operation::{self as widget_op, Operation, Outcome, Scrollable};
use iced::advanced::widget::{operate, Id as WidgetId};
use iced::widget::operation::AbsoluteOffset;
use iced::widget::text_editor;
use iced::widget::{
    button, column, container, mouse_area, pick_list, row, scrollable, space, text, tooltip, Column, Id,
};
use iced::{keyboard, Background, Border, Color, Element, Fill as FillLength, Font, Length, Padding,
    Rectangle, Vector};
use neverliie_iced_widgets::advanced_dropdown::{advanced_dropdown, Footer, Item, MenuItem};
use std::fmt::{Display, Formatter};

use crate::event::{EditOrigin, SettingsTab, TargetProfileSelection, ToolbarAction, TranslationPanelMode, UiEvent};
use crate::loaded::LoadedImage;
use crate::panel::{MUTED_FG, PANEL_BG};
use crate::scale;
use crate::segmented::{segment_icon, segmented_group};
use crate::state::UiState;
use crate::translation;
use lucide_icons::Icon;
use easyscanlate_model::{EntryId, OcrEntry, ProfileId};

/// Widget id of the multi-line editor shown in a row while the entry is
/// edited from the panel; must match the app's focus id.
const PANEL_EDIT_INPUT_ID: &str = "panel-editor";

/// Widget id of the scrollable results list; used by the app to scroll a
/// selected entry's row into view.
pub const PANEL_LIST_ID: &str = "panel-results-list";

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
/// Highlight behind a selected row — slightly lighter and more opaque than `PANEL_BG`
/// so selection is obvious without a border.
const SELECTED_BG: Color = Color::from_rgba8(52, 58, 76, 0.90);

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

/// The current profile's text of an entry, boxed like the other inputs;
/// read-only until clicked, which starts the inline edit.
fn current_box(value: String, font: Font) -> Element<'static, UiEvent> {
    input_box(text(value).size(scale::s(12.0)).font(font))
}

fn tip_label(label: &str) -> container::Container<'_, UiEvent> {
    container(text(label).size(scale::s(11.0)))
        .padding(scale::s(6.0))
        .style(container::rounded_box)
}

/// One results row: behavior depends on TranslationPanelMode.
/// Edit: single input (current profile's display_text, with panel edit).
/// Translate: two inputs (base vs target), editing forbidden; clicks only select.
fn entry_row<'a, S: UiState + ?Sized>(
    state: &'a S,
    index: usize,
    entry: &'a OcrEntry,
) -> Element<'a, UiEvent> {
    let entry_id = entry.id;
    let selected = state.selected() == Some((index, entry_id));
    let font = state.font().unwrap_or(Font::DEFAULT);
    let mode = state.translation_panel_mode();

    let delete_btn = button(crate::icon::lucide(Icon::Trash2).size(scale::s(12.0)).center())
        .padding(scale::s(4.0))
        .style(crate::panel::button_style)
        .on_press(UiEvent::EntryToolbar((index, entry_id, ToolbarAction::Delete)));
    let delete_tip: Element<'_, UiEvent> =
        tooltip(delete_btn, tip_label("Delete entry"), tooltip::Position::Top)
            .gap(scale::s(4.0))
            .into();
    let retranslate_btn = button(crate::icon::lucide(Icon::RefreshCw).size(scale::s(12.0)).center())
        .padding(scale::s(4.0))
        .style(crate::panel::button_style)
        .on_press_maybe(
            (!state.is_bulk_busy()
                && !easyscanlate_settings::get(|s| s.connections.is_empty()))
            .then_some(UiEvent::RetranslateEntry((index, entry_id))),
        );
    let retranslate_tip: Element<'_, UiEvent> =
        tooltip(retranslate_btn, tip_label("Retranslate"), tooltip::Position::Top)
            .gap(scale::s(4.0))
            .into();
    let buttons: Vec<Element<'_, UiEvent>> = vec![retranslate_tip, delete_tip];

    let project = state.project();
    let content: Element<'_, UiEvent> = match mode {
        TranslationPanelMode::Edit => {
            let editing_here = state.editing() == Some((index, entry_id));
            let editing_from_panel = editing_here && state.editing_origin() == EditOrigin::Panel;
            let profile_name = project.profiles.selected().name.clone();
            let current: Element<'_, UiEvent> = if editing_from_panel {
                panel_editor(state)
            } else {
                let shown = if editing_here {
                    state
                        .edit_content()
                        .map(|content| content.text().to_string())
                        .unwrap_or_else(|| project.display_text(entry).to_string())
                } else {
                    project.display_text(entry).to_string()
                };
                mouse_area(current_box(shown, font))
                    .on_press(UiEvent::PanelEntryEdit((index, entry_id)))
                    .into()
            };
            let row_inner = container(
                row![
                    column![text(profile_name).size(scale::s(10.0)).color(MUTED_FG), current]
                        .spacing(scale::s(2.0))
                        .width(FillLength),
                    row(buttons).spacing(scale::s(4.0)),
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
                background: Some(if selected { SELECTED_BG } else { PANEL_BG }.into()),
                border: Border::default()
                    .width(0.0)
                    .rounded(scale::s(12.0)),
                ..container::Style::default()
            });
            if editing_from_panel {
                row_inner.into()
            } else {
                mouse_area(row_inner).on_press(UiEvent::EntryClicked(Some((index, entry_id)))).into()
            }
        }
        TranslationPanelMode::Translate => {
            // Resolve base and target display strings per image (project is chapter-wide)
            let base_id = state.base_profile();
            let target_sel = state.target_profile();
            // base text
            let base_text: String = if let Some(pid) = base_id {
                if let Some(p) = project.profiles.iter().find(|p| p.id == pid) {
                    p.translation_of(entry_id).unwrap_or(&entry.text).to_string()
                } else {
                    entry.text.clone()
                }
            } else {
                entry.text.clone()
            };
            let base_name: String = if let Some(pid) = base_id {
                project.profiles.iter().find(|p| p.id == pid).map(|p| p.name.clone()).unwrap_or_else(|| "—".to_string())
            } else {
                "—".to_string()
            };
            // target text
            let (target_text, target_name) = match &target_sel {
                TargetProfileSelection::Existing(pid) => {
                    let name = project.profiles.iter().find(|p| p.id == *pid).map(|p| p.name.clone()).unwrap_or_else(|| "—".to_string());
                    let txt = project.profiles.iter().find(|p| p.id == *pid).and_then(|p| p.translation_of(entry_id)).unwrap_or("").to_string();
                    (txt, name)
                }
                TargetProfileSelection::AutoPlaceholder(name) => {
                    // If placeholder already exists as real profile, show its content (state getter usually converts, but per-image fallback)
                    if let Some(id) = project.profiles.find_by_name(name) {
                        let txt = project.profiles.iter().find(|p| p.id == id).and_then(|p| p.translation_of(entry_id)).unwrap_or("").to_string();
                        let real_name = project.profiles.iter().find(|p| p.id == id).map(|p| p.name.clone()).unwrap_or_else(|| name.clone());
                        (txt, real_name)
                    } else {
                        (String::new(), name.clone())
                    }
                }
            };
            let left = input_box(text(base_text).size(scale::s(12.0)).font(font));
            let right = input_box(text(target_text).size(scale::s(12.0)).font(font));
            // In translate mode editing is forbidden: whole row is just selection
            let row_inner = container(
                row![
                    column![text(base_name).size(scale::s(10.0)).color(MUTED_FG), left]
                        .spacing(scale::s(2.0))
                        .width(FillLength),
                    column![text(target_name).size(scale::s(10.0)).color(MUTED_FG), right]
                        .spacing(scale::s(2.0))
                        .width(FillLength),
                    row(buttons).spacing(scale::s(4.0)),
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
                background: Some(if selected { SELECTED_BG } else { PANEL_BG }.into()),
                border: Border::default()
                    .width(0.0)
                    .rounded(scale::s(12.0)),
                ..container::Style::default()
            });
            mouse_area(row_inner).on_press(UiEvent::EntryClicked(Some((index, entry_id)))).into()
        }
    };

    content
}

/// All OCR entry rows of one image in the results list. Images without
/// entries contribute nothing: the list is simply empty for them.
fn image_results<'a, S: UiState + ?Sized>(
    state: &'a S,
    image: &'a LoadedImage,
    index: usize,
) -> Vec<Element<'a, UiEvent>> {
    let project = state.project();
    let mut elements = Vec::new();
    if project.visible_count_for(image.image_id) > 0 {
        for entry in project.visible_for(image.image_id) {
            elements.push(entry_row(state, index, entry));
        }
    }
    elements
}

/// One entry of the merged model dropdown of the translation bar: the model
/// wire `id` (sent to the API) plus its display `name` and provider display.
/// The closed dropdown renders as `{provider}:{display name}`; selection still
/// sends the `id`.
#[derive(Debug, Clone, PartialEq)]
struct ModelOption {
    provider_id: String,
    provider_name: String,
    model_id: String,
    model_name: String,
}

impl Display for ModelOption {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.provider_name, self.model_name)
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

impl Display for ProfileOption {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TargetPickOption {
    sel: TargetProfileSelection,
    label: String,
}

impl Display for TargetPickOption {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

/// Segmented switcher for the translation panel (Edit | Translate), styled like main_area mode switcher.
fn translation_mode_switcher<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    let mode = state.translation_panel_mode();
    let pill = segmented_group(vec![
        segment_icon(
            mode == TranslationPanelMode::Edit,
            Icon::Pencil,
            Some(UiEvent::TranslationPanelMode(TranslationPanelMode::Edit)),
        ),
        segment_icon(
            mode == TranslationPanelMode::Translate,
            Icon::Languages,
            Some(UiEvent::TranslationPanelMode(TranslationPanelMode::Translate)),
        ),
    ]);
    container(pill)
        .width(Length::Fixed(scale::s(88.0)))
        .into()
}

/// The pinned header of the results column: "TRANSLATION" label, optional profile dropdown (edit only) and the mode switcher on the right.
fn profile_header<'a, S: UiState + ?Sized>(state: &'a S) -> Element<'a, UiEvent> {
    let mode = state.translation_panel_mode();
    let switcher = translation_mode_switcher(state);
    if mode == TranslationPanelMode::Edit {
        let mut entries: Vec<MenuItem<'a, ProfileOption, UiEvent, iced::Theme, iced::Renderer>> =
            Vec::new();
        let mut selected = None;
        {
            let project = state.project();
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
            switcher,
        ]
        .spacing(scale::s(6.0))
        .align_y(iced::Alignment::Center)
        .width(FillLength)
        .into()
    } else {
        row![
            text("TRANSLATION").size(scale::s(11.0)).color(MUTED_FG),
            space::horizontal(),
            switcher,
        ]
        .spacing(scale::s(6.0))
        .align_y(iced::Alignment::Center)
        .width(FillLength)
        .into()
    }
}

/// Two pick_lists for translate mode: base (left) and target (right). Normal pick_list (not advanced_dropdown).
fn translate_profile_pickers<'a, S: UiState + ?Sized>(state: &'a S) -> Element<'a, UiEvent> {
    if state.images().is_empty() {
        return space::horizontal().into();
    }
    let base = state.base_profile();
    let target_sel = state.target_profile();
    let placeholder_name = state.target_placeholder_name();
    // Build base options: all profiles
    let profiles: Vec<(ProfileId, String)> = state
        .project()
        .profiles
        .iter()
        .map(|p| (p.id, p.name.clone()))
        .collect();
    let base_options: Vec<ProfileOption> = profiles
        .iter()
        .map(|(id, name)| ProfileOption { id: *id, name: name.clone() })
        .collect();
    let base_selected = base.and_then(|bid| base_options.iter().find(|o| o.id == bid).cloned());

    // Target options: all profiles except base + optional placeholder
    let placeholder_exists = profiles.iter().any(|(_, n)| n == &placeholder_name);
    let mut target_options: Vec<TargetPickOption> = Vec::new();
    for (id, name) in &profiles {
        if Some(*id) == base {
            continue;
        }
        target_options.push(TargetPickOption { sel: TargetProfileSelection::Existing(*id), label: name.clone() });
    }
    // Add placeholder if not already present as a real profile
    if !placeholder_exists {
        // ensure placeholder not equal to base name (already filtered base, but also name compare)
        let base_name = base.and_then(|bid| profiles.iter().find(|(id,_)| *id==bid).map(|(_, n)| n.clone()));
        if base_name.as_deref() != Some(&placeholder_name) {
            target_options.push(TargetPickOption { sel: TargetProfileSelection::AutoPlaceholder(placeholder_name.clone()), label: placeholder_name.clone() });
        }
    }
    // If placeholder exists, it is already in target_options as Existing; no virtual needed.

    let target_selected_label = match &target_sel {
        TargetProfileSelection::Existing(id) => profiles.iter().find(|(pid,_)| pid==id).map(|(_,n)| n.clone()),
        TargetProfileSelection::AutoPlaceholder(name) => Some(name.clone()),
    };
    let target_selected = target_selected_label.and_then(|lbl| target_options.iter().find(|o| o.label==lbl).cloned());

    row![
        column![
            text("Base").size(scale::s(10.0)).color(MUTED_FG),
            pick_list(base_options, base_selected, |o| UiEvent::BaseProfileSelect(o.id)).text_size(scale::s(12.0)).placeholder("Base…")
        ].spacing(scale::s(2.0)).width(FillLength),
        column![
            text("Target").size(scale::s(10.0)).color(MUTED_FG),
            pick_list(target_options, target_selected, |o| UiEvent::TargetProfileSelect(o.sel.clone())).text_size(scale::s(12.0)).placeholder("Target…")
        ].spacing(scale::s(2.0)).width(FillLength),
    ]
    .spacing(scale::s(8.0))
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
    let connected = easyscanlate_settings::get(|s| !s.connections.is_empty());
    let body: Element<'_, UiEvent> = if connected {
        let (sel_provider, sel_model) = state.translate_model_selection();
        let mut entries: Vec<MenuItem<'a, ModelOption, UiEvent, iced::Theme, iced::Renderer>> =
            Vec::new();
        let mut selected = None;
        for (provider_id, provider_name, models) in state.translate_model_groups() {
            entries.push(MenuItem::Label(provider_name.as_str()));
            for (model_id, model_name) in models {
                let option = ModelOption {
                    provider_id: provider_id.clone(),
                    provider_name: provider_name.clone(),
                    model_id: model_id.clone(),
                    model_name: model_name.clone(),
                };
                if provider_id.as_str() == sel_provider && model_id.as_str() == sel_model {
                    selected = Some(option.clone());
                }
                entries.push(MenuItem::Item(Item::new(option, model_name.as_str())));
            }
        }

        let translate_btn = button(crate::icon::lucide(Icon::Send).size(scale::s(14.0)).center())
            .padding(scale::s(6.0))
            .style(crate::panel::button_style)
            .on_press_maybe(
                (has_entries && !state.is_bulk_busy())
                    .then_some(UiEvent::Translate)
            );
        let translate: Element<'_, UiEvent> =
            tooltip(translate_btn, tip_label("Translate"), tooltip::Position::Top)
                .gap(scale::s(4.0))
                .into();
        row![
            advanced_dropdown(entries, selected, |option| UiEvent::TranslateModelSelect {
                provider: option.provider_id.clone(),
                model: option.model_id.clone(),
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
            translate,
        ]
        .spacing(scale::s(6.0))
        .align_y(iced::Alignment::Center)
        .into()
    } else {
        let not_connected_btn = button(crate::icon::lucide(Icon::Settings).size(scale::s(14.0)).center())
            .padding(scale::s(6.0))
            .style(crate::panel::button_style)
            .on_press(UiEvent::SettingsOpenTab(SettingsTab::Translation));
        let not_connected: Element<'_, UiEvent> =
            tooltip(not_connected_btn, tip_label("Open translation settings"), tooltip::Position::Top)
                .gap(scale::s(4.0))
                .into();
        row![
            text("Translation service: Not connected").size(scale::s(12.0)),
            space::horizontal(),
            not_connected,
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
    let project = state.project();
    let total_results: usize = state.images().iter().map(|i| project.visible_count_for(i.image_id)).sum();
    let has_entries = total_results > 0;
    let mode = state.translation_panel_mode();

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

    let pickers: Option<Element<'_, UiEvent>> = if mode == TranslationPanelMode::Translate {
        Some(translate_profile_pickers(state))
    } else {
        None
    };

    let bar: Option<Element<'_, UiEvent>> = if mode == TranslationPanelMode::Translate {
        Some(translate_bar(state, has_entries))
    } else {
        None
    };

    let mut col = column![profile_header(state)];
    if let Some(p) = pickers {
        col = col.push(p);
    }
    col = col.push(
        scrollable(Column::with_children(results_list).spacing(scale::s(8.0)))
            .id(PANEL_LIST_ID)
            .height(FillLength)
            .width(FillLength),
    );
    if state.translating() {
        col = col.push(crate::loading_bar::LoadingBar::new(state.translation_anim_phase()).view());
    }
    if let Some(b) = bar {
        col = col.push(b);
    }

    col.spacing(scale::s(8.0))
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
