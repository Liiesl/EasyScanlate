use iced::{Color, Element, Length};
use iced::widget::{button, center, column, container, image, opaque, progress_bar, row, space, stack, text};
use easyscanlate_ui::panel;
use easyscanlate_ui::scale;

use super::{App, Message};

pub fn view(app: &App) -> Element<'_, Message> {
    // Canonical inner composition now lives in `easyscanlate-ui::shell`.
    let inner: Element<'_, easyscanlate_ui::event::UiEvent> = easyscanlate_ui::shell::view(app);
    let inner_mapped: Element<'_, Message> = inner.map(Message::from);

    let framed: Element<'_, Message> = if let Some(window_id) = app.frame.primary_window() {
        if app.onboarding.is_some() {
            // Onboarding page hides tab bar (blocking, like Home/Editor page spec) — no tabs while setup is mandatory
            app.frame.view(window_id, "", None, None, inner_mapped, Message::Frame)
        } else {
            let tab_bar = crate::app::tabs::titlebar_view(app);
            app.frame.view(window_id, "", None, Some(tab_bar), inner_mapped, Message::Frame)
        }
    } else {
        inner_mapped
    };

    let aurora_cfg = easyscanlate_ui::background::AuroraConfig::from_store();
    let aurora: Element<'_, Message> =
        easyscanlate_ui::background::AuroraBackground::new(aurora_cfg)
            .view()
            .map(Message::from);
    let base_with_aurora: Element<'_, Message> =
        iced::widget::Stack::with_children(vec![aurora, framed]).into();

    let with_update: Element<'_, Message> = crate::app::update_popup::view(app, base_with_aurora);

    let with_close: Element<'_, Message> = if app.pending_close.is_some() {
        crate::app::confirm_close::view(app, with_update)
    } else {
        with_update
    };

    // Loading splash overlay: Photoshop-style — centered "Opening project…" with
    // top-left cycling status (Unpacking / Parsing / Decoding…). Active tab is
    // already the placeholder (titlebar chip exists), underlying editor is empty until hydrate.
    let is_loading = !app.active_is_home()
        && app.tabs.get(app.active).is_some_and(|t| t.loading);
    let with_loading: Element<'_, Message> = if is_loading {
        loading_overlay(app, with_close)
    } else {
        with_close
    };

    // Raster-export progress overlay: same blocking card language as the
    // loading splash (dim + blurred cover + opaque content area) with a
    // progress bar, counts and Cancel. Loading takes precedence if both.
    let is_exporting = !is_loading
        && !app.active_is_home()
        && app.tabs.get(app.active).is_some_and(|t| t.exporting);
    let with_export: Element<'_, Message> = if is_exporting {
        export_overlay(app, with_loading)
    } else {
        with_loading
    };

    // Dim titlebar for inner overlays (settings, manage_models, connect, new_project)
    // while keeping it interactive (visual only, no opaque/mouse_area). The inner
    // overlays already dim the content area; this adds the matching strip over the
    // titlebar so the whole window looks dimmed but tabs/drag still work.
    let has_inner_overlay = (app.settings_open
        || app.manage_models_open
        || app.connect_modal.is_some()
        || app.new_project.is_some())
        && app.onboarding.is_none()
        && !is_loading
        && !is_exporting
        && app.pending_close.is_none();
    let with_titlebar_dim: Element<'_, Message> = if has_inner_overlay {
        let h = app.frame.config().title_bar_height;
        let alpha = if app.manage_models_open {
            0.55
        } else if app.connect_modal.is_some() {
            0.40
        } else if app.settings_open {
            0.70
        } else {
            0.45
        };
        let title_dim = container(
            space::horizontal()
                .width(Length::Fill)
                .height(Length::Fixed(h)),
        )
        .width(Length::Fill)
        .height(Length::Fixed(h))
        .style(move |_| container::Style {
            background: Some(
                Color {
                    a: alpha,
                    ..Color::BLACK
                }
                .into(),
            ),
            ..container::Style::default()
        });
        stack![
            with_export,
            column![
                title_dim,
                space::vertical()
                    .width(Length::Fill)
                    .height(Length::Fill)
            ]
            .width(Length::Fill)
            .height(Length::Fill)
        ]
        .into()
    } else {
        with_export
    };

    // Onboarding is now a page (inner), not an overlay — no extra Stack here
    with_titlebar_dim
}

fn splash_status(phase: f32, is_creating: bool) -> String {
    // 6s loop → 5 stages @ 1.2s each, matches LoadingTick 60fps cycle.
    let t = phase.rem_euclid(6.0);
    let idx = (t / 1.2) as usize;
    if is_creating {
        match idx {
            0 => "Collecting sources…",
            1 => "Laying out pages…",
            2 => "Writing archive…",
            3 => "Finalizing project…",
            _ => "Almost there…",
        }
    } else {
        match idx {
            0 => "Unpacking archive…",
            1 => "Parsing manifest…",
            2 => "Decoding pages…",
            3 => "Hydrating workspace…",
            _ => "Almost there…",
        }
    }
    .to_string()
}

fn loading_overlay<'a>(app: &'a App, base: Element<'a, Message>) -> Element<'a, Message> {
    let tab = &app.tabs[app.active];
    let phase = tab.loading_phase;

    let lower = tab.status.to_lowercase();
    let is_failed = lower.contains("failed") || lower.contains("error");
    let is_creating = !is_failed && lower.contains("creating");

    let status_text = if is_failed {
        tab.status.clone()
    } else {
        splash_status(phase, is_creating)
    };
    let status_color = if is_failed {
        Color::from_rgb8(240, 200, 80)
    } else {
        Color::from_rgb8(148, 163, 184)
    };

    let status_row: Element<'_, Message> = text(status_text)
        .size(scale::s(11.0))
        .color(status_color)
        .into();

    let headline: Element<'_, Message> = container(
        text("Opening project…")
            .size(scale::s(22.0))
            .color(Color::WHITE),
    )
    .width(Length::Fill)
    .center_x(Length::Fill)
    .into();

    let center_block: Element<'_, Message> = container(headline)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into();

    let card_content = column![status_row, center_block]
        .spacing(scale::s(8.0))
        .width(Length::Fill)
        .height(Length::Fill);

    let card = container(card_content)
        .width(Length::Fixed(scale::s(520.0)))
        .height(Length::Fixed(scale::s(280.0)))
        .padding(scale::s(18.0))
        .style(|_| container::Style {
            background: Some(panel::PANEL_BG.into()),
            border: iced::Border::default()
                .rounded(scale::s(16.0))
                .color(Color::from_rgba8(255, 255, 255, 0.08))
                .width(scale::s(1.0)),
            ..container::Style::default()
        });

    // Blurred snapshot cropped to this card rect at capture time, stacked
    // directly behind it (same fixed size, so always aligned). The dim around
    // the card stays plain dim over the live base.
    let blur_cover: Element<'_, Message> = match &app.loading_blur {
        Some(handle) => container(
            image(handle)
                .width(Length::Fixed(scale::s(520.0)))
                .height(Length::Fixed(scale::s(280.0)))
                .content_fit(iced::ContentFit::Fill)
                .border_radius(scale::s(16.0)),
        )
        .width(Length::Fixed(scale::s(520.0)))
        .height(Length::Fixed(scale::s(280.0)))
        .into(),
        // No capture (tests / capture in flight / failed): just the card.
        None => space::horizontal()
            .width(Length::Fixed(0.0))
            .height(Length::Fixed(0.0))
            .into(),
    };

    // Split dim: titlebar strip is visual-only (no opaque/mouse_area) so drag
    // and tab clicks still reach the NativeFrame titlebar underneath. Content
    // area below remains opaque-blocking.
    let h = app.frame.config().title_bar_height;
    let title_dim = container(
        space::horizontal()
            .width(Length::Fill)
            .height(Length::Fixed(h)),
    )
    .width(Length::Fill)
    .height(Length::Fixed(h))
    .style(|_| container::Style {
        background: Some(
            Color {
                a: 0.45,
                ..Color::BLACK
            }
            .into(),
        ),
        ..container::Style::default()
    });

    let content_dim = container(center(stack![blur_cover, card]))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(
                Color {
                    a: 0.45,
                    ..Color::BLACK
                }
                .into(),
            ),
            ..container::Style::default()
        })
        .center_x(Length::Fill)
        .center_y(Length::Fill);

    stack![
        base,
        column![title_dim, opaque(content_dim)]
            .width(Length::Fill)
            .height(Length::Fill)
    ]
    .into()
}

fn export_overlay<'a>(app: &'a App, base: Element<'a, Message>) -> Element<'a, Message> {
    let tab = &app.tabs[app.active];
    let total = tab.export_total.max(1);
    let done = tab.export_done.min(tab.export_total);
    let failed = tab.export_failed;
    let frac = (done as f32 / total as f32).clamp(0.0, 1.0);
    let pct = format!("{:.0}%", frac * 100.0);

    let status_line = if failed > 0 {
        format!("Exporting {done} of {total} image(s)… ({failed} failed)")
    } else {
        format!("Exporting {done} of {total} image(s)…")
    };
    let folder_line = tab
        .export_folder
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    let status_row: Element<'_, Message> = text(status_line)
        .size(scale::s(11.0))
        .color(Color::from_rgb8(148, 163, 184))
        .into();

    let headline: Element<'_, Message> = container(
        text("Exporting images…")
            .size(scale::s(22.0))
            .color(Color::WHITE),
    )
    .width(Length::Fill)
    .center_x(Length::Fill)
    .into();

    let bar_row: Element<'_, Message> = row![
        progress_bar(0.0..=1.0, frac)
            .girth(Length::Fixed(scale::s(8.0)))
            .length(Length::Fill)
            .style(|_: &iced::Theme| {
                let cfg = easyscanlate_ui::background::AuroraConfig::from_store();
                iced::widget::progress_bar::Style {
                    background: easyscanlate_ui::accent::aurora_track(&cfg).into(),
                    bar: easyscanlate_ui::accent::aurora_accent(&cfg).into(),
                    border: iced::Border::default().rounded(scale::s(4.0)),
                }
            }),
        text(pct)
            .size(scale::s(11.0))
            .color(Color::from_rgb8(148, 163, 184))
            .width(Length::Fixed(scale::s(42.0))),
    ]
    .spacing(scale::s(8.0))
    .align_y(iced::Alignment::Center)
    .into();

    let folder_row: Element<'_, Message> = text(folder_line)
        .size(scale::s(11.0))
        .color(Color::from_rgb8(100, 116, 139))
        .into();

    let cancel_btn: Element<'_, Message> = container(
        button(text("Cancel").size(scale::s(12.0)))
            .style(panel::button_style)
            .on_press(super::Message::Ui(
                easyscanlate_ui::event::UiEvent::ExportCancel,
            ))
            .padding(scale::s(6.0)),
    )
    .width(Length::Fill)
    .center_x(Length::Fill)
    .into();

    let card_content = column![status_row, headline, bar_row, folder_row, cancel_btn]
        .spacing(scale::s(10.0))
        .width(Length::Fill);

    let card = container(card_content)
        .width(Length::Fixed(scale::s(520.0)))
        .height(Length::Fixed(scale::s(280.0)))
        .padding(scale::s(18.0))
        .style(|_| container::Style {
            background: Some(panel::PANEL_BG.into()),
            border: iced::Border::default()
                .rounded(scale::s(16.0))
                .color(Color::from_rgba8(255, 255, 255, 0.08))
                .width(scale::s(1.0)),
            ..container::Style::default()
        });

    // Blurred snapshot cropped to this card rect at capture time, stacked
    // directly behind it (same fixed size, so always aligned). Falls back to
    // the plain card when headless/in tests or while the capture is in flight.
    let blur_cover: Element<'_, Message> = match &app.export_blur {
        Some(handle) => container(
            image(handle)
                .width(Length::Fixed(scale::s(520.0)))
                .height(Length::Fixed(scale::s(280.0)))
                .content_fit(iced::ContentFit::Fill)
                .border_radius(scale::s(16.0)),
        )
        .width(Length::Fixed(scale::s(520.0)))
        .height(Length::Fixed(scale::s(280.0)))
        .into(),
        None => space::horizontal()
            .width(Length::Fixed(0.0))
            .height(Length::Fixed(0.0))
            .into(),
    };

    // Split dim: titlebar strip is visual-only (no opaque/mouse_area) so drag
    // and tab clicks still reach the NativeFrame titlebar underneath. Content
    // area below remains opaque-blocking, like the loading splash.
    let h = app.frame.config().title_bar_height;
    let title_dim = container(
        space::horizontal()
            .width(Length::Fill)
            .height(Length::Fixed(h)),
    )
    .width(Length::Fill)
    .height(Length::Fixed(h))
    .style(|_| container::Style {
        background: Some(
            Color {
                a: 0.45,
                ..Color::BLACK
            }
            .into(),
        ),
        ..container::Style::default()
    });

    let content_dim = container(center(stack![blur_cover, card]))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(
                Color {
                    a: 0.45,
                    ..Color::BLACK
                }
                .into(),
            ),
            ..container::Style::default()
        })
        .center_x(Length::Fill)
        .center_y(Length::Fill);

    stack![
        base,
        column![title_dim, opaque(content_dim)]
            .width(Length::Fill)
            .height(Length::Fill)
    ]
    .into()
}
