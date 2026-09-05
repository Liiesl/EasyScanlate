//! Update-available popup: blocking modal shown after the startup
//! auto-check finds a Velopack release. Renders over a dimmed + blurred
//! snapshot of the window (captured clean before the popup opens, see
//! `src/app/backdrop.rs::BackdropKind::Update`).
//!
//! States mirror Settings → Updates: available → downloading → ready.
//! `Later` only hides the popup; the update stays offered in Settings.

use iced::widget::{
    button, center, column, container, image, opaque, progress_bar, row, scrollable, space,
    stack, text,
};
use iced::{Color, Element, Length};

use crate::event::UiEvent;
use crate::panel::PANEL_BG;
use crate::scale;
use crate::state::UiState;

/// Popup card size in logical px (mirrored by
/// `src/app/backdrop.rs::UPDATE_W/H` for the blur crop).
pub const UPDATE_W: f32 = 480.0;
pub const UPDATE_H: f32 = 320.0;

const MUTED_FG: Color = Color::from_rgb(0.6, 0.6, 0.6);

/// Truncate release notes so the card keeps its fixed size.
fn short_notes(notes: &str) -> String {
    const MAX: usize = 800;
    let trimmed = notes.trim();
    if trimmed.len() <= MAX {
        return trimmed.to_string();
    }
    let mut end = MAX;
    while end > 0 && !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", trimmed[..end].trim_end())
}

fn card<'a, S: UiState + ?Sized>(state: &'a S, available: &str) -> Element<'a, UiEvent> {
    let current = state.update_current_version();
    let downloading = state.update_downloading();
    let progress = state.update_progress().clamp(0, 100);
    let ready = state.update_ready();

    let title: Element<'_, UiEvent> = text("Update available")
        .size(scale::s(18.0))
        .color(Color::WHITE)
        .into();
    let subtitle: Element<'_, UiEvent> = if current.is_empty() {
        text(format!("v{available} is ready to download."))
    } else {
        text(format!("v{current} → v{available} is ready to download."))
    }
    .size(scale::s(12.0))
    .color(MUTED_FG)
    .into();

    let mut items: Vec<Element<'_, UiEvent>> = vec![title, subtitle];

    if let Some(notes) = state.update_notes() {
        let body = short_notes(&notes);
        if !body.is_empty() {
            items.push(
                container(
                    scrollable(text(body).size(scale::s(11.0)).color(MUTED_FG))
                        .height(Length::Fixed(scale::s(72.0))),
                )
                .padding(scale::s(6.0))
                .style(|_| container::Style {
                    background: Some(Color::from_rgba8(255, 255, 255, 0.04).into()),
                    border: iced::Border::default().rounded(scale::s(6.0)),
                    ..Default::default()
                })
                .into(),
            );
        }
    }

    if ready {
        items.push(
            text("Update ready — restart to apply.")
                .size(scale::s(12.0))
                .color(crate::accent::accent())
                .into(),
        );
        items.push(
            row![
                button(text("Restart & Update").size(scale::s(12.0)))
                    .padding([scale::s(6.0), scale::s(14.0)])
                    .style(crate::panel::button_style)
                    .on_press(UiEvent::UpdateApply),
                button(text("Later").size(scale::s(12.0)))
                    .padding([scale::s(6.0), scale::s(14.0)])
                    .style(crate::panel::button_style)
                    .on_press(UiEvent::UpdateDismiss),
            ]
            .spacing(scale::s(8.0))
            .align_y(iced::Alignment::Center)
            .into(),
        );
    } else if downloading {
        let frac = (progress as f32 / 100.0).clamp(0.0, 1.0);
        items.push(
            text(format!("Downloading update {progress}% — please don't close"))
                .size(scale::s(12.0))
                .color(crate::accent::accent())
                .into(),
        );
        items.push(
            row![
                progress_bar(0.0..=1.0, frac)
                    .girth(Length::Fixed(scale::s(8.0)))
                    .length(Length::Fill)
                    .style(|_: &iced::Theme| iced::widget::progress_bar::Style {
                        background: crate::accent::track().into(),
                        bar: crate::accent::accent().into(),
                        border: iced::Border::default().rounded(scale::s(4.0)),
                    }),
                text(format!("{progress}%"))
                    .size(scale::s(11.0))
                    .color(MUTED_FG)
                    .width(Length::Fixed(scale::s(42.0))),
            ]
            .spacing(scale::s(8.0))
            .align_y(iced::Alignment::Center)
            .into(),
        );
    } else {
        items.push(
            row![
                button(text("Download").size(scale::s(12.0)))
                    .padding([scale::s(6.0), scale::s(14.0)])
                    .style(crate::panel::button_style)
                    .on_press(UiEvent::UpdateDownload),
                button(text("Later").size(scale::s(12.0)))
                    .padding([scale::s(6.0), scale::s(14.0)])
                    .style(crate::panel::button_style)
                    .on_press(UiEvent::UpdateDismiss),
            ]
            .spacing(scale::s(8.0))
            .align_y(iced::Alignment::Center)
            .into(),
        );
        items.push(
            text("Downloaded via Velopack from GitHub releases (per-user, no admin).")
                .size(scale::s(11.0))
                .color(MUTED_FG)
                .into(),
        );
    }

    container(column(items).spacing(scale::s(10.0)).width(Length::Fill))
        .width(Length::Fixed(scale::s(UPDATE_W)))
        .height(Length::Fixed(scale::s(UPDATE_H)))
        .padding(scale::s(18.0))
        .style(|_| container::Style {
            background: Some(PANEL_BG.into()),
            border: iced::Border::default()
                .rounded(scale::s(16.0))
                .color(Color::from_rgba8(255, 255, 255, 0.08))
                .width(scale::s(1.0)),
            ..container::Style::default()
        })
        .into()
}

/// The popup window: blurred snapshot cropped to the card rect stacked
/// directly behind the card. Returns `None` when the popup should not show.
pub fn window<'a, S: UiState + ?Sized>(state: &'a S) -> Option<Element<'a, UiEvent>> {
    if !state.update_popup_visible() {
        return None;
    }
    let available = state.update_available_version()?;
    if available.trim().is_empty() && !state.update_downloading() && !state.update_ready() {
        return None;
    }

    // Blurred snapshot cropped to this card rect at capture time, stacked
    // directly behind it (same fixed size, so always aligned).
    let blur_cover: Element<'_, UiEvent> = match state.update_blur() {
        Some(handle) => container(
            image(handle)
                .width(Length::Fixed(scale::s(UPDATE_W)))
                .height(Length::Fixed(scale::s(UPDATE_H)))
                .content_fit(iced::ContentFit::Fill)
                .border_radius(scale::s(16.0)),
        )
        .width(Length::Fixed(scale::s(UPDATE_W)))
        .height(Length::Fixed(scale::s(UPDATE_H)))
        .into(),
        None => space::horizontal()
            .width(Length::Fixed(0.0))
            .height(Length::Fixed(0.0))
            .into(),
    };

    Some(stack![blur_cover, card(state, &available)].into())
}

/// Blocking update popup over `base` with dim + blurred card backdrop.
/// Returns `base` unchanged when the popup is not visible or offers nothing.
pub fn view<'a, S: UiState + ?Sized>(
    state: &'a S,
    base: Element<'a, UiEvent>,
) -> Element<'a, UiEvent> {
    let Some(win) = window(state) else {
        return base;
    };
    // Split dim: titlebar strip is visual-only (no opaque/mouse_area) so drag
    // still reaches the NativeFrame titlebar underneath. Content area below
    // is opaque-blocking with no backdrop-click dismiss (blocking modal).
    let h = state.titlebar_height();
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

    let content_dim = opaque(
        container(center(opaque(win)))
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
            .center_y(Length::Fill),
    );

    stack![
        base,
        column![title_dim, content_dim]
            .width(Length::Fill)
            .height(Length::Fill)
    ]
    .into()
}
