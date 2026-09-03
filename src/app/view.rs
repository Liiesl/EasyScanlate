use iced::{Color, Element, Length};
use iced::widget::{center, column, container, opaque, row, stack, text};
use iced::widget::pane_grid;
use easyscanlate_ui::event::UiEvent;
use easyscanlate_ui::{main_area, panel, scale, toolbar};
use easyscanlate_ui::settings as settings_modal;

use super::layout::{CARD_RADIUS, GAP, MAIN_AREA_MIN_WIDTH, OUTER_PADDING, STYLING_MIN_WIDTH};
use super::{App, Message};

pub fn view(app: &App) -> Element<'_, Message> {
    let inner: Element<'_, UiEvent> = if app.onboarding.is_some() {
        // Onboarding as dedicated page (like Home/Editor) — fills window, not overlay
        easyscanlate_ui::onboarding::view_page(app)
    } else if app.active_is_home() {
            let base: Element<'_, UiEvent> = iced::widget::container(easyscanlate_ui::home::view(app))
                .padding(scale::s(OUTER_PADDING))
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
            let with_new: Element<'_, UiEvent> = if app.new_project.is_some() {
                easyscanlate_ui::new_project::view(app, base)
            } else {
                base
            };
            let with_settings: Element<'_, UiEvent> = if app.settings_open {
                settings_modal::view(app, with_new)
            } else {
                with_new
            };
            let with_connect: Element<'_, UiEvent> = if app.connect_modal.is_some() {
                easyscanlate_ui::connect::view(app, with_settings)
            } else {
                with_settings
            };
            if app.manage_models_open {
                easyscanlate_ui::manage_models::view(app, with_connect)
            } else {
                with_connect
            }
        } else {
            let grid: Element<'_, UiEvent> = pane_grid::PaneGrid::new(&app.tabs[app.active].panes, |_, kind, _| {
                pane_grid::Content::new(match kind {
                    super::layout::PaneKind::MainArea => {
                        let el: Element<'_, UiEvent> = iced::widget::container(main_area::view(app))
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .style(|_theme| iced::widget::container::Style {
                                background: Some(panel::PANEL_BG.into()),
                                border: iced::Border::default().rounded(scale::s(CARD_RADIUS)),
                                ..Default::default()
                            })
                            .into();
                        el
                    }
                    super::layout::PaneKind::Panel => {
                        let side_grid: Element<'_, UiEvent> =
                            pane_grid::PaneGrid::new(&app.tabs[app.active].side_panes, |_, inner, _| {
                                pane_grid::Content::new(match inner {
                                    super::layout::SidePaneKind::Styling => {
                                        let el: Element<'_, UiEvent> = pane_grid::PaneGrid::new(
                                            &app.tabs[app.active].styling_panes,
                                            |_, kind, _| {
                                                let body: Element<'_, UiEvent> = match kind {
                                                    super::layout::StylingPaneKind::Inspector => {
                                                        iced::widget::container(panel::styling::view(app))
                                                            .padding(scale::s(10.0))
                                                            .width(Length::Fill)
                                                            .height(Length::Fill)
                                                            .style(|_theme| {
                                                                iced::widget::container::Style {
                                                                    background: Some(
                                                                        panel::PANEL_BG.into(),
                                                                    ),
                                                                    border: iced::Border::default()
                                                                        .rounded(scale::s(CARD_RADIUS)),
                                                                    ..Default::default()
                                                                }
                                                            })
                                                            .into()
                                                    }
                                                    super::layout::StylingPaneKind::Layers => {
                                                        iced::widget::container(
                                                            panel::inpaint::view(app),
                                                        )
                                                        .padding(scale::s(10.0))
                                                        .width(Length::Fill)
                                                        .height(Length::Fill)
                                                        .style(|_theme| {
                                                            iced::widget::container::Style {
                                                                background: Some(
                                                                    panel::PANEL_BG.into(),
                                                                ),
                                                                border: iced::Border::default()
                                                                    .rounded(scale::s(CARD_RADIUS)),
                                                                ..Default::default()
                                                            }
                                                        })
                                                        .into()
                                                    }
                                                };
                                                pane_grid::Content::new(body)
                                            },
                                        )
                                        .spacing(scale::s(GAP))
                                        .min_size(scale::s(90.0))
                                        .on_resize(scale::s(GAP), UiEvent::StylingPaneResized)
                                        .width(Length::Fill)
                                        .height(Length::Fill)
                                        .into();
                                        el
                                    }
                                    super::layout::SidePaneKind::Results => {
                                        let el: Element<'_, UiEvent> = iced::widget::container(
                                            panel::results::view(app),
                                        )
                                        .padding(scale::s(10.0))
                                        .width(Length::Fill)
                                        .height(Length::Fill)
                                        .style(|_theme| iced::widget::container::Style {
                                            background: Some(panel::PANEL_BG.into()),
                                            border: iced::Border::default().rounded(scale::s(CARD_RADIUS)),
                                            ..Default::default()
                                        })
                                        .into();
                                        el
                                    }
                                })
                            })
                            .spacing(scale::s(GAP))
                            .min_size(STYLING_MIN_WIDTH)
                            .on_resize(scale::s(GAP), UiEvent::SidePanelResized)
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .into();

                        let el: Element<'_, UiEvent> =
                            iced::widget::column![panel::actions::view(app), side_grid]
                                .spacing(scale::s(GAP))
                                .width(Length::Fill)
                                .height(Length::Fill)
                                .into();
                        el
                    }
                })
            })
            .spacing(scale::s(GAP))
            .min_size(MAIN_AREA_MIN_WIDTH)
            .on_resize(scale::s(GAP), UiEvent::PanelResized)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
            let content: Element<'_, UiEvent> = iced::widget::row![toolbar::view(app), grid]
                .spacing(scale::s(GAP))
                .height(Length::Fill)
                .into();
            let padded_content: Element<'_, UiEvent> = iced::widget::container(content)
                .padding(scale::s(OUTER_PADDING))
                .width(Length::Fill)
                .height(Length::Fill)
                .into();

            let inner_with_modals: Element<'_, UiEvent> = {
                let base: Element<'_, UiEvent> = padded_content;
                let v: Element<'_, UiEvent> = if app.settings_open {
                    settings_modal::view(app, base)
                } else {
                    base
                };
                let v: Element<'_, UiEvent> = if app.connect_modal.is_some() {
                    easyscanlate_ui::connect::view(app, v)
                } else {
                    v
                };
                if app.manage_models_open {
                    easyscanlate_ui::manage_models::view(app, v)
                } else {
                    v
                }
            };
            inner_with_modals
        };
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

    let with_close: Element<'_, Message> = if app.pending_close.is_some() {
        crate::app::confirm_close::view(app, base_with_aurora)
    } else {
        base_with_aurora
    };

    // Loading splash overlay: Photoshop-style — centered "Opening project…" with
    // top-left cycling status (Unpacking / Parsing / Decoding…). Active tab is
    // already the placeholder (titlebar chip exists), underlying editor is empty until hydrate.
    let with_loading: Element<'_, Message> = if !app.active_is_home()
        && app
            .tabs
            .get(app.active)
            .is_some_and(|t| t.loading)
    {
        loading_overlay(app, with_close)
    } else {
        with_close
    };

    // Onboarding is now a page (inner), not an overlay — no extra Stack here
    with_loading
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

    stack![
        base,
        opaque(
            center(card).style(|_| container::Style {
                background: Some(
                    Color {
                        a: 0.45,
                        ..Color::BLACK
                    }
                    .into()
                ),
                ..container::Style::default()
            })
        )
    ]
    .into()
}
