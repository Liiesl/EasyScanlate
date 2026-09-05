//! First-run onboarding wizard: blocking modal that forces model downloads
//! and collects core preferences before the app is usable.
//!
//! All 5 `available` models are mandatory (q1), the wizard is blocking (q2),
//! and preferences from q3 (appearance + automation) are collected here.
//! The wizard can be re-opened via Settings → General → Replay onboarding (q4).

use iced::widget::{button, column, container, progress_bar, row, rule, scrollable, space, stack, text, text_input};
use iced::{Color, Element, Length, Fill as FillLength};
use lucide_icons::Icon;

use crate::event::UiEvent;
use crate::scale;
use crate::panel::PANEL_BG;
use crate::state::UiState;

const MUTED_FG: Color = Color::from_rgb(0.6, 0.6, 0.6);
const CARD_BG: Color = Color::from_rgba8(255, 255, 255, 0.06);
const CARD_BORDER: Color = Color::from_rgba8(255, 255, 255, 0.08);
const ERROR_FG: Color = Color::from_rgb8(255, 110, 110);

fn card_style() -> container::Style {
    container::Style {
        background: Some(CARD_BG.into()),
        border: iced::Border::default().rounded(scale::s(8.0)).color(CARD_BORDER).width(scale::s(1.0)),
        ..Default::default()
    }
}

/// The last four characters of a key, for the "connected" status display.
fn mask_key(key: &str) -> String {
    if key.len() > 8 {
        format!("{}…{}", &key[..6], &key[key.len() - 4..])
    } else {
        "••••".to_string()
    }
}

fn item_separator() -> Element<'static, UiEvent> {
    rule::horizontal(1)
        .style(|_theme| rule::Style {
            color: Color::from_rgba8(255, 255, 255, 0.08),
            radius: 0.0.into(),
            fill_mode: rule::FillMode::Full,
            snap: true,
        })
        .into()
}

/// One step dot.
fn step_dot(active: bool, done: bool) -> Element<'static, UiEvent> {
    let bg = if done {
        crate::accent::accent()
    } else if active {
        crate::accent::accent_translucent(0.5)
    } else {
        Color::from_rgba8(255, 255, 255, 0.12)
    };
    let border = if active { crate::accent::accent() } else { Color::TRANSPARENT };
    container(space::horizontal().width(Length::Fixed(scale::s(1.0))).height(Length::Fixed(scale::s(1.0))))
        .width(Length::Fixed(scale::s(10.0)))
        .height(Length::Fixed(scale::s(10.0)))
        .style(move |_| container::Style {
            background: Some(bg.into()),
            border: iced::Border::default().rounded(scale::s(5.0)).color(border).width(if active { scale::s(1.0) } else { 0.0 }),
            ..Default::default()
        })
        .into()
}

fn step_dots(current: u8) -> Element<'static, UiEvent> {
    // 0 welcome, 1 models, 2 preferences, 3 translation, 4 done — show 5 dots
    let mut r = row![].spacing(scale::s(6.0)).align_y(iced::Alignment::Center);
    for i in 0..5 {
        let done = i < current as usize;
        let active = i == current as usize;
        r = r.push(step_dot(active, done));
    }
    r.into()
}

fn primary_button<'a>(label: &'a str, enabled: bool, msg: Option<UiEvent>) -> Element<'a, UiEvent> {
    let style = move |_: &iced::Theme, status: iced::widget::button::Status| {
        let bg = if !enabled {
            Color::from_rgba8(255, 255, 255, 0.08)
        } else if status == iced::widget::button::Status::Hovered {
            crate::accent::accent_hover()
        } else {
            crate::accent::accent()
        };
        iced::widget::button::Style {
            background: Some(bg.into()),
            border: iced::Border::default().rounded(scale::s(6.0)),
            text_color: if enabled { Color::WHITE } else { MUTED_FG },
            ..Default::default()
        }
    };
    crate::button::with_disabled_cursor(
        button(text(label).size(scale::s(13.0)).center())
            .padding([scale::s(8.0), scale::s(16.0)])
            .style(style)
            .on_press_maybe(if enabled { msg } else { None })
            .into(),
    )
}

fn secondary_button<'a>(label: &'a str, msg: UiEvent) -> Element<'a, UiEvent> {
    button(text(label).size(scale::s(12.0)).center())
        .padding([scale::s(8.0), scale::s(14.0)])
        .style(crate::panel::button_style)
        .on_press(msg)
        .into()
}

// ---------------------------------------------------------------------------
// Inline connect form (mirrored from connect.rs but rendered inside card)
// ---------------------------------------------------------------------------

fn inline_form(modal: &crate::connect::ConnectModal) -> Element<'static, UiEvent> {
    let is_local = crate::translation::is_local(&modal.provider_id);
    let mut fields: Vec<Element<'static, UiEvent>> = Vec::new();
    if !is_local {
        fields.push(text("API key").size(scale::s(11.0)).color(MUTED_FG).into());
        let api_key = modal.api_key.clone();
        fields.push(
            text_input("sk-…", &api_key)
                .on_input(UiEvent::ConnectModalKey)
                .secure(true)
                .padding(scale::s(4.0))
                .size(scale::s(12.0))
                .width(FillLength)
                .into(),
        );
    }
    if modal.is_custom || is_local {
        let placeholder = if is_local {
            match modal.provider_id.as_str() {
                crate::translation::LOCAL_OLLAMA => "http://localhost:11434",
                crate::translation::LOCAL_VLLM => "http://localhost:8000/v1",
                crate::translation::LOCAL_LLAMA_CPP => "http://localhost:8080/v1",
                _ => "http://localhost:11434",
            }
        } else {
            "http://localhost:11434/v1"
        };
        let base = modal.base_url.clone();
        fields.push(text("Base URL").size(scale::s(11.0)).color(MUTED_FG).into());
        fields.push(
            text_input(placeholder, &base)
                .on_input(UiEvent::ConnectModalBaseUrl)
                .padding(scale::s(4.0))
                .size(scale::s(12.0))
                .width(FillLength)
                .into(),
        );
        if is_local {
            fields.push(
                text("Models are discovered automatically from the endpoint.")
                    .size(scale::s(11.0))
                    .color(MUTED_FG)
                    .into(),
            );
        }
    }
    if modal.is_custom {
        let model = modal.model.clone();
        fields.push(text("Model").size(scale::s(11.0)).color(MUTED_FG).into());
        fields.push(
            text_input("llama-3.1-8b", &model)
                .on_input(UiEvent::ConnectModalModel)
                .padding(scale::s(4.0))
                .size(scale::s(12.0))
                .width(FillLength)
                .into(),
        );
    }
    if let Some(error) = &modal.error {
        let e = error.clone();
        fields.push(text(e).size(scale::s(11.0)).color(ERROR_FG).into());
    }
    let actions: Element<'static, UiEvent> = row![
        space::horizontal(),
        button(text("Cancel").size(scale::s(11.0)))
            .padding([scale::s(4.0), scale::s(10.0)])
            .style(crate::panel::button_style)
            .on_press(UiEvent::ConnectModalCancel),
        button(text("Connect").size(scale::s(11.0)).color(Color::WHITE))
            .padding([scale::s(4.0), scale::s(10.0)])
            .style(|_, status| {
                let bg = if status == iced::widget::button::Status::Hovered {
                    crate::accent::accent_hover()
                } else {
                    crate::accent::accent()
                };
                iced::widget::button::Style {
                    background: Some(bg.into()),
                    border: iced::Border::default().rounded(scale::s(6.0)),
                    text_color: Color::WHITE,
                    ..Default::default()
                }
            })
            .on_press(UiEvent::ConnectModalSubmit),
    ]
    .spacing(scale::s(6.0))
    .align_y(iced::Alignment::Center)
    .into();
    fields.push(actions);
    column(fields).spacing(scale::s(6.0)).into()
}

fn onboarding_provider_row(
    provider: &crate::translation::Provider,
    connected: Option<easyscanlate_settings::Connection>,
    modal: Option<&crate::connect::ConnectModal>,
) -> Element<'static, UiEvent> {
    let is_expanded = modal.is_some_and(|m| m.provider_id == provider.id);
    let is_local = crate::translation::is_local(&provider.id);
    let status = connected
        .as_ref()
        .map(|c| {
            if is_local {
                let base = c.base_url.as_deref().unwrap_or(provider.api.as_str());
                if base.is_empty() {
                    "Connected · Local".to_string()
                } else {
                    format!("Connected · Local · {base}")
                }
            } else {
                format!("Connected · {}", mask_key(&c.api_key))
            }
        })
        .unwrap_or_else(|| "Not connected".to_string());
    let dot_color = if status.starts_with("Connected") { crate::accent::accent() } else { MUTED_FG };
    let dot = container(space::horizontal().width(Length::Fixed(scale::s(6.0))).height(Length::Fixed(scale::s(6.0))))
        .style(move |_| container::Style {
            background: Some(dot_color.into()),
            border: iced::Border::default().rounded(scale::s(3.0)),
            ..Default::default()
        });
    // Main row
    let main_row: Element<'static, UiEvent> = if is_expanded {
        row![
            dot,
            column![
                text(provider.name.clone()).size(scale::s(12.0)).color(Color::WHITE),
                text(status.clone()).size(scale::s(11.0)).color(MUTED_FG),
            ].spacing(scale::s(1.0)).width(FillLength),
            button(text("Close").size(scale::s(11.0)))
                .padding([scale::s(3.0), scale::s(8.0)])
                .style(crate::panel::button_style)
                .on_press(UiEvent::ConnectModalCancel),
        ]
        .spacing(scale::s(8.0))
        .align_y(iced::Alignment::Center)
        .padding([scale::s(5.0), 0.0])
        .into()
    } else {
        let btn: Element<'static, UiEvent> = match connected {
            Some(_) => button(text("Disconnect").size(scale::s(11.0)))
                .padding([scale::s(3.0), scale::s(8.0)])
                .style(crate::panel::button_style)
                .on_press(UiEvent::TranslateDisconnect(provider.id.clone()))
                .into(),
            None => button(text("Connect").size(scale::s(11.0)))
                .padding([scale::s(3.0), scale::s(8.0)])
                .style(crate::panel::button_style)
                .on_press(UiEvent::TranslateConnect(provider.id.clone()))
                .into(),
        };
        row![
            dot,
            column![
                text(provider.name.clone()).size(scale::s(12.0)).color(Color::WHITE),
                text(status.clone()).size(scale::s(11.0)).color(MUTED_FG),
            ].spacing(scale::s(1.0)).width(FillLength),
            btn,
        ]
        .spacing(scale::s(8.0))
        .align_y(iced::Alignment::Center)
        .padding([scale::s(5.0), 0.0])
        .into()
    };
    if is_expanded {
        if let Some(m) = modal {
            let form: Element<'static, UiEvent> = container(inline_form(m))
                .padding(scale::s(8.0))
                .style(|_| container::Style {
                    background: Some(Color::from_rgba8(255, 255, 255, 0.04).into()),
                    border: iced::Border::default().rounded(scale::s(6.0)).color(Color::from_rgba8(255,255,255,0.06)).width(scale::s(1.0)),
                    ..Default::default()
                })
                .into();
            column![main_row, form].spacing(scale::s(6.0)).into()
        } else {
            main_row
        }
    } else {
        main_row
    }
}

fn onboarding_custom_row(
    id: &'static str,
    label: &'static str,
    connected: Option<easyscanlate_settings::Connection>,
    modal: Option<&crate::connect::ConnectModal>,
) -> Element<'static, UiEvent> {
    let is_expanded = modal.is_some_and(|m| m.provider_id == id);
    let status = connected
        .as_ref()
        .map(|c| format!("Connected · {}", mask_key(&c.api_key)))
        .unwrap_or_else(|| "Not connected".to_string());
    let dot_color = if status.starts_with("Connected") { crate::accent::accent() } else { MUTED_FG };
    let dot = container(space::horizontal().width(Length::Fixed(scale::s(6.0))).height(Length::Fixed(scale::s(6.0))))
        .style(move |_| container::Style {
            background: Some(dot_color.into()),
            border: iced::Border::default().rounded(scale::s(3.0)),
            ..Default::default()
        });
    let main_row: Element<'static, UiEvent> = if is_expanded {
        row![
            dot,
            column![
                text(label).size(scale::s(12.0)).color(Color::WHITE),
                text(status.clone()).size(scale::s(11.0)).color(MUTED_FG),
            ].spacing(scale::s(1.0)).width(FillLength),
            button(text("Close").size(scale::s(11.0)))
                .padding([scale::s(3.0), scale::s(8.0)])
                .style(crate::panel::button_style)
                .on_press(UiEvent::ConnectModalCancel),
        ]
        .spacing(scale::s(8.0))
        .align_y(iced::Alignment::Center)
        .padding([scale::s(5.0), 0.0])
        .into()
    } else {
        let btn: Element<'static, UiEvent> = match connected {
            Some(_) => button(text("Disconnect").size(scale::s(11.0)))
                .padding([scale::s(3.0), scale::s(8.0)])
                .style(crate::panel::button_style)
                .on_press(UiEvent::TranslateDisconnect(id.to_string()))
                .into(),
            None => button(text("Connect…").size(scale::s(11.0)))
                .padding([scale::s(3.0), scale::s(8.0)])
                .style(crate::panel::button_style)
                .on_press(UiEvent::TranslateConnect(id.to_string()))
                .into(),
        };
        row![
            dot,
            column![
                text(label).size(scale::s(12.0)).color(Color::WHITE),
                text(status.clone()).size(scale::s(11.0)).color(MUTED_FG),
            ].spacing(scale::s(1.0)).width(FillLength),
            btn,
        ]
        .spacing(scale::s(8.0))
        .align_y(iced::Alignment::Center)
        .padding([scale::s(5.0), 0.0])
        .into()
    };
    if is_expanded {
        if let Some(m) = modal {
            let form: Element<'static, UiEvent> = container(inline_form(m))
                .padding(scale::s(8.0))
                .style(|_| container::Style {
                    background: Some(Color::from_rgba8(255, 255, 255, 0.04).into()),
                    border: iced::Border::default().rounded(scale::s(6.0)).color(Color::from_rgba8(255,255,255,0.06)).width(scale::s(1.0)),
                    ..Default::default()
                })
                .into();
            column![main_row, form].spacing(scale::s(6.0)).into()
        } else {
            main_row
        }
    } else {
        main_row
    }
}

fn onboarding_recommended_row(
    provider: &crate::translation::Provider,
    info: &crate::translation::RecommendedInfo,
    modal: Option<&crate::connect::ConnectModal>,
) -> Element<'static, UiEvent> {
    let is_expanded = modal.is_some_and(|m| m.provider_id == provider.id);
    let badge = container(text("Recommended").size(scale::s(9.0)).color(crate::accent::accent()))
        .padding([scale::s(2.0), scale::s(6.0)])
        .style(|_| container::Style {
            background: Some(crate::accent::accent_translucent(0.15).into()),
            border: iced::Border::default().rounded(scale::s(4.0)),
            ..Default::default()
        });
    let main_row: Element<'static, UiEvent> = if is_expanded {
        column![
            row![
                column![
                    row![
                        text(provider.name.clone()).size(scale::s(12.0)).color(Color::WHITE),
                        badge,
                    ].spacing(scale::s(6.0)).align_y(iced::Alignment::Center),
                    text(info.description).size(scale::s(11.0)).color(MUTED_FG),
                ].spacing(scale::s(2.0)).width(FillLength),
                button(text("Close").size(scale::s(11.0)))
                    .padding([scale::s(3.0), scale::s(8.0)])
                    .style(crate::panel::button_style)
                    .on_press(UiEvent::ConnectModalCancel),
            ].spacing(scale::s(6.0)).align_y(iced::Alignment::Center),
            row![
                crate::icon::lucide(Icon::Info).size(scale::s(12.0)).color(MUTED_FG),
                text("Not connected").size(scale::s(11.0)).color(MUTED_FG),
            ].spacing(scale::s(4.0)).align_y(iced::Alignment::Center),
        ].spacing(scale::s(6.0)).padding([scale::s(5.0), 0.0]).into()
    } else {
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
                    text(provider.name.clone()).size(scale::s(12.0)).color(Color::WHITE),
                    badge,
                ].spacing(scale::s(6.0)).align_y(iced::Alignment::Center),
                text(info.description).size(scale::s(11.0)).color(MUTED_FG),
                text("Not connected").size(scale::s(11.0)).color(MUTED_FG),
            ].spacing(scale::s(2.0)).width(FillLength),
            docs_button,
            connect_button,
        ].spacing(scale::s(6.0)).align_y(iced::Alignment::Center).padding([scale::s(5.0), 0.0]).into()
    };
    if is_expanded {
        if let Some(m) = modal {
            let form: Element<'static, UiEvent> = container(inline_form(m))
                .padding(scale::s(8.0))
                .style(|_| container::Style {
                    background: Some(Color::from_rgba8(255, 255, 255, 0.04).into()),
                    border: iced::Border::default().rounded(scale::s(6.0)).color(Color::from_rgba8(255,255,255,0.06)).width(scale::s(1.0)),
                    ..Default::default()
                })
                .into();
            column![main_row, form].spacing(scale::s(6.0)).into()
        } else {
            main_row
        }
    } else {
        main_row
    }
}

// ---------------------------------------------------------------------------
// Step views
// ---------------------------------------------------------------------------

fn welcome_step() -> Element<'static, UiEvent> {
    column![
        crate::icon::lucide(Icon::Sparkles).size(scale::s(28.0)).color(crate::accent::accent()),
        text("Welcome to EasyScanlate").size(scale::s(22.0)).color(Color::WHITE),
        text("EasyScanlate — manga / manhwa OCR, translation and inpainting.")
            .size(scale::s(12.0))
            .color(MUTED_FG),
        container(
            column![
                row![
                    crate::icon::lucide(Icon::Download).size(scale::s(14.0)).color(crate::accent::accent()),
                    text("Downloads required models (OCR, styling, segmentation, inpainting)").size(scale::s(12.0)).color(Color::WHITE),
                ].spacing(scale::s(8.0)).align_y(iced::Alignment::Center),
                row![
                    crate::icon::lucide(Icon::Palette).size(scale::s(14.0)).color(crate::accent::accent()),
                    text("Pick appearance & automation preferences").size(scale::s(12.0)).color(Color::WHITE),
                ].spacing(scale::s(8.0)).align_y(iced::Alignment::Center),
                row![
                    crate::icon::lucide(Icon::Languages).size(scale::s(14.0)).color(crate::accent::accent()),
                    text("Connect a translation service (optional, can skip)").size(scale::s(12.0)).color(Color::WHITE),
                ].spacing(scale::s(8.0)).align_y(iced::Alignment::Center),
            ].spacing(scale::s(8.0))
        )
        .padding(scale::s(12.0))
        .style(|_| card_style())
        .width(FillLength),
        text("All 5 models are mandatory for full features. The wizard is blocking until downloads finish.")
            .size(scale::s(11.0))
            .color(MUTED_FG),
    ]
    .spacing(scale::s(14.0))
    .align_x(iced::Alignment::Center)
    .into()
}

fn models_step<S: UiState + ?Sized>(state: &S) -> Element<'static, UiEvent> {
    let statuses = state.onboarding_models();
    let overall = state.onboarding_overall_progress();
    let downloading = state.onboarding_downloading();
    let has_error = statuses.iter().any(|(_, _, s)| matches!(s, crate::state::ModelDownloadStatus::Failed(_)));
    let all_done = statuses.iter().all(|(_, _, s)| matches!(s, crate::state::ModelDownloadStatus::Done));

    let header = column![
        text("Download models").size(scale::s(16.0)).color(Color::WHITE),
        text("All models are mandatory. Downloads are resumable (fast-down, 16 threads).")
            .size(scale::s(11.0))
            .color(MUTED_FG),
        row![
            text(format!("Overall {}%", (overall * 100.0).round() as u32)).size(scale::s(11.0)).color(MUTED_FG),
            progress_bar(0.0..=1.0, overall)
                .girth(Length::Fixed(scale::s(6.0)))
                .style(|_| iced::widget::progress_bar::Style {
                    background: crate::accent::track().into(),
                    bar: crate::accent::accent().into(),
                    border: iced::Border::default().rounded(scale::s(3.0)),
                }),
        ].spacing(scale::s(8.0)).align_y(iced::Alignment::Center),
    ].spacing(scale::s(6.0));

    let mut list = column![].spacing(scale::s(6.0));
    for (id, desc, status) in statuses {
        let is_failed = matches!(&status, crate::state::ModelDownloadStatus::Failed(_));
        let (pct, label, action) = match &status {
            crate::state::ModelDownloadStatus::NotStarted => (0.0, "Not started".to_string(), None),
            crate::state::ModelDownloadStatus::Downloading { percent, downloaded, total } => {
                let pct = percent / 100.0;
                let label = if *total > 0 {
                    format!(
                        "{:.0}% ({:.1}/{:.1} MB)",
                        percent,
                        *downloaded as f64 / 1_000_000.0,
                        *total as f64 / 1_000_000.0
                    )
                } else if *downloaded > 0 {
                    format!("{:.0}% ({:.1} MB)", percent, *downloaded as f64 / 1_000_000.0)
                } else {
                    format!("{:.0}%", percent)
                };
                (pct, label, None)
            }
            crate::state::ModelDownloadStatus::Done => (1.0, "Ready".to_string(), None),
            crate::state::ModelDownloadStatus::Failed(msg) => (0.0, format!("Failed: {}", msg), Some(id.clone())),
        };
        let bar: Element<'static, UiEvent> = progress_bar(0.0..=1.0, pct)
            .girth(Length::Fixed(scale::s(5.0)))
            .style(move |_| iced::widget::progress_bar::Style {
                background: crate::accent::track().into(),
                bar: if is_failed { ERROR_FG.into() } else { crate::accent::accent().into() },
                border: iced::Border::default().rounded(scale::s(2.0)),
            })
            .into();
        let retry_btn: Element<'static, UiEvent> = if let Some(mid) = action {
            button(text("Retry").size(scale::s(11.0)))
                .padding([scale::s(3.0), scale::s(8.0)])
                .style(crate::panel::button_style)
                .on_press(UiEvent::OnboardingRetry(mid))
                .into()
        } else {
            space::horizontal().width(Length::Fixed(0.0)).into()
        };
        let is_done = matches!(&status, crate::state::ModelDownloadStatus::Done);
        let row_el: Element<'static, UiEvent> = container(
            column![
                row![
                    text(id.clone()).size(scale::s(12.0)).color(Color::WHITE).width(Length::Fixed(scale::s(120.0))),
                    text(desc.clone()).size(scale::s(11.0)).color(MUTED_FG).width(FillLength),
                    text(label).size(scale::s(11.0)).color(if is_done { crate::accent::accent() } else { MUTED_FG }),
                    retry_btn,
                ].spacing(scale::s(8.0)).align_y(iced::Alignment::Center),
                bar,
            ].spacing(scale::s(4.0))
        )
        .padding(scale::s(8.0))
        .style(|_| card_style())
        .into();
        list = list.push(row_el);
    }

    let download_btn: Element<'static, UiEvent> = if all_done {
        container(
            row![
                crate::icon::lucide(Icon::Check).size(scale::s(14.0)).color(crate::accent::accent()),
                text("All models ready").size(scale::s(12.0)).color(crate::accent::accent()),
            ].spacing(scale::s(6.0)).align_y(iced::Alignment::Center)
        ).padding(scale::s(8.0)).into()
    } else if downloading {
        row![
            crate::icon::lucide(Icon::RefreshCw).size(scale::s(14.0)).color(crate::accent::accent()),
            text("Downloading…").size(scale::s(12.0)).color(MUTED_FG),
            text("(you can keep the app open; progress is resumable)").size(scale::s(11.0)).color(MUTED_FG),
        ].spacing(scale::s(8.0)).align_y(iced::Alignment::Center).into()
    } else {
        button(
            row![
                crate::icon::lucide(Icon::Download).size(scale::s(14.0)).color(Color::WHITE),
                text(if has_error { "Retry failed" } else { "Download all models" }).size(scale::s(13.0)).color(Color::WHITE),
            ].spacing(scale::s(6.0)).align_y(iced::Alignment::Center)
        )
        .padding([scale::s(8.0), scale::s(16.0)])
        .style(|_, s| {
            let bg = if s == iced::widget::button::Status::Hovered { crate::accent::accent_hover() } else { crate::accent::accent() };
            iced::widget::button::Style { background: Some(bg.into()), border: iced::Border::default().rounded(scale::s(6.0)), text_color: Color::WHITE, ..Default::default() }
        })
        .on_press(UiEvent::OnboardingDownloadAll)
        .into()
    };

    let scroll = scrollable(column![header, list, download_btn].spacing(scale::s(10.0))).height(Length::Fill);

    scroll.into()
}

fn preferences_step() -> Element<'static, UiEvent> {
    // Reuse settings controls but simplified: write-through via easyscanlate_settings directly
    // Appearance + Automation cards (subset of settings.rs)
    let is_dark = easyscanlate_settings::get(|s| s.aurora_is_dark);
    let font_size = easyscanlate_settings::get(|s| s.ui_font_size);
    let (auto_style, auto_sfx, auto_inpaint) = easyscanlate_settings::get(|s| (s.auto_style_detect, s.auto_sfx_filter, s.auto_inpaint));

    let appearance: Element<'static, UiEvent> = container(
        column![
            row![
                crate::icon::lucide(Icon::Palette).size(scale::s(14.0)).color(crate::accent::accent()),
                text("Appearance").size(scale::s(13.0)).color(Color::WHITE),
            ].spacing(scale::s(6.0)).align_y(iced::Alignment::Center),
            row![
                text("Theme").size(scale::s(12.0)).color(Color::WHITE).width(Length::Fixed(scale::s(90.0))),
                button(text(if is_dark { "Dark" } else { "Light" }).size(scale::s(12.0)))
                    .padding([scale::s(4.0), scale::s(10.0)])
                    .style(crate::panel::button_style)
                    .on_press(UiEvent::OnboardingToggleTheme),
                space::horizontal().width(Length::Fixed(scale::s(12.0))),
                text("Font size").size(scale::s(12.0)).color(Color::WHITE).width(Length::Fixed(scale::s(90.0))),
                button(text("-").size(scale::s(14.0))).padding(scale::s(4.0)).style(crate::panel::button_style).on_press(UiEvent::OnboardingFontSize(false)),
                text(font_size.to_string()).size(scale::s(12.0)).color(Color::WHITE).width(Length::Fixed(scale::s(30.0))),
                button(text("+").size(scale::s(14.0))).padding(scale::s(4.0)).style(crate::panel::button_style).on_press(UiEvent::OnboardingFontSize(true)),
            ].spacing(scale::s(8.0)).align_y(iced::Alignment::Center),
            text("Changes apply instantly; you can fine-tune in Settings → Appearance.").size(scale::s(11.0)).color(MUTED_FG),
        ].spacing(scale::s(8.0))
    ).padding(scale::s(10.0)).style(|_| card_style()).into();

    let automation = {
        let mk_row = |label: &'static str, sub: &'static str, value: bool, msg: UiEvent| -> Element<'static, UiEvent> {
            let togg: Element<'static, UiEvent> = iced::widget::toggler(value)
                .size(scale::s(20.0))
                .style(crate::toggler_style::style)
                .on_toggle(move |_| msg.clone())
                .into();
            row![
                column![
                    text(label).size(scale::s(12.0)).color(Color::WHITE),
                    text(sub).size(scale::s(11.0)).color(MUTED_FG),
                ].spacing(scale::s(2.0)).width(FillLength),
                togg,
            ].spacing(scale::s(8.0)).align_y(iced::Alignment::Center).padding([scale::s(4.0), 0.0]).into()
        };
        let col: Element<'static, UiEvent> = column![
            row![crate::icon::lucide(Icon::Sparkles).size(scale::s(14.0)).color(crate::accent::accent()), text("Automation").size(scale::s(13.0)).color(Color::WHITE)].spacing(scale::s(6.0)).align_y(iced::Alignment::Center),
            mk_row("Auto-detect entry styles", "Classify OCR entries via ONNX styling model", auto_style, UiEvent::OnboardingToggleAutoStyle),
            mk_row("Auto-filter SFX", "Remove SFX outside balloons via segmentation", auto_sfx, UiEvent::OnboardingToggleAutoSfx),
            mk_row("Auto inpaint (bg-aware)", "Gradient/artwork bubbles → transparent + inpaint", auto_inpaint, UiEvent::OnboardingToggleAutoInpaint),
            text("You can change these anytime in Settings → General / Inpaint.").size(scale::s(11.0)).color(MUTED_FG),
        ].spacing(scale::s(6.0)).into();
        let automation: Element<'static, UiEvent> = container(col).padding(scale::s(10.0)).style(|_| card_style()).into();
        automation
    };

    scrollable(column![appearance, automation].spacing(scale::s(10.0))).height(Length::Fill).into()
}

fn translation_step<S: UiState + ?Sized>(state: &S) -> Element<'static, UiEvent> {
    // Mirrored from Settings → Translation but only connection-related (no free-models toggle, no Manage Models)
    // Inline connect form baked into onboarding (no Settings modal).
    let connections = easyscanlate_settings::get(|s| s.connections.clone());
    let modal_opt = state.connect_modal().cloned();

    // Intro
    let intro: Element<'static, UiEvent> = column![
        text("Translation service").size(scale::s(14.0)).color(Color::WHITE),
        text("Connect an LLM gateway for machine translation (optional). Inline setup — no Settings popup. You can Skip for now and connect later in Settings → Translation.")
            .size(scale::s(11.0)).color(MUTED_FG),
    ].spacing(scale::s(4.0)).into();

    // Partition: Connected, Recommended, Available (deduped)
    use std::collections::HashSet;
    let mut connected_ids: HashSet<String> = HashSet::new();
    for k in connections.keys() { connected_ids.insert(k.clone()); }

    // Connected card rows
    let mut connected_rows: Vec<Element<'static, UiEvent>> = Vec::new();
    for provider in crate::translation::SUPPORTED_PROVIDERS.iter() {
        if let Some(conn) = connections.get(&provider.id).cloned() {
            connected_rows.push(onboarding_provider_row(provider, Some(conn), modal_opt.as_ref()));
        }
    }
    for (id, label) in [(crate::translation::CUSTOM_OPENAI, "OpenAI-compatible"), (crate::translation::CUSTOM_ANTHROPIC, "Anthropic-compatible")] {
        if let Some(conn) = connections.get(id).cloned() {
            connected_rows.push(onboarding_custom_row(id, label, Some(conn), modal_opt.as_ref()));
        }
    }

    let recommended_set: HashSet<&str> = crate::translation::RECOMMENDED.iter().map(|r| r.id).collect();
    let mut recommended_rows: Vec<Element<'static, UiEvent>> = Vec::new();
    for info in crate::translation::RECOMMENDED.iter() {
        if connections.contains_key(info.id) { continue; }
        if let Some(provider) = crate::translation::catalog_provider(info.id) {
            recommended_rows.push(onboarding_recommended_row(provider, info, modal_opt.as_ref()));
        }
    }

    let mut available_rows: Vec<Element<'static, UiEvent>> = Vec::new();
    for provider in crate::translation::SUPPORTED_PROVIDERS.iter() {
        if connections.contains_key(&provider.id) { continue; }
        if recommended_set.contains(provider.id.as_str()) { continue; }
        available_rows.push(onboarding_provider_row(provider, None, modal_opt.as_ref()));
    }
    for (id, label) in [(crate::translation::CUSTOM_OPENAI, "OpenAI-compatible"), (crate::translation::CUSTOM_ANTHROPIC, "Anthropic-compatible")] {
        if connections.contains_key(id) { continue; }
        available_rows.push(onboarding_custom_row(id, label, None, modal_opt.as_ref()));
    }

    // Build cards with interleaved separators for clarity
    let connected_card: Element<'static, UiEvent> = {
        let mut col: Vec<Element<'static, UiEvent>> = Vec::new();
        col.push(
            row![
                crate::icon::lucide(Icon::PlugZap).size(scale::s(14.0)).color(crate::accent::accent()),
                text("Connected").size(scale::s(13.0)).color(Color::WHITE),
                space::horizontal(),
                text(if connected_rows.is_empty() { "—".to_string() } else { format!("{} connected", connected_rows.len()) }).size(scale::s(11.0)).color(MUTED_FG),
            ].align_y(iced::Alignment::Center).spacing(scale::s(6.0)).into()
        );
        col.push(item_separator());
        if connected_rows.is_empty() {
            col.push(text("No connected providers — connect one below.").size(scale::s(11.0)).color(MUTED_FG).into());
        } else {
            for el in connected_rows {
                col.push(el);
            }
        }
        // To properly insert separators, rebuild: we need len. Simplify: if we have at least 2 rows, add separators via interleaving in a second pass
        // Instead, we will keep as is and rely on row padding; separators already between cards are enough for minimal inline. Keep simple.
        container(column(col).spacing(scale::s(6.0))).padding(scale::s(10.0)).style(|_| card_style()).into()
    };
    // For connected_rows separator handling: we skipped interleaving for brevity; we will instead build with explicit loop that adds separator after each except last using length stored earlier
    // To avoid complexity, we accept no interleaved separators inside cards for now — outer card_style provides grouping.

    // Instead build recommended and available cards with same pattern but ensure we display properly
    let recommended_card: Option<Element<'static, UiEvent>> = if recommended_rows.is_empty() {
        None
    } else {
        let len = recommended_rows.len();
        let mut col: Vec<Element<'static, UiEvent>> = Vec::new();
        col.push(
            row![
                crate::icon::lucide(Icon::Star).size(scale::s(14.0)).color(crate::accent::accent()),
                text("Recommended").size(scale::s(13.0)).color(Color::WHITE),
                space::horizontal(),
                text(format!("{} recommended", len)).size(scale::s(11.0)).color(MUTED_FG),
            ].align_y(iced::Alignment::Center).spacing(scale::s(6.0)).into()
        );
        col.push(text("Not sure where to start? Try one of these.").size(scale::s(11.0)).color(MUTED_FG).into());
        col.push(item_separator());
        for el in recommended_rows {
            col.push(el);
        }
        Some(container(column(col).spacing(scale::s(6.0))).padding(scale::s(10.0)).style(|_| card_style()).into())
    };

    let available_card: Element<'static, UiEvent> = {
        let len = available_rows.len();
        let mut col: Vec<Element<'static, UiEvent>> = Vec::new();
        col.push(
            row![
                crate::icon::lucide(Icon::Globe).size(scale::s(14.0)).color(MUTED_FG),
                text("Available").size(scale::s(13.0)).color(Color::WHITE),
                space::horizontal(),
                text(format!("{} available", len)).size(scale::s(11.0)).color(MUTED_FG),
            ].align_y(iced::Alignment::Center).spacing(scale::s(6.0)).into()
        );
        col.push(item_separator());
        if available_rows.is_empty() {
            col.push(text("All providers connected.").size(scale::s(11.0)).color(MUTED_FG).into());
        } else {
            for el in available_rows {
                col.push(el);
            }
        }
        container(column(col).spacing(scale::s(6.0))).padding(scale::s(10.0)).style(|_| card_style()).into()
    };

    let tip: Element<'static, UiEvent> = container(
        column![
            text("Tip").size(scale::s(12.0)).color(Color::WHITE),
            text("You need an API key for cloud providers, or a local endpoint (Ollama / vLLM) running at http://localhost:11434. Local providers need only a Base URL, no API key.").size(scale::s(11.0)).color(MUTED_FG),
        ].spacing(scale::s(4.0))
    ).padding(scale::s(10.0)).style(|_| card_style()).into();

    let mut content: Vec<Element<'static, UiEvent>> = Vec::new();
    content.push(intro);
    content.push(connected_card);
    if let Some(card) = recommended_card { content.push(card); }
    content.push(available_card);
    content.push(tip);
    scrollable(column(content).spacing(scale::s(10.0))).height(Length::Fill).into()
}

fn done_step() -> Element<'static, UiEvent> {
    column![
        crate::icon::lucide(Icon::Sparkles).size(scale::s(28.0)).color(crate::accent::accent()),
        text("You're all set!").size(scale::s(20.0)).color(Color::WHITE),
        text("Models are ready, preferences saved. You can replay this wizard from Settings → General.").size(scale::s(12.0)).color(MUTED_FG),
        container(
            column![
                text("Next steps").size(scale::s(12.0)).color(Color::WHITE),
                text("• Create a new project or open a .mmtl").size(scale::s(11.0)).color(MUTED_FG),
                text("• Fine-tune in Settings (appearance, OCR, inpaint)").size(scale::s(11.0)).color(MUTED_FG),
            ].spacing(scale::s(4.0))
        ).padding(scale::s(10.0)).style(|_| card_style()).width(FillLength),
    ].spacing(scale::s(12.0)).align_x(iced::Alignment::Center).into()
}

fn build_card_content<S: UiState + ?Sized>(state: &S, step: u8) -> (Element<'static, UiEvent>, bool, &'static str, UiEvent) {
    let all_done = state.onboarding_all_done();
    let content: Element<'static, UiEvent> = match step {
        0 => welcome_step(),
        1 => models_step(state),
        2 => preferences_step(),
        3 => translation_step(state),
        4 => done_step(),
        _ => welcome_step(),
    };
    let can_next = match step {
        1 => all_done,
        _ => true,
    };
    let next_label: &'static str = match step {
        0 => "Get started",
        1 => "Next",
        2 => "Next",
        3 => "Next",
        4 => "Finish",
        _ => "Next",
    };
    let next_msg = match step {
        4 => UiEvent::OnboardingFinish,
        _ => UiEvent::OnboardingNext,
    };
    (content, can_next, next_label, next_msg)
}

fn build_wizard_card<S: UiState + ?Sized>(state: &S) -> Element<'static, UiEvent> {
    let step = state.onboarding_step();
    let all_done = state.onboarding_all_done();
    let (content, can_next, next_label, next_msg) = build_card_content(state, step);
    let can_back = step > 0 && step < 4;
    let nav = if step == 3 {
        row![
            if can_back {
                secondary_button("Back", UiEvent::OnboardingBack)
            } else {
                container(space::horizontal().width(Length::Fixed(0.0))).into()
            },
            space::horizontal(),
            secondary_button("Skip for now", UiEvent::OnboardingSkipTranslation),
            primary_button(next_label, can_next, Some(next_msg)),
        ]
        .spacing(scale::s(8.0))
        .align_y(iced::Alignment::Center)
    } else {
        row![
            if can_back {
                secondary_button("Back", UiEvent::OnboardingBack)
            } else {
                container(space::horizontal().width(Length::Fixed(0.0))).into()
            },
            space::horizontal(),
            if step == 1 && !all_done {
                {
                    let hint: Element<'static, UiEvent> = container(text("Download all models to continue").size(scale::s(11.0)).color(ERROR_FG)).padding(scale::s(4.0)).into();
                    hint
                }
            } else {
                space::horizontal().width(Length::Fixed(0.0)).into()
            },
            primary_button(next_label, can_next, Some(next_msg)),
        ]
        .spacing(scale::s(8.0))
        .align_y(iced::Alignment::Center)
    };

    let card = container(
        column![
            row![
                text("Setup").size(scale::s(13.0)).color(MUTED_FG),
                space::horizontal(),
                step_dots(step),
                space::horizontal(),
                text(format!("{}/5", step + 1)).size(scale::s(11.0)).color(MUTED_FG),
            ].spacing(scale::s(8.0)).align_y(iced::Alignment::Center),
            container(content).height(Length::Fill).width(FillLength),
            nav,
        ]
        .spacing(scale::s(12.0))
        .height(Length::Fill),
    )
    .width(Length::Fixed(scale::s(560.0)))
    .height(Length::Fixed(scale::s(520.0)))
    .padding(scale::s(16.0))
    .style(|_| container::Style {
        background: Some(PANEL_BG.into()),
        border: iced::Border::default().rounded(scale::s(12.0)),
        ..Default::default()
    });
    card.into()
}

/// Full-page onboarding (like Home/Editor page) — centered card filling the window.
/// Used as a dedicated page in `src/app/view.rs` when `onboarding.is_some()`.
pub fn view_page<'a, S: UiState + ?Sized>(state: &'a S) -> Element<'a, UiEvent> {
    let card: Element<'static, UiEvent> = build_wizard_card(state);
    let centered = container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);
    // Page outer container with padding like Home, fills window
    container(centered)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(scale::s(16.0))
        .into()
}

/// The blocking onboarding modal. `base` is dimmed underneath; the wizard is centered.
pub fn view<'a, S: UiState + ?Sized>(state: &'a S, base: Element<'a, UiEvent>) -> Element<'a, UiEvent> {
    let step = state.onboarding_step();
    let all_done = state.onboarding_all_done();

    let content: Element<'static, UiEvent> = match step {
        0 => welcome_step(),
        1 => models_step(state),
        2 => preferences_step(),
        3 => translation_step(state),
        4 => done_step(),
        _ => welcome_step(),
    };

    let can_next = match step {
        1 => all_done, // models step blocking until all mandatory downloaded
        _ => true,
    };
    let can_back = step > 0 && step < 4;

    let next_label = match step {
        0 => "Get started",
        1 => "Next",
        2 => "Next",
        3 => "Next",
        4 => "Finish",
        _ => "Next",
    };
    let next_msg = match step {
        4 => UiEvent::OnboardingFinish,
        _ => UiEvent::OnboardingNext,
    };

    let nav = if step == 3 {
        row![
            if can_back {
                secondary_button("Back", UiEvent::OnboardingBack)
            } else {
                container(space::horizontal().width(Length::Fixed(0.0))).into()
            },
            space::horizontal(),
            secondary_button("Skip for now", UiEvent::OnboardingSkipTranslation),
            primary_button(next_label, can_next, Some(next_msg)),
        ]
        .spacing(scale::s(8.0))
        .align_y(iced::Alignment::Center)
    } else {
        row![
            if can_back {
                secondary_button("Back", UiEvent::OnboardingBack)
            } else {
                container(space::horizontal().width(Length::Fixed(0.0))).into()
            },
            space::horizontal(),
            if step == 1 && !all_done {
                {
                    let hint: Element<'static, UiEvent> = container(text("Download all models to continue").size(scale::s(11.0)).color(ERROR_FG)).padding(scale::s(4.0)).into();
                    hint
                }
            } else {
                space::horizontal().width(Length::Fixed(0.0)).into()
            },
            primary_button(next_label, can_next, Some(next_msg)),
        ]
        .spacing(scale::s(8.0))
        .align_y(iced::Alignment::Center)
    };

    let card = container(
        column![
            row![
                text("Setup").size(scale::s(13.0)).color(MUTED_FG),
                space::horizontal(),
                step_dots(step),
                space::horizontal(),
                text(format!("{}/5", step + 1)).size(scale::s(11.0)).color(MUTED_FG),
            ].spacing(scale::s(8.0)).align_y(iced::Alignment::Center),
            container(content).height(Length::Fill).width(FillLength),
            nav,
        ]
        .spacing(scale::s(12.0))
        .height(Length::Fill),
    )
    .width(Length::Fixed(scale::s(560.0)))
    .height(Length::Fixed(scale::s(520.0)))
    .padding(scale::s(16.0))
    .style(|_| container::Style {
        background: Some(PANEL_BG.into()),
        border: iced::Border::default().rounded(scale::s(12.0)),
        ..Default::default()
    });

    let centered = container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);

    let dimmed = container(centered)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Color { a: 0.72, ..Color::BLACK }.into()),
            ..Default::default()
        });

    // No backdrop click to close — blocking. Only Back/Next/Finish.
    stack![base, dimmed].into()
}
