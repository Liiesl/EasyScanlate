//! Manage Models overlay: opened from Translation settings, lets the user
//! toggle each model per provider. Deprecated and non-text models are never
//! listed here (filtered at provider layer). All usable models are fetched
//! (no family de-duplication); older paid family members are auto-hidden by
//! default via `default_hidden_ids_for_models` (free and `*-latest` stay
//! visible, newest per family via `release_date`/`last_updated` stays visible)
//! and stored in `hidden_models`. The basic configuration with a hidden entry
//! shows the default older-family-hidden list; clearing the entry shows all.
//! Hiding is written straight into the shared settings store; "Reset" restores
//! the default hidden set, not an empty one.

use iced::widget::{
    button, center, column, container, mouse_area, opaque, row, rule, scrollable, space, stack,
    text, text_input, toggler, tooltip,
};
use iced::{Color, Element, Fill as FillLength, Length};
use lucide_icons::Icon;

use crate::event::{SettingEdit, UiEvent};
use crate::panel::PANEL_BG;
use crate::scale;
use crate::state::UiState;

const MODAL_WIDTH: f32 = 540.0;
const MODAL_HEIGHT: f32 = 500.0;
const MUTED_FG: Color = Color::from_rgb(0.6, 0.6, 0.6);

fn item_separator<'a>() -> Element<'a, UiEvent> {
    rule::horizontal(1)
        .style(|_theme| rule::Style {
            color: Color::from_rgba8(255, 255, 255, 0.08),
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        })
        .into()
}

/// Writes one change into the shared settings store (write-through) and
/// returns the single announcement event for the app.
fn set(f: impl FnOnce(&mut easyscanlate_settings::Settings)) -> UiEvent {
    let _ = easyscanlate_settings::modify(f);
    UiEvent::SettingsChanged
}

fn normalize_for_search(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '-' | '/' | '.' | '_') {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out.to_lowercase()
}

fn model_matches(query: &str, model: &str) -> bool {
    if query.trim().is_empty() {
        return true;
    }
    let q_norm = normalize_for_search(query);
    let m_norm = normalize_for_search(model);
    q_norm
        .split_whitespace()
        .all(|tok| m_norm.contains(tok))
}

pub fn view<'a, S: UiState + ?Sized>(
    state: &'a S,
    base: Element<'a, UiEvent>,
) -> Element<'a, UiEvent> {
    let groups = state.all_model_groups();
    let query = state.manage_models_search().to_string();
    let is_filtering = !query.trim().is_empty();

    let header = row![
        text("Manage Models").size(scale::s(16.0)),
        space::horizontal(),
        tooltip(
            button(crate::icon::lucide(Icon::X).size(scale::s(14.0)).center())
                .padding(scale::s(4.0))
                .style(crate::panel::button_style)
            .on_press(UiEvent::ManageModelsClose),
            container(text("Close").size(scale::s(11.0))).padding(scale::s(6.0)).style(container::rounded_box),
            tooltip::Position::Top
        ).gap(scale::s(4.0)),
    ];

    let description = column![
        text("Toggle models per provider. Hidden models disappear from the translation dropdown.")
            .size(scale::s(11.0))
            .color(MUTED_FG),
        text("Deprecated models are always hidden and never shown here.")
            .size(scale::s(11.0))
            .color(MUTED_FG),
    ]
    .spacing(scale::s(2.0));

    let search: Element<'_, UiEvent> = if groups.is_empty() {
        // No providers – no need for search field.
        space::vertical().height(Length::Fixed(0.0)).into()
    } else {
        text_input("Search models…", &query)
            .on_input(UiEvent::ManageModelsSearch)
            .padding([scale::s(6.0), scale::s(8.0)])
            .size(scale::s(12.0))
            .width(FillLength)
            .into()
    };

    let body: Element<'_, UiEvent> = if groups.is_empty() {
        container(
            text("No connected providers – connect a translation service first.")
                .size(scale::s(12.0))
                .color(MUTED_FG),
        )
        .padding(scale::s(12.0))
        .into()
    } else {
        let mut provider_cols: Vec<Element<'_, UiEvent>> = Vec::new();
        for (provider_id, provider_name, models) in groups {
            // Filter models by search query (case-insensitive, space aliases -, /, ., _).
            // Match against both wire id and display name.
            // Example: "meta spark" matches "meta/muse-spark-1.2" or its display.
            let filtered: Vec<(String, String)> = models
                .iter()
                .filter(|(id, name)| model_matches(&query, id) || model_matches(&query, name))
                .cloned()
                .collect();
            if filtered.is_empty() {
                continue;
            }

            // Snapshot hidden set for this provider once per frame (keyed by id).
            let hidden_set = easyscanlate_settings::get(|s| {
                s.hidden_models
                    .get(&provider_id)
                    .cloned()
                    .unwrap_or_default()
            });
            let visible_cnt = filtered
                .iter()
                .filter(|(id, _)| !hidden_set.contains(id))
                .count();
            // Master toggler is ON if any filtered model is visible (per requirement).
            let master_on = visible_cnt > 0;

            let mut rows: Vec<Element<'_, UiEvent>> = Vec::new();
            // Header row: name + count + master toggler
            {
                let total = filtered.len();
                // When filtering show filtered counts; otherwise total counts are the same.
                let count_label = if is_filtering {
                    format!("{visible_cnt}/{total}")
                } else {
                    format!("{visible_cnt}/{total}")
                };
                let pid = provider_id.clone();
                let filtered_ids: Vec<String> = filtered.iter().map(|(id, _)| id.clone()).collect();
                let filtered_for_toggle = filtered_ids.clone();
                let master_toggler: Element<'_, UiEvent> = toggler(master_on)
                    .size(scale::s(18.0))
                    .style(crate::toggler_style::style)
                    .on_toggle(move |v| {
                        let pid = pid.clone();
                        let models = filtered_for_toggle.clone();
                        set(move |s| {
                            if v {
                                // Enable: make filtered models visible (remove from hidden)
                                if let Some(set) = s.hidden_models.get_mut(&pid) {
                                    for m in &models {
                                        set.remove(m);
                                    }
                                    if set.is_empty() {
                                        s.hidden_models.remove(&pid);
                                    }
                                }
                            } else {
                                // Disable: hide all filtered models (by id)
                                let entry = s.hidden_models.entry(pid).or_default();
                                for m in models {
                                    entry.insert(m);
                                }
                            }
                        })
                    })
                    .into();

                rows.push(
                    row![
                        text(provider_name.clone()).size(scale::s(13.0)),
                        text(count_label).size(scale::s(11.0)).color(MUTED_FG),
                        space::horizontal(),
                        master_toggler,
                    ]
                    .align_y(iced::Alignment::Center)
                    .spacing(scale::s(8.0))
                    .padding([scale::s(2.0), scale::s(0.0)])
                    .into(),
                );
            }
            if !filtered.is_empty() {
                rows.push(item_separator());
            }
            for (idx, (model_id, model_name)) in filtered.iter().enumerate() {
                let hidden = easyscanlate_settings::get(|s| {
                    s.hidden_models
                        .get(&provider_id)
                        .is_some_and(|set| set.contains(model_id))
                });
                let visible = !hidden;
                let pid = provider_id.clone();
                let mid = model_id.clone();
                let model_row: Element<'_, UiEvent> = row![
                    text(model_name.clone()).size(scale::s(12.0)).width(FillLength),
                    toggler(visible)
                        .size(scale::s(18.0))
                        .style(crate::toggler_style::style)
                        .on_toggle(move |v| {
                            let pid = pid.clone();
                            let mid = mid.clone();
                            set(move |s| {
                                if v {
                                    if let Some(set) = s.hidden_models.get_mut(&pid) {
                                        set.remove(&mid);
                                        if set.is_empty() {
                                            s.hidden_models.remove(&pid);
                                        }
                                    }
                                } else {
                                    s.hidden_models
                                        .entry(pid)
                                        .or_default()
                                        .insert(mid);
                                }
                            })
                        }),
                ]
                .spacing(scale::s(12.0))
                .align_y(iced::Alignment::Center)
                .padding([scale::s(4.0), scale::s(0.0)])
                .into();
                rows.push(model_row);
                if idx + 1 < filtered.len() {
                    rows.push(item_separator());
                }
            }
            // Provider card — keep card, now with separators between each model row
            let card = container(column(rows).spacing(scale::s(4.0)))
                .padding(scale::s(8.0))
                .style(|_theme| container::Style {
                    background: Some(Color::from_rgba8(255, 255, 255, 0.06).into()),
                    border: iced::Border::default().rounded(scale::s(6.0)),
                    ..Default::default()
                })
                .width(FillLength)
                .into();
            provider_cols.push(card);
        }
        if provider_cols.is_empty() {
            // Filtering hid everything
            container(
                column![
                    text(format!("No models match “{query}”.")).size(scale::s(12.0)).color(MUTED_FG),
                    text("Try a different search term.").size(scale::s(11.0)).color(MUTED_FG),
                ]
                .spacing(scale::s(4.0)),
            )
            .padding(scale::s(12.0))
            .into()
        } else {
            scrollable(column(provider_cols).spacing(scale::s(10.0)))
                .height(Length::Fill)
                .into()
        }
    };

    let footer = row![
        tooltip(
            button(crate::icon::lucide(Icon::RotateCcw).size(scale::s(14.0)).center())
                .padding(scale::s(6.0))
                .style(crate::panel::button_style)
            .on_press(UiEvent::SettingEdit(SettingEdit::HiddenModelsResetAll)),
            container(text("Reset hidden models").size(scale::s(11.0))).padding(scale::s(6.0)).style(container::rounded_box),
            tooltip::Position::Top
        ).gap(scale::s(4.0)),
        space::horizontal(),
        tooltip(
            button(crate::icon::lucide(Icon::X).size(scale::s(14.0)).center())
                .padding(scale::s(6.0))
                .style(crate::panel::button_style)
            .on_press(UiEvent::ManageModelsClose),
            container(text("Close").size(scale::s(11.0))).padding(scale::s(6.0)).style(container::rounded_box),
            tooltip::Position::Top
        ).gap(scale::s(4.0)),
    ]
    .spacing(scale::s(6.0))
    .align_y(iced::Alignment::Center);

    let window = container(
        column![header, description, search, body, footer]
            .spacing(scale::s(10.0))
            .height(FillLength),
    )
    .width(Length::Fixed(scale::s(MODAL_WIDTH)))
    .height(Length::Fixed(scale::s(MODAL_HEIGHT)))
    .padding(scale::s(12.0))
    .style(|_theme| container::Style {
        background: Some(PANEL_BG.into()),
        border: iced::Border::default()
            .rounded(scale::s(8.0))
            .color(Color::from_rgb8(60, 63, 74))
            .width(scale::s(1.0)),
        ..container::Style::default()
    });

    stack![
        base,
        opaque(
            mouse_area(
                center(opaque(window)).style(|_theme| container::Style {
                    background: Some(
                        Color {
                            a: 0.55,
                            ..Color::BLACK
                        }
                        .into()
                    ),
                    ..container::Style::default()
                })
            )
            .on_press(UiEvent::ManageModelsClose)
        )
    ]
    .into()
}
