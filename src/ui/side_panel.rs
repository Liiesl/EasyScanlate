use iced::widget::{button, column, container, row, scrollable, text, Column};
use iced::{Color, Element, Fill as FillLength};

use crate::app::{App, Message};

fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

pub fn view(app: &App) -> Element<'_, Message> {
    let mut results_list: Vec<Element<'_, Message>> = Vec::new();
    for image in &app.images {
        results_list.push(
            text(file_name(&image.path)).size(13).color(Color::from_rgb(0.8, 0.8, 1.0)).into(),
        );
        if image.project.ocr.visible_count() == 0 {
            results_list.push(
                text("  No results yet.")
                    .size(12)
                    .color(Color::from_rgb(0.6, 0.6, 0.6))
                    .into(),
            );
        } else {
            for entry in image.project.ocr.visible() {
                let [min_x, min_y, _, _] = entry.quad.bounds();
                results_list.push(
                    text(format!(
                        "  {:.2}  {}  ({:.0}, {:.0})",
                        entry.score, entry.text, min_x, min_y
                    ))
                    .size(12)
                    .into(),
                );
            }
        }
    }
    if results_list.is_empty() {
        results_list.push(
            text("No images loaded. Open images to begin.")
                .size(12)
                .color(Color::from_rgb(0.6, 0.6, 0.6))
                .into(),
        );
    }

    let total_results: usize = app.images.iter().map(|i| i.project.ocr.visible_count()).sum();

    container(
        column![
            text("Scanlateit").size(24),
            button("Open Images...").on_press(Message::OpenImages),
            row![
                button("Start OCR").on_press_maybe(
                    (!app.images.is_empty() && !app.running).then_some(Message::StartOcr)
                ),
                button("Stop").on_press_maybe(app.running.then_some(Message::StopOcr)),
            ]
            .spacing(6),
            row![
                text(format!(
                    "Profile: {}",
                    app.images
                        .first()
                        .map(|i| i.project.profiles.selected().name.clone())
                        .unwrap_or_else(|| "Default".to_string())
                ))
                .size(12),
                button("Next").on_press_maybe(
                    (app.images.first().is_some_and(|i| i.project.profiles.len() > 1))
                        .then_some(Message::CycleProfile)
                ),
            ]
            .spacing(6),
            text(&app.status).size(12),
            text(format!("{} image(s), {} result(s)", app.images.len(), total_results)).size(13),
            scrollable(Column::with_children(results_list).spacing(2))
                .height(FillLength)
                .width(FillLength),
        ]
        .spacing(8),
    )
    .width(300)
    .height(FillLength)
    .padding(10)
    .style(|_theme| container::Style {
        background: Some(Color::from_rgb8(34, 36, 44).into()),
        border: iced::Border::default().rounded(4),
        ..container::Style::default()
    })
    .into()
}
