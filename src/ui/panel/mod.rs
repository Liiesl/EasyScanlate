//! The right panel, stacked in three sections: an action row on top
//! (open images, start/stop OCR, profile cycling), a styling placeholder
//! below it (upcoming feature), and the OCR results list combined with the
//! translation controls on the bottom.

pub mod actions;
pub mod results;
pub mod styling;

use iced::widget::{column, container};
use iced::{Color, Element, Fill as FillLength};

use crate::app::{App, Message};

pub const PANEL_BG: Color = Color::from_rgb8(34, 36, 44);
pub const MUTED_FG: Color = Color::from_rgb(0.6, 0.6, 0.6);

pub fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

pub fn view(app: &App) -> Element<'_, Message> {
    container(
        column![actions::view(app), styling::view(), results::view(app)]
            .spacing(8)
            .height(FillLength),
    )
    .width(300)
    .height(FillLength)
    .padding(10)
    .style(|_theme| container::Style {
        background: Some(PANEL_BG.into()),
        border: iced::Border::default().rounded(4),
        ..container::Style::default()
    })
    .into()
}
