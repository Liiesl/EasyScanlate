use iced::widget::{button, center, column, container, mouse_area, opaque, row, stack, text, text_input};
use iced::{Background, Border, Color, Element, Length, Fill as FillLength};
use neverliie_iced_widgets::advanced_dropdown::{Footer, Item, MenuItem, advanced_dropdown};

use crate::event::UiEvent;
use crate::panel::PANEL_BG;
use crate::scale;
use crate::segmented::{BORDER, INPUT_BG, MUTED_FG, TEXT_MAIN};
use crate::state::UiState;

const MODAL_WIDTH: f32 = 640.0;

/// Dropdown option for Series pickers: `None` = standalone ("No series").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesOption {
    pub name: Option<String>,
}

impl std::fmt::Display for SeriesOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.name {
            Some(n) => f.write_str(n),
            None => f.write_str("No series"),
        }
    }
}

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
        selection: crate::accent::accent(),
        icon: MUTED_FG,
    }
}

pub fn view<'a, S: UiState + ?Sized>(state: &'a S, base: Element<'a, UiEvent>) -> Element<'a, UiEvent> {
    let Some(np) = state.new_project_overlay() else {
        return base;
    };
    let source_value = if np.source_paths.is_empty() {
        String::new()
    } else if np.source_paths.len() == 1 {
        np.source_paths[0].clone()
    } else {
        format!("{} files selected", np.source_paths.len())
    };
    let location_value = np.project_location.clone().unwrap_or_default();
    let can_create = !np.source_paths.is_empty() && np.project_location.is_some();

    // Series picker (advanced_dropdown): None = standalone, footer adds new.
    let mut series_entries: Vec<MenuItem<SeriesOption, UiEvent, iced::Theme, iced::Renderer>> =
        Vec::with_capacity(np.available_series.len() + 2);
    series_entries.push(MenuItem::Item(Item::new(
        SeriesOption { name: None },
        "No series",
    )));
    if !np.available_series.is_empty() {
        series_entries.push(MenuItem::Separator);
        series_entries.push(MenuItem::Label("Series"));
        for name in &np.available_series {
            // Skip a duplicate of the selected-but-unknown series (e.g. just
            // created this session): it is still selectable via `selected`.
            series_entries.push(MenuItem::Item(Item::new(
                SeriesOption { name: Some(name.clone()) },
                name.clone(),
            )));
        }
    }
    let series_selected = Some(SeriesOption { name: np.series.clone() });
    let series_dropdown: Element<'_, UiEvent> = advanced_dropdown(
        series_entries,
        series_selected,
        |opt: SeriesOption| UiEvent::NewProjectSeriesSelect(opt.name.clone()),
    )
    .placeholder("No series")
    .searchable(true)
    .text_size(scale::s(12.0))
    .width(FillLength)
    .menu_max_height(240.0)
    .footer(Footer::new(
        "+ New series",
        UiEvent::NewProjectSeriesCreateStart,
    ))
    .into();
    let series_row: Element<'_, UiEvent> = if np.creating_series {
        column![
            row![
                text("Series:").size(scale::s(12.0)).color(Color::WHITE).width(FillLength),
                container(series_dropdown).width(Length::Fixed(scale::s(280.0))),
            ]
            .spacing(scale::s(8.0))
            .align_y(iced::Alignment::Center),
            row![
                text_input("New series name...", &np.new_series_name)
                    .on_input(UiEvent::NewProjectSeriesName)
                    .on_submit(UiEvent::NewProjectSeriesCreateConfirm)
                    .padding(scale::s(6.0))
                    .size(scale::s(12.0))
                    .width(FillLength)
                    .style(input_style),
                button(text("Add").size(scale::s(12.0)).width(FillLength).center())
                    .padding(scale::s(6.0))
                    .width(Length::Fixed(scale::s(90.0)))
                    .style(crate::panel::button_style)
                    .on_press(UiEvent::NewProjectSeriesCreateConfirm),
                button(text("Cancel").size(scale::s(12.0)).width(FillLength).center())
                    .padding(scale::s(6.0))
                    .width(Length::Fixed(scale::s(90.0)))
                    .style(crate::panel::button_style)
                    .on_press(UiEvent::NewProjectSeriesCancel),
            ]
            .spacing(scale::s(8.0))
            .align_y(iced::Alignment::Center),
        ]
        .spacing(scale::s(6.0))
        .into()
    } else {
        row![
            text("Series:").size(scale::s(12.0)).color(Color::WHITE).width(FillLength),
            container(series_dropdown).width(Length::Fixed(scale::s(280.0))),
        ]
        .spacing(scale::s(8.0))
        .align_y(iced::Alignment::Center)
        .into()
    };

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
        series_row,
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
            crate::button::with_disabled_cursor(
                button(text("Create").size(scale::s(12.0)).width(FillLength).center())
                    .padding([scale::s(6.0), scale::s(16.0)])
                    .style(crate::panel::button_style)
                    .on_press_maybe(can_create.then_some(UiEvent::NewProjectCreate))
                    .into(),
            ),
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
