//! Manage Models overlay: opened from Translation settings, lets the user
//! toggle each model per provider. Deprecated models are never listed here
//! (they are filtered at the provider layer). The basic configuration (no
//! hidden entry) shows the default latest-per-family filtered list; hiding
//! is written straight into the shared settings store.

use iced::widget::{
    button, center, column, container, mouse_area, opaque, row, rule, scrollable, space, stack,
    text, text_input, toggler,
};
use iced::{Color, Element, Fill as FillLength, Length};

use crate::event::{SettingEdit, UiEvent};
use crate::panel::PANEL_BG;
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
fn set(f: impl FnOnce(&mut scanlateit_settings::Settings)) -> UiEvent {
    let _ = scanlateit_settings::modify(f);
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
        text("Manage Models").size(16),
        space::horizontal(),
        button(text("✕")).padding(2).on_press(UiEvent::ManageModelsClose),
    ];

    let description = column![
        text("Toggle models per provider. Hidden models disappear from the translation dropdown.")
            .size(11)
            .color(MUTED_FG),
        text("Deprecated models are always hidden and never shown here.")
            .size(11)
            .color(MUTED_FG),
    ]
    .spacing(2);

    let search: Element<'_, UiEvent> = if groups.is_empty() {
        // No providers – no need for search field.
        space::vertical().height(Length::Fixed(0.0)).into()
    } else {
        text_input("Search models…", &query)
            .on_input(UiEvent::ManageModelsSearch)
            .padding([6, 8])
            .size(12)
            .width(FillLength)
            .into()
    };

    let body: Element<'_, UiEvent> = if groups.is_empty() {
        container(
            text("No connected providers – connect a translation service first.")
                .size(12)
                .color(MUTED_FG),
        )
        .padding(12)
        .into()
    } else {
        let mut provider_cols: Vec<Element<'_, UiEvent>> = Vec::new();
        for (provider_id, provider_name, models) in groups {
            // Filter models by search query (case-insensitive, space aliases -, /, ., _).
            // Example: "meta spark" matches "meta/muse-spark-1.2".
            let filtered: Vec<String> = models
                .iter()
                .filter(|m| model_matches(&query, m))
                .cloned()
                .collect();
            if filtered.is_empty() {
                continue;
            }

            // Snapshot hidden set for this provider once per frame.
            let hidden_set = scanlateit_settings::get(|s| {
                s.hidden_models
                    .get(&provider_id)
                    .cloned()
                    .unwrap_or_default()
            });
            let visible_cnt = filtered
                .iter()
                .filter(|m| !hidden_set.contains(*m))
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
                let filtered_for_toggle = filtered.clone();
                let master_toggler: Element<'_, UiEvent> = toggler(master_on)
                    .size(18.0)
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
                                // Disable: hide all filtered models
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
                        text(provider_name.clone()).size(13),
                        text(count_label).size(11).color(MUTED_FG),
                        space::horizontal(),
                        master_toggler,
                    ]
                    .align_y(iced::Alignment::Center)
                    .spacing(8)
                    .padding([2, 0])
                    .into(),
                );
            }
            if !filtered.is_empty() {
                rows.push(item_separator());
            }
            for (idx, model) in filtered.iter().enumerate() {
                let hidden = scanlateit_settings::get(|s| {
                    s.hidden_models
                        .get(&provider_id)
                        .is_some_and(|set| set.contains(model))
                });
                let visible = !hidden;
                let pid = provider_id.clone();
                let mid = model.clone();
                let model_row: Element<'_, UiEvent> = row![
                    text(model.clone()).size(12).width(FillLength),
                    toggler(visible)
                        .size(18.0)
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
                .spacing(12)
                .align_y(iced::Alignment::Center)
                .padding([4, 0])
                .into();
                rows.push(model_row);
                if idx + 1 < filtered.len() {
                    rows.push(item_separator());
                }
            }
            // Provider card — keep card, now with separators between each model row
            let card = container(column(rows).spacing(4))
                .padding(8)
                .style(|_theme| container::Style {
                    background: Some(Color::from_rgba8(255, 255, 255, 0.06).into()),
                    border: iced::Border::default().rounded(6),
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
                    text(format!("No models match “{query}”.")).size(12).color(MUTED_FG),
                    text("Try a different search term.").size(11).color(MUTED_FG),
                ]
                .spacing(4),
            )
            .padding(12)
            .into()
        } else {
            scrollable(column(provider_cols).spacing(10))
                .height(Length::Fill)
                .into()
        }
    };

    let footer = row![
        button(text("Reset all").size(11))
            .padding([4, 10])
            .on_press(UiEvent::SettingEdit(SettingEdit::HiddenModelsResetAll)),
        space::horizontal(),
        button(text("Close").size(11))
            .padding([4, 10])
            .on_press(UiEvent::ManageModelsClose),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);

    let window = container(
        column![header, description, search, body, footer]
            .spacing(10)
            .height(FillLength),
    )
    .width(Length::Fixed(MODAL_WIDTH))
    .height(Length::Fixed(MODAL_HEIGHT))
    .padding(12)
    .style(|_theme| container::Style {
        background: Some(PANEL_BG.into()),
        border: iced::Border::default()
            .rounded(8)
            .color(Color::from_rgb8(60, 63, 74))
            .width(1),
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
