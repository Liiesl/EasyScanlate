//! Shared panel primitives (backgrounds, button style). Layout lives in
//! `crate::shell`: the left column holds the results column (a pinned
//! header with the "TRANSLATION" label over the scrollable OCR results
//! list over the translation bar, with the inpaint/layers list below it),
//! the narrow toolbar sits in the middle, and the right side holds the
//! action row (open images, start/stop OCR, live status, settings) over the
//! main canvas and the styling inspector.

pub mod actions;
pub mod inpaint;
pub mod results;
pub mod styling;

use iced::{Background, Border, Color, Shadow};

pub const PANEL_BG: Color = Color::from_rgba8(34, 36, 44, 0.70);
pub const PANEL_BG_SOLID: Color = Color::from_rgb8(34, 36, 44);
pub use crate::segmented::MUTED_FG;

pub fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Shared button style: background matches the panel, with distinct
/// fills for `Hovered` / `Pressed` / `Disabled` so interaction is visible.
pub fn button_style(
    _theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    use iced::widget::button::Status;
    let bg = match status {
        Status::Active => PANEL_BG,
        Status::Hovered => Color::from_rgba8(46, 48, 62, 0.82),
        Status::Pressed => Color::from_rgba8(55, 57, 72, 0.87),
        Status::Disabled => Color::from_rgba8(34, 36, 44, 0.35),
    };
    let txt = match status {
        Status::Disabled => crate::segmented::MUTED_FG,
        _ => crate::segmented::TEXT_MAIN,
    };
    iced::widget::button::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            radius: crate::scale::s(4.0).into(),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        shadow: Shadow::default(),
        text_color: txt,
        ..iced::widget::button::Style::default()
    }
}
