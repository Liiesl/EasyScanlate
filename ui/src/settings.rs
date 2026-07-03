//! The settings modal: a centered overlay opened from the toolbar with a
//! vertical tab list on the left and the selected tab's fields on the right.
//! Field edits flow through `UiEvent`s; the app persists them to
//! `settings.json` next to the executable when the modal closes.

use iced::widget::{
    button, center, checkbox, column, container, mouse_area, opaque, row, scrollable, space, stack,
    text,
};
#[cfg(feature = "inpaint")]
use iced::widget::pick_list;
#[cfg(any(feature = "ocr", feature = "inpaint"))]
use iced::widget::text_input;
use iced::{Color, Element, Fill as FillLength, Length};

#[cfg(feature = "inpaint")]
use scanlateit_model::InpaintBackend;

use crate::translation::{self, CUSTOM_ANTHROPIC, CUSTOM_OPENAI};

use crate::background::AuroraWheel;
use crate::event::{SettingsTab, UiEvent};
use crate::panel::PANEL_BG;
use crate::segmented::{segment, segmented_group};
use crate::state::UiState;

const MODAL_WIDTH: f32 = 520.0;
const MODAL_HEIGHT: f32 = 400.0;
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

/// The last four characters of a key, for the "connected" status display.
fn mask_key(key: &str) -> String {
    if key.len() > 8 {
        format!("{}…{}", &key[..6], &key[key.len() - 4..])
    } else {
        "••••".to_string()
    }
}

/// One row of the supported-provider list: name, connection status and the
/// Connect/Disconnect button.
fn provider_row<'a, S: UiState + ?Sized>(
    state: &'a S,
    provider: &'a translation::Provider,
) -> Element<'a, UiEvent> {
    let connected = state.connections().get(&provider.id);
    let is_local = translation::is_local(&provider.id);
    let status = connected
        .map(|connection| {
            if is_local {
                let base = connection
                    .base_url
                    .as_deref()
                    .unwrap_or(provider.api.as_str());
                if base.is_empty() {
                    "Connected · Local".to_string()
                } else {
                    format!("Connected · Local · {base}")
                }
            } else {
                format!("Connected · {}", mask_key(&connection.api_key))
            }
        })
        .unwrap_or_else(|| "Not connected".to_string());
    let button = match connected {
        Some(_) => button(text("Disconnect").size(11))
            .padding([3, 8])
            .on_press(UiEvent::TranslateDisconnect(provider.id.clone())),
        None => button(text("Connect").size(11))
            .padding([3, 8])
            .on_press(UiEvent::TranslateConnect(provider.id.clone())),
    };
    row![
        column![
            text(&provider.name).size(12),
            text(status).size(11).color(MUTED_FG),
        ]
        .spacing(1)
        .width(FillLength),
        button,
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center)
    .into()
}

/// One row of the custom-endpoint section.
fn custom_row<'a, S: UiState + ?Sized>(
    state: &'a S,
    id: &'static str,
    label: &'static str,
) -> Element<'a, UiEvent> {
    let connected = state.connections().get(id);
    let status = connected
        .map(|connection| format!("Connected · {}", mask_key(&connection.api_key)))
        .unwrap_or_else(|| "Not connected".to_string());
    let button = match connected {
        Some(_) => button(text("Disconnect").size(11))
            .padding([3, 8])
            .on_press(UiEvent::TranslateDisconnect(id.to_string())),
        None => button(text("Connect…").size(11))
            .padding([3, 8])
            .on_press(UiEvent::TranslateConnect(id.to_string())),
    };
    row![
        column![
            text(label).size(12),
            text(status).size(11).color(MUTED_FG),
        ]
        .spacing(1)
        .width(FillLength),
        button,
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center)
    .into()
}

/// Appearance tab — port of `ManhwaOCR/app/ui/components/background_settings.AuroraEditorPanel`.
fn appearance_tab<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    let cfg = state.aurora_config();
    let is_dark = cfg.is_dark;
    let count = cfg.blob_count;
    let schema = cfg.schema;
    let hex = cfg.to_hex();

    // Mode toggle (Light | Dark) — mirrors ManhwaOCR's two-button toggle.
    let mode_row = segmented_group(vec![
        segment(
            !is_dark,
            "Light",
            Some(UiEvent::AuroraDarkMode(false)),
            iced::Font::DEFAULT,
        ),
        segment(
            is_dark,
            "Dark",
            Some(UiEvent::AuroraDarkMode(true)),
            iced::Font::DEFAULT,
        ),
    ]);

    // Wheel
    let wheel = AuroraWheel::new(cfg.clone()).view();

    // Count + schema row — mirrors background_settings.py: minus / "Solid" or "n | Schema" / plus / ⟳
    let count_label = if count == 1 {
        "Solid".to_string()
    } else {
        format!("{} | {}", count, schema.label())
    };
    let dec_btn: Element<'_, UiEvent> = button(text("−").size(14).width(FillLength).center())
        .width(Length::Fixed(30.0))
        .height(Length::Fixed(30.0))
        .padding(0)
        .on_press_maybe((count > 1).then_some(UiEvent::AuroraCount(count - 1)))
        .style(|_theme: &iced::Theme, status| {
            let bg = if status == iced::widget::button::Status::Hovered {
                Color::from_rgba8(255, 255, 255, 0.30)
            } else {
                Color::from_rgba8(255, 255, 255, 0.15)
            };
            iced::widget::button::Style {
                background: Some(bg.into()),
                border: iced::Border::default().rounded(15.0),
                text_color: Color::WHITE,
                ..Default::default()
            }
        })
        .into();
    let inc_btn: Element<'_, UiEvent> = button(text("+").size(14).width(FillLength).center())
        .width(Length::Fixed(30.0))
        .height(Length::Fixed(30.0))
        .padding(0)
        .on_press_maybe((count < 5).then_some(UiEvent::AuroraCount(count + 1)))
        .style(|_theme: &iced::Theme, status| {
            let bg = if status == iced::widget::button::Status::Hovered {
                Color::from_rgba8(255, 255, 255, 0.30)
            } else {
                Color::from_rgba8(255, 255, 255, 0.15)
            };
            iced::widget::button::Style {
                background: Some(bg.into()),
                border: iced::Border::default().rounded(15.0),
                text_color: Color::WHITE,
                ..Default::default()
            }
        })
        .into();
    let schema_btn: Element<'_, UiEvent> = button(text("⟳").size(16).width(FillLength).center())
        .width(Length::Fixed(30.0))
        .height(Length::Fixed(30.0))
        .padding(0)
        .on_press_maybe((count > 1).then_some(UiEvent::AuroraSchema(schema.index().wrapping_add(1) % 4)))
        .style(|_theme: &iced::Theme, status| {
            let bg = if status == iced::widget::button::Status::Hovered {
                Color::from_rgba8(255, 255, 255, 0.30)
            } else {
                Color::from_rgba8(255, 255, 255, 0.15)
            };
            iced::widget::button::Style {
                background: Some(bg.into()),
                border: iced::Border::default().rounded(15.0),
                text_color: Color::WHITE,
                ..Default::default()
            }
        })
        .into();

    let count_row: Element<'_, UiEvent> = row![dec_btn, container(text(count_label).size(12).color(Color::WHITE).width(FillLength).center()).width(Length::Fixed(80.0)), inc_btn, space::horizontal(), schema_btn]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into();

    let hex_row: Element<'_, UiEvent> = row![
        container(text("Hex").size(12).color(MUTED_FG)).width(Length::Fixed(40.0)),
        iced::widget::text_input(&hex, &hex)
            .on_input(UiEvent::AuroraHex)
            .padding(4)
            .size(12)
            .width(Length::Fixed(100.0)),
        text(hex.clone()).size(11).color(MUTED_FG),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center)
    .into();

    let inner: Element<'_, UiEvent> = column![
        text("Appearance").size(14),
        text("Animated aurora background — primary color, blobs, light/dark and color-theory schema (Vibrant / Analogous / Contrast / Neon). Mirrors ManhwaOCR's Background Editor.")
            .size(11)
            .color(MUTED_FG),
        mode_row,
        container(text("Primary Color").size(13).color(Color::WHITE).width(FillLength).center()).padding(4),
        container(wheel).center_x(FillLength),
        hex_row,
        count_row,
        text(if count == 1 { "Solid — single color, no blobs." } else { "Blobs blend with radial gradients at card corners/edges; schema shifts hue." })
            .size(11)
            .color(MUTED_FG),
    ]
    .spacing(10)
    .into();

    // Wrap in a translucent card like ManhwaOCR's AuroraEditorPanel (rgba 20,20,20,220 + border)
    container(scrollable(inner).height(Length::Fill))
        .padding(8)
        .style(|_theme| container::Style {
            background: Some(Color::from_rgba8(20, 20, 20, 0.86).into()),
            border: iced::Border::default().rounded(12).color(Color::from_rgba8(255, 255, 255, 0.15)).width(1),
            ..Default::default()
        })
        .into()
}

/// The field area of the currently selected tab.
fn tab_fields<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    match state.settings_tab() {
        SettingsTab::General => {
            #[cfg_attr(not(any(feature = "styling", feature = "ocr")), allow(unused_mut))]
            let mut items: Vec<Element<'_, UiEvent>> = vec![text("General").size(14).into()];
            #[cfg(feature = "styling")]
            {
                items.push(
                    text("Classify newly OCR-detected entries with the ONNX styling \
                          model and set their text style from the prediction.")
                        .size(12)
                        .color(MUTED_FG)
                        .into(),
                );
                items.push(
                    checkbox(state.auto_style_detect())
                        .label("Auto-detect entry styles")
                        .text_size(12)
                        .on_toggle(UiEvent::StyleAutoDetectToggle)
                        .into(),
                );
            }
            #[cfg(feature = "ocr")]
            {
                items.push(
                    row![
                        container(text("OCR detection workers").size(12).color(MUTED_FG))
                            .width(Length::Fixed(150.0)),
                        text_input("2", state.ocr_workers())
                            .on_input(UiEvent::OcrWorkers)
                            .padding(4)
                            .size(12)
                            .width(Length::Fixed(64.0)),
                    ]
                    .spacing(6)
                    .into(),
                );
                items.push(
                    text("Parallel OCR detection sessions; 2 fits a potato-laptop CPU.")
                        .size(11)
                        .color(MUTED_FG)
                        .into(),
                );
            }
            #[cfg(feature = "inpaint")]
            {
                items.push(
                    row![
                        container(text("Inpaint backend").size(12).color(MUTED_FG))
                            .width(Length::Fixed(150.0)),
                        pick_list(
                            [InpaintBackend::Telea, InpaintBackend::Lama],
                            Some(state.inpaint_backend()),
                            UiEvent::InpaintBackend,
                        )
                        .padding(4)
                        .text_size(12),
                    ]
                    .spacing(6)
                    .into(),
                );
                items.push(
                    text("Telea (the `inpaint` crate) needs no model and is instant; \
                          LaMa runs the ONNX model and handles complex backgrounds better.")
                        .size(11)
                        .color(MUTED_FG)
                        .into(),
                );
                items.push(
                    row![
                        container(text("Telea radius").size(12).color(MUTED_FG))
                            .width(Length::Fixed(150.0)),
                        text_input("5", state.inpaint_radius())
                            .on_input(UiEvent::InpaintRadius)
                            .padding(4)
                            .size(12)
                            .width(Length::Fixed(64.0)),
                    ]
                    .spacing(6)
                    .into(),
                );
                items.push(
                    text("How many pixels around the mask Telea samples; larger \
                          smooths more but blurs. Ignored by LaMa.")
                        .size(11)
                        .color(MUTED_FG)
                        .into(),
                );
            }
            column(items).spacing(6).into()
        }
        SettingsTab::Translation => {
            let mut rows: Vec<Element<'_, UiEvent>> = Vec::new();
            rows.push(text("Translation Service").size(14).into());
            rows.push(
                text("Connect the gateway used by the machine translator. \
                      Disconnect removes its API key.")
                    .size(12)
                    .color(MUTED_FG)
                    .into(),
            );
            for provider in translation::SUPPORTED_PROVIDERS.iter() {
                rows.push(provider_row(state, provider));
            }
            rows.push(text("Custom service").size(14).into());
            rows.push(
                text("Any other endpoint speaking the OpenAI or Anthropic \
                      API, e.g. a local Ollama server.")
                    .size(12)
                    .color(MUTED_FG)
                    .into(),
            );
            rows.push(custom_row(state, CUSTOM_OPENAI, "OpenAI-compatible"));
            rows.push(custom_row(state, CUSTOM_ANTHROPIC, "Anthropic-compatible"));
            rows.push(
                checkbox(state.free_models_only())
                    .label("Only show free models")
                    .text_size(12)
                    .on_toggle(UiEvent::FreeModelsOnlyToggle)
                    .into(),
            );
            rows.push(
                text("Hide paid models from the translation picker.")
                    .size(11)
                    .color(MUTED_FG)
                    .into(),
            );
            rows.push(
                row![
                    text("Filter unused models from the translation dropdown.")
                        .size(11)
                        .color(MUTED_FG)
                        .width(FillLength),
                    button(text("Manage models…").size(11))
                        .padding([3, 8])
                        .on_press(UiEvent::ManageModelsOpen),
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center)
                .into(),
            );
            rows.push(
                text("Connections are saved to settings.json beside the executable.")
                    .size(11)
                    .color(MUTED_FG)
                    .into(),
            );
            scrollable(column(rows).spacing(4)).into()
        }
        SettingsTab::Appearance => appearance_tab(state),
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
                        tab_button(state, SettingsTab::Appearance, "Appearance"),
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