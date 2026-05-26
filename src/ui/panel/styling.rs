//! Middle section: placeholder rectangle for the upcoming styling feature.

use iced::widget::{container, text};
use iced::{Color, Element, Fill as FillLength};

use crate::app::Message;

const PLACEHOLDER_BG: Color = Color::from_rgb8(42, 45, 55);
const PLACEHOLDER_FG: Color = Color::from_rgb8(130, 135, 150);

pub fn view() -> Element<'static, Message> {
    container(
        text("Styling — coming soon")
            .size(12)
            .color(PLACEHOLDER_FG)
            .width(FillLength)
            .center(),
    )
    .width(FillLength)
    .height(90)
    .padding(10)
    .style(|_theme| container::Style {
        background: Some(PLACEHOLDER_BG.into()),
        border: iced::Border::default().rounded(4),
        ..container::Style::default()
    })
    .into()
}
