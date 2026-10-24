use iced::widget::{
    button, center, column, container, image, mouse_area, opaque, row, scrollable, space, stack,
    text, text_input,
};
use iced::widget::canvas::{self, Geometry, LineDash, Path, Stroke};
use iced::{Background, Border, Color, Element, Length, Point, Rectangle, Size, Fill as FillLength};
use iced::mouse;

use neverliie_iced_widgets::advanced_dropdown::{Footer, Item, MenuItem, advanced_dropdown};

use crate::event::UiEvent;
use crate::panel::PANEL_BG;
use crate::scale;
use crate::segmented::{BORDER, INPUT_BG, MUTED_FG, TEXT_MAIN};
use crate::state::UiState;

const MODAL_WIDTH: f32 = 640.0;
const MODAL_HEIGHT: f32 = 520.0;

const LANGS: [&str; 3] = ["Korean", "Japanese", "Chinese"];

const SUMMARY_BG: Color = Color::from_rgba8(52, 54, 68, 0.55);
const THUMB_W: f32 = 48.0;
const THUMB_H: f32 = 64.0;

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

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
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

fn card_style() -> container::Style {
    container::Style {
        background: Some(SUMMARY_BG.into()),
        border: Border {
            color: BORDER,
            width: scale::s(1.0),
            radius: scale::s(8.0).into(),
        },
        ..Default::default()
    }
}

fn drop_zone_style() -> container::Style {
    // Transparent: the dashed outline is drawn by `DashedDropBorder` canvas.
    container::Style {
        background: None,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: scale::s(10.0).into(),
        },
        ..Default::default()
    }
}

/// Dashed rounded-rect outline for the drop zone (iced containers have no
/// dashed borders). Accent + brighter dash while a file drag hovers.
struct DashedDropBorder {
    hovered: bool,
}

impl canvas::Program<UiEvent> for DashedDropBorder {
    type State = ();

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        // Pure decoration: never steal cursor or events from content below.
        mouse::Interaction::None
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        if bounds.width < 8.0 || bounds.height < 8.0 {
            return vec![];
        }
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let inset = scale::s(1.5);
        let top_left = Point::new(inset, inset);
        let size = Size::new(
            (bounds.width - inset * 2.0).max(1.0),
            (bounds.height - inset * 2.0).max(1.0),
        );
        let path = Path::rounded_rectangle(
            top_left,
            size,
            iced::border::Radius::from(scale::s(10.0)),
        );
        let color = if self.hovered {
            crate::accent::accent()
        } else {
            MUTED_FG
        };
        frame.stroke(
            &path,
            Stroke {
                line_dash: LineDash {
                    segments: &[10.0, 8.0],
                    offset: 0,
                },
                ..Stroke::default()
                    .with_color(color)
                    .with_width(scale::s(1.5))
            },
        );
        vec![frame.into_geometry()]
    }
}

fn link_button<'a>(label: &'a str, on_press: UiEvent) -> Element<'a, UiEvent> {
    button(
        text(label)
            .size(scale::s(12.0))
            .color(crate::accent::accent()),
    )
    .padding([scale::s(4.0), scale::s(8.0)])
    .style(|_, status: button::Status| {
        use button::Status;
        let bg = match status {
            Status::Hovered => Color::from_rgba8(46, 48, 62, 0.6),
            Status::Pressed => Color::from_rgba8(55, 57, 72, 0.7),
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: scale::s(4.0).into(),
                ..Default::default()
            },
            text_color: crate::accent::accent(),
            ..button::Style::default()
        }
    })
    .on_press(on_press)
    .into()
}

fn lang_pill<'a>(label: &'a str, active: bool) -> Element<'a, UiEvent> {
    let lang = label.to_string();
    button(text(label).size(scale::s(12.0)).center())
        .padding([scale::s(7.0), scale::s(14.0)])
        .style(move |_, status: button::Status| {
            use button::Status;
            let accent = crate::accent::accent();
            let bg = if active {
                Color::from_rgba8(90, 60, 160, 0.25)
            } else {
                match status {
                    Status::Hovered => Color::from_rgba8(46, 48, 62, 0.82),
                    Status::Pressed => Color::from_rgba8(55, 57, 72, 0.87),
                    _ => INPUT_BG,
                }
            };
            let border_color = if active { accent } else { BORDER };
            button::Style {
                background: Some(Background::Color(bg)),
                border: Border {
                    color: border_color,
                    width: scale::s(1.0),
                    radius: scale::s(20.0).into(),
                },
                text_color: if active { accent } else { TEXT_MAIN },
                ..button::Style::default()
            }
        })
        .on_press(UiEvent::NewProjectOriginalLang(lang))
        .into()
}

pub fn view<'a, S: UiState + ?Sized>(state: &'a S, base: Element<'a, UiEvent>) -> Element<'a, UiEvent> {
    let Some(np) = state.new_project_overlay() else {
        return base;
    };
    let has_pages = !np.source_paths.is_empty();
    let can_create = has_pages && np.project_location.is_some();

    let title = text("New project")
        .size(scale::s(16.0))
        .color(Color::WHITE);

    let body: Element<'_, UiEvent> = if !has_pages {
        // ---- Empty state: dashed drop zone (transparent, canvas outline) ----
        let drop_inner = column![
            text("Drop a folder or images here")
                .size(scale::s(15.0))
                .color(TEXT_MAIN)
                .width(FillLength)
                .center(),
            text("JPG, PNG, WebP. Pages are sorted by file name.")
                .size(scale::s(12.0))
                .color(MUTED_FG)
                .width(FillLength)
                .center(),
            container(
                row![
                    button(text("Choose folder").size(scale::s(12.0)).center())
                        .padding([scale::s(8.0), scale::s(16.0)])
                        .style(crate::panel::button_style)
                        .on_press(UiEvent::NewProjectSourceFolder),
                    button(text("Choose images").size(scale::s(12.0)).center())
                        .padding([scale::s(8.0), scale::s(16.0)])
                        .style(crate::panel::button_style)
                        .on_press(UiEvent::NewProjectSourceImage),
                ]
                .spacing(scale::s(8.0))
                .align_y(iced::Alignment::Center),
            )
            .width(FillLength)
            .center_x(Length::Fill),
            text("…or input image / folder path below")
                .size(scale::s(11.0))
                .color(MUTED_FG)
                .width(FillLength)
                .center(),
            row![
                text_input("Image or folder path", &np.path_input)
                    .on_input(UiEvent::NewProjectPathInput)
                    .on_submit(UiEvent::NewProjectPathSubmit)
                    .padding(scale::s(6.0))
                    .size(scale::s(12.0))
                    .width(FillLength)
                    .style(input_style),
                button(text("Load").size(scale::s(12.0)).width(FillLength).center())
                    .padding(scale::s(6.0))
                    .width(Length::Fixed(scale::s(90.0)))
                    .style(crate::panel::button_style)
                    .on_press(UiEvent::NewProjectPathSubmit),
            ]
            .spacing(scale::s(8.0))
            .align_y(iced::Alignment::Center),
        ]
        .spacing(scale::s(10.0))
        .align_x(iced::Alignment::Center)
        .width(FillLength);
        let drop_zone: Element<'_, UiEvent> = container(
            stack![
                container(drop_inner)
                    .padding([scale::s(36.0), scale::s(16.0)])
                    .width(FillLength),
                iced::widget::canvas(DashedDropBorder {
                    hovered: np.drag_hovered,
                })
                .width(FillLength)
                .height(FillLength),
            ]
            .width(FillLength),
        )
        .width(FillLength)
        .style(|_| drop_zone_style())
        .into();
        column![title, drop_zone]
            .spacing(scale::s(14.0))
            .into()
    } else {
        // ---- Filled state ----
        let count = np.source_paths.len();
        let folder_name = np
            .source_dir
            .as_deref()
            .and_then(|d| d.rsplit(['\\', '/']).next())
            .filter(|s| !s.is_empty())
            .unwrap_or("images")
            .to_string();
        let summary_title = format!("{folder_name} · {count}");
        let subtitle = format!(
            "{count} pages · {} · {}",
            format_bytes(np.total_bytes),
            np.source_dir.clone().unwrap_or_default()
        );
        let mut thumb_row = row![].spacing(scale::s(6.0)).align_y(iced::Alignment::Center);
        let shown = np.thumbs.len().min(8);
        for handle in np.thumbs.iter().take(8) {
            let thumb: Element<'_, UiEvent> = container(
                image(handle.clone())
                    .width(Length::Fixed(scale::s(THUMB_W)))
                    .height(Length::Fixed(scale::s(THUMB_H)))
                    .content_fit(iced::ContentFit::Fill)
                    .border_radius(scale::s(4.0)),
            )
            .width(Length::Fixed(scale::s(THUMB_W)))
            .height(Length::Fixed(scale::s(THUMB_H)))
            .style(|_| container::Style {
                background: Some(INPUT_BG.into()),
                border: Border {
                    color: BORDER,
                    width: scale::s(1.0),
                    radius: scale::s(4.0).into(),
                },
                ..Default::default()
            })
            .into();
            thumb_row = thumb_row.push(thumb);
        }
        // Placeholders while thumbs decode (or failed): keep strip shape stable.
        if shown == 0 {
            for _ in 0..count.min(8) {
                let ph: Element<'_, UiEvent> = container(space::horizontal())
                    .width(Length::Fixed(scale::s(THUMB_W)))
                    .height(Length::Fixed(scale::s(THUMB_H)))
                    .style(|_| container::Style {
                        background: Some(Color::from_rgba8(60, 50, 80, 0.5).into()),
                        border: Border {
                            color: BORDER,
                            width: scale::s(1.0),
                            radius: scale::s(4.0).into(),
                        },
                        ..Default::default()
                    })
                    .into();
                thumb_row = thumb_row.push(ph);
            }
        }
        if count > shown {
            thumb_row = thumb_row.push(
                text(format!("+{}", count - shown))
                    .size(scale::s(12.0))
                    .color(MUTED_FG),
            );
        }
        let summary: Element<'_, UiEvent> = container(
            column![
                row![
                    text(summary_title)
                        .size(scale::s(14.0))
                        .color(TEXT_MAIN)
                        .width(FillLength),
                    link_button("Change", UiEvent::NewProjectSourceFolder),
                ]
                .align_y(iced::Alignment::Center),
                text(subtitle).size(scale::s(11.0)).color(MUTED_FG),
                thumb_row,
            ]
            .spacing(scale::s(8.0)),
        )
        .padding(scale::s(12.0))
        .width(FillLength)
        .style(|_| card_style())
        .into();

        let mut pills = row![].spacing(scale::s(8.0)).align_y(iced::Alignment::Center);
        for lang in LANGS {
            pills = pills.push(lang_pill(lang, np.original_lang == lang));
        }

        let location = np.project_location.clone().unwrap_or_default();
        let save_card: Element<'_, UiEvent> = container(
            row![
                text(location)
                    .size(scale::s(12.0))
                    .color(MUTED_FG)
                    .width(FillLength),
                link_button("Change", UiEvent::NewProjectLocationBrowse),
            ]
            .spacing(scale::s(8.0))
            .align_y(iced::Alignment::Center),
        )
        .padding([scale::s(8.0), scale::s(12.0)])
        .width(FillLength)
        .style(|_| card_style())
        .into();

        let divider: Element<'_, UiEvent> = container(space::horizontal().width(FillLength))
            .width(FillLength)
            .height(Length::Fixed(scale::s(1.0)))
            .style(|_| container::Style {
                background: Some(BORDER.into()),
                ..Default::default()
            })
            .into();

        let series_section: Element<'_, UiEvent> = if np.series_open || np.series.is_some() {
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
                    series_entries.push(MenuItem::Item(Item::new(
                        SeriesOption {
                            name: Some(name.clone()),
                        },
                        name.clone(),
                    )));
                }
            }
            let series_selected = Some(SeriesOption {
                name: np.series.clone(),
            });
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
            let series_picker: Element<'_, UiEvent> = if np.creating_series {
                column![
                    series_dropdown,
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
                series_dropdown
            };
            column![divider, series_picker,]
            .spacing(scale::s(8.0))
            .into()
        } else {
            column![
                divider,
                button(text("+ Add to a series").size(scale::s(12.0)).color(MUTED_FG))
                    .padding(scale::s(4.0))
                    .style(|_, status: button::Status| {
                        use button::Status;
                        button::Style {
                            background: Some(Background::Color(match status {
                                Status::Hovered => Color::from_rgba8(46, 48, 62, 0.6),
                                _ => Color::TRANSPARENT,
                            })),
                            text_color: MUTED_FG,
                            ..button::Style::default()
                        }
                    })
                    .on_press(UiEvent::NewProjectSeriesToggle),
            ]
            .spacing(scale::s(4.0))
            .into()
        };

        column![
            title,
            summary,
            column![
                text("Project name").size(scale::s(12.0)).color(Color::WHITE),
                text_input("Project name...", &np.project_name)
                    .on_input(UiEvent::NewProjectName)
                    .padding(scale::s(8.0))
                    .size(scale::s(12.0))
                    .width(FillLength)
                    .style(input_style),
            ]
            .spacing(scale::s(6.0)),
            column![
                text("Original language").size(scale::s(12.0)).color(Color::WHITE),
                pills,
            ]
            .spacing(scale::s(6.0)),
            column![
                text("Save to").size(scale::s(12.0)).color(Color::WHITE),
                save_card,
            ]
            .spacing(scale::s(6.0)),
            series_section,
        ]
        .spacing(scale::s(12.0))
        .into()
    };

    let footer_hint = if has_pages {
        format!("{} pages, {}", np.source_paths.len(), np.original_lang)
    } else {
        "Add pages to get started.".to_string()
    };
    let create_label = if has_pages && !np.project_name.trim().is_empty() {
        format!("Create “{}”", np.project_name.trim())
    } else {
        "Create project".to_string()
    };
    let create_btn: Element<'_, UiEvent> = if can_create {
        button(text(create_label).size(scale::s(12.0)).center())
            .padding([scale::s(8.0), scale::s(16.0)])
            .style(|_, status: button::Status| {
                use button::Status;
                let accent = crate::accent::accent();
                let bg = match status {
                    Status::Hovered => crate::accent::accent_hover(),
                    Status::Pressed => accent,
                    Status::Disabled => Color::from_rgba8(34, 36, 44, 0.35),
                    Status::Active => accent,
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    border: Border {
                        radius: scale::s(6.0).into(),
                        ..Default::default()
                    },
                    text_color: Color::WHITE,
                    ..button::Style::default()
                }
            })
            .on_press(UiEvent::NewProjectCreate)
            .into()
    } else {
        crate::button::with_disabled_cursor(
            button(text(create_label).size(scale::s(12.0)).center())
                .padding([scale::s(8.0), scale::s(16.0)])
                .style(crate::panel::button_style)
                .on_press_maybe(None)
                .into(),
        )
    };
    let footer = row![
        text(footer_hint).size(scale::s(12.0)).color(MUTED_FG).width(FillLength),
        button(text("Cancel").size(scale::s(12.0)).center())
            .padding([scale::s(8.0), scale::s(16.0)])
            .style(crate::panel::button_style)
            .on_press(UiEvent::NewProjectClose),
        create_btn,
    ]
    .spacing(scale::s(8.0))
    .align_y(iced::Alignment::Center);

    let scroll_body: Element<'_, UiEvent> = scrollable(body)
        .spacing(scale::s(crate::scroll::EMBEDDED_SPACING))
        .height(FillLength)
        .width(FillLength)
        .into();
    let content = column![scroll_body, footer]
        .spacing(scale::s(12.0))
        .height(FillLength);

    let window = container(content)
        .width(Length::Fixed(scale::s(MODAL_WIDTH)))
        .height(Length::Fixed(scale::s(MODAL_HEIGHT)))
        .padding(scale::s(20.0))
        .style(|_| container::Style {
            background: Some(PANEL_BG.into()),
            border: iced::Border::default()
                .rounded(scale::s(10.0))
                .color(Color::from_rgb8(90, 60, 160))
                .width(scale::s(1.0)),
            ..Default::default()
        });

    // Blurred snapshot cropped to this window rect at capture time, stacked
    // directly behind the panel (same fixed size, so always aligned). The dim
    // outside the panel stays plain dim over the live base.
    let blur_cover: Element<'_, UiEvent> = match state.backdrop_blur() {
        Some(handle) => container(
            image(handle)
                .width(Length::Fixed(scale::s(MODAL_WIDTH)))
                .height(Length::Fixed(scale::s(MODAL_HEIGHT)))
                .content_fit(iced::ContentFit::Fill)
                .border_radius(scale::s(10.0)),
        )
        .width(Length::Fixed(scale::s(MODAL_WIDTH)))
        .height(Length::Fixed(scale::s(MODAL_HEIGHT)))
        .into(),
        // No capture yet (tests / capture in flight): just the window on its own.
        None => space::horizontal()
            .width(Length::Fixed(0.0))
            .height(Length::Fixed(0.0))
            .into(),
    };

    let overlay = center(opaque(stack![blur_cover, window])).style(|_| container::Style {
        background: Some(Color { a: 0.55, ..Color::BLACK }.into()),
        ..Default::default()
    });

    stack![base, opaque(mouse_area(overlay).on_press(UiEvent::NewProjectClose))].into()
}
