//! Dirty-close confirmation modal — `Save / Don't Save / Cancel`.
//! Reuses `ui/src/connect.rs` overlay pattern (`stack![base, opaque(mouse_area(center(window)))])`)
//! and panel colors (`PANEL_BG`). Shown when `App.pending_close.is_some()` and
//! `tab_by_id(pending_close).dirty == true`.

use iced::widget::{button, center, column, container, mouse_area, opaque, row, space, stack, text};
use iced::{Color, Element, Length};

use super::{App, Message};
use easyscanlate_ui::event::UiEvent;
use easyscanlate_ui::panel::PANEL_BG;
use easyscanlate_ui::scale;

const ACCENT: Color = Color::from_rgb8(92, 190, 255);
const MUTED_FG: Color = Color::from_rgb(0.6, 0.6, 0.6);

fn accent_button_style(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => Color::from_rgba8(92, 190, 255, 0.90),
        button::Status::Pressed => Color::from_rgba8(72, 170, 235, 0.95),
        button::Status::Active => ACCENT,
        button::Status::Disabled => Color::from_rgba8(92, 190, 255, 0.40),
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

/// Overlay that asks `Save changes to {title}?` with `Save / Don't Save / Cancel`.
/// Caller must only call when `app.pending_close.is_some()`.
pub fn view<'a>(app: &'a App, base: Element<'a, Message>) -> Element<'a, Message> {
    let tid = match app.pending_close {
        Some(id) => id,
        None => return base,
    };
    let tab = app.tab_by_id(tid).or_else(|| app.tabs.get(app.active));
    let title = tab.map(|t| t.title.as_str()).unwrap_or("this project");
    let raw = tid.0;

    let window = container(
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
                    .on_press(Message::Ui(UiEvent::TabCloseCancel)),
                button(text("Don't Save").size(scale::s(12.0)))
                    .padding([scale::s(6.0), scale::s(14.0)])
                    .style(danger_button_style)
                    .on_press(Message::Ui(UiEvent::TabCloseConfirmed(raw, false))),
                button(text("Save").size(scale::s(12.0)))
                    .padding([scale::s(6.0), scale::s(14.0)])
                    .style(accent_button_style)
                    .on_press(Message::Ui(UiEvent::TabCloseConfirmed(raw, true))),
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

    // Split dim: titlebar strip is visual-only so tabs/drag still work while
    // the modal is open. Content area below remains opaque-blocking with
    // backdrop click to cancel.
    let h = app.frame.config().title_bar_height;
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
    .on_press(Message::Ui(UiEvent::TabCloseCancel)));

    stack![
        base,
        column![title_dim, content_dim]
            .width(Length::Fill)
            .height(Length::Fill)
    ]
    .into()
}
