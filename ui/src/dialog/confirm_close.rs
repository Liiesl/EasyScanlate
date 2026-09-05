//! Dirty-close confirmation modal — `Save / Don't Save / Cancel`.
//! Reuses `ui/src/connect.rs` overlay pattern (`stack![base, opaque(mouse_area(center(window)))])`)
//! and panel colors (`PANEL_BG`). Shown when `pending_close.is_some()`.

use iced::widget::{button, center, column, container, mouse_area, opaque, row, space, stack, text};
use iced::{Color, Element, Length};

use crate::event::UiEvent;
use crate::panel::PANEL_BG;
use crate::scale;
use crate::state::UiState;

const MUTED_FG: Color = Color::from_rgb(0.6, 0.6, 0.6);

fn accent_button_style(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => crate::accent::accent_hover(),
        button::Status::Pressed => crate::accent::accent(),
        button::Status::Active => crate::accent::accent(),
        button::Status::Disabled => crate::accent::accent_translucent(0.40),
    };
    button::Style {
        background: Some(bg.into()),
        border: iced::Border::default().rounded(scale::s(6.0)),
        text_color: Color::WHITE,
        ..button::Style::default()
    }
}

fn ghost_button_style(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => Color::from_rgba8(255, 255, 255, 0.10),
        button::Status::Pressed => Color::from_rgba8(255, 255, 255, 0.15),
        _ => Color::TRANSPARENT,
    };
    let txt = match status {
        button::Status::Disabled => Color::from_rgba8(160, 160, 160, 0.5),
        _ => Color::WHITE,
    };
    button::Style {
        background: Some(bg.into()),
        border: iced::Border::default()
            .rounded(scale::s(6.0))
            .color(Color::from_rgba8(255, 255, 255, 0.10))
            .width(scale::s(1.0)),
        text_color: txt,
        ..button::Style::default()
    }
}

fn danger_button_style(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => Color::from_rgba8(220, 60, 60, 0.90),
        button::Status::Pressed => Color::from_rgba8(180, 40, 40, 0.95),
        button::Status::Active => Color::from_rgba8(200, 50, 50, 0.85),
        button::Status::Disabled => Color::from_rgba8(200, 50, 50, 0.40),
    };
    button::Style {
        background: Some(bg.into()),
        border: iced::Border::default().rounded(scale::s(6.0)),
        text_color: Color::WHITE,
        ..button::Style::default()
    }
}

pub fn window<'a, S: UiState + ?Sized>(state: &S) -> Option<Element<'a, UiEvent>> {
    let pending = state.pending_close()?;
    let title = pending.title;
    let raw = pending.id;
    let w = container(
        column![
            text(format!("Save changes to \"{}\"?", title))
                .size(scale::s(14.0))
                .color(Color::WHITE),
            text("Your changes will be lost if you don't save them.")
                .size(scale::s(11.0))
                .color(MUTED_FG),
            space::vertical().height(Length::Fixed(scale::s(8.0))),
            row![
                space::horizontal(),
                button(text("Cancel").size(scale::s(12.0)))
                    .padding([scale::s(6.0), scale::s(14.0)])
                    .style(ghost_button_style)
                    .on_press(UiEvent::TabCloseCancel),
                button(text("Don't Save").size(scale::s(12.0)))
                    .padding([scale::s(6.0), scale::s(14.0)])
                    .style(danger_button_style)
                    .on_press(UiEvent::TabCloseConfirmed(raw, false)),
                button(text("Save").size(scale::s(12.0)))
                    .padding([scale::s(6.0), scale::s(14.0)])
                    .style(accent_button_style)
                    .on_press(UiEvent::TabCloseConfirmed(raw, true)),
            ]
            .spacing(scale::s(8.0))
            .align_y(iced::Alignment::Center),
        ]
        .spacing(scale::s(10.0)),
    )
    .width(Length::Fixed(scale::s(380.0)))
    .padding(scale::s(16.0))
    .style(|_theme| container::Style {
        background: Some(PANEL_BG.into()),
        border: iced::Border::default()
            .rounded(scale::s(8.0))
            .color(Color::from_rgb8(60, 63, 74))
            .width(scale::s(1.0)),
        ..container::Style::default()
    });
    Some(w.into())
}

/// Overlay that asks `Save changes to {title}?` with `Save / Don't Save / Cancel`.
/// Returns `base` unchanged when `state.pending_close()` is `None`.
pub fn view<'a, S: UiState + ?Sized>(state: &S, base: Element<'a, UiEvent>) -> Element<'a, UiEvent> {
    let Some(window) = window(state) else {
        return base;
    };
    let h = state.titlebar_height();
    let title_dim = container(space::horizontal().width(Length::Fill).height(Length::Fixed(h)))
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

    let content_dim = opaque(mouse_area(
        container(center(opaque(window)))
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
    )
    .on_press(UiEvent::TabCloseCancel));

    stack![
        base,
        column![title_dim, content_dim]
            .width(Length::Fill)
            .height(Length::Fill)
    ]
    .into()
}
