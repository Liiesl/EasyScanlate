use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Color, Element, Length, Fill as FillLength};

use crate::event::UiEvent;
use crate::panel::PANEL_BG;
use crate::scale;

const SIDEBAR_WIDTH: f32 = 200.0;

fn sidebar_button(label: &str, event: UiEvent) -> Element<'_, UiEvent> {
    button(text(label).size(scale::s(13.0)).width(FillLength).center())
        .width(Length::Fill)
        .padding(scale::s(14.0))
        .style(crate::panel::button_style)
        .on_press(event)
        .into()
}

pub fn view<'a, S: crate::state::UiState + ?Sized>(state: &'a S) -> Element<'a, UiEvent> {
    let sidebar = container(
        column![
            sidebar_button("New Project", UiEvent::HomeNewProject),
            sidebar_button("Open Project", UiEvent::HomeOpenProject),
            sidebar_button("Settings", UiEvent::HomeSettings),
        ]
        .spacing(scale::s(12.0))
        .width(Length::Fill),
    )
    .width(Length::Fixed(scale::s(SIDEBAR_WIDTH)))
    .height(Length::Fill)
    .padding(scale::s(12.0))
    .style(|_| container::Style {
        background: Some(Color::from_rgba8(20, 20, 20, 0.85).into()),
        border: iced::Border::default().rounded(scale::s(12.0)),
        ..Default::default()
    });

    // Recent projects header
    let header = container(
        row![
            text("Name").size(scale::s(12.0)).color(Color::WHITE).width(FillLength),
            text("Last Opened").size(scale::s(12.0)).color(Color::WHITE).width(Length::Fixed(scale::s(140.0))),
        ]
        .spacing(scale::s(8.0)),
    )
    .padding(scale::s(12.0))
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(Color::from_rgba8(60, 60, 65, 0.9).into()),
        border: iced::Border::default().rounded(scale::s(8.0)),
        ..Default::default()
    });

    let recents = state.recent_projects();
    let rows: Element<'_, UiEvent> = if recents.is_empty() {
        container(
            text("No recent projects. Create or open one to begin.")
                .size(scale::s(13.0))
                .color(Color::from_rgb(0.6, 0.6, 0.6)),
        )
        .padding(scale::s(20.0))
        .width(Length::Fill)
        .center_x(Length::Fill)
        .into()
    } else {
        let mut col = column![].spacing(scale::s(2.0));
        for rp in recents {
            let path = rp.path.clone();
            let name = rp.name.clone();
            let rel = easyscanlate_settings::format_relative(rp.last_opened);
            let row_btn = button(
                row![
                    text(name).size(scale::s(13.0)).color(Color::WHITE).width(FillLength),
                    text(rel).size(scale::s(12.0)).color(Color::from_rgb(0.6, 0.6, 0.6)).width(Length::Fixed(scale::s(140.0))),
                ]
                .spacing(scale::s(8.0))
                .align_y(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .padding(scale::s(10.0))
            .style(|_, status| {
                let bg = if status == iced::widget::button::Status::Hovered {
                    Color::from_rgba8(255, 255, 255, 0.08)
                } else {
                    Color::TRANSPARENT
                };
                iced::widget::button::Style {
                    background: Some(bg.into()),
                    border: iced::Border::default().rounded(scale::s(6.0)),
                    ..Default::default()
                }
            })
            .on_press(UiEvent::HomeRecentClicked(path));
            col = col.push(row_btn);
        }
        scrollable(col).height(Length::Fill).into()
    };

    let main = container(
        column![
            text("Recent Projects").size(scale::s(22.0)).color(Color::WHITE),
            column![header, rows].spacing(scale::s(6.0)),
        ]
        .spacing(scale::s(16.0)),
    )
    .padding(scale::s(16.0))
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|_| container::Style {
        background: Some(PANEL_BG.into()),
        border: iced::Border::default().rounded(scale::s(12.0)),
        ..Default::default()
    });

    let content = row![sidebar, main].spacing(scale::s(12.0)).height(Length::Fill);

    container(content)
        .padding(scale::s(16.0))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
