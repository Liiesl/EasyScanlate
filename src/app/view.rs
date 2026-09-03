use iced::{Element, Length};
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

    // Onboarding is now a page (inner), not an overlay — no extra Stack here
    with_close
}
