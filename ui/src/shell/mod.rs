use iced::widget::pane_grid;
use iced::{Element, Length};

use crate::event::UiEvent;
use crate::layout::{CARD_RADIUS, GAP, MAIN_AREA_MIN_WIDTH, OUTER_PADDING, STYLING_MIN_WIDTH, PaneKind, SidePaneKind, StylingPaneKind};
use crate::state::UiState;
use crate::{main_area, panel, scale, toolbar};
use crate::settings as settings_modal;

/// Canonical shell: the `inner: Element<UiEvent>` that `src/app/view.rs` used to build inline.
/// Covers onboarding page vs Home (with new-project/settings/connect/manage_models overlays)
/// vs Editor (toolbar + 3-level pane_grid + modals). The outer frame/aurora/loading/dimming
/// stays in `src/app/view.rs` because it needs `NativeFrame` and `Message::Frame`.
pub fn view<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    if state.onboarding_open() {
        return crate::onboarding::view_page(state);
    }
    if state.app_view() == crate::state::AppView::Home {
        let base: Element<'_, UiEvent> = iced::widget::container(crate::home::view(state))
            .padding(scale::s(OUTER_PADDING))
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
        let with_new: Element<'_, UiEvent> = if state.new_project_overlay().is_some() {
            crate::new_project::view(state, base)
        } else {
            base
        };
        let with_settings: Element<'_, UiEvent> = if state.settings_open() {
            settings_modal::view(state, with_new)
        } else {
            with_new
        };
        let with_connect: Element<'_, UiEvent> = if state.connect_modal().is_some() {
            crate::connect::view(state, with_settings)
        } else {
            with_settings
        };
        if state.manage_models_open() {
            crate::manage_models::view(state, with_connect)
        } else {
            with_connect
        }
    } else {
        // Editor: need pane states from UiState; fallback to empty if missing (e.g. tests)
        let Some((panes, side_panes, styling_panes)) = state.editor_panes() else {
            let fallback: Element<'_, UiEvent> = iced::widget::container(iced::widget::text("No editor panes").size(scale::s(12.0)))
                .width(Length::Fill).height(Length::Fill).into();
            return fallback;
        };

        let grid: Element<'_, UiEvent> = pane_grid::PaneGrid::new(panes, |_, kind, _| {
            pane_grid::Content::new(match kind {
                PaneKind::MainArea => {
                    let el: Element<'_, UiEvent> = iced::widget::container(main_area::view(state))
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
                PaneKind::Panel => {
                    // side_panes needs to be cloned for inner closure? We capture via state reference
                    // but PaneGrid::new borrows side_panes which is already a ref from state.
                    // To avoid double borrow, we handle via helper that re-borrows from state inside closure.
                    // For simplicity, we build side_grid with direct borrow of side_panes/styling_panes from state.
                    // Since those refs are valid for '_, the inner closures can capture state.
                    let side_grid: Element<'_, UiEvent> =
                        pane_grid::PaneGrid::new(side_panes, |_, inner, _| {
                            pane_grid::Content::new(match inner {
                                SidePaneKind::Styling => {
                                    let el: Element<'_, UiEvent> = pane_grid::PaneGrid::new(
                                        styling_panes,
                                        |_, kind, _| {
                                            let body: Element<'_, UiEvent> = match kind {
                                                StylingPaneKind::Inspector => {
                                                    iced::widget::container(panel::styling::view(state))
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
                                                StylingPaneKind::Layers => {
                                                    iced::widget::container(
                                                        panel::inpaint::view(state),
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
                                SidePaneKind::Results => {
                                    let el: Element<'_, UiEvent> = iced::widget::container(
                                        panel::results::view(state),
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
                        iced::widget::column![panel::actions::view(state), side_grid]
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
        let content: Element<'_, UiEvent> = iced::widget::row![toolbar::view(state), grid]
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
            let v: Element<'_, UiEvent> = if state.settings_open() {
                settings_modal::view(state, base)
            } else {
                base
            };
            let v: Element<'_, UiEvent> = if state.connect_modal().is_some() {
                crate::connect::view(state, v)
            } else {
                v
            };
            if state.manage_models_open() {
                crate::manage_models::view(state, v)
            } else {
                v
            }
        };
        inner_with_modals
    }
}

/// Loading splash helpers — Photoshop-style "Opening project…" with cycling status.
/// Split from `src/app/view.rs` so the overlay can be reused; outer dim/frame handling stays in app.
pub fn splash_status(phase: f32, is_creating: bool) -> String {
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

pub fn loading_overlay_data<S: UiState + ?Sized>(state: &S) -> Option<(f32, String, bool, bool)> {
    if !state.is_loading() {
        return None;
    }
    let phase = state.loading_phase();
    let status = state.status().to_string();
    let lower = status.to_lowercase();
    let is_failed = lower.contains("failed") || lower.contains("error");
    let is_creating = !is_failed && lower.contains("creating");
    Some((phase, status, is_failed, is_creating))
}
