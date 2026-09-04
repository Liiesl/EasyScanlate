//! Titlebar tab strip — 32px custom frame (`NativeFrame` fill row).
//!
//! Renders `easyscanlate` (pinned, no ×, with icon) + project tabs (`title • ×`) + `+`
//! immediately after last tab. Scroll area grows with tabs until 80% of titlebar
//! then becomes fixed and scrollable. Inactive bg = transparent, active bg =
//! `PANEL_BG` (`34,36,44,0.70`) with top-only radius 6, no border/underline.
//! Trailing `Fill` gap falls through to `draggable` in
//! `NeverLiieIcedWidgets/src/title_bar/mod.rs:628,653`.
//!
//! Overflow via `scrollable` horizontal with `Scrollbar::new()`.

use iced::border::Radius;
use iced::widget::{Responsive, button, container, row, scrollable, space, text};
use iced::{Background, Border, Color, Element, Length, Shadow, Theme};

use crate::event::UiEvent;
use crate::state::UiState;

const ACCENT: Color = Color::from_rgb8(92, 190, 255);
const PANEL_BG: Color = Color::from_rgba8(34, 36, 44, 0.70);
const PANEL_HOVER: Color = Color::from_rgba8(46, 48, 62, 0.82);
const PANEL_PRESSED: Color = Color::from_rgba8(55, 57, 72, 0.87);

fn chip_button_style(active: bool, is_dark: bool) -> impl Fn(&Theme, button::Status) -> button::Style + Clone {
    move |_theme: &Theme, status: button::Status| {
        // Active bg = PANEL_BG, inactive = TRANSPARENT (same for dark/light per spec Q3).
        // Text stays theme-aware for readability on transparent.
        let (base_bg, text_color) = if active {
            (PANEL_BG, Color::WHITE)
        } else if is_dark {
            (Color::TRANSPARENT, Color::from_rgb8(220, 220, 225))
        } else {
            (Color::TRANSPARENT, Color::from_rgb8(40, 40, 45))
        };

        let bg = match status {
            button::Status::Hovered => {
                if active {
                    PANEL_HOVER
                } else if is_dark {
                    Color::from_rgba8(255, 255, 255, 0.08)
                } else {
                    Color::from_rgba8(0, 0, 0, 0.06)
                }
            }
            button::Status::Pressed => {
                if active {
                    PANEL_PRESSED
                } else if is_dark {
                    Color::from_rgba8(255, 255, 255, 0.12)
                } else {
                    Color::from_rgba8(0, 0, 0, 0.10)
                }
            }
            button::Status::Disabled => Color::from_rgba8(60, 60, 65, 0.35),
            button::Status::Active => base_bg,
        };

        let border = Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: Radius::default().top(6.0),
        };

        button::Style {
            background: Some(Background::Color(bg)),
            border,
            shadow: Shadow::default(),
            text_color,
            ..button::Style::default()
        }
    }
}

fn ghost_close_style(_theme: &Theme, status: button::Status) -> button::Style {
    let (bg, txt) = match status {
        button::Status::Hovered => (Color::from_rgba8(255, 70, 70, 0.18), Color::from_rgb8(255, 120, 120)),
        button::Status::Pressed => (Color::from_rgba8(180, 40, 40, 0.25), Color::WHITE),
        button::Status::Active => (Color::TRANSPARENT, Color::from_rgb8(180, 180, 185)),
        button::Status::Disabled => (Color::TRANSPARENT, Color::from_rgba8(100, 100, 100, 0.5)),
    };
    button::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: Radius::default().top(4.0),
        },
        shadow: Shadow::default(),
        text_color: txt,
        ..button::Style::default()
    }
}

fn ghost_add_style(is_dark: bool) -> impl Fn(&Theme, button::Status) -> button::Style + Clone {
    move |_theme: &Theme, status: button::Status| {
        let bg = match status {
            button::Status::Hovered => {
                if is_dark {
                    Color::from_rgba8(255, 255, 255, 0.08)
                } else {
                    Color::from_rgba8(0, 0, 0, 0.06)
                }
            }
            button::Status::Pressed => {
                if is_dark {
                    Color::from_rgba8(255, 255, 255, 0.12)
                } else {
                    Color::from_rgba8(0, 0, 0, 0.10)
                }
            }
            _ => Color::TRANSPARENT,
        };
        let txt = if is_dark { Color::from_rgb8(200, 200, 210) } else { Color::from_rgb8(60, 60, 70) };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: if matches!(status, button::Status::Hovered) {
                    if is_dark { Color::from_rgba8(255, 255, 255, 0.10) } else { Color::from_rgba8(0,0,0,0.08) }
                } else { Color::TRANSPARENT },
                width: if matches!(status, button::Status::Hovered) { 1.0 } else { 0.0 },
                radius: Radius::default().top(6.0),
            },
            shadow: Shadow::default(),
            text_color: txt,
            ..button::Style::default()
        }
    }
}

/// 32px titlebar tab strip — `easyscanlate | proj • × | … + | Fill(drag gap)`.
///
/// `+` is inside the scrollable row immediately after last chip, so it sits
/// right on the side of rightmost tab. The scroll viewport grows with tab
/// count until 80% of titlebar width, then becomes fixed and scrollable.
pub fn view<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    let h = state.titlebar_height();
    let metas = state.tab_metas();
    let active_id = state.active_tab_id();

    Responsive::new(move |size| {
        let is_dark = easyscanlate_settings::get(|s| s.aurora_is_dark);
        let max_w = (size.width * 0.80).max(160.0);
        let n = metas.len() as f32;
        let content_w = n * 160.0 + 28.0 + n * 4.0;
        let viewport_w = content_w.min(max_w);

        let mut chip_plus: Vec<Element<'_, UiEvent>> = Vec::with_capacity(metas.len() + 1);

        for meta in &metas {
            let is_active = meta.id == active_id;
            let is_home = meta.is_home;

            let title: Element<'_, UiEvent> = if is_home {
                let icon_elem: Element<'_, UiEvent> =
                    crate::chrome::title_icon_handle()
                        .map(|h| iced::widget::image(h).width(14).height(14).into())
                        .unwrap_or_else(|| space::horizontal().width(Length::Fixed(0.0)).into());
                let label: Element<'_, UiEvent> =
                    text("easyscanlate").size(12).width(Length::Fill).into();
                row![icon_elem, label]
                    .spacing(6)
                    .align_y(iced::Alignment::Center)
                    .width(Length::Fill)
                    .into()
            } else {
                text(meta.title.clone())
                    .size(12)
                    .width(Length::Fill)
                    .into()
            };

            let dirty: Element<'_, UiEvent> = if meta.dirty {
                container(text("•").size(11).color(ACCENT))
                    .width(Length::Fixed(8.0))
                    .center_x(Length::Fixed(8.0))
                    .center_y(Length::Fixed(14.0))
                    .into()
            } else {
                space::horizontal().width(Length::Fixed(0.0)).into()
            };

            let close: Element<'_, UiEvent> = if is_home {
                space::horizontal()
                    .width(Length::Fixed(14.0))
                    .height(Length::Fixed(14.0))
                    .into()
            } else {
                let id = meta.id;
                button(
                    text("×")
                        .size(11)
                        .width(Length::Fixed(14.0))
                        .height(Length::Fixed(14.0))
                        .center(),
                )
                .width(Length::Fixed(14.0))
                .height(Length::Fixed(14.0))
                .padding(0)
                .style(ghost_close_style)
                .on_press(UiEvent::TabClose(id))
                .into()
            };

            let inner_row: Element<'_, UiEvent> = row![title, dirty, close]
                .spacing(4)
                .align_y(iced::Alignment::Center)
                .width(Length::Fill)
                .height(Length::Fixed(24.0))
                .into();

            let chip_btn: Element<'_, UiEvent> = button(inner_row)
                .width(Length::Fixed(160.0))
                .height(Length::Fixed(24.0))
                .padding([2, 6])
                .style(chip_button_style(is_active, is_dark))
                .on_press(UiEvent::TabSelected(meta.id))
                .into();

            chip_plus.push(chip_btn);
        }

        let add_btn: Element<'_, UiEvent> =
            button(text("+").size(14).width(Length::Fixed(24.0)).height(Length::Fixed(24.0)).center())
                .width(Length::Fixed(28.0))
                .height(Length::Fixed(24.0))
                .padding([2, 6])
                .style(ghost_add_style(is_dark))
                .on_press(UiEvent::TabNew)
                .into();
        chip_plus.push(add_btn);

        let chips_row: Element<'_, UiEvent> = row(chip_plus)
            .spacing(4)
            .align_y(iced::Alignment::Center)
            .width(Length::Shrink)
            .height(Length::Fixed(h))
            .into();

        let chips_scroll: Element<'_, UiEvent> = scrollable::Scrollable::with_direction(
            chips_row,
            scrollable::Direction::Horizontal(scrollable::Scrollbar::new()),
        )
        .width(Length::Fixed(viewport_w))
        .height(Length::Fixed(h))
        .into();

        let scroll_container: Element<'_, UiEvent> = container(chips_scroll)
            .width(Length::Fixed(viewport_w))
            .height(Length::Fixed(h))
            .center_y(Length::Fixed(h))
            .into();

        let drag_gap: Element<'_, UiEvent> = space::horizontal()
            .width(Length::Fill)
            .height(Length::Fixed(h))
            .into();

        row![scroll_container, drag_gap]
            .spacing(4)
            .align_y(iced::Alignment::Center)
            .width(Length::Fill)
            .height(Length::Fixed(h))
            .padding([0, 8])
            .into()
    })
    .width(Length::Fill)
    .height(Length::Fixed(h))
    .into()
}
