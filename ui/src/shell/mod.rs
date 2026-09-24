use iced::widget::pane_grid;
use iced::{Element, Length};

use crate::event::UiEvent;
use crate::layout::{CARD_RADIUS, GAP, MAIN_AREA_MIN_WIDTH, OUTER_PADDING, EditorPaneKind, PaneKind, ResultsPaneKind};
use crate::state::UiState;
use crate::{main_area, panel, scale, toolbar};
use crate::settings as settings_modal;

/// Canonical shell: the `inner: Element<UiEvent>` that `src/app/view.rs` used to build inline.
/// Covers onboarding page vs Home (with new-project/settings/connect/manage_models overlays)
/// vs Editor (left Translation/Inpaint column + toolbar + right Main/Styling + modals). The outer frame/aurora/loading/dimming
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
        // Editor: resizable left column (Translation on top, Inpaint below)
        // vs everything right of the toolbar (fixed-width toolbar + actions
        // + Main/Styling). Need pane states from UiState; fallback to empty
        // if missing (e.g. tests)
        let Some((panes, results_panes, outer_panes)) = state.editor_panes() else {
            let fallback: Element<'_, UiEvent> = iced::widget::container(iced::widget::text("No editor panes").size(scale::s(12.0)))
                .width(Length::Fill).height(Length::Fill).into();
            return fallback;
        };

        // Outer split: draggable divider resizes the left column; the
        // toolbar itself stays a fixed 36px at the left edge of the right
        // side. Default left share comes from `EDITOR_LEFT_DEFAULT_RATIO`.
        // Each arm builds its content inline (from shared borrows) because
        // the outer closure runs once per pane.
        let content: Element<'_, UiEvent> = pane_grid::PaneGrid::new(outer_panes, |_, kind, _| {
            pane_grid::Content::new(match kind {
                EditorPaneKind::Left => {
                    let left: Element<'_, UiEvent> = pane_grid::PaneGrid::new(
                        results_panes,
                        |_, kind, _| {
                            let body: Element<'_, UiEvent> = match kind {
                                ResultsPaneKind::Translation => iced::widget::container(panel::results::view(state))
                                    .padding(scale::s(10.0))
                                    .width(Length::Fill)
                                    .height(Length::Fill)
                                    .style(|_theme| iced::widget::container::Style {
                                        background: Some(panel::PANEL_BG.into()),
                                        border: iced::Border::default().rounded(scale::s(CARD_RADIUS)),
                                        ..Default::default()
                                    })
                                    .into(),
                                ResultsPaneKind::Layers => iced::widget::container(panel::inpaint::view(state))
                                    .padding(scale::s(10.0))
                                    .width(Length::Fill)
                                    .height(Length::Fill)
                                    .style(|_theme| iced::widget::container::Style {
                                        background: Some(panel::PANEL_BG.into()),
                                        border: iced::Border::default().rounded(scale::s(CARD_RADIUS)),
                                        ..Default::default()
                                    })
                                    .into(),
                            };
                            pane_grid::Content::new(body)
                        },
                    )
                    .spacing(scale::s(GAP))
                    .min_size(scale::s(90.0))
                    .on_resize(scale::s(GAP), UiEvent::ResultsPaneResized)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
                    left
                }
                EditorPaneKind::Right => {
                    let right_grid: Element<'_, UiEvent> = pane_grid::PaneGrid::new(
                        panes,
                        |_, kind, _| {
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
                                PaneKind::Styling => {
                                    let el: Element<'_, UiEvent> = iced::widget::container(panel::styling::view(state))
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
                        },
                    )
                    .spacing(scale::s(GAP))
                    .min_size(MAIN_AREA_MIN_WIDTH)
                    .on_resize(scale::s(GAP), UiEvent::PanelResized)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();
                    let right: Element<'_, UiEvent> =
                        iced::widget::column![panel::actions::view(state), right_grid]
                            .spacing(scale::s(GAP))
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .into();
                    iced::widget::row![
                        toolbar::view(state),
                        iced::widget::container(right)
                            .width(Length::Fill)
                            .height(Length::Fill),
                    ]
                    .spacing(scale::s(GAP))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
                }
            })
        })
        .spacing(scale::s(GAP))
        .min_size(scale::s(90.0))
        .on_resize(scale::s(GAP), UiEvent::EditorResized)
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
        let padded_content: Element<'_, UiEvent> = iced::widget::container(content)
            .padding(scale::s(OUTER_PADDING))
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        // Single floating color picker above the pane grid but below the
        // modals: outside every scrollable so the header drag clamps against
        // the full window viewport and the window stays movable.
        let with_picker: Element<'_, UiEvent> =
            panel::styling::overlay_host(state, padded_content);

        let inner_with_modals: Element<'_, UiEvent> = {
            let base: Element<'_, UiEvent> = with_picker;
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

/// Export progress overlay data: `(done, total, failed, folder)`.
/// Mirrors `loading_overlay_data`; `None` when no export is running.
pub fn export_overlay_data<S: UiState + ?Sized>(state: &S) -> Option<(usize, usize, usize, Option<String>)> {
    if !state.is_exporting() {
        return None;
    }
    let (done, total, failed) = state.export_progress()?;
    Some((done, total, failed, state.export_folder()))
}
