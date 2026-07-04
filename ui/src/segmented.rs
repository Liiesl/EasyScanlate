//! Shared segmented controls (the "alignment pill" style): used by the
//! styling panel and the main-area mode switcher.

use iced::widget::button::Status;
use iced::widget::{button, container, row, text};
use iced::{Background, Border, Color, Element, Fill as FillLength, Font, Shadow};

use crate::event::UiEvent;
use crate::scale;

/// Accent color of active segments, tabs and glyphs.
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
    button(text(glyph).size(scale::s(12.0)).font(font).width(FillLength).center())
        .width(FillLength)
        .padding([scale::s(8.0), scale::s(0.0)])
        .on_press_maybe(on_press)
        .style(move |_theme, status: Status| {
            let hovered = matches!(status, Status::Hovered | Status::Pressed);
            button::Style {
                background: (active || hovered).then_some(Background::Color(INPUT_HOVER)),
                border: Border::default(),
                shadow: Shadow::default(),
                text_color: if active {
                    ACCENT
                } else if hovered {
                    TEXT_MAIN
                } else {
                    MUTED_FG
                },
                ..button::Style::default()
            }
        })
        .into()
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