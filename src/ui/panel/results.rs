//! Bottom section: translation controls (model, target language, API key,
//! start button) above the scrollable OCR results list. The list shows the
//! selected profile's display text, so it updates when the profile changes.

use iced::widget::{button, column, pick_list, row, scrollable, text, text_input, Column};
use iced::{Color, Element, Fill as FillLength};

use crate::app::{App, LoadedImage, Message};
use crate::translation;
use crate::ui::panel::{file_name, MUTED_FG};

/// One image header plus its OCR entries in the results list.
fn image_results(image: &LoadedImage) -> Vec<Element<'_, Message>> {
    let mut elements = vec![
        text(file_name(&image.path))
            .size(13)
            .color(Color::from_rgb(0.8, 0.8, 1.0))
            .into(),
    ];
    if image.project.ocr.visible_count() == 0 {
        elements.push(text("  No results yet.").size(12).color(MUTED_FG).into());
    } else {
        for entry in image.project.ocr.visible() {
            let [min_x, min_y, _, _] = entry.quad.bounds();
            elements.push(
                text(format!(
                    "  {:.2}  {}  ({:.0}, {:.0})",
                    entry.score,
                    image.project.display_text(entry),
                    min_x,
                    min_y
                ))
                .size(12)
                .into(),
            );
        }
    }
    elements
}

pub fn view(app: &App) -> Element<'_, Message> {
    let total_results: usize = app.images.iter().map(|i| i.project.ocr.visible_count()).sum();
    let has_entries = total_results > 0;

    let mut results_list: Vec<Element<'_, Message>> = Vec::new();
    for image in &app.images {
        results_list.extend(image_results(image));
    }
    if results_list.is_empty() {
        results_list.push(
            text("No images loaded. Open images to begin.")
                .size(12)
                .color(MUTED_FG)
                .into(),
        );
    }

    column![
        text("Translate").size(16),
        row![
            text("Model:").size(12),
            pick_list(
                translation::MODELS,
                Some(app.translate_model.as_str()),
                |m| Message::TranslateModel(m.to_string()),
            )
            .text_size(12)
            .width(FillLength),
        ]
        .spacing(6),
        row![
            text("To:").size(12),
            pick_list(
                translation::LANGUAGES,
                Some(app.translate_lang.as_str()),
                |l| Message::TranslateLang(l.to_string()),
            )
            .text_size(12)
            .width(FillLength),
        ]
        .spacing(6),
        text_input("API key (optional, in-memory)", &app.translate_api_key)
            .on_input(Message::TranslateApiKey)
            .padding(4)
            .size(12)
            .width(FillLength),
        row![
            button("Translate").on_press_maybe(
                (has_entries && !app.translating && !app.running).then_some(Message::Translate)
            ),
            text(format!(
                "{} image(s), {} result(s)",
                app.images.len(),
                total_results
            ))
            .size(12),
        ]
        .spacing(6),
        text(&app.status).size(12),
        scrollable(Column::with_children(results_list).spacing(2))
            .height(FillLength)
            .width(FillLength),
    ]
    .spacing(8)
    .height(FillLength)
    .into()
}
