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
use iced::{Color, Element, Fill as FillLength, Length};

use crate::event::UiEvent;
use crate::scale;
use crate::state::UiState;

pub const PANEL_BG: Color = Color::from_rgba8(34, 36, 44, 0.78);
pub const PANEL_BG_SOLID: Color = Color::from_rgb8(34, 36, 44);
pub use crate::segmented::MUTED_FG;

/// Width share of the styling column vs the results column (weighted so the
/// results list keeps each row's two side-by-side boxes readable).
pub const STYLE_COL_PORTS: u16 = 5;
pub const RESULTS_COL_PORTS: u16 = 9;

pub fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
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
