//! Shared segmented controls (the "alignment pill" style): used by the
//! styling panel and the main-area mode switcher.

use iced::widget::button::Status;
use iced::widget::{button, container, row, text};
use iced::{Background, Border, Color, Element, Fill as FillLength, Font, Shadow};
use lucide_icons::Icon;

use crate::event::UiEvent;
use crate::scale;

/// Accent color of active segments, tabs and glyphs.
/// Aurora-synced at render time via `crate::accent`; kept as fallback for
/// callers that need a const context.
pub const ACCENT: Color = Color::from_rgb8(92, 190, 255);
/// Near-white label color, for active tab labels.
pub const TEXT_MAIN: Color = Color::from_rgb8(243, 244, 246);
/// Fill of inputs and segmented groups (darker than the panel background).
pub const INPUT_BG: Color = Color::from_rgb8(20, 20, 25);
/// Hover / active-segment fill.
pub const INPUT_HOVER: Color = Color::from_rgb8(29, 30, 38);
/// Border of inputs and segmented groups.
pub const BORDER: Color = Color::from_rgb8(50, 50, 62);
/// Muted, inactive label color.
pub const MUTED_FG: Color = Color::from_rgb(0.6, 0.6, 0.6);

/// One cell of a segmented control: equal-width button that lights up with
/// the accent when `active`. `on_press` is `None` (inert); the disabled
/// state renders identically to the idle one.
pub fn segment<'a>(
    active: bool,
    glyph: &'a str,
    on_press: Option<UiEvent>,
    font: Font,
) -> Element<'a, UiEvent> {
    crate::button::with_disabled_cursor(
        button(text(glyph).size(scale::s(12.0)).font(font).width(FillLength).center())
            .width(FillLength)
            .padding([scale::s(8.0), scale::s(0.0)])
            .on_press_maybe(on_press)
            .style(move |_theme, status: Status| {
                let bg = match status {
                    Status::Disabled => Color::from_rgba8(34, 36, 44, 0.35),
                    Status::Hovered => Color::from_rgba8(46, 48, 62, 0.82),
                    Status::Pressed => Color::from_rgba8(55, 57, 72, 0.87),
                    Status::Active => crate::panel::PANEL_BG,
                };
                let txt = match status {
                    Status::Disabled => MUTED_FG,
                    _ if active => crate::accent::accent(),
                    _ => TEXT_MAIN,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border::default(),
                    shadow: Shadow::default(),
                    text_color: txt,
                    ..button::Style::default()
                }
            })
            .into(),
    )
}

/// One cell of a segmented control with a Lucide icon.
pub fn segment_icon<'a>(
    active: bool,
    icon: Icon,
    on_press: Option<UiEvent>,
) -> Element<'a, UiEvent> {
    crate::button::with_disabled_cursor(
        button(crate::icon::lucide(icon).size(scale::s(12.0)).width(FillLength).center())
            .width(FillLength)
            .padding([scale::s(4.0), scale::s(0.0)])
            .on_press_maybe(on_press)
            .style(move |_theme, status: Status| {
                let bg = match status {
                    Status::Disabled => Color::from_rgba8(34, 36, 44, 0.35),
                    Status::Hovered => Color::from_rgba8(46, 48, 62, 0.82),
                    Status::Pressed => Color::from_rgba8(55, 57, 72, 0.87),
                    Status::Active => crate::panel::PANEL_BG,
                };
                let txt = match status {
                    Status::Disabled => MUTED_FG,
                    _ if active => crate::accent::accent(),
                    _ => TEXT_MAIN,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border::default(),
                    shadow: Shadow::default(),
                    text_color: txt,
                    ..button::Style::default()
                }
            })
            .into(),
    )
}

/// A bordered pill holding equally-sized [`segment`]s.
pub fn segmented_group<'a>(segments: Vec<Element<'a, UiEvent>>) -> Element<'a, UiEvent> {
    container(row(segments).spacing(scale::s(2.0)))
        .padding(scale::s(2.0))
        .width(FillLength)
        .style(|_theme| container::Style {
            background: Some(INPUT_BG.into()),
            border: Border {
                radius: scale::s(4.0).into(),
                width: scale::s(1.0),
                color: BORDER,
            },
            ..container::Style::default()
        })
        .into()
}