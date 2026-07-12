//! The settings modal: a centered overlay opened from the toolbar with a
//! vertical tab list on the left and the selected tab's fields on the right.
//! Field edits are written straight into the shared `scanlateit_settings`
//! store (write-through) and announced with the single
//! [`UiEvent::SettingsChanged`]; the app re-syncs its runtime mirrors from
//! the store on that one message.

#[allow(unused_imports)]
use iced::widget::{
    button, center, checkbox, column, container, mouse_area, opaque, row, rule, scrollable, space,
    stack, text, toggler,
};
#[cfg(feature = "inpaint")]
use iced::widget::pick_list;
use iced::widget::text_input;
use iced::{Color, Element, Fill as FillLength, Length};

#[cfg(feature = "inpaint")]
use scanlateit_settings::InpaintBackend;

use crate::translation::{self, CUSTOM_ANTHROPIC, CUSTOM_OPENAI};

use crate::background::AuroraWheel;
use crate::event::{SettingEdit, SettingsTab, UiEvent};
use crate::panel::PANEL_BG;
use crate::scale;
use crate::segmented::{segment, segmented_group};
use crate::state::UiState;

const TAB_WIDTH: f32 = 140.0;
const ACCENT: Color = Color::from_rgb8(92, 190, 255);
const MUTED_FG: Color = Color::from_rgb(0.6, 0.6, 0.6);

/// Writes one change into the shared settings store (write-through) and
/// returns the single announcement event for the app.
fn set(f: impl FnOnce(&mut scanlateit_settings::Settings)) -> UiEvent {
    let _ = scanlateit_settings::modify(f);
    UiEvent::SettingsChanged
}

/// One tab button of the vertical tab list; the active tab is highlighted.
fn tab_button<'a, S: UiState + ?Sized>(
    state: &'a S,
    tab: SettingsTab,
    label: &'a str,
) -> Element<'a, UiEvent> {
    let selected = state.settings_tab() == tab;
    button(
        text(label)
            .size(scale::s(13.0))
            .color(if selected { Color::WHITE } else { MUTED_FG }),
    )
    .width(Length::Fill)
    .padding(scale::s(6.0))
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
        border: iced::Border::default().rounded(scale::s(4.0)),
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

/// Thin separator after each provider / model row so its action button is
/// not confused with the next item.
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

fn section_separator<'a>() -> Element<'a, UiEvent> {
    rule::horizontal(1)
        .style(|_theme| rule::Style {
            color: Color::from_rgba8(255, 255, 255, 0.14),
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        })
        .into()
}

/// One row of the supported-provider list: name, connection status and the
/// Connect/Disconnect button. When `connected` is `Some` the row shows the
/// masked key / local base URL, otherwise "Not connected".
fn provider_row_with_connection<'a>(
    provider: &'a translation::Provider,
    connected: Option<scanlateit_settings::Connection>,
) -> Element<'a, UiEvent> {
    let is_local = translation::is_local(&provider.id);
    let status = connected
        .as_ref()
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
        Some(_) => button(text("Disconnect").size(scale::s(11.0)))
            .padding([scale::s(3.0), scale::s(8.0)])
            .on_press(UiEvent::TranslateDisconnect(provider.id.clone())),
        None => button(text("Connect").size(scale::s(11.0)))
            .padding([scale::s(3.0), scale::s(8.0)])
            .on_press(UiEvent::TranslateConnect(provider.id.clone())),
    };
    row![
        column![
            text(&provider.name).size(scale::s(12.0)),
            text(status).size(scale::s(11.0)).color(MUTED_FG),
        ]
        .spacing(scale::s(1.0))
        .width(FillLength),
        button,
    ]
    .spacing(scale::s(6.0))
    .align_y(iced::Alignment::Center)
    .padding([scale::s(4.0), 0.0])
    .into()
}

/// One row of the supported-provider list: name, connection status and the
/// Connect/Disconnect button. The connection state is read straight from
/// the settings store.
#[allow(dead_code)]
fn provider_row<'a>(provider: &'a translation::Provider) -> Element<'a, UiEvent> {
    let connected = scanlateit_settings::get(|s| s.connections.get(&provider.id).cloned());
    provider_row_with_connection(provider, connected)
}

/// One row of the custom-endpoint section with an explicit connection.
fn custom_row_with_connection<'a>(
    id: &'static str,
    label: &'static str,
    connected: Option<scanlateit_settings::Connection>,
) -> Element<'a, UiEvent> {
    let status = connected
        .as_ref()
        .map(|connection| format!("Connected · {}", mask_key(&connection.api_key)))
        .unwrap_or_else(|| "Not connected".to_string());
    let button = match connected {
        Some(_) => button(text("Disconnect").size(scale::s(11.0)))
            .padding([scale::s(3.0), scale::s(8.0)])
            .on_press(UiEvent::TranslateDisconnect(id.to_string())),
        None => button(text("Connect…").size(scale::s(11.0)))
            .padding([scale::s(3.0), scale::s(8.0)])
            .on_press(UiEvent::TranslateConnect(id.to_string())),
    };
    row![
        column![
            text(label).size(scale::s(12.0)),
            text(status).size(scale::s(11.0)).color(MUTED_FG),
        ]
        .spacing(scale::s(1.0))
        .width(FillLength),
        button,
    ]
    .spacing(scale::s(6.0))
    .align_y(iced::Alignment::Center)
    .padding([scale::s(4.0), 0.0])
    .into()
}

/// One row of the custom-endpoint section.
#[allow(dead_code)]
fn custom_row<'a>(id: &'static str, label: &'static str) -> Element<'a, UiEvent> {
    let connected = scanlateit_settings::get(|s| s.connections.get(id).cloned());
    custom_row_with_connection(id, label, connected)
}

/// One row of the recommended section: provider name with a generic
/// "Recommended" badge, the polished description, and Docs + Connect
/// buttons. Always shows "Not connected" because connected providers are
/// deduped (they already appear in the Connected section above).
fn recommended_row<'a>(
    provider: &'a translation::Provider,
    info: &'a translation::RecommendedInfo,
) -> Element<'a, UiEvent> {
    let badge = container(
        text("Recommended")
            .size(scale::s(9.0))
            .color(ACCENT),
    )
    .padding([scale::s(2.0), scale::s(6.0)])
    .style(|_theme| container::Style {
        background: Some(Color::from_rgba8(92, 190, 255, 0.15).into()),
        border: iced::Border::default().rounded(scale::s(4.0)),
        ..container::Style::default()
    });

    let docs_button = button(text("Docs").size(scale::s(11.0)))
        .padding([scale::s(3.0), scale::s(8.0)])
        .on_press(UiEvent::OpenUrl(info.docs_url.to_string()));

    let connect_button = button(text("Connect").size(scale::s(11.0)))
        .padding([scale::s(3.0), scale::s(8.0)])
        .on_press(UiEvent::TranslateConnect(provider.id.clone()));

    row![
        column![
            row![
                text(&provider.name).size(scale::s(12.0)),
                badge,
            ]
            .spacing(scale::s(6.0))
            .align_y(iced::Alignment::Center),
            text(info.description).size(scale::s(11.0)).color(MUTED_FG),
            text("Not connected").size(scale::s(11.0)).color(MUTED_FG),
        ]
        .spacing(scale::s(2.0))
        .width(FillLength),
        docs_button,
        connect_button,
    ]
    .spacing(scale::s(6.0))
    .align_y(iced::Alignment::Center)
    .padding([scale::s(4.0), 0.0])
    .into()
}

/// Appearance tab — port of `ManhwaOCR/app/ui/components/background_settings.AuroraEditorPanel`.
/// Reads and writes the aurora theme directly in the settings store.
fn appearance_tab() -> Element<'static, UiEvent> {
    let cfg = crate::background::AuroraConfig::from_store();
    let is_dark = cfg.is_dark;
    let count = cfg.blob_count;
    let schema = cfg.schema;
    let hex = cfg.to_hex();

    // Mode toggle (Light | Dark) — mirrors ManhwaOCR's two-button toggle.
    let mode_row = segmented_group(vec![
        segment(
            !is_dark,
            "Light",
            Some(UiEvent::SettingEdit(SettingEdit::AuroraDarkMode(false))),
            iced::Font::DEFAULT,
        ),
        segment(
            is_dark,
            "Dark",
            Some(UiEvent::SettingEdit(SettingEdit::AuroraDarkMode(true))),
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
    let dec_btn: Element<'_, UiEvent> = button(text("−").size(scale::s(14.0)).width(FillLength).center())
        .width(Length::Fixed(scale::s(30.0)))
        .height(Length::Fixed(scale::s(30.0)))
        .padding(0)
        .on_press_maybe(
            (count > 1).then(|| UiEvent::SettingEdit(SettingEdit::AuroraBlobCount(count - 1))),
        )
        .style(|_theme: &iced::Theme, status| {
            let bg = if status == iced::widget::button::Status::Hovered {
                Color::from_rgba8(255, 255, 255, 0.30)
            } else {
                Color::from_rgba8(255, 255, 255, 0.15)
            };
            iced::widget::button::Style {
                background: Some(bg.into()),
                border: iced::Border::default().rounded(scale::s(15.0)),
                text_color: Color::WHITE,
                ..Default::default()
            }
        })
        .into();
    let inc_btn: Element<'_, UiEvent> = button(text("+").size(scale::s(14.0)).width(FillLength).center())
        .width(Length::Fixed(scale::s(30.0)))
        .height(Length::Fixed(scale::s(30.0)))
        .padding(0)
        .on_press_maybe(
            (count < 5).then(|| UiEvent::SettingEdit(SettingEdit::AuroraBlobCount(count + 1))),
        )
        .style(|_theme: &iced::Theme, status| {
            let bg = if status == iced::widget::button::Status::Hovered {
                Color::from_rgba8(255, 255, 255, 0.30)
            } else {
                Color::from_rgba8(255, 255, 255, 0.15)
            };
            iced::widget::button::Style {
                background: Some(bg.into()),
                border: iced::Border::default().rounded(scale::s(15.0)),
                text_color: Color::WHITE,
                ..Default::default()
            }
        })
        .into();
    let schema_btn: Element<'_, UiEvent> = button(text("⟳").size(scale::s(16.0)).width(FillLength).center())
        .width(Length::Fixed(scale::s(30.0)))
        .height(Length::Fixed(scale::s(30.0)))
        .padding(0)
        .on_press_maybe(
            (count > 1).then(|| {
                UiEvent::SettingEdit(SettingEdit::AuroraSchema(
                    schema.index().wrapping_add(1) % 4,
                ))
            }),
        )
        .style(|_theme: &iced::Theme, status| {
            let bg = if status == iced::widget::button::Status::Hovered {
                Color::from_rgba8(255, 255, 255, 0.30)
            } else {
                Color::from_rgba8(255, 255, 255, 0.15)
            };
            iced::widget::button::Style {
                background: Some(bg.into()),
                border: iced::Border::default().rounded(scale::s(15.0)),
                text_color: Color::WHITE,
                ..Default::default()
            }
        })
        .into();

    let count_row: Element<'_, UiEvent> = row![dec_btn, container(text(count_label).size(scale::s(12.0)).color(Color::WHITE).width(FillLength).center()).width(Length::Fixed(scale::s(80.0))), inc_btn, space::horizontal(), schema_btn]
        .spacing(scale::s(6.0))
        .align_y(iced::Alignment::Center)
        .into();

    let hex_row: Element<'_, UiEvent> = row![
        container(text("Hex").size(scale::s(12.0)).color(MUTED_FG)).width(Length::Fixed(scale::s(40.0))),
        iced::widget::text_input(&hex, &hex)
            .on_input(|input| {
                // Only valid hex reaches the store; invalid input keeps the
                // previous color (the field snaps back on the next frame).
                if crate::background::AuroraConfig::from_hex(&input).is_some() {
                    set(move |s| s.aurora_color = input.clone())
                } else {
                    UiEvent::SettingsChanged
                }
            })
            .padding(scale::s(4.0))
            .size(scale::s(12.0))
            .width(Length::Fixed(scale::s(100.0))),
        text(hex.clone()).size(scale::s(11.0)).color(MUTED_FG),
    ]
    .spacing(scale::s(6.0))
    .align_y(iced::Alignment::Center)
    .into();

    let inner: Element<'_, UiEvent> = column![
        text("Appearance").size(scale::s(14.0)),
        text("Animated aurora background — primary color, blobs, light/dark and color-theory schema (Vibrant / Analogous / Contrast / Neon). Mirrors ManhwaOCR's Background Editor.")
            .size(scale::s(11.0))
            .color(MUTED_FG),
        mode_row,
        container(text("Primary Color").size(scale::s(13.0)).color(Color::WHITE).width(FillLength).center()).padding(scale::s(4.0)),
        container(wheel).center_x(FillLength),
        hex_row,
        count_row,
        text(if count == 1 { "Solid — single color, no blobs." } else { "Blobs blend with radial gradients at card corners/edges; schema shifts hue." })
            .size(scale::s(11.0))
            .color(MUTED_FG),
    ]
    .spacing(scale::s(10.0))
    .into();

    // Wrap in a translucent card like ManhwaOCR's AuroraEditorPanel (rgba 20,20,20,220 + border)
    container(scrollable(inner).height(Length::Fill))
        .padding(scale::s(8.0))
        .style(|_theme| container::Style {
            background: Some(Color::from_rgba8(20, 20, 20, 0.86).into()),
            border: iced::Border::default().rounded(scale::s(12.0)).color(Color::from_rgba8(255, 255, 255, 0.15)).width(scale::s(1.0)),
            ..Default::default()
        })
        .into()
}

/// The field area of the currently selected tab.
fn tab_fields<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    match state.settings_tab() {
        SettingsTab::General => {
            #[cfg_attr(not(any(feature = "styling", feature = "ocr")), allow(unused_mut))]
            let mut items: Vec<Element<'_, UiEvent>> = vec![text("General").size(scale::s(14.0)).into()];
            // ── UI font size: VS Code-style integer with stepper + free text input ──
            {
                let raw = scanlateit_settings::get(|s| s.ui_font_size);
                let font_str = raw.to_string();
                let clamped = scale::clamp_font_size(raw);
                let dec: Element<'_, UiEvent> = button(text("−").size(scale::s(14.0)).width(FillLength).center())
                    .width(Length::Fixed(scale::s(30.0)))
                    .height(Length::Fixed(scale::s(30.0)))
                    .padding(0)
                    .on_press_maybe((clamped > scale::MIN_FONT_SIZE).then_some(UiEvent::SettingEdit(SettingEdit::UiFontSize(clamped - 1))))
                    .style(|_theme: &iced::Theme, status| {
                        let bg = if status == iced::widget::button::Status::Hovered {
                            Color::from_rgba8(255, 255, 255, 0.30)
                        } else {
                            Color::from_rgba8(255, 255, 255, 0.15)
                        };
                        iced::widget::button::Style {
                            background: Some(bg.into()),
                            border: iced::Border::default().rounded(scale::s(15.0)),
                            text_color: Color::WHITE,
                            ..Default::default()
                        }
                    })
                    .into();
                let inc: Element<'_, UiEvent> = button(text("+").size(scale::s(14.0)).width(FillLength).center())
                    .width(Length::Fixed(scale::s(30.0)))
                    .height(Length::Fixed(scale::s(30.0)))
                    .padding(0)
                    .on_press_maybe((clamped < scale::MAX_FONT_SIZE).then_some(UiEvent::SettingEdit(SettingEdit::UiFontSize(clamped + 1))))
                    .style(|_theme: &iced::Theme, status| {
                        let bg = if status == iced::widget::button::Status::Hovered {
                            Color::from_rgba8(255, 255, 255, 0.30)
                        } else {
                            Color::from_rgba8(255, 255, 255, 0.15)
                        };
                        iced::widget::button::Style {
                            background: Some(bg.into()),
                            border: iced::Border::default().rounded(scale::s(15.0)),
                            text_color: Color::WHITE,
                            ..Default::default()
                        }
                    })
                    .into();
                items.push(
                    row![
                        container(text("UI font size").size(scale::s(12.0)).color(MUTED_FG))
                            .width(Length::Fixed(scale::s(150.0))),
                        dec,
                        text_input("12", &font_str)
                            .on_input(move |input| {
                                if let Ok(v) = input.trim().parse::<u32>() {
                                    set(move |s| s.ui_font_size = v)
                                } else if input.trim().is_empty() {
                                    // keep previous while empty / half-typed; snap back next frame
                                    UiEvent::SettingsChanged
                                } else {
                                    UiEvent::SettingsChanged
                                }
                            })
                            .padding(scale::s(4.0))
                            .size(scale::s(12.0))
                            .width(Length::Fixed(scale::s(64.0))),
                        inc,
                    ]
                    .spacing(scale::s(6.0))
                    .align_y(iced::Alignment::Center)
                    .into(),
                );
                items.push(
                    text(format!(
                        "Base font size for all UI text ({}–{}). Padding, spacing, border radius and item gaps scale with it. Window chrome and image overlays stay fixed.",
                        scale::MIN_FONT_SIZE, scale::MAX_FONT_SIZE
                    ))
                        .size(scale::s(11.0))
                        .color(MUTED_FG)
                        .into(),
                );
            }
            #[cfg(feature = "styling")]
            {
                let auto = scanlateit_settings::get(|s| s.auto_style_detect);
                items.push(
                    text("Classify newly OCR-detected entries with the ONNX styling \
                          model and set their text style from the prediction.")
                        .size(scale::s(12.0))
                        .color(MUTED_FG)
                        .into(),
                );
                items.push(
                    checkbox(auto)
                        .label("Auto-detect entry styles")
                        .text_size(scale::s(12.0))
                        .on_toggle(|v| set(move |s| s.auto_style_detect = v))
                        .into(),
                );
            }
            #[cfg(feature = "segment")]
            {
                let auto = scanlateit_settings::get(|s| s.auto_sfx_filter);
                items.push(
                    text(
                        "Remove SFX outside balloons via segmentation (manga-mimic grid, 1:6 col). \
                         True SFX lives outside any balloon; SFX inside a balloon is a hallucination and ignored."
                    )
                    .size(scale::s(12.0))
                    .color(MUTED_FG)
                    .into(),
                );
                items.push(
                    checkbox(auto)
                        .label("Auto-filter SFX")
                        .text_size(scale::s(12.0))
                        .on_toggle(|v| set(move |s| s.auto_sfx_filter = v))
                        .into(),
                );
            }
            #[cfg(feature = "ocr")]
            {
                let workers = scanlateit_settings::get(|s| s.ocr_workers.clone());
                items.push(
                    row![
                        container(text("OCR detection workers").size(scale::s(12.0)).color(MUTED_FG))
                            .width(Length::Fixed(scale::s(150.0))),
                        text_input("2", &workers)
                            .on_input(|input| set(move |s| s.ocr_workers = input.clone()))
                            .padding(scale::s(4.0))
                            .size(scale::s(12.0))
                            .width(Length::Fixed(scale::s(64.0))),
                    ]
                    .spacing(scale::s(6.0))
                    .into(),
                );
                items.push(
                    text("Parallel OCR detection sessions; 2 fits a potato-laptop CPU.")
                        .size(scale::s(11.0))
                        .color(MUTED_FG)
                        .into(),
                );
            }
            #[cfg(feature = "inpaint")]
            {
                let backend = scanlateit_settings::get(|s| s.inpaint_backend);
                let radius = scanlateit_settings::get(|s| s.inpaint_radius.clone());
                items.push(
                    row![
                        container(text("Inpaint backend").size(scale::s(12.0)).color(MUTED_FG))
                            .width(Length::Fixed(scale::s(150.0))),
                        pick_list(
                            [InpaintBackend::Telea, InpaintBackend::Lama],
                            Some(backend),
                            |backend| set(move |s| s.inpaint_backend = backend),
                        )
                        .padding(scale::s(4.0))
                        .text_size(scale::s(12.0)),
                    ]
                    .spacing(scale::s(6.0))
                    .into(),
                );
                items.push(
                    text("Telea (the `inpaint` crate) needs no model and is instant; \
                          LaMa runs the ONNX model and handles complex backgrounds better.")
                        .size(scale::s(11.0))
                        .color(MUTED_FG)
                        .into(),
                );
                items.push(
                    row![
                        container(text("Telea radius").size(scale::s(12.0)).color(MUTED_FG))
                            .width(Length::Fixed(scale::s(150.0))),
                        text_input("5", &radius)
                            .on_input(|input| set(move |s| s.inpaint_radius = input.clone()))
                            .padding(scale::s(4.0))
                            .size(scale::s(12.0))
                            .width(Length::Fixed(scale::s(64.0))),
                    ]
                    .spacing(scale::s(6.0))
                    .into(),
                );
                items.push(
                    text("How many pixels around the mask Telea samples; larger \
                          smooths more but blurs. Ignored by LaMa.")
                        .size(scale::s(11.0))
                        .color(MUTED_FG)
                        .into(),
                );
            }
            column(items).spacing(scale::s(6.0)).into()
        }
        SettingsTab::Translation => {
            // Single read for both connections and free-only flag to keep the
            // partition consistent for this frame.
            let (connections, free_only) =
                scanlateit_settings::get(|s| (s.connections.clone(), s.free_models_only));

            // Partition every provider (cloud + local, already in SUPPORTED)
            // and the two custom slots into connected vs available, preserving
            // display order but with all connected items on top.  "Same" per
            // user choice: custom + local share the same sections.
            let mut connected_rows: Vec<Element<'_, UiEvent>> = Vec::new();
            let mut available_rows: Vec<Element<'_, UiEvent>> = Vec::new();
            for provider in translation::SUPPORTED_PROVIDERS.iter() {
                let conn = connections.get(&provider.id).cloned();
                let el = provider_row_with_connection(provider, conn.clone());
                if conn.is_some() {
                    connected_rows.push(el);
                } else {
                    available_rows.push(el);
                }
            }
            for (id, label) in [
                (CUSTOM_OPENAI, "OpenAI-compatible"),
                (CUSTOM_ANTHROPIC, "Anthropic-compatible"),
            ] {
                let conn = connections.get(id).cloned();
                let el = custom_row_with_connection(id, label, conn.clone());
                if conn.is_some() {
                    connected_rows.push(el);
                } else {
                    available_rows.push(el);
                }
            }

            let mut rows: Vec<Element<'_, UiEvent>> = Vec::new();
            rows.push(text("Translation Service").size(scale::s(14.0)).into());
            rows.push(
                text("Connect the gateway used by the machine translator. \
                      Disconnect removes its API key.")
                    .size(scale::s(12.0))
                    .color(MUTED_FG)
                    .into(),
            );

            // ── Connected section (on top) ──────────────────────────────
            rows.push(
                row![
                    text("Connected").size(scale::s(12.0)).color(Color::WHITE),
                    space::horizontal(),
                    text(if connected_rows.is_empty() {
                        "—".to_string()
                    } else {
                        format!("{} connected", connected_rows.len())
                    })
                    .size(scale::s(11.0))
                    .color(MUTED_FG),
                ]
                .align_y(iced::Alignment::Center)
                .into(),
            );
            if connected_rows.is_empty() {
                rows.push(
                    text("No connected providers — connect one below.")
                        .size(scale::s(11.0))
                        .color(MUTED_FG)
                        .into(),
                );
            } else {
                let len = connected_rows.len();
                for (idx, el) in connected_rows.into_iter().enumerate() {
                    rows.push(el);
                    if idx + 1 < len {
                        rows.push(item_separator());
                    }
                }
            }

            rows.push(section_separator());

            // ── Recommended section (between Connected and Available) ───
            // Shown when RECOMMENDED is non-empty. Connected providers are
            // deduped (they already appear in Connected above); not-connected
            // ones remain in Available as well per "keep them".
            if !translation::RECOMMENDED.is_empty() {
                let mut recommended_rows: Vec<Element<'_, UiEvent>> = Vec::new();
                for info in translation::RECOMMENDED.iter() {
                    if connections.contains_key(info.id) {
                        continue;
                    }
                    if let Some(provider) = translation::catalog_provider(info.id) {
                        recommended_rows.push(recommended_row(provider, info));
                    }
                }
                rows.push(
                    row![
                        text("Recommended").size(scale::s(12.0)).color(Color::WHITE),
                        space::horizontal(),
                        text(if recommended_rows.is_empty() {
                            "—".to_string()
                        } else {
                            format!("{} recommended", recommended_rows.len())
                        })
                        .size(scale::s(11.0))
                        .color(MUTED_FG),
                    ]
                    .align_y(iced::Alignment::Center)
                    .into(),
                );
                rows.push(
                    text("Not sure where to start? Try one of these recommendations.")
                        .size(scale::s(11.0))
                        .color(MUTED_FG)
                        .into(),
                );
                if recommended_rows.is_empty() {
                    rows.push(
                        text("All recommended providers connected.")
                            .size(scale::s(11.0))
                            .color(MUTED_FG)
                            .into(),
                    );
                } else {
                    let len = recommended_rows.len();
                    for (idx, el) in recommended_rows.into_iter().enumerate() {
                        rows.push(el);
                        if idx + 1 < len {
                            rows.push(item_separator());
                        }
                    }
                }
                rows.push(section_separator());
            }

            // ── Available section ───────────────────────────────────────
            rows.push(
                row![
                    text("Available").size(scale::s(12.0)).color(Color::WHITE),
                    space::horizontal(),
                    text(format!("{} available", available_rows.len()))
                        .size(scale::s(11.0))
                        .color(MUTED_FG),
                ]
                .align_y(iced::Alignment::Center)
                .into(),
            );
            if available_rows.is_empty() {
                rows.push(
                    text("All providers connected.")
                        .size(scale::s(11.0))
                        .color(MUTED_FG)
                        .into(),
                );
            } else {
                let len = available_rows.len();
                for (idx, el) in available_rows.into_iter().enumerate() {
                    rows.push(el);
                    if idx + 1 < len {
                        rows.push(item_separator());
                    }
                }
            }

            rows.push(section_separator());

            // ── Toggler + Manage models ─────────────────────────────────
            rows.push(
                row![
                    column![
                        text("Only show free models").size(scale::s(12.0)),
                        text("Hide paid models from the translation picker.")
                            .size(scale::s(11.0))
                            .color(MUTED_FG),
                    ]
                    .spacing(scale::s(1.0))
                    .width(FillLength),
                    toggler(free_only)
                        .size(scale::s(20.0))
                        .style(crate::toggler_style::style)
                        .on_toggle(|v| set(move |s| s.free_models_only = v)),
                ]
                .spacing(scale::s(12.0))
                .align_y(iced::Alignment::Center)
                .padding([scale::s(4.0), 0.0])
                .into(),
            );
            rows.push(item_separator());
            rows.push(
                row![
                    column![
                        text("Filter unused models from the translation dropdown.")
                            .size(scale::s(11.0))
                            .color(MUTED_FG),
                        text("Hide models you never use; deprecated are always hidden.")
                            .size(scale::s(11.0))
                            .color(MUTED_FG),
                    ]
                    .spacing(scale::s(1.0))
                    .width(FillLength),
                    button(text("Manage models…").size(scale::s(11.0)))
                        .padding([scale::s(3.0), scale::s(8.0)])
                        .on_press(UiEvent::ManageModelsOpen),
                ]
                .spacing(scale::s(6.0))
                .align_y(iced::Alignment::Center)
                .padding([scale::s(4.0), 0.0])
                .into(),
            );
            rows.push(item_separator());
            rows.push(
                text("Connections are saved to the app's settings file in the \
                      system configuration directory.")
                    .size(scale::s(11.0))
                    .color(MUTED_FG)
                    .into(),
            );
            scrollable(column(rows).spacing(scale::s(6.0)))
                .height(Length::Fill)
                .into()
        }
        SettingsTab::Appearance => appearance_tab(),
    }
}

/// The settings overlay: `base` (the whole window) dimmed under a centered
/// modal window with the vertical tab list and the selected tab's fields.
/// The modal occupies 80% of the window in both axes (1-8-1 FillPortion split).
/// No header — closing is only by clicking the dimmed backdrop outside.
/// The darker background covers the whole left section (full height), not just
/// the button cluster.
pub fn view<'a, S: UiState + ?Sized>(
    state: &'a S,
    base: Element<'a, UiEvent>,
) -> Element<'a, UiEvent> {
    let left = container(
        column![
            tab_button(state, SettingsTab::General, "General"),
            tab_button(state, SettingsTab::Appearance, "Appearance"),
            tab_button(state, SettingsTab::Translation, "Translation"),
        ]
        .spacing(scale::s(4.0))
        .width(Length::Fixed(scale::s(TAB_WIDTH))),
    )
    .width(Length::Fixed(scale::s(TAB_WIDTH)))
    .height(Length::Fill)
    .padding(scale::s(12.0))
    .style(|_theme| container::Style {
        background: Some(
            Color {
                a: 0.5,
                ..Color::BLACK
            }
            .into()
        ),
        border: iced::Border::default().rounded(iced::border::left(scale::s(8.0))),
        ..container::Style::default()
    });

    let right = container(tab_fields(state))
        .width(FillLength)
        .height(Length::Fill)
        .padding(scale::s(12.0));

    let window = container(row![left, right].height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(PANEL_BG.into()),
            border: iced::Border::default()
                .rounded(scale::s(8.0))
                .color(Color::from_rgb8(60, 63, 74))
                .width(scale::s(1.0)),
            ..container::Style::default()
        });

    // 80% centered modal: 1-8-1 split both axes => 8/10 = 80%
    let dimmed = container(
        row![
            space::horizontal().width(Length::FillPortion(1)),
            column![
                space::vertical().height(Length::FillPortion(1)),
                container(opaque(window))
                    .width(Length::Fill)
                    .height(Length::FillPortion(8)),
                space::vertical().height(Length::FillPortion(1)),
            ]
            .width(Length::FillPortion(8))
            .height(Length::Fill),
            space::horizontal().width(Length::FillPortion(1)),
        ]
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|_theme| container::Style {
        background: Some(
            Color {
                a: 0.7,
                ..Color::BLACK
            }
            .into()
        ),
        ..container::Style::default()
    });

    stack![
        base,
        opaque(mouse_area(dimmed).on_press(UiEvent::SettingsClose))
    ]
    .into()
}