use iced::{Color, Element, Length};
use iced::widget::pane_grid;
use scanlateit_ui::event::UiEvent;
use scanlateit_ui::{main_area, panel, scale, toolbar};
use scanlateit_ui::settings as settings_modal;

use super::layout::{CARD_RADIUS, GAP, OUTER_PADDING};
use super::chrome::title_icon_handle;
use super::{App, Message};

pub fn view(app: &App) -> Element<'_, Message> {
    let grid: Element<'_, UiEvent> = pane_grid::PaneGrid::new(&app.panes, |_, kind, _| {
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
                    pane_grid::PaneGrid::new(&app.side_panes, |_, inner, _| {
                        pane_grid::Content::new(match inner {
                            super::layout::SidePaneKind::Styling => {
                                let el: Element<'_, UiEvent> = pane_grid::PaneGrid::new(
                                    &app.styling_panes,
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
                    .min_size(scale::s(120.0))
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
    .min_size(scale::s(160.0))
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
            scanlateit_ui::connect::view(app, v)
        } else {
            v
        };
        if app.manage_models_open {
            scanlateit_ui::manage_models::view(app, v)
        } else {
            v
        }
    };
    let inner_mapped: Element<'_, Message> = inner_with_modals.map(Message::from);

    let framed: Element<'_, Message> = if let Some(window_id) = app.frame.primary_window() {
        app.frame.view(window_id, "", None, None, inner_mapped, Message::Frame)
    } else {
        inner_mapped
    };

    let aurora_cfg = scanlateit_ui::background::AuroraConfig::from_store();
    let aurora: Element<'_, Message> =
        scanlateit_ui::background::AuroraBackground::new(aurora_cfg)
            .view()
            .map(Message::from);
    let base_with_aurora: Element<'_, Message> =
        iced::widget::Stack::with_children(vec![aurora, framed]).into();

    let title_overlay: Element<'_, Message> = {
        let h = app.frame.config().title_bar_height;
        let is_dark = scanlateit_settings::get(|s| s.aurora_is_dark);
        let title_color = if is_dark {
            Color::from_rgb(0.92, 0.92, 0.92)
        } else {
            Color::from_rgb(0.12, 0.12, 0.12)
        };
        let icon_element: Element<'_, Message> = match title_icon_handle() {
            Some(handle) => iced::widget::image(handle)
                .width(Length::Fixed(16.0))
                .height(Length::Fixed(16.0))
                .into(),
            None => iced::widget::space::horizontal()
                .width(Length::Fixed(16.0))
                .height(Length::Fixed(16.0))
                .into(),
        };
        let row = iced::widget::row![
            icon_element,
            iced::widget::text("Scanlateit").size(13).color(title_color)
        ]
        .spacing(8)
        .align_y(iced::Center);
        iced::widget::container(row)
            .width(Length::Fill)
            .height(Length::Fixed(h))
            .center_x(Length::Fill)
            .center_y(Length::Fixed(h))
            .into()
    };

    let title_bar_container: Element<'_, Message> = iced::widget::container(title_overlay)
        .width(Length::Fill)
        .height(Length::Fixed(app.frame.config().title_bar_height))
        .into();

    iced::widget::Stack::with_children(vec![base_with_aurora, title_bar_container]).into()
}
