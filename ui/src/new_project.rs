use iced::widget::{button, center, column, container, mouse_area, opaque, row, stack, text, text_input};
use iced::{Background, Border, Color, Element, Length, Fill as FillLength};

use crate::event::UiEvent;
use crate::panel::PANEL_BG;
use crate::scale;
use crate::segmented::{ACCENT, BORDER, INPUT_BG, MUTED_FG, TEXT_MAIN};
use crate::state::UiState;

const MODAL_WIDTH: f32 = 640.0;

fn input_style(_theme: &iced::Theme, _status: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: Background::Color(INPUT_BG),
        border: Border {
            color: BORDER,
            width: scale::s(1.0),
            radius: scale::s(4.0).into(),
        },
        placeholder: MUTED_FG,
        value: TEXT_MAIN,
        selection: ACCENT,
        icon: MUTED_FG,
    }
}

pub fn view<'a, S: UiState + ?Sized>(state: &'a S, base: Element<'a, UiEvent>) -> Element<'a, UiEvent> {
    let Some(np) = state.new_project_overlay() else {
        return base;
    };
    let np = np;
    let source_value = if np.source_paths.is_empty() {
        String::new()
    } else if np.source_paths.len() == 1 {
        np.source_paths[0].clone()
    } else {
        format!("{} files selected", np.source_paths.len())
    };
    let location_value = np.project_location.clone().unwrap_or_default();
    let can_create = !np.source_paths.is_empty() && np.project_location.is_some();

    let content = column![
        text("New Project").size(scale::s(16.0)).color(Color::WHITE),
        // Source row
        column![
            text("Source:").size(scale::s(12.0)).color(Color::WHITE),
            row![
                text_input("Select an image or folder...", &source_value)
                    .padding(scale::s(6.0))
                    .size(scale::s(12.0))
                    .width(FillLength)
                    .style(input_style),
                button(text("Image").size(scale::s(12.0)).width(FillLength).center())
                    .padding(scale::s(6.0))
                    .width(Length::Fixed(scale::s(90.0)))
                    .style(crate::panel::button_style)
                    .on_press(UiEvent::NewProjectSourceImage),
                button(text("Folder").size(scale::s(12.0)).width(FillLength).center())
                    .padding(scale::s(6.0))
                    .width(Length::Fixed(scale::s(90.0)))
                    .style(crate::panel::button_style)
                    .on_press(UiEvent::NewProjectSourceFolder),
            ]
            .spacing(scale::s(8.0))
            .align_y(iced::Alignment::Center),
        ]
        .spacing(scale::s(6.0)),
        // Original language row (CJK placeholder, no persistence)
        row![
            text("Original Language:").size(scale::s(12.0)).color(Color::WHITE).width(FillLength),
            container(
                iced::widget::pick_list(
                    vec!["Korean".to_string(), "Japanese".to_string(), "Chinese".to_string()],
                    Some(np.original_lang.clone()),
                    UiEvent::NewProjectOriginalLang,
                )
                .padding(scale::s(6.0))
                .text_size(scale::s(12.0))
                .width(Length::Fixed(scale::s(200.0)))
            )
            .width(Length::Shrink),
        ]
        .spacing(scale::s(8.0))
        .align_y(iced::Alignment::Center),
        // Project location
        column![
            text("Project Location:").size(scale::s(12.0)).color(Color::WHITE),
            row![
                text_input("Choose project save location...", &location_value)
                    .padding(scale::s(6.0))
                    .size(scale::s(12.0))
                    .width(FillLength)
                    .style(input_style),
                button(text("Browse").size(scale::s(12.0)).width(FillLength).center())
                    .padding(scale::s(6.0))
                    .width(Length::Fixed(scale::s(90.0)))
                    .style(crate::panel::button_style)
                    .on_press(UiEvent::NewProjectLocationBrowse),
            ]
            .spacing(scale::s(8.0))
            .align_y(iced::Alignment::Center),
        ]
        .spacing(scale::s(6.0)),
        // Buttons
        row![
            iced::widget::space::horizontal().width(FillLength),
            button(text("Create").size(scale::s(12.0)).width(FillLength).center())
                .padding([scale::s(6.0), scale::s(16.0)])
                .style(crate::panel::button_style)
                .on_press_maybe(can_create.then_some(UiEvent::NewProjectCreate)),
            button(text("Cancel").size(scale::s(12.0)).width(FillLength).center())
                .padding([scale::s(6.0), scale::s(16.0)])
                .style(crate::panel::button_style)
                .on_press(UiEvent::NewProjectClose),
        ]
        .spacing(scale::s(8.0)),
    ]
    .spacing(scale::s(14.0));

    let window = container(content)
        .width(Length::Fixed(scale::s(MODAL_WIDTH)))
        .padding(scale::s(16.0))
        .style(|_| container::Style {
            background: Some(PANEL_BG.into()),
            border: iced::Border::default()
                .rounded(scale::s(10.0))
                .color(Color::from_rgb8(90, 60, 160))
                .width(scale::s(1.0)),
            ..Default::default()
        });

    let overlay = center(opaque(window)).style(|_| container::Style {
        background: Some(Color { a: 0.45, ..Color::BLACK }.into()),
        ..Default::default()
    });

    stack![base, opaque(mouse_area(overlay).on_press(UiEvent::NewProjectClose))].into()
}
