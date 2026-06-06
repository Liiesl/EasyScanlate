//! The settings modal: a centered overlay opened from the toolbar with a
//! vertical tab list on the left and the selected tab's fields on the right.
//! Field edits flow through `UiEvent`s; the app persists them to
//! `settings.json` next to the executable when the modal closes.

use iced::widget::{
    button, center, column, container, mouse_area, opaque, row, space, stack, text, text_input,
};
use iced::{Color, Element, Fill as FillLength, Length};

use crate::event::{SettingsTab, UiEvent};
use crate::panel::PANEL_BG;
use crate::state::UiState;

const MODAL_WIDTH: f32 = 520.0;
const MODAL_HEIGHT: f32 = 340.0;
const TAB_WIDTH: f32 = 140.0;
const ACCENT: Color = Color::from_rgb8(92, 190, 255);
const MUTED_FG: Color = Color::from_rgb(0.6, 0.6, 0.6);

/// One tab button of the vertical tab list; the active tab is highlighted.
fn tab_button<'a, S: UiState + ?Sized>(
    state: &'a S,
    tab: SettingsTab,
    label: &'a str,
) -> Element<'a, UiEvent> {
    let selected = state.settings_tab() == tab;
    button(
        text(label)
            .size(13)
            .color(if selected { Color::WHITE } else { MUTED_FG }),
    )
    .width(Length::Fill)
    .padding(6)
    .on_press(UiEvent::SettingsTab(tab))
    .style(move |_theme, status| button::Style {
        background: Some(if selected {
            Color {
                a: 0.35,
                ..ACCENT
            }
        } else if status == button::Status::Hovered {
            Color {
                a: 0.15,
                ..Color::WHITE
            }
        } else {
            Color::TRANSPARENT
        }.into()),
        border: iced::Border::default().rounded(4),
        text_color: if selected { Color::WHITE } else { MUTED_FG },
        ..button::Style::default()
    })
    .into()
}

/// The field area of the currently selected tab.
fn tab_fields<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    match state.settings_tab() {
        SettingsTab::General => column![
            text("General").size(14),
            text("No general settings yet.").size(12).color(MUTED_FG),
        ]
        .spacing(6)
        .into(),
        SettingsTab::Translation => column![
            text("Translation").size(14),
            text("API key used by the machine translator; empty falls back to the \
                  OPENCODE_API_KEY environment variable.")
                .size(12)
                .color(MUTED_FG),
            row![
                container(text("API key").size(12).color(MUTED_FG))
                    .width(Length::Fixed(84.0)),
                text_input("sk-…", state.translate_api_key())
                    .on_input(UiEvent::TranslateApiKey)
                    .secure(true)
                    .padding(4)
                    .size(12)
                    .width(FillLength),
            ]
            .spacing(6),
            text("Saved to settings.json beside the executable.").size(11).color(MUTED_FG),
        ]
        .spacing(8)
        .into(),
    }
}

/// The settings overlay: `base` (the whole window) dimmed under a centered
/// modal window with the vertical tab list and the selected tab's fields.
/// Clicking the backdrop closes the modal; clicks inside are consumed by the
/// modal's own widgets.
pub fn view<'a, S: UiState + ?Sized>(
    state: &'a S,
    base: Element<'a, UiEvent>,
) -> Element<'a, UiEvent> {
    let window = container(
        column![
            row![
                text("Settings").size(18),
                space::horizontal(),
                button(text("✕"))
                    .padding(2)
                    .on_press(UiEvent::SettingsClose),
            ],
            row![
                container(
                    column![
                        tab_button(state, SettingsTab::General, "General"),
                        tab_button(state, SettingsTab::Translation, "Translation"),
                    ]
                    .spacing(4)
                    .width(Length::Fixed(TAB_WIDTH)),
                )
                .padding(6)
                .style(|_theme| container::Style {
                    background: Some(
                        Color {
                            a: 0.5,
                            ..Color::BLACK
                        }
                        .into()
                    ),
                    border: iced::Border::default().rounded(4),
                    ..container::Style::default()
                }),
                container(tab_fields(state)).width(FillLength),
            ]
            .spacing(10)
            .height(FillLength),
        ]
        .spacing(10),
    )
    .width(Length::Fixed(MODAL_WIDTH))
    .height(Length::Fixed(MODAL_HEIGHT))
    .padding(12)
    .style(|_theme| container::Style {
        background: Some(PANEL_BG.into()),
        border: iced::Border::default().rounded(8).color(Color::from_rgb8(60, 63, 74)).width(1),
        ..container::Style::default()
    });

    stack![
        base,
        opaque(
            mouse_area(
                center(opaque(window)).style(|_theme| container::Style {
                    background: Some(
                        Color {
                            a: 0.7,
                            ..Color::BLACK
                        }
                        .into()
                    ),
                    ..container::Style::default()
                })
            )
            .on_press(UiEvent::SettingsClose)
        )
    ]
    .into()
}