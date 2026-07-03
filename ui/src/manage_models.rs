//! Manage Models overlay: opened from Translation settings, lets the user
//! toggle each model per provider. Deprecated models are never listed here
//! (they are filtered at the provider layer). The basic configuration (no
//! hidden entry) shows the default latest-per-family filtered list; hiding
//! is persisted to `settings.json`.

use iced::widget::{
    button, center, checkbox, column, container, mouse_area, opaque, row, scrollable, space,
    stack, text,
};
use iced::{Color, Element, Fill as FillLength, Length};

use crate::event::UiEvent;
use crate::panel::PANEL_BG;
use crate::state::UiState;

const MODAL_WIDTH: f32 = 540.0;
const MODAL_HEIGHT: f32 = 460.0;
const MUTED_FG: Color = Color::from_rgb(0.6, 0.6, 0.6);

pub fn view<'a, S: UiState + ?Sized>(
    state: &'a S,
    base: Element<'a, UiEvent>,
) -> Element<'a, UiEvent> {
    let groups = state.all_model_groups();

    let header = row![
        text("Manage Models").size(16),
        space::horizontal(),
        button(text("✕")).padding(2).on_press(UiEvent::ManageModelsClose),
    ];

    let description = column![
        text("Toggle models per provider. Hidden models disappear from the translation dropdown.")
            .size(11)
            .color(MUTED_FG),
        text("Deprecated models are always hidden and never shown here.")
            .size(11)
            .color(MUTED_FG),
    ]
    .spacing(2);

    let body: Element<'_, UiEvent> = if groups.is_empty() {
        container(
            text("No connected providers – connect a translation service first.")
                .size(12)
                .color(MUTED_FG),
        )
        .padding(12)
        .into()
    } else {
        let mut provider_cols: Vec<Element<'_, UiEvent>> = Vec::new();
        for (provider_id, provider_name, models) in groups {
            let mut rows: Vec<Element<'_, UiEvent>> = Vec::new();
            rows.push(
                row![
                    text(provider_name.clone()).size(13),
                    space::horizontal(),
                    button(text("Show all").size(10))
                        .padding([2, 6])
                        .on_press(UiEvent::ManageModelsReset(provider_id.clone())),
                ]
                .align_y(iced::Alignment::Center)
                .spacing(6)
                .into(),
            );
            for model in models {
                let hidden = state.is_model_hidden(&provider_id, &model);
                let visible = !hidden;
                let pid = provider_id.clone();
                let mid = model.clone();
                rows.push(
                    checkbox(visible)
                        .label(model.clone())
                        .text_size(12)
                        .on_toggle(move |v| UiEvent::ManageModelsToggle {
                            provider: pid.clone(),
                            model: mid.clone(),
                            visible: v,
                        })
                        .into(),
                );
            }
            // Provider card
            let card = container(column(rows).spacing(4))
                .padding(8)
                .style(|_theme| container::Style {
                    background: Some(Color::from_rgba8(255, 255, 255, 0.06).into()),
                    border: iced::Border::default().rounded(6),
                    ..Default::default()
                })
                .width(FillLength)
                .into();
            provider_cols.push(card);
        }
        scrollable(column(provider_cols).spacing(10))
            .height(Length::Fill)
            .into()
    };

    let footer = row![
        button(text("Reset all").size(11))
            .padding([4, 10])
            .on_press(UiEvent::ManageModelsResetAll),
        space::horizontal(),
        button(text("Close").size(11))
            .padding([4, 10])
            .on_press(UiEvent::ManageModelsClose),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);

    let window = container(
        column![header, description, body, footer]
            .spacing(10)
            .height(FillLength),
    )
    .width(Length::Fixed(MODAL_WIDTH))
    .height(Length::Fixed(MODAL_HEIGHT))
    .padding(12)
    .style(|_theme| container::Style {
        background: Some(PANEL_BG.into()),
        border: iced::Border::default()
            .rounded(8)
            .color(Color::from_rgb8(60, 63, 74))
            .width(1),
        ..container::Style::default()
    });

    stack![
        base,
        opaque(
            mouse_area(
                center(opaque(window)).style(|_theme| container::Style {
                    background: Some(
                        Color {
                            a: 0.55,
                            ..Color::BLACK
                        }
                        .into()
                    ),
                    ..container::Style::default()
                })
            )
            .on_press(UiEvent::ManageModelsClose)
        )
    ]
    .into()
}
