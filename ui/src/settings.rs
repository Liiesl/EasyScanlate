//! The settings modal: a centered overlay opened from the toolbar with a
//! vertical tab list on the left and the selected tab's fields on the right.
//! Field edits are written straight into the shared `easyscanlate_settings`
//! store (write-through) and announced with the single
//! [`UiEvent::SettingsChanged`]; the app re-syncs its runtime mirrors from
//! the store on that one message.

#[allow(unused_imports)]
use iced::widget::{
    button, center, checkbox, column, container, image, mouse_area, opaque, progress_bar, row,
    rule, scrollable, space, stack, text, toggler,
};
#[cfg(feature = "inpaint")]
use iced::widget::pick_list;
use iced::widget::text_input;
use iced::{Color, Element, Fill as FillLength, Length};
use lucide_icons::Icon;

#[cfg(feature = "inpaint")]
use easyscanlate_settings::{AutoInpaintModel, InpaintBackend};

use crate::translation::{self, CUSTOM_ANTHROPIC, CUSTOM_OPENAI};

use crate::background::AuroraWheel;
use crate::event::{SettingEdit, SettingsTab, UiEvent};
use crate::panel::PANEL_BG;
use crate::scale;
use crate::segmented::{segment, segmented_group};
use crate::state::UiState;

const TAB_WIDTH: f32 = 170.0;
const MUTED_FG: Color = Color::from_rgb(0.6, 0.6, 0.6);
const CARD_BG: Color = Color::from_rgba8(255, 255, 255, 0.06);
const CARD_BORDER: Color = Color::from_rgba8(255, 255, 255, 0.08);

const DOCS_URL: &str = "https://docs.easyscanlate.site/workflow/your-first-project/";
const BUG_URL: &str = "https://github.com/Liiesl/EasyScanlate/issues/new?template=bug_report.yml";
const FEATURE_URL: &str = "https://github.com/Liiesl/EasyScanlate/issues/new?template=feature_request.yml";

/// Writes one change into the shared settings store (write-through) and
/// returns the single announcement event for the app.
fn set(f: impl FnOnce(&mut easyscanlate_settings::Settings)) -> UiEvent {
    let _ = easyscanlate_settings::modify(f);
    UiEvent::SettingsChanged
}

// ---------------------------------------------------------------------------
// search helpers — sidebar filters current tab's fields
// ---------------------------------------------------------------------------

fn normalize_for_search(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '-' | '/' | '.' | '_' | ',') {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out.to_lowercase()
}

fn matches_query(query: &str, haystack: &str) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return true;
    }
    let q_norm = normalize_for_search(q);
    let h_norm = normalize_for_search(haystack);
    q_norm.split_whitespace().all(|tok| h_norm.contains(tok))
}

fn matches_any(query: &str, keywords: &[&str]) -> bool {
    if query.trim().is_empty() {
        return true;
    }
    let joined = keywords.join(" ");
    matches_query(query, &joined)
}

// ---------------------------------------------------------------------------
// shared card / separator styles
// ---------------------------------------------------------------------------

fn card_style() -> container::Style {
    container::Style {
        background: Some(CARD_BG.into()),
        border: iced::Border::default()
            .rounded(scale::s(8.0))
            .color(CARD_BORDER)
            .width(scale::s(1.0)),
        ..Default::default()
    }
}

fn card_header<'a>(icon: Icon, title: &'a str, subtitle: Option<&'a str>) -> Element<'a, UiEvent> {
    let subtitle_el: Element<'a, UiEvent> = if let Some(sub) = subtitle {
        text(sub).size(scale::s(11.0)).color(MUTED_FG).into()
    } else {
        space::vertical().height(Length::Fixed(0.0)).into()
    };
    row![
        crate::icon::lucide(icon)
            .size(scale::s(14.0))
            .color(crate::accent::accent()),
        column![
            text(title).size(scale::s(13.0)).color(Color::WHITE),
            subtitle_el,
        ]
        .spacing(scale::s(1.0))
        .width(FillLength),
    ]
    .spacing(scale::s(8.0))
    .align_y(iced::Alignment::Center)
    .into()
}

/// One tab button of the vertical tab list; the active tab is highlighted
/// with a left accent bar + subtle fill and an icon.
fn tab_button<'a, S: UiState + ?Sized>(
    state: &'a S,
    tab: SettingsTab,
    icon: Icon,
    label: &'a str,
) -> Element<'a, UiEvent> {
    let selected = state.settings_tab() == tab;
    let icon_el = crate::icon::lucide(icon)
        .size(scale::s(14.0))
        .color(if selected { crate::accent::accent() } else { MUTED_FG });
    let label_el = text(label)
        .size(scale::s(13.0))
        .color(if selected { Color::WHITE } else { MUTED_FG });
    // left accent bar 3px
    let accent = container(space::horizontal().width(Length::Fixed(scale::s(3.0))).height(Length::Fixed(scale::s(18.0))))
        .style(move |_| container::Style {
            background: Some(if selected { crate::accent::accent().into() } else { Color::TRANSPARENT.into() }),
            border: iced::Border::default().rounded(scale::s(2.0)),
            ..Default::default()
        });
    let btn_content = row![accent, icon_el, label_el]
        .spacing(scale::s(7.0))
        .align_y(iced::Alignment::Center);
    button(btn_content)
        .width(Length::Fill)
        .padding([scale::s(7.0), scale::s(8.0)])
        .style(move |_theme, status| {
            let bg = if selected {
                Color::from_rgba8(255, 255, 255, 0.09)
            } else if status == iced::widget::button::Status::Hovered {
                Color::from_rgba8(255, 255, 255, 0.06)
            } else {
                Color::TRANSPARENT
            };
            iced::widget::button::Style {
                background: Some(bg.into()),
                border: iced::Border::default().rounded(scale::s(6.0)),
                text_color: if selected { Color::WHITE } else { MUTED_FG },
                ..Default::default()
            }
        })
        .on_press(UiEvent::SettingsTab(tab))
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

/// One row of the supported-provider list: name, connection status and the
/// Connect/Disconnect button. When `connected` is `Some` the row shows the
/// masked key / local base URL, otherwise "Not connected".
fn provider_row_with_connection<'a>(
    provider: &'a translation::Provider,
    connected: Option<easyscanlate_settings::Connection>,
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
            .style(crate::panel::button_style)
            .on_press(UiEvent::TranslateDisconnect(provider.id.clone())),
        None => button(text("Connect").size(scale::s(11.0)))
            .padding([scale::s(3.0), scale::s(8.0)])
            .style(crate::panel::button_style)
            .on_press(UiEvent::TranslateConnect(provider.id.clone())),
    };
    let dot_color = if status.starts_with("Connected") { crate::accent::accent() } else { MUTED_FG };
    row![
        container(space::horizontal().width(Length::Fixed(scale::s(6.0))).height(Length::Fixed(scale::s(6.0))))
            .style(move |_| container::Style {
                background: Some(dot_color.into()),
                border: iced::Border::default().rounded(scale::s(3.0)),
                ..Default::default()
            }),
        column![
            text(&provider.name).size(scale::s(12.0)),
            text(status).size(scale::s(11.0)).color(MUTED_FG),
        ]
        .spacing(scale::s(1.0))
        .width(FillLength),
        button,
    ]
    .spacing(scale::s(8.0))
    .align_y(iced::Alignment::Center)
    .padding([scale::s(5.0), 0.0])
    .into()
}

/// One row of the custom-endpoint section with an explicit connection.
fn custom_row_with_connection<'a>(
    id: &'static str,
    label: &'static str,
    connected: Option<easyscanlate_settings::Connection>,
) -> Element<'a, UiEvent> {
    let status = connected
        .as_ref()
        .map(|connection| format!("Connected · {}", mask_key(&connection.api_key)))
        .unwrap_or_else(|| "Not connected".to_string());
    let dot_color = if status.starts_with("Connected") { crate::accent::accent() } else { MUTED_FG };
    let button = match connected {
        Some(_) => button(text("Disconnect").size(scale::s(11.0)))
            .padding([scale::s(3.0), scale::s(8.0)])
            .style(crate::panel::button_style)
            .on_press(UiEvent::TranslateDisconnect(id.to_string())),
        None => button(text("Connect…").size(scale::s(11.0)))
            .padding([scale::s(3.0), scale::s(8.0)])
            .style(crate::panel::button_style)
            .on_press(UiEvent::TranslateConnect(id.to_string())),
    };
    row![
        container(space::horizontal().width(Length::Fixed(scale::s(6.0))).height(Length::Fixed(scale::s(6.0))))
            .style(move |_| container::Style {
                background: Some(dot_color.into()),
                border: iced::Border::default().rounded(scale::s(3.0)),
                ..Default::default()
            }),
        column![
            text(label).size(scale::s(12.0)),
            text(status).size(scale::s(11.0)).color(MUTED_FG),
        ]
        .spacing(scale::s(1.0))
        .width(FillLength),
        button,
    ]
    .spacing(scale::s(8.0))
    .align_y(iced::Alignment::Center)
    .padding([scale::s(5.0), 0.0])
    .into()
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
            .color(crate::accent::accent()),
    )
    .padding([scale::s(2.0), scale::s(6.0)])
    .style(|_theme| container::Style {
        background: Some(crate::accent::accent_translucent(0.15).into()),
        border: iced::Border::default().rounded(scale::s(4.0)),
        ..container::Style::default()
    });

    let docs_button = button(text("Docs").size(scale::s(11.0)))
        .padding([scale::s(3.0), scale::s(8.0)])
        .style(crate::panel::button_style)
            .on_press(UiEvent::OpenUrl(info.docs_url.to_string()));

    let connect_button = button(text("Connect").size(scale::s(11.0)))
        .padding([scale::s(3.0), scale::s(8.0)])
        .style(crate::panel::button_style)
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
    .padding([scale::s(5.0), 0.0])
    .into()
}

// ---------------------------------------------------------------------------
// small field helpers
// ---------------------------------------------------------------------------

fn stepper_button<'a>(enabled: bool, label: &'a str, msg: Option<UiEvent>) -> Element<'a, UiEvent> {
    crate::button::with_disabled_cursor(
        button(text(label).size(scale::s(14.0)).width(FillLength).center())
            .width(Length::Fixed(scale::s(30.0)))
            .height(Length::Fixed(scale::s(30.0)))
            .padding(0)
            .on_press_maybe(if enabled { msg } else { None })
            .style(move |_theme: &iced::Theme, status| {
                let bg = if !enabled {
                    Color::from_rgba8(255, 255, 255, 0.06)
                } else if status == iced::widget::button::Status::Hovered {
                    Color::from_rgba8(255, 255, 255, 0.30)
                } else {
                    Color::from_rgba8(255, 255, 255, 0.15)
                };
                iced::widget::button::Style {
                    background: Some(bg.into()),
                    border: iced::Border::default().rounded(scale::s(15.0)),
                    text_color: if enabled { Color::WHITE } else { MUTED_FG },
                    ..Default::default()
                }
            })
            .into(),
    )
}

// Used by the ocr/inpaint cards; dead in translation-only builds.
#[allow(dead_code)]
fn field_row<'a>(label: &'a str, control: Element<'a, UiEvent>) -> Element<'a, UiEvent> {
    row![
        container(text(label).size(scale::s(12.0)).color(Color::WHITE))
            .width(Length::Fixed(scale::s(160.0))),
        control,
    ]
    .spacing(scale::s(8.0))
    .align_y(iced::Alignment::Center)
    .into()
}

fn helper_text<'a>(s: &'a str) -> Element<'a, UiEvent> {
    text(s).size(scale::s(11.0)).color(MUTED_FG).into()
}

// Used by the bg-aware inpaint cards; dead in translation-only builds.
#[allow(dead_code)]
fn warning_text<'a>(s: String) -> Element<'a, UiEvent> {
    text(s).size(scale::s(11.0)).color(Color::from_rgb8(240, 200, 80)).into()
}

// ---------------------------------------------------------------------------
// Appearance tab — port of ManhwaOCR's AuroraEditorPanel, now card-grouped
// ---------------------------------------------------------------------------

fn appearance_cards(query: &str) -> Vec<Element<'static, UiEvent>> {
    let cfg = crate::background::AuroraConfig::from_store();
    let is_dark = cfg.is_dark;
    let count = cfg.blob_count;
    let schema = cfg.schema;
    let hex = cfg.to_hex();

    let q = query;
    let show_theme = matches_any(q, &["appearance", "theme", "mode", "light", "dark", "aurora", "background"]);
    let show_palette = matches_any(q, &["appearance", "palette", "color", "primary", "wheel", "hex", "aurora", "background"]);
    let show_density = matches_any(q, &["appearance", "density", "blob", "count", "schema", "vibrant", "analogous", "contrast", "neon", "solid"]);
    let show_font = matches_any(q, &["appearance", "interface", "font", "size", "scale", "ui"]);

    if !q.trim().is_empty() && !show_theme && !show_palette && !show_density && !show_font {
        return Vec::new();
    }

    let mut outer: Vec<Element<'static, UiEvent>> = Vec::new();
    if show_font {
        let raw = easyscanlate_settings::get(|s| s.ui_font_size);
        let font_str = raw.to_string();
        let clamped = scale::clamp_font_size(raw);
        let dec = stepper_button(clamped > scale::MIN_FONT_SIZE, "−", Some(UiEvent::SettingEdit(SettingEdit::UiFontSize(clamped - 1))));
        let inc = stepper_button(clamped < scale::MAX_FONT_SIZE, "+", Some(UiEvent::SettingEdit(SettingEdit::UiFontSize(clamped + 1))));
        let control: Element<'static, UiEvent> = row![
            dec,
            text_input("12", &font_str)
                .on_input(move |input| {
                    if let Ok(v) = input.trim().parse::<u32>() {
                        set(move |s| s.ui_font_size = v)
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
        .into();
        let font_section = column![
            row![
                crate::icon::lucide(Icon::Type).size(scale::s(14.0)).color(crate::accent::accent()),
                text("UI Font Size").size(scale::s(13.0)).color(Color::WHITE),
            ].spacing(scale::s(6.0)).align_y(iced::Alignment::Center),
            row![
                container(text("Size").size(scale::s(11.0)).color(Color::WHITE)).width(Length::Fixed(scale::s(90.0))),
                control,
            ].spacing(scale::s(6.0)).align_y(iced::Alignment::Center),
            text(format!("{}–{} — scales padding, spacing and radii. Chrome stays fixed.", scale::MIN_FONT_SIZE, scale::MAX_FONT_SIZE)).size(scale::s(10.0)).color(MUTED_FG),
        ].spacing(scale::s(6.0));
        outer.push(
            container(font_section)
                .width(FillLength)
                .padding(scale::s(10.0))
                .style(|_| card_style())
                .into()
        );
    }

    const AURORA_WIDTH: f32 = 260.0;
    let show_aurora_any = show_theme || show_palette || show_density;
    if show_aurora_any {
        let mut inner: Vec<Element<'static, UiEvent>> = Vec::new();
        if show_theme {
            let mode_row = segmented_group(vec![
                segment(!is_dark, "Light", Some(UiEvent::SettingEdit(SettingEdit::AuroraDarkMode(false))), iced::Font::DEFAULT),
                segment(is_dark, "Dark", Some(UiEvent::SettingEdit(SettingEdit::AuroraDarkMode(true))), iced::Font::DEFAULT),
            ]);
            inner.push(mode_row);
            if show_palette || show_density {
                inner.push(space::vertical().height(Length::Fixed(scale::s(8.0))).into());
            }
        }
        if show_palette {
            inner.push(container(text("Primary Color").size(scale::s(12.0)).color(Color::WHITE).width(FillLength).center()).padding(scale::s(2.0)).into());
            let wheel = AuroraWheel::new(cfg.clone()).view();
            inner.push(container(wheel).center_x(FillLength).into());
            let hex_owned = hex.clone();
            let hex_row: Element<'static, UiEvent> = row![
                container(text("Hex").size(scale::s(11.0)).color(Color::WHITE)).width(Length::Fixed(scale::s(36.0))),
                text_input(&hex, &hex_owned)
                    .on_input(|input| {
                        if crate::background::AuroraConfig::from_hex(&input).is_some() {
                            set(move |s| s.aurora_color = input.clone())
                        } else { UiEvent::SettingsChanged }
                    })
                    .padding(scale::s(4.0))
                    .size(scale::s(11.0))
                    .width(Length::Fixed(scale::s(100.0))),
                text(hex.clone()).size(scale::s(10.0)).color(MUTED_FG),
            ].spacing(scale::s(6.0)).align_y(iced::Alignment::Center).into();
            inner.push(container(hex_row).center_x(FillLength).into());
            if show_density {
                inner.push(space::vertical().height(Length::Fixed(scale::s(8.0))).into());
            }
        }
        if show_density {
            let count_label = if count == 1 { "Solid".to_string() } else { format!("{} | {}", count, schema.label()) };
            let dec_btn = stepper_button(count > 1, "−", Some(UiEvent::SettingEdit(SettingEdit::AuroraBlobCount(count - 1))));
            let inc_btn = stepper_button(count < 5, "+", Some(UiEvent::SettingEdit(SettingEdit::AuroraBlobCount(count + 1))));
            let schema_btn: Element<'static, UiEvent> = crate::button::with_disabled_cursor(
                button(text("⟳").size(scale::s(16.0)).width(FillLength).center())
                    .width(Length::Fixed(scale::s(30.0))).height(Length::Fixed(scale::s(30.0))).padding(0)
                    .on_press_maybe((count > 1).then(|| UiEvent::SettingEdit(SettingEdit::AuroraSchema(schema.index().wrapping_add(1) % 4))))
                    .style(|_theme: &iced::Theme, status| {
                        let bg = if status == iced::widget::button::Status::Hovered { Color::from_rgba8(255,255,255,0.30) } else { Color::from_rgba8(255,255,255,0.15) };
                        iced::widget::button::Style{ background: Some(bg.into()), border: iced::Border::default().rounded(scale::s(15.0)), text_color: Color::WHITE, ..Default::default() }
                    }).into(),
            );
            let count_row: Element<'static, UiEvent> = row![
                dec_btn,
                container(text(count_label).size(scale::s(11.0)).color(Color::WHITE).width(FillLength).center()).width(Length::Fixed(scale::s(80.0))),
                inc_btn,
                space::horizontal(),
                schema_btn
            ].spacing(scale::s(6.0)).align_y(iced::Alignment::Center).into();
            inner.push(container(count_row).center_x(FillLength).into());
            inner.push(container(text(if count == 1 { "Solid — single color, no blobs." } else { "Blobs blend with radial gradients; schema shifts hue." }).size(scale::s(10.0)).color(MUTED_FG).width(FillLength).center()).into());
        }
        let aurora_card = container(column(inner).spacing(scale::s(12.0)).align_x(iced::Alignment::Center))
            .width(Length::Fixed(scale::s(AURORA_WIDTH)))
            .padding(scale::s(14.0))
            .style(|_| container::Style{
                background: Some(Color::from_rgba8(20,20,20,0.86).into()),
                border: iced::Border::default().rounded(scale::s(20.0)).color(Color::from_rgba8(255,255,255,0.10)).width(scale::s(1.0)),
                ..Default::default()
            });
        let section = column![
            card_header(Icon::Palette, "Aurora Background", Some("Animated aurora — ManhwaOCR style")),
            container(aurora_card).width(FillLength).center_x(FillLength),
        ].spacing(scale::s(10.0));
        outer.push(container(section).width(FillLength).padding(scale::s(10.0)).style(|_| card_style()).into());
    }
    outer
}

fn appearance_tab_filtered(query: String) -> Element<'static, UiEvent> {
    let q = query.as_str();
    let outer = appearance_cards(q);
    if outer.is_empty() {
        return scrollable(
            container(
                text(format!("No appearance settings match “{query}”."))
                    .size(scale::s(12.0))
                    .color(MUTED_FG),
            )
            .padding(scale::s(14.0))
            .style(|_| card_style()),
        )
        .height(Length::Fill)
        .into();
    }

    let content = column(outer).spacing(scale::s(16.0)).width(FillLength);
    let padded = container(content).width(FillLength).padding(scale::s(8.0));
    scrollable(padded).height(Length::Fill).into()
}

// ---------------------------------------------------------------------------
// General tab — now grouped into cards + filtered
// ---------------------------------------------------------------------------

fn help_support_card() -> Element<'static, UiEvent> {
    let col: Vec<Element<'static, UiEvent>> = vec![
        card_header(
            Icon::Info,
            "Help & Support",
            Some("Docs, ideas & bug reports"),
        ),
        row![
            column![
                text("First-project guide").size(scale::s(12.0)),
                text("New here? Read the first-project guide.")
                    .size(scale::s(11.0))
                    .color(MUTED_FG),
            ]
            .spacing(scale::s(1.0))
            .width(FillLength),
            button(text("Documentation").size(scale::s(11.0)))
                .padding([scale::s(6.0), scale::s(10.0)])
                .style(crate::panel::button_style)
                .on_press(UiEvent::OpenUrl(DOCS_URL.to_string())),
        ]
        .spacing(scale::s(8.0))
        .align_y(iced::Alignment::Center)
        .into(),
        item_separator(),
        row![
            column![
                text("Request a feature").size(scale::s(12.0)),
                text("Have an idea? Request a feature.")
                    .size(scale::s(11.0))
                    .color(MUTED_FG),
            ]
            .spacing(scale::s(1.0))
            .width(FillLength),
            button(text("Request feature").size(scale::s(11.0)))
                .padding([scale::s(6.0), scale::s(10.0)])
                .style(crate::panel::button_style)
                .on_press(UiEvent::OpenUrl(FEATURE_URL.to_string())),
        ]
        .spacing(scale::s(8.0))
        .align_y(iced::Alignment::Center)
        .into(),
        item_separator(),
        row![
            column![
                text("Report an issue").size(scale::s(12.0)),
                text("Found a bug? Report the problem.")
                    .size(scale::s(11.0))
                    .color(MUTED_FG),
            ]
            .spacing(scale::s(1.0))
            .width(FillLength),
            button(text("Report issue").size(scale::s(11.0)))
                .padding([scale::s(6.0), scale::s(10.0)])
                .style(crate::panel::button_style)
                .on_press(UiEvent::OpenUrl(BUG_URL.to_string())),
        ]
        .spacing(scale::s(8.0))
        .align_y(iced::Alignment::Center)
        .into(),
    ];
    container(column(col).spacing(scale::s(8.0)))
        .padding(scale::s(10.0))
        .style(|_| card_style())
        .into()
}

fn help_support_visible(query: &str) -> bool {
    matches_any(
        query,
        &[
            "help",
            "support",
            "docs",
            "documentation",
            "manual",
            "guide",
            "workflow",
            "first",
            "project",
            "feature",
            "request",
            "idea",
            "bug",
            "issue",
            "report",
            "github",
            "general",
        ],
    )
}

fn general_tab_filtered(query: String) -> Element<'static, UiEvent> {
    let query_ref = query.as_str();
    let mut cards: Vec<Element<'static, UiEvent>> = Vec::new();

    // ── Automation card: auto-detect / SFX / auto-inpaint mirror ──
    {
        // collect whether any automation sub-field matches
        let show_auto_detect = matches_any(query_ref, &["automation", "auto", "detect", "style", "classify", "onnx", "styling"]);
        let show_sfx = matches_any(query_ref, &["automation", "auto", "sfx", "filter", "balloon", "segment", "manga"]);
        let show_inpaint = matches_any(query_ref, &["automation", "auto", "inpaint", "bg-aware"]);
        let show_automation_header = matches_any(query_ref, &["automation", "general", "auto"]);
        let show_any = show_automation_header || show_auto_detect || show_sfx || show_inpaint;
        if show_any {
            let mut col: Vec<Element<'static, UiEvent>> = Vec::new();
            col.push(card_header(Icon::Sparkles, "Automation", Some("Style & SFX filtering")));

            #[cfg(feature = "styling")]
            if show_auto_detect || show_automation_header || query_ref.trim().is_empty() {
                let auto = easyscanlate_settings::get(|s| s.auto_style_detect);
                col.push(
                    column![
                        checkbox(auto)
                            .label("Auto-detect entry styles")
                            .text_size(scale::s(12.0))
                            .on_toggle(|v| set(move |s| s.auto_style_detect = v)),
                        helper_text("Classify newly OCR-detected entries with the ONNX styling model."),
                    ].spacing(scale::s(4.0)).into()
                );
                if col.len() > 1 { col.push(item_separator()); }
            }
            #[cfg(feature = "segment")]
            if show_sfx || show_automation_header || query_ref.trim().is_empty() {
                let auto = easyscanlate_settings::get(|s| s.auto_sfx_filter);
                col.push(
                    column![
                        checkbox(auto)
                            .label("Auto-filter SFX")
                            .text_size(scale::s(12.0))
                            .on_toggle(|v| set(move |s| s.auto_sfx_filter = v)),
                        helper_text("Remove SFX outside balloons via segmentation (manga-mimic grid, 1:6 col). True SFX lives outside balloons."),
                    ].spacing(scale::s(4.0)).into()
                );
                if col.len() > 2 { col.push(item_separator()); }
            }
            #[cfg(feature = "inpaint")]
            if show_inpaint || show_automation_header || query_ref.trim().is_empty() {
                if col.len() > 1 { col.push(item_separator()); }
                let auto = easyscanlate_settings::get(|s| s.auto_inpaint);
                col.push(
                    checkbox(auto)
                        .label("Auto inpaint (bg-aware)")
                        .text_size(scale::s(12.0))
                        .on_toggle(|v| set(move |s| s.auto_inpaint = v))
                        .into()
                );
            }

            // only push card if it actually has content beyond header
            if col.len() > 1 {
                cards.push(container(column(col).spacing(scale::s(8.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
            }
        }
    }

    // ── Onboarding replay card
    {
        let show_onboarding = matches_any(query_ref, &["onboarding", "setup", "wizard", "replay", "general"]);
        if show_onboarding {
            let col: Vec<Element<'static, UiEvent>> = vec![
                card_header(Icon::Sparkles, "Onboarding", Some("First-run setup wizard")),
                column![
                    text("Replay the first-run setup (models + preferences). The wizard is blocking until all mandatory models are downloaded.").size(scale::s(11.0)).color(MUTED_FG),
                    button(text("Replay onboarding…").size(scale::s(11.0)))
                        .padding([scale::s(6.0), scale::s(10.0)])
                        .style(crate::panel::button_style)
                        .on_press(UiEvent::OnboardingReplay),
                ].spacing(scale::s(6.0)).into(),
            ];
            cards.push(container(column(col).spacing(scale::s(8.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
        }
    }

    // ── Help & Support card
    if help_support_visible(query_ref) {
        cards.push(help_support_card());
    }

    // ── Empty state ───────────────────────────────────────────────
    if cards.is_empty() {
        cards.push(
            container(column![
                row![
                    crate::icon::lucide(Icon::SearchX).size(scale::s(16.0)).color(MUTED_FG),
                    text(format!("No settings match “{query}”")).size(scale::s(12.0)).color(MUTED_FG),
                ].spacing(scale::s(6.0)).align_y(iced::Alignment::Center),
                text("Try a different term — e.g. font, ocr, inpaint, sfx.").size(scale::s(11.0)).color(MUTED_FG),
            ].spacing(scale::s(6.0)))
            .padding(scale::s(14.0))
            .style(|_| card_style())
            .into(),
        );
    }

    // wrap
    scrollable(
        column(cards).spacing(scale::s(10.0))
    )
    .height(Length::Fill)
    .into()
}

fn general_cards(query: &str) -> Vec<Element<'static, UiEvent>> {
    let mut cards: Vec<Element<'static, UiEvent>> = Vec::new();
    {
        let show_auto_detect = matches_any(query, &["automation", "auto", "detect", "style", "classify", "onnx", "styling"]);
        let show_sfx = matches_any(query, &["automation", "auto", "sfx", "filter", "balloon", "segment", "manga"]);
        let show_inpaint = matches_any(query, &["automation", "auto", "inpaint", "bg-aware"]);
        let show_automation_header = matches_any(query, &["automation", "general", "auto"]);
        let show_any = show_automation_header || show_auto_detect || show_sfx || show_inpaint;
        if show_any {
            let mut col: Vec<Element<'static, UiEvent>> = Vec::new();
            col.push(card_header(Icon::Sparkles, "Automation", Some("Style & SFX filtering")));
            #[cfg(feature = "styling")]
            if show_auto_detect || show_automation_header || query.trim().is_empty() {
                let auto = easyscanlate_settings::get(|s| s.auto_style_detect);
                col.push(
                    column![
                        checkbox(auto)
                            .label("Auto-detect entry styles")
                            .text_size(scale::s(12.0))
                            .on_toggle(|v| set(move |s| s.auto_style_detect = v)),
                        helper_text("Classify newly OCR-detected entries with the ONNX styling model."),
                    ].spacing(scale::s(4.0)).into()
                );
                if col.len() > 1 { col.push(item_separator()); }
            }
            #[cfg(feature = "segment")]
            if show_sfx || show_automation_header || query.trim().is_empty() {
                let auto = easyscanlate_settings::get(|s| s.auto_sfx_filter);
                col.push(
                    column![
                        checkbox(auto)
                            .label("Auto-filter SFX")
                            .text_size(scale::s(12.0))
                            .on_toggle(|v| set(move |s| s.auto_sfx_filter = v)),
                        helper_text("Remove SFX outside balloons via segmentation (manga-mimic grid, 1:6 col). True SFX lives outside balloons."),
                    ].spacing(scale::s(4.0)).into()
                );
                if col.len() > 2 { col.push(item_separator()); }
            }
            #[cfg(feature = "inpaint")]
            if show_inpaint || show_automation_header || query.trim().is_empty() {
                if col.len() > 1 { col.push(item_separator()); }
                let auto = easyscanlate_settings::get(|s| s.auto_inpaint);
                col.push(
                    checkbox(auto)
                        .label("Auto inpaint (bg-aware)")
                        .text_size(scale::s(12.0))
                        .on_toggle(|v| set(move |s| s.auto_inpaint = v))
                        .into()
                );
            }
            if col.len() > 1 {
                cards.push(container(column(col).spacing(scale::s(8.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
            }
        }
    }
    // Onboarding replay card
    {
        let show_onboarding = matches_any(query, &["onboarding", "setup", "wizard", "replay", "general"]);
        if show_onboarding {
            let col: Vec<Element<'static, UiEvent>> = vec![
                card_header(Icon::Sparkles, "Onboarding", Some("First-run setup wizard")),
                column![
                    text("Replay the first-run setup (models + preferences). The wizard is blocking until all mandatory models are downloaded.").size(scale::s(11.0)).color(MUTED_FG),
                    button(text("Replay onboarding…").size(scale::s(11.0)))
                        .padding([scale::s(6.0), scale::s(10.0)])
                        .style(crate::panel::button_style)
                        .on_press(UiEvent::OnboardingReplay),
                ].spacing(scale::s(6.0)).into(),
            ];
            cards.push(container(column(col).spacing(scale::s(8.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
        }
    }
    if help_support_visible(query) {
        cards.push(help_support_card());
    }
    cards
}

// ---------------------------------------------------------------------------
// OCR tab — dedicated OCR tuning
// ---------------------------------------------------------------------------

// `query` only feeds the `#[cfg(feature = "ocr")]` block below.
#[allow(unused_variables)]
fn ocr_tab_filtered(query: String) -> Element<'static, UiEvent> {
    let query_ref = query.as_str();
    let mut cards: Vec<Element<'static, UiEvent>> = Vec::new();

    #[cfg(feature = "ocr")]
    {
        let show_ocr = matches_any(query_ref, &["ocr", "engine", "detection", "recognition", "confidence", "text", "height", "bbox", "merge", "distance", "threshold", "side", "len", "workers", "parallel", "tuning"]);
        if show_ocr {
            let workers = easyscanlate_settings::get(|s| s.ocr_workers.clone());
            let (text_score, min_bbox_h, max_bbox_h, max_side, merge_thr) =
                easyscanlate_settings::get(|s| {
                    (
                        s.ocr_text_score.clone(),
                        s.ocr_min_text_height.clone(),
                        s.ocr_max_text_height.clone(),
                        s.ocr_max_side_len.clone(),
                        s.ocr_merge_threshold.clone(),
                    )
                });
            let mut col: Vec<Element<'static, UiEvent>> = Vec::new();
            col.push(card_header(Icon::ScanSearch, "OCR Engine", Some("Detection & recognition tuning — next run")));
            if matches_any(query_ref, &["ocr", "workers", "parallel", "detection", "engine"]) || query_ref.trim().is_empty() {
                col.push(field_row("Detection workers",
                    text_input("2", &workers)
                        .on_input(|input| set(move |s| s.ocr_workers = input.clone()))
                        .padding(scale::s(4.0))
                        .size(scale::s(12.0))
                        .width(Length::Fixed(scale::s(80.0)))
                        .into()
                ));
                col.push(helper_text("Parallel detection sessions; 2 fits a potato-laptop CPU."));
                col.push(item_separator());
            }
            if matches_any(query_ref, &["ocr", "tuning", "confidence"]) || query_ref.trim().is_empty() {
                col.push(field_row("Min confidence",
                    text_input("0.7", &text_score)
                        .on_input(|input| set(move |s| s.ocr_text_score = input.clone()))
                        .padding(scale::s(4.0))
                        .size(scale::s(12.0))
                        .width(Length::Fixed(scale::s(80.0)))
                        .into()
                ));
                col.push(helper_text("Minimum recognition confidence 0.0–1.0. Lower keeps more lines."));
            }
            if matches_any(query_ref, &["ocr", "height", "bbox", "minimum", "min"]) || query_ref.trim().is_empty() {
                col.push(field_row("Min text height",
                    text_input("40", &min_bbox_h)
                        .on_input(|input| set(move |s| s.ocr_min_text_height = input.clone()))
                        .padding(scale::s(4.0))
                        .size(scale::s(12.0))
                        .width(Length::Fixed(scale::s(80.0)))
                        .into()
                ));
                col.push(helper_text("Minimum bbox height (px). Drops boxes shorter than this."));
            }
            if matches_any(query_ref, &["ocr", "height", "bbox", "maximum", "max"]) || query_ref.trim().is_empty() {
                col.push(field_row("Max text height",
                    text_input("100", &max_bbox_h)
                        .on_input(|input| set(move |s| s.ocr_max_text_height = input.clone()))
                        .padding(scale::s(4.0))
                        .size(scale::s(12.0))
                        .width(Length::Fixed(scale::s(80.0)))
                        .into()
                ));
                col.push(helper_text("Maximum bbox height (px). Drops boxes taller than this."));
            }
            if matches_any(query_ref, &["ocr", "merge", "distance", "threshold", "gap"]) || query_ref.trim().is_empty() {
                col.push(field_row("Merge threshold",
                    text_input("0.5", &merge_thr)
                        .on_input(|input| set(move |s| s.ocr_merge_threshold = input.clone()))
                        .padding(scale::s(4.0))
                        .size(scale::s(12.0))
                        .width(Length::Fixed(scale::s(80.0)))
                        .into()
                ));
                col.push(helper_text("Gap as ratio of height 0.0–2.0 (0.5 = 50% of height)."));
            }
            if matches_any(query_ref, &["ocr", "side", "len", "max", "resize"]) || query_ref.trim().is_empty() {
                col.push(field_row("Max side len",
                    text_input("2000", &max_side)
                        .on_input(|input| set(move |s| s.ocr_max_side_len = input.clone()))
                        .padding(scale::s(4.0))
                        .size(scale::s(12.0))
                        .width(Length::Fixed(scale::s(80.0)))
                        .into()
                ));
                col.push(helper_text("Max longer side before resize (px). Larger keeps more detail but uses more RAM."));
            }
            let has_field = col.len() > 1;
            if has_field {
                cards.push(container(column(col).spacing(scale::s(7.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
            }
        }
    }
    #[cfg(not(feature = "ocr"))]
    {
        cards.push(
            container(column![
                card_header(Icon::ScanSearch, "OCR Engine", Some("OCR feature not enabled")),
                helper_text("Rebuild with --features ocr to enable OCR tuning."),
            ].spacing(scale::s(8.0))).padding(scale::s(10.0)).style(|_| card_style()).into()
        );
    }
    if cards.is_empty() {
        cards.push(
            container(column![
                row![crate::icon::lucide(Icon::SearchX).size(scale::s(16.0)).color(MUTED_FG), text(format!("No OCR settings match “{query}”")).size(scale::s(12.0)).color(MUTED_FG)].spacing(scale::s(6.0)).align_y(iced::Alignment::Center),
                helper_text("Try a different term."),
            ].spacing(scale::s(6.0))).padding(scale::s(14.0)).style(|_| card_style()).into()
        );
    }
    scrollable(column(cards).spacing(scale::s(10.0))).height(Length::Fill).into()
}

// `query` only feeds the `#[cfg(feature = "ocr")]` block below.
#[allow(unused_variables)]
fn ocr_cards(query: &str) -> Vec<Element<'static, UiEvent>> {
    let mut cards: Vec<Element<'static, UiEvent>> = Vec::new();
    #[cfg(feature = "ocr")]
    {
        let show_ocr = matches_any(query, &["ocr", "engine", "detection", "recognition", "confidence", "text", "height", "bbox", "merge", "distance", "threshold", "side", "len", "workers", "parallel", "tuning"]);
        if show_ocr {
            let workers = easyscanlate_settings::get(|s| s.ocr_workers.clone());
            let (text_score, min_bbox_h, max_bbox_h, max_side, merge_thr) =
                easyscanlate_settings::get(|s| {
                    (
                        s.ocr_text_score.clone(),
                        s.ocr_min_text_height.clone(),
                        s.ocr_max_text_height.clone(),
                        s.ocr_max_side_len.clone(),
                        s.ocr_merge_threshold.clone(),
                    )
                });
            let mut col: Vec<Element<'static, UiEvent>> = Vec::new();
            col.push(card_header(Icon::ScanSearch, "OCR Engine", Some("Detection & recognition tuning — next run")));
            if matches_any(query, &["ocr", "workers", "parallel", "detection", "engine"]) || query.trim().is_empty() {
                col.push(field_row("Detection workers",
                    text_input("2", &workers)
                        .on_input(|input| set(move |s| s.ocr_workers = input.clone()))
                        .padding(scale::s(4.0))
                        .size(scale::s(12.0))
                        .width(Length::Fixed(scale::s(80.0)))
                        .into()
                ));
                col.push(helper_text("Parallel detection sessions; 2 fits a potato-laptop CPU."));
                col.push(item_separator());
            }
            if matches_any(query, &["ocr", "tuning", "confidence"]) || query.trim().is_empty() {
                col.push(field_row("Min confidence",
                    text_input("0.7", &text_score)
                        .on_input(|input| set(move |s| s.ocr_text_score = input.clone()))
                        .padding(scale::s(4.0))
                        .size(scale::s(12.0))
                        .width(Length::Fixed(scale::s(80.0)))
                        .into()
                ));
                col.push(helper_text("Minimum recognition confidence 0.0–1.0. Lower keeps more lines."));
            }
            if matches_any(query, &["ocr", "height", "bbox", "minimum", "min"]) || query.trim().is_empty() {
                col.push(field_row("Min text height",
                    text_input("40", &min_bbox_h)
                        .on_input(|input| set(move |s| s.ocr_min_text_height = input.clone()))
                        .padding(scale::s(4.0))
                        .size(scale::s(12.0))
                        .width(Length::Fixed(scale::s(80.0)))
                        .into()
                ));
                col.push(helper_text("Minimum bbox height (px). Drops boxes shorter than this."));
            }
            if matches_any(query, &["ocr", "height", "bbox", "maximum", "max"]) || query.trim().is_empty() {
                col.push(field_row("Max text height",
                    text_input("100", &max_bbox_h)
                        .on_input(|input| set(move |s| s.ocr_max_text_height = input.clone()))
                        .padding(scale::s(4.0))
                        .size(scale::s(12.0))
                        .width(Length::Fixed(scale::s(80.0)))
                        .into()
                ));
                col.push(helper_text("Maximum bbox height (px). Drops boxes taller than this."));
            }
            if matches_any(query, &["ocr", "merge", "distance", "threshold", "gap"]) || query.trim().is_empty() {
                col.push(field_row("Merge threshold",
                    text_input("0.5", &merge_thr)
                        .on_input(|input| set(move |s| s.ocr_merge_threshold = input.clone()))
                        .padding(scale::s(4.0))
                        .size(scale::s(12.0))
                        .width(Length::Fixed(scale::s(80.0)))
                        .into()
                ));
                col.push(helper_text("Gap as ratio of height 0.0–2.0 (0.5 = 50% of height)."));
            }
            if matches_any(query, &["ocr", "side", "len", "max", "resize"]) || query.trim().is_empty() {
                col.push(field_row("Max side len",
                    text_input("2000", &max_side)
                        .on_input(|input| set(move |s| s.ocr_max_side_len = input.clone()))
                        .padding(scale::s(4.0))
                        .size(scale::s(12.0))
                        .width(Length::Fixed(scale::s(80.0)))
                        .into()
                ));
                col.push(helper_text("Max longer side before resize (px). Larger keeps more detail but uses more RAM."));
            }
            let has_field = col.len() > 1;
            if has_field {
                cards.push(container(column(col).spacing(scale::s(7.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
            }
        }
    }
    #[cfg(not(feature = "ocr"))]
    {
        cards.push(
            container(column![
                card_header(Icon::ScanSearch, "OCR Engine", Some("OCR feature not enabled")),
                helper_text("Rebuild with --features ocr to enable OCR tuning."),
            ].spacing(scale::s(8.0))).padding(scale::s(10.0)).style(|_| card_style()).into()
        );
    }
    cards
}

 // ---------------------------------------------------------------------------
// Inpaint tab — manual + auto bg-aware
// ---------------------------------------------------------------------------

fn inpaint_tab_filtered(query: String) -> Element<'static, UiEvent> {
    let query_ref = query.as_str();
    let mut cards: Vec<Element<'static, UiEvent>> = Vec::new();

    // Auto inpaint (bg-aware) — moved from General automation
    {
        let show_auto = matches_any(query_ref, &["inpaint", "auto", "bg", "telea", "lama", "aot", "mixed", "pipeline", "artwork", "gradient", "solid", "automation"]);
        if show_auto {
            #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
            {
                let (auto_inpaint, auto_model, auto_style) = easyscanlate_settings::get(|s| (s.auto_inpaint, s.auto_inpaint_model, s.auto_style_detect));
                let mut col: Vec<Element<'static, UiEvent>> = Vec::new();
                col.push(card_header(Icon::Sparkles, "Auto Inpaint (bg-aware)", Some("After OCR: style-detect → per bg type")));
                col.push(checkbox(auto_inpaint).label("Auto inpaint (bg-aware)").text_size(scale::s(12.0)).on_toggle(move |v| set(move |s| s.auto_inpaint = v)).into());
                col.push(helper_text("Solid keeps bg color, Gradient → Telea, Artwork → LaMa. Mixed needs Auto-detect style."));
                let all_models = [AutoInpaintModel::Telea, AutoInpaintModel::Lama, AutoInpaintModel::Aot, AutoInpaintModel::Mixed];
                let available: Vec<AutoInpaintModel> = if auto_style { all_models.to_vec() } else { vec![AutoInpaintModel::Telea, AutoInpaintModel::Lama, AutoInpaintModel::Aot] };
                let pick_value = if auto_style || auto_model != AutoInpaintModel::Mixed { Some(auto_model) } else { Some(AutoInpaintModel::Telea) };
                col.push(row![
                    container(text("Auto inpaint model").size(scale::s(12.0)).color(Color::WHITE)).width(Length::Fixed(scale::s(150.0))),
                    pick_list(available, pick_value, move |model| set(move |s| s.auto_inpaint_model = model)).padding(scale::s(4.0)).text_size(scale::s(12.0)),
                    if auto_inpaint && auto_model == AutoInpaintModel::Mixed && !auto_style { warning_text("Mixed disabled — falling back to Telea.".to_string()) } else { text("").size(scale::s(11.0)).color(MUTED_FG).into() },
                ].spacing(scale::s(6.0)).align_y(iced::Alignment::Center).into());
                if !auto_style && auto_model == AutoInpaintModel::Mixed {
                    col.push(warning_text("Pick Telea, Lama or AOT while Auto-detect style is off; Mixed will auto-fallback to Telea.".to_string()));
                }
                col.push(helper_text("Full pipeline runs once when OCR finishes if all toggles are on. Telea in parallel, LaMa/AOT sequentially."));
                cards.push(container(column(col).spacing(scale::s(7.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
            }
            #[cfg(all(feature = "inpaint", not(all(feature = "styling", feature = "segment"))))]
            {
                let (auto_inpaint, auto_model) = easyscanlate_settings::get(|s| (s.auto_inpaint, s.auto_inpaint_model));
                let mut col: Vec<Element<'static, UiEvent>> = Vec::new();
                col.push(card_header(Icon::Sparkles, "Auto Inpaint (bg-aware)", Some("Bg-aware pipeline")).into());
                col.push(checkbox(auto_inpaint).label("Auto inpaint (bg-aware)").text_size(scale::s(12.0)).on_toggle(move |v| set(move |s| s.auto_inpaint = v)).into());
                col.push(helper_text("Needs Styling + Segmentation for full bg-aware pipeline; fallback is Telea.").into());
                col.push(row![
                    container(text("Auto inpaint model").size(scale::s(12.0)).color(Color::WHITE)).width(Length::Fixed(scale::s(150.0))),
                    pick_list([AutoInpaintModel::Telea, AutoInpaintModel::Lama, AutoInpaintModel::Aot], Some(if auto_model == AutoInpaintModel::Mixed { AutoInpaintModel::Telea } else { auto_model }), move |model| set(move |s| s.auto_inpaint_model = model)).padding(scale::s(4.0)).text_size(scale::s(12.0)),
                ].spacing(scale::s(6.0)).into());
                cards.push(container(column(col).spacing(scale::s(7.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
            }
            #[cfg(not(feature = "inpaint"))]
            {
                cards.push(container(column![card_header(Icon::Sparkles, "Auto Inpaint", Some("Inpaint feature not enabled")), helper_text("Rebuild with --features inpaint.")].spacing(scale::s(8.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
            }
        }
    }

    // Manual inpaint
    {
        let show_manual = matches_any(query_ref, &["inpaint", "backend", "telea", "lama", "aot", "radius", "manual", "brush"]);
        if show_manual {
            #[cfg(feature = "inpaint")]
            {
                let backend = easyscanlate_settings::get(|s| s.inpaint_backend);
                let radius = easyscanlate_settings::get(|s| s.inpaint_radius.clone());
                let col: Vec<Element<'static, UiEvent>> = vec![
                    card_header(Icon::Brush, "Inpaint (Manual)", Some("Brush tool — Telea vs ONNX")),
                    field_row("Backend", pick_list([InpaintBackend::Telea, InpaintBackend::Lama, InpaintBackend::Aot], Some(backend), |backend| set(move |s| s.inpaint_backend = backend)).padding(scale::s(4.0)).text_size(scale::s(12.0)).into()),
                    helper_text("Telea is instant (no model); LaMa is high-quality ONNX; AOT-GAN is 2-4× faster than LaMa (pad 8, max 1024)."),
                    item_separator(),
                    field_row("Telea radius", text_input("5", &radius).on_input(|input| set(move |s| s.inpaint_radius = input.clone())).padding(scale::s(4.0)).size(scale::s(12.0)).width(Length::Fixed(scale::s(80.0))).into()),
                    helper_text("Pixels around mask Telea samples; larger smooths more but blurs. Ignored by LaMa/AOT."),
                ];
                cards.push(container(column(col).spacing(scale::s(7.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
            }
            #[cfg(not(feature = "inpaint"))]
            {
                // already pushed auto placeholder if not inpaint; manual not needed
            }
        }
    }

    if cards.is_empty() {
        cards.push(container(column![row![crate::icon::lucide(Icon::SearchX).size(scale::s(16.0)).color(MUTED_FG), text(format!("No inpaint settings match “{query}”")).size(scale::s(12.0)).color(MUTED_FG)].spacing(scale::s(6.0)).align_y(iced::Alignment::Center)].spacing(scale::s(6.0))).padding(scale::s(14.0)).style(|_| card_style()).into());
    }
    scrollable(column(cards).spacing(scale::s(10.0))).height(Length::Fill).into()
}

fn inpaint_cards(query: &str) -> Vec<Element<'static, UiEvent>> {
    let mut cards: Vec<Element<'static, UiEvent>> = Vec::new();
    {
        let show_auto = matches_any(query, &["inpaint", "auto", "bg", "telea", "lama", "aot", "mixed", "pipeline", "artwork", "gradient", "solid", "automation"]);
        if show_auto {
            #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
            {
                let (auto_inpaint, auto_model, auto_style) = easyscanlate_settings::get(|s| (s.auto_inpaint, s.auto_inpaint_model, s.auto_style_detect));
                let mut col: Vec<Element<'static, UiEvent>> = Vec::new();
                col.push(card_header(Icon::Sparkles, "Auto Inpaint (bg-aware)", Some("After OCR: style-detect → per bg type")));
                col.push(checkbox(auto_inpaint).label("Auto inpaint (bg-aware)").text_size(scale::s(12.0)).on_toggle(move |v| set(move |s| s.auto_inpaint = v)).into());
                col.push(helper_text("Solid keeps bg color, Gradient → Telea, Artwork → LaMa. Mixed needs Auto-detect style."));
                let all_models = [AutoInpaintModel::Telea, AutoInpaintModel::Lama, AutoInpaintModel::Aot, AutoInpaintModel::Mixed];
                let available: Vec<AutoInpaintModel> = if auto_style { all_models.to_vec() } else { vec![AutoInpaintModel::Telea, AutoInpaintModel::Lama, AutoInpaintModel::Aot] };
                let pick_value = if auto_style || auto_model != AutoInpaintModel::Mixed { Some(auto_model) } else { Some(AutoInpaintModel::Telea) };
                col.push(row![
                    container(text("Auto inpaint model").size(scale::s(12.0)).color(Color::WHITE)).width(Length::Fixed(scale::s(150.0))),
                    pick_list(available, pick_value, move |model| set(move |s| s.auto_inpaint_model = model)).padding(scale::s(4.0)).text_size(scale::s(12.0)),
                    if auto_inpaint && auto_model == AutoInpaintModel::Mixed && !auto_style { warning_text("Mixed disabled — falling back to Telea.".to_string()) } else { text("").size(scale::s(11.0)).color(MUTED_FG).into() },
                ].spacing(scale::s(6.0)).align_y(iced::Alignment::Center).into());
                if !auto_style && auto_model == AutoInpaintModel::Mixed {
                    col.push(warning_text("Pick Telea, Lama or AOT while Auto-detect style is off; Mixed will auto-fallback to Telea.".to_string()));
                }
                col.push(helper_text("Full pipeline runs once when OCR finishes if all toggles are on. Telea in parallel, LaMa/AOT sequentially."));
                cards.push(container(column(col).spacing(scale::s(7.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
            }
            #[cfg(all(feature = "inpaint", not(all(feature = "styling", feature = "segment"))))]
            {
                let (auto_inpaint, auto_model) = easyscanlate_settings::get(|s| (s.auto_inpaint, s.auto_inpaint_model));
                let mut col: Vec<Element<'static, UiEvent>> = Vec::new();
                col.push(card_header(Icon::Sparkles, "Auto Inpaint (bg-aware)", Some("Bg-aware pipeline")).into());
                col.push(checkbox(auto_inpaint).label("Auto inpaint (bg-aware)").text_size(scale::s(12.0)).on_toggle(move |v| set(move |s| s.auto_inpaint = v)).into());
                col.push(helper_text("Needs Styling + Segmentation for full bg-aware pipeline; fallback is Telea.").into());
                col.push(row![
                    container(text("Auto inpaint model").size(scale::s(12.0)).color(Color::WHITE)).width(Length::Fixed(scale::s(150.0))),
                    pick_list([AutoInpaintModel::Telea, AutoInpaintModel::Lama, AutoInpaintModel::Aot], Some(if auto_model == AutoInpaintModel::Mixed { AutoInpaintModel::Telea } else { auto_model }), move |model| set(move |s| s.auto_inpaint_model = model)).padding(scale::s(4.0)).text_size(scale::s(12.0)),
                ].spacing(scale::s(6.0)).into());
                cards.push(container(column(col).spacing(scale::s(7.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
            }
            #[cfg(not(feature = "inpaint"))]
            {
                cards.push(container(column![card_header(Icon::Sparkles, "Auto Inpaint", Some("Inpaint feature not enabled")), helper_text("Rebuild with --features inpaint.")].spacing(scale::s(8.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
            }
        }
    }
    {
        let show_manual = matches_any(query, &["inpaint", "backend", "telea", "lama", "aot", "radius", "manual", "brush"]);
        if show_manual {
            #[cfg(feature = "inpaint")]
            {
                let backend = easyscanlate_settings::get(|s| s.inpaint_backend);
                let radius = easyscanlate_settings::get(|s| s.inpaint_radius.clone());
                let col: Vec<Element<'static, UiEvent>> = vec![
                    card_header(Icon::Brush, "Inpaint (Manual)", Some("Brush tool — Telea vs ONNX")),
                    field_row("Backend", pick_list([InpaintBackend::Telea, InpaintBackend::Lama, InpaintBackend::Aot], Some(backend), |backend| set(move |s| s.inpaint_backend = backend)).padding(scale::s(4.0)).text_size(scale::s(12.0)).into()),
                    helper_text("Telea is instant (no model); LaMa is high-quality ONNX; AOT-GAN is 2-4× faster than LaMa (pad 8, max 1024)."),
                    item_separator(),
                    field_row("Telea radius", text_input("5", &radius).on_input(|input| set(move |s| s.inpaint_radius = input.clone())).padding(scale::s(4.0)).size(scale::s(12.0)).width(Length::Fixed(scale::s(80.0))).into()),
                    helper_text("Pixels around mask Telea samples; larger smooths more but blurs. Ignored by LaMa/AOT."),
                ];
                cards.push(container(column(col).spacing(scale::s(7.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
            }
        }
    }
    cards
}

// ---------------------------------------------------------------------------
// Translation tab — card-grouped + filtered
// ---------------------------------------------------------------------------

fn translation_tab_filtered(query: String) -> Element<'static, UiEvent> {
    let (connections, free_only) =
        easyscanlate_settings::get(|s| (s.connections.clone(), s.free_models_only));

    let q = query.trim().to_lowercase();
    let query_active = !q.is_empty();

    // helper to test provider match
    let provider_matches = |id: &str, name: &str| -> bool {
        if !query_active { return true; }
        matches_query(&q, &format!("{id} {name}"))
    };
    let custom_matches = |id: &str, label: &str| -> bool {
        if !query_active { return true; }
        matches_query(&q, &format!("{id} {label}"))
    };

    // partition with filtering
    let mut connected_rows: Vec<Element<'static, UiEvent>> = Vec::new();
    let mut available_rows: Vec<Element<'static, UiEvent>> = Vec::new();
    let mut connected_ids: Vec<String> = Vec::new();
    for provider in translation::SUPPORTED_PROVIDERS.iter() {
        if !provider_matches(&provider.id, &provider.name) { continue; }
        let conn = connections.get(&provider.id).cloned();
        let el = provider_row_with_connection(provider, conn.clone());
        if conn.is_some() {
            connected_rows.push(el);
            connected_ids.push(provider.id.clone());
        } else {
            available_rows.push(el);
        }
    }
    for (id, label) in [
        (CUSTOM_OPENAI, "OpenAI-compatible"),
        (CUSTOM_ANTHROPIC, "Anthropic-compatible"),
    ] {
        if !custom_matches(id, label) { continue; }
        let conn = connections.get(id).cloned();
        let el = custom_row_with_connection(id, label, conn.clone());
        if conn.is_some() {
            connected_rows.push(el);
            connected_ids.push(id.to_string());
        } else {
            available_rows.push(el);
        }
    }

    // recommended rows (also filtered)
    let mut recommended_rows: Vec<Element<'static, UiEvent>> = Vec::new();
    if !translation::RECOMMENDED.is_empty() {
        for info in translation::RECOMMENDED.iter() {
            if connections.contains_key(info.id) { continue; }
            if let Some(provider) = translation::catalog_provider(info.id) {
                if !provider_matches(provider.id.as_str(), &provider.name) { continue; }
                // also match description/docs
                if query_active && !matches_query(&q, &format!("{} {}", info.description, info.docs_url)) && !matches_query(&q, &provider.name) {
                    // provider already tested, but description may be the match
                    // if provider didn't match, we already continued; if it did, keep
                    // So only skip when neither provider nor description matches.
                    // To keep logic simple: if query and neither provider nor desc matches, skip
                    // But provider already passed, so we keep.
                }
                recommended_rows.push(recommended_row(provider, info));
            }
        }
    }

    let show_connected = !query_active || !connected_rows.is_empty() || matches_query(&q, "connected");
    let show_recommended = !translation::RECOMMENDED.is_empty() && (!query_active || !recommended_rows.is_empty() || matches_query(&q, "recommended"));
    let show_available = !query_active || !available_rows.is_empty() || matches_query(&q, "available");

    let mut cards: Vec<Element<'static, UiEvent>> = Vec::new();

    // intro card (hidden when searching and no match)
    if !query_active || matches_any(&q, &["translation", "service", "connect", "gateway", "provider"]) {
        cards.push(
            container(column![
                text("Translation Service").size(scale::s(14.0)).color(Color::WHITE),
                text("Connect the gateway used by the machine translator. Disconnect removes its API key.")
                    .size(scale::s(11.0))
                    .color(MUTED_FG),
            ].spacing(scale::s(4.0)))
            .padding(scale::s(10.0))
            .style(|_| card_style())
            .into(),
        );
    }

    // Connected card
    if show_connected {
        let mut content: Vec<Element<'static, UiEvent>> = Vec::new();
        content.push(
            row![
                crate::icon::lucide(Icon::PlugZap).size(scale::s(14.0)).color(crate::accent::accent()),
                text("Connected").size(scale::s(13.0)).color(Color::WHITE),
                space::horizontal(),
                text(if connected_rows.is_empty() { "—".to_string() } else { format!("{} connected", connected_rows.len()) }).size(scale::s(11.0)).color(MUTED_FG),
            ].align_y(iced::Alignment::Center).spacing(scale::s(6.0)).into()
        );
        content.push(item_separator());
        if connected_rows.is_empty() {
            content.push(
                text(if query_active { format!("No connected providers match “{query}”.") } else { "No connected providers — connect one below.".to_string() })
                    .size(scale::s(11.0)).color(MUTED_FG).into()
            );
        } else {
            let len = connected_rows.len();
            for (idx, el) in connected_rows.into_iter().enumerate() {
                content.push(el);
                if idx + 1 < len { content.push(item_separator()); }
            }
        }
        cards.push(container(column(content).spacing(scale::s(6.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
    }

    // Recommended card
    if show_recommended {
        let mut content: Vec<Element<'static, UiEvent>> = Vec::new();
        content.push(
            row![
                crate::icon::lucide(Icon::Star).size(scale::s(14.0)).color(crate::accent::accent()),
                text("Recommended").size(scale::s(13.0)).color(Color::WHITE),
                space::horizontal(),
                text(if recommended_rows.is_empty() { "—".to_string() } else { format!("{} recommended", recommended_rows.len()) }).size(scale::s(11.0)).color(MUTED_FG),
            ].align_y(iced::Alignment::Center).spacing(scale::s(6.0)).into()
        );
        content.push(text("Not sure where to start? Try one of these.").size(scale::s(11.0)).color(MUTED_FG).into());
        content.push(item_separator());
        if recommended_rows.is_empty() {
            content.push(text(if query_active { format!("No recommendations match “{query}”.") } else { "All recommended providers connected.".to_string() }).size(scale::s(11.0)).color(MUTED_FG).into());
        } else {
            let len = recommended_rows.len();
            for (idx, el) in recommended_rows.into_iter().enumerate() {
                content.push(el);
                if idx + 1 < len { content.push(item_separator()); }
            }
        }
        cards.push(container(column(content).spacing(scale::s(6.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
    }

    // Available card
    if show_available {
        let mut content: Vec<Element<'static, UiEvent>> = Vec::new();
        content.push(
            row![
                crate::icon::lucide(Icon::Globe).size(scale::s(14.0)).color(MUTED_FG),
                text("Available").size(scale::s(13.0)).color(Color::WHITE),
                space::horizontal(),
                text(format!("{} available", available_rows.len())).size(scale::s(11.0)).color(MUTED_FG),
            ].align_y(iced::Alignment::Center).spacing(scale::s(6.0)).into()
        );
        content.push(item_separator());
        if available_rows.is_empty() {
            content.push(
                text(if query_active { format!("No available providers match “{query}”.") } else { "All providers connected.".to_string() }).size(scale::s(11.0)).color(MUTED_FG).into()
            );
        } else {
            let len = available_rows.len();
            for (idx, el) in available_rows.into_iter().enumerate() {
                content.push(el);
                if idx + 1 < len { content.push(item_separator()); }
            }
        }
        cards.push(container(column(content).spacing(scale::s(6.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
    }

    // Options card — only when query matches its keywords or empty
    if matches_any(&q, &["free", "paid", "filter", "models", "manage", "dropdown", "only show free"]) || !query_active {
        let content: Vec<Element<'static, UiEvent>> = vec![
            card_header(Icon::SlidersHorizontal, "Options", None),
            item_separator(),
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
            item_separator(),
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
                    .style(crate::panel::button_style)
                    .on_press(UiEvent::ManageModelsOpen),
            ]
            .spacing(scale::s(6.0))
            .align_y(iced::Alignment::Center)
            .padding([scale::s(4.0), 0.0])
            .into(),
            item_separator(),
            text("Connections are saved to the app's settings file in the system configuration directory.")
                .size(scale::s(11.0))
                .color(MUTED_FG)
                .into(),
        ];
        cards.push(container(column(content).spacing(scale::s(6.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
    }

    if cards.is_empty() {
        cards.push(
            container(column![
                row![
                    crate::icon::lucide(Icon::SearchX).size(scale::s(16.0)).color(MUTED_FG),
                    text(format!("No translation settings match “{query}”")).size(scale::s(12.0)).color(MUTED_FG),
                ].spacing(scale::s(6.0)).align_y(iced::Alignment::Center),
            ].spacing(scale::s(6.0)))
            .padding(scale::s(14.0))
            .style(|_| card_style())
            .into(),
        );
    }

    scrollable(column(cards).spacing(scale::s(10.0)))
        .height(Length::Fill)
        .into()
}

fn translation_cards(query: &str) -> Vec<Element<'static, UiEvent>> {
    let (connections, free_only) =
        easyscanlate_settings::get(|s| (s.connections.clone(), s.free_models_only));
    let q = query.trim().to_lowercase();
    let query_active = !q.is_empty();
    let provider_matches = |id: &str, name: &str| -> bool {
        if !query_active { return true; }
        matches_query(&q, &format!("{id} {name}"))
    };
    let custom_matches = |id: &str, label: &str| -> bool {
        if !query_active { return true; }
        matches_query(&q, &format!("{id} {label}"))
    };
    let mut connected_rows: Vec<Element<'static, UiEvent>> = Vec::new();
    let mut available_rows: Vec<Element<'static, UiEvent>> = Vec::new();
    let mut connected_ids: Vec<String> = Vec::new();
    for provider in translation::SUPPORTED_PROVIDERS.iter() {
        if !provider_matches(&provider.id, &provider.name) { continue; }
        let conn = connections.get(&provider.id).cloned();
        let el = provider_row_with_connection(provider, conn.clone());
        if conn.is_some() {
            connected_rows.push(el);
            connected_ids.push(provider.id.clone());
        } else {
            available_rows.push(el);
        }
    }
    for (id, label) in [
        (CUSTOM_OPENAI, "OpenAI-compatible"),
        (CUSTOM_ANTHROPIC, "Anthropic-compatible"),
    ] {
        if !custom_matches(id, label) { continue; }
        let conn = connections.get(id).cloned();
        let el = custom_row_with_connection(id, label, conn.clone());
        if conn.is_some() {
            connected_rows.push(el);
            connected_ids.push(id.to_string());
        } else {
            available_rows.push(el);
        }
    }
    let mut recommended_rows: Vec<Element<'static, UiEvent>> = Vec::new();
    if !translation::RECOMMENDED.is_empty() {
        for info in translation::RECOMMENDED.iter() {
            if connections.contains_key(info.id) { continue; }
            if let Some(provider) = translation::catalog_provider(info.id) {
                if !provider_matches(provider.id.as_str(), &provider.name) { continue; }
                recommended_rows.push(recommended_row(provider, info));
            }
        }
    }
    let show_connected = !query_active || !connected_rows.is_empty() || matches_query(&q, "connected");
    let show_recommended = !translation::RECOMMENDED.is_empty() && (!query_active || !recommended_rows.is_empty() || matches_query(&q, "recommended"));
    let show_available = !query_active || !available_rows.is_empty() || matches_query(&q, "available");
    let mut cards: Vec<Element<'static, UiEvent>> = Vec::new();
    if !query_active || matches_any(&q, &["translation", "service", "connect", "gateway", "provider"]) {
        cards.push(
            container(column![
                text("Translation Service").size(scale::s(14.0)).color(Color::WHITE),
                text("Connect the gateway used by the machine translator. Disconnect removes its API key.")
                    .size(scale::s(11.0))
                    .color(MUTED_FG),
            ].spacing(scale::s(4.0)))
            .padding(scale::s(10.0))
            .style(|_| card_style())
            .into(),
        );
    }
    if show_connected {
        let mut content: Vec<Element<'static, UiEvent>> = Vec::new();
        content.push(
            row![
                crate::icon::lucide(Icon::PlugZap).size(scale::s(14.0)).color(crate::accent::accent()),
                text("Connected").size(scale::s(13.0)).color(Color::WHITE),
                space::horizontal(),
                text(if connected_rows.is_empty() { "—".to_string() } else { format!("{} connected", connected_rows.len()) }).size(scale::s(11.0)).color(MUTED_FG),
            ].align_y(iced::Alignment::Center).spacing(scale::s(6.0)).into()
        );
        content.push(item_separator());
        if connected_rows.is_empty() {
            content.push(
                text(if query_active { format!("No connected providers match “{query}”.") } else { "No connected providers — connect one below.".to_string() })
                    .size(scale::s(11.0)).color(MUTED_FG).into()
            );
        } else {
            let len = connected_rows.len();
            for (idx, el) in connected_rows.into_iter().enumerate() {
                content.push(el);
                if idx + 1 < len { content.push(item_separator()); }
            }
        }
        cards.push(container(column(content).spacing(scale::s(6.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
    }
    if show_recommended {
        let mut content: Vec<Element<'static, UiEvent>> = Vec::new();
        content.push(
            row![
                crate::icon::lucide(Icon::Star).size(scale::s(14.0)).color(crate::accent::accent()),
                text("Recommended").size(scale::s(13.0)).color(Color::WHITE),
                space::horizontal(),
                text(if recommended_rows.is_empty() { "—".to_string() } else { format!("{} recommended", recommended_rows.len()) }).size(scale::s(11.0)).color(MUTED_FG),
            ].align_y(iced::Alignment::Center).spacing(scale::s(6.0)).into()
        );
        content.push(text("Not sure where to start? Try one of these.").size(scale::s(11.0)).color(MUTED_FG).into());
        content.push(item_separator());
        if recommended_rows.is_empty() {
            content.push(text(if query_active { format!("No recommendations match “{query}”.") } else { "All recommended providers connected.".to_string() }).size(scale::s(11.0)).color(MUTED_FG).into());
        } else {
            let len = recommended_rows.len();
            for (idx, el) in recommended_rows.into_iter().enumerate() {
                content.push(el);
                if idx + 1 < len { content.push(item_separator()); }
            }
        }
        cards.push(container(column(content).spacing(scale::s(6.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
    }
    if show_available {
        let mut content: Vec<Element<'static, UiEvent>> = Vec::new();
        content.push(
            row![
                crate::icon::lucide(Icon::Globe).size(scale::s(14.0)).color(MUTED_FG),
                text("Available").size(scale::s(13.0)).color(Color::WHITE),
                space::horizontal(),
                text(format!("{} available", available_rows.len())).size(scale::s(11.0)).color(MUTED_FG),
            ].align_y(iced::Alignment::Center).spacing(scale::s(6.0)).into()
        );
        content.push(item_separator());
        if available_rows.is_empty() {
            content.push(
                text(if query_active { format!("No available providers match “{query}”.") } else { "All providers connected.".to_string() }).size(scale::s(11.0)).color(MUTED_FG).into()
            );
        } else {
            let len = available_rows.len();
            for (idx, el) in available_rows.into_iter().enumerate() {
                content.push(el);
                if idx + 1 < len { content.push(item_separator()); }
            }
        }
        cards.push(container(column(content).spacing(scale::s(6.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
    }
    if matches_any(&q, &["free", "paid", "filter", "models", "manage", "dropdown", "only show free"]) || !query_active {
        let content: Vec<Element<'static, UiEvent>> = vec![
            card_header(Icon::SlidersHorizontal, "Options", None),
            item_separator(),
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
            item_separator(),
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
                    .style(crate::panel::button_style)
                    .on_press(UiEvent::ManageModelsOpen),
            ]
            .spacing(scale::s(6.0))
            .align_y(iced::Alignment::Center)
            .padding([scale::s(4.0), 0.0])
            .into(),
            item_separator(),
            text("Connections are saved to the app's settings file in the system configuration directory.")
                .size(scale::s(11.0))
                .color(MUTED_FG)
                .into(),
        ];
        cards.push(container(column(content).spacing(scale::s(6.0))).padding(scale::s(10.0)).style(|_| card_style()).into());
    }
    cards
}

fn updates_cards<S: UiState + ?Sized>(state: &S, query: &str) -> Vec<Element<'static, UiEvent>> {
    if !matches_any(query, &["updates", "update", "version", "download", "install", "velopack", "github", "release", "startup", "automatic", "auto", "check"]) && !query.trim().is_empty() {
        return Vec::new();
    }
    let current = state.update_current_version();
    let available = state.update_available_version();
    let downloading = state.update_downloading();
    let progress = state.update_progress();
    let ready = state.update_ready();
    let notes = state.update_notes();

    let mut col: Vec<Element<'static, UiEvent>> = Vec::new();
    col.push(card_header(Icon::Download, "Updates", Some("Velopack — GitHub releases dotliie/EasyScanlate-test")));
    let auto_check = easyscanlate_settings::get(|s| s.auto_check_updates);
    col.push(
        column![
            checkbox(auto_check)
                .label("Check for updates on startup")
                .text_size(scale::s(12.0))
                .on_toggle(|v| UiEvent::SettingEdit(SettingEdit::AutoCheckUpdates(v))),
            helper_text("When on, the app checks GitHub releases at startup and shows a popup when an update is found."),
        ].spacing(scale::s(4.0)).into()
    );
    col.push(item_separator());
    col.push(text(format!("Current version: {}", if current.is_empty() { env!("CARGO_PKG_VERSION").to_string() } else { current.clone() })).size(scale::s(11.0)).color(MUTED_FG).into());
    col.push(item_separator());

    if ready {
        col.push(text("Update ready — restart to apply.").size(scale::s(12.0)).color(crate::accent::accent()).into());
        if let Some(v) = available.clone() {
            col.push(text(format!("Ready: v{} → v{}", current, v)).size(scale::s(11.0)).color(MUTED_FG).into());
        }
        if let Some(n) = notes.clone() {
            col.push(container(text(n).size(scale::s(11.0)).color(MUTED_FG)).padding(scale::s(6.0)).style(|_| container::Style{ background: Some(Color::from_rgba8(255,255,255,0.04).into()), border: iced::Border::default().rounded(scale::s(6.0)), ..Default::default() }).into());
        }
        col.push(row![
            button(text("Restart & Update").size(scale::s(11.0))).padding([scale::s(6.0), scale::s(12.0)]).style(crate::panel::button_style).on_press(UiEvent::UpdateApply),
            button(text("Later").size(scale::s(11.0))).padding([scale::s(6.0), scale::s(12.0)]).style(crate::panel::button_style).on_press(UiEvent::UpdateDismiss),
        ].spacing(scale::s(8.0)).into());
        col.push(helper_text("The app will restart and install the update (Velopack — per-user, no admin)."));
    } else if downloading {
        let pct = progress.clamp(0, 100);
        col.push(text(format!("Downloading update {}% — please don't close", pct)).size(scale::s(12.0)).color(crate::accent::accent()).into());
        col.push(
            progress_bar(0.0..=100.0, pct as f32)
                .girth(Length::Fixed(scale::s(6.0)))
                .style(|_theme: &iced::Theme| iced::widget::progress_bar::Style {
                    background: crate::accent::track().into(),
                    bar: crate::accent::accent().into(),
                    border: iced::Border::default().rounded(scale::s(3.0)),
                })
                .into(),
        );
        col.push(text(format!("{}% — Velopack will restart when done", pct)).size(scale::s(11.0)).color(MUTED_FG).into());
    } else if let Some(v) = available.clone() {
        col.push(text(format!("Update available: v{} → v{}", current, v)).size(scale::s(12.0)).color(crate::accent::accent()).into());
        if let Some(n) = notes.clone() {
            col.push(container(text(n).size(scale::s(11.0)).color(MUTED_FG)).padding(scale::s(6.0)).style(|_| container::Style{ background: Some(Color::from_rgba8(255,255,255,0.04).into()), border: iced::Border::default().rounded(scale::s(6.0)), ..Default::default() }).into());
        }
        col.push(row![
            button(text("Download").size(scale::s(11.0))).padding([scale::s(6.0), scale::s(12.0)]).style(crate::panel::button_style).on_press(UiEvent::UpdateDownload),
            button(text("Dismiss").size(scale::s(11.0))).padding([scale::s(6.0), scale::s(12.0)]).style(crate::panel::button_style).on_press(UiEvent::UpdateDismiss),
        ].spacing(scale::s(8.0)).into());
        col.push(helper_text("Downloaded via Velopack (delta if available) from GitHub releases."));
    } else {
        col.push(text("You're up to date.").size(scale::s(12.0)).color(Color::WHITE).into());
        col.push(text(format!("Current: v{}", current)).size(scale::s(11.0)).color(MUTED_FG).into());
        col.push(button(text("Check again").size(scale::s(11.0))).padding([scale::s(6.0), scale::s(12.0)]).style(crate::panel::button_style).on_press(UiEvent::UpdateCheck).into());
        col.push(helper_text("Checks GitHub dotliie/EasyScanlate-test — same endpoint old app used (update.py)."));
    }

    if available.is_none() && !ready && !downloading {
        // also offer check when up-to-date already has button; downloading/ready hide extra check
    } else if !downloading && !ready {
        col.push(space::vertical().height(Length::Fixed(scale::s(6.0))).into());
        col.push(button(text("Check for updates").size(scale::s(11.0))).padding([scale::s(6.0), scale::s(12.0)]).style(crate::panel::button_style).on_press(UiEvent::UpdateCheck).into());
    }

    vec![container(column(col).spacing(scale::s(7.0))).padding(scale::s(10.0)).style(|_| card_style()).into()]
}

fn updates_tab_filtered<S: UiState + ?Sized>(state: &S, query: String) -> Element<'static, UiEvent> {
    let cards = updates_cards(state, &query);
    if cards.is_empty() {
        return scrollable(container(text(format!("No update settings match “{query}”.")).size(scale::s(12.0)).color(MUTED_FG)).padding(scale::s(14.0)).style(|_| card_style())).height(Length::Fill).into();
    }
    scrollable(column(cards).spacing(scale::s(10.0))).height(Length::Fill).into()
}

fn global_search_filtered<S: UiState + ?Sized>(state: &S, query: String) -> Element<'static, UiEvent> {
    let q = query.as_str();
    let mut all: Vec<Element<'static, UiEvent>> = Vec::new();

    let g = general_cards(q);
    if !g.is_empty() {
        all.extend(g);
    }
    let a = appearance_cards(q);
    if !a.is_empty() {
        all.extend(a);
    }
    let o = ocr_cards(q);
    if !o.is_empty() {
        all.extend(o);
    }
    let i = inpaint_cards(q);
    if !i.is_empty() {
        all.extend(i);
    }
    let t = translation_cards(q);
    if !t.is_empty() {
        all.extend(t);
    }
    let u = updates_cards(state, q);
    if !u.is_empty() {
        all.extend(u);
    }

    if all.is_empty() {
        all.push(
            container(column![
                row![
                    crate::icon::lucide(Icon::SearchX).size(scale::s(16.0)).color(MUTED_FG),
                    text(format!("No settings match “{query}”")).size(scale::s(12.0)).color(MUTED_FG),
                ].spacing(scale::s(6.0)).align_y(iced::Alignment::Center),
                text("Try a different term — e.g. font, ocr, inpaint, translation, automation.").size(scale::s(11.0)).color(MUTED_FG),
            ].spacing(scale::s(6.0)))
            .padding(scale::s(14.0))
            .style(|_| card_style())
            .into(),
        );
    }

    scrollable(column(all).spacing(scale::s(10.0))).height(Length::Fill).into()
}

/// The field area of the currently selected tab.
fn tab_fields<S: UiState + ?Sized>(state: &S) -> Element<'static, UiEvent> {
    let query = state.settings_search().to_string();
    if !query.trim().is_empty() {
        return global_search_filtered(state, query);
    }
    match state.settings_tab() {
        SettingsTab::General => general_tab_filtered(query.clone()),
        SettingsTab::Appearance => appearance_tab_filtered(query.clone()),
        SettingsTab::Ocr => ocr_tab_filtered(query.clone()),
        SettingsTab::Inpaint => inpaint_tab_filtered(query.clone()),
        SettingsTab::Translation => translation_tab_filtered(query.clone()),
        SettingsTab::Updates => updates_tab_filtered(state, query.clone()),
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
    let query = state.settings_search().to_string();

    let left = container(
        column![
            // ── Sidebar search — top of sidebar, settings-wide ──────
            column![
                row![
                    crate::icon::lucide(Icon::Search).size(scale::s(12.0)).color(MUTED_FG),
                    text("Search").size(scale::s(11.0)).color(MUTED_FG),
                ].spacing(scale::s(4.0)).align_y(iced::Alignment::Center),
                text_input("Filter settings…", &query)
                    .on_input(UiEvent::SettingsSearch)
                    .padding([scale::s(6.0), scale::s(8.0)])
                    .size(scale::s(12.0))
                    .width(Length::Fill),
                text("Search all settings")
                    .size(scale::s(10.0))
                    .color(MUTED_FG),
            ].spacing(scale::s(4.0)),
            rule::horizontal(1)
                .style(|_| rule::Style {
                    color: Color::from_rgba8(255, 255, 255, 0.08),
                    radius: 0.0.into(),
                    fill_mode: rule::FillMode::Full,
                    snap: true,
                }),
            column![
                tab_button(state, SettingsTab::General, Icon::Settings, "General"),
                tab_button(state, SettingsTab::Appearance, Icon::Palette, "Appearance"),
                tab_button(state, SettingsTab::Ocr, Icon::ScanSearch, "OCR"),
                tab_button(state, SettingsTab::Inpaint, Icon::Brush, "Inpaint"),
                tab_button(state, SettingsTab::Translation, Icon::Languages, "Translation"),
                tab_button(state, SettingsTab::Updates, Icon::Download, "Updates"),
            ]
            .spacing(scale::s(4.0))
            .width(Length::Fill),
            space::vertical().height(Length::Fill),
            text("Click outside to close").size(scale::s(10.0)).color(Color::from_rgba8(255,255,255,0.35)),
        ]
        .spacing(scale::s(10.0))
        .height(Length::Fill)
        .width(Length::Fill),
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

    // 80% centered modal: 1-8-1 split both axes => 8/10 = 80%.
    // The blurred snapshot is cropped to this window rect at capture time and
    // rendered only behind the panel (same cell, stacked under it); the dim
    // outside the panel stays plain dim over the live base.
    let blur_cover: Element<'a, UiEvent> = match state.backdrop_blur() {
        Some(handle) => container(
            image(handle)
                .width(Length::Fill)
                .height(Length::Fill)
                .content_fit(iced::ContentFit::Fill)
                .border_radius(scale::s(8.0)),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into(),
        // No capture yet: the panel cell is just the window on its own.
        None => space::horizontal()
            .width(Length::Fixed(0.0))
            .height(Length::Fixed(0.0))
            .into(),
    };
    let dimmed = container(
        row![
            space::horizontal().width(Length::FillPortion(1)),
            column![
                space::vertical().height(Length::FillPortion(1)),
                container(opaque(stack![blur_cover, window]))
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
