//! The right panel, laid out in two rows. The top row holds the action
//! buttons (open images, start/stop OCR, live status, settings). Below it
//! two columns share the remaining height: the styling controls on the left
//! and, on the right, the results column — a pinned header with the
//! "TRANSLATION" label and the profile dropdown over the tall scrollable OCR
//! results list over the short translation bar (the merged model dropdown,
//! the language picker and the translate button).

pub mod actions;
pub mod inpaint;
pub mod results;
pub mod styling;

use iced::widget::{column, container, row};
use iced::{Background, Border, Color, Element, Fill as FillLength, Length, Shadow};

use crate::event::UiEvent;
use crate::scale;
use crate::state::UiState;

pub const PANEL_BG: Color = Color::from_rgba8(34, 36, 44, 0.70);
pub const PANEL_BG_SOLID: Color = Color::from_rgb8(34, 36, 44);
pub use crate::segmented::MUTED_FG;

/// Width share of the styling column vs the results column (weighted so the
/// results list keeps each row's two side-by-side boxes readable).
pub const STYLE_COL_PORTS: u16 = 5;
pub const RESULTS_COL_PORTS: u16 = 9;

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

pub fn view<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    container(
        column![
            actions::view(state),
            row![
                container(styling::view(state)).width(Length::FillPortion(STYLE_COL_PORTS)),
                container(results::view(state)).width(Length::FillPortion(RESULTS_COL_PORTS)),
            ]
            .spacing(scale::s(8.0))
            .height(FillLength),
        ]
        .spacing(scale::s(8.0))
        .width(FillLength)
        .height(FillLength),
    )
    .width(FillLength)
    .height(FillLength)
    .padding(scale::s(10.0))
    .style(|_theme| container::Style {
        background: Some(PANEL_BG.into()),
        border: iced::Border::default().rounded(scale::s(4.0)),
        ..container::Style::default()
    })
    .into()
}
