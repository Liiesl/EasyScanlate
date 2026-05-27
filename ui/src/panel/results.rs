//! Bottom section: translation controls (model, target language, API key,
//! start button) above the scrollable OCR results list. The list shows the
//! selected profile's display text, so it updates when the profile changes.

use iced::widget::{button, column, pick_list, row, scrollable, text, text_input, Column};
use iced::{Color, Element, Fill as FillLength};

use crate::event::UiEvent;
use crate::loaded::LoadedImage;
use crate::panel::{file_name, MUTED_FG};
use crate::state::UiState;
use scanlateit_translation as translation;

/// One image header plus its OCR entries in the results list.
fn image_results(image: &LoadedImage) -> Vec<Element<'_, UiEvent>> {
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

pub fn view<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    let total_results: usize = state.images().iter().map(|i| i.project.ocr.visible_count()).sum();
    let has_entries = total_results > 0;

    let mut results_list: Vec<Element<'_, UiEvent>> = Vec::new();
    for image in state.images() {
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
                Some(state.translate_model()),
                |m| UiEvent::TranslateModel(m.to_string()),
            )
            .text_size(12)
            .width(FillLength),
        ]
        .spacing(6),
        row![
            text("To:").size(12),
            pick_list(
                translation::LANGUAGES,
                Some(state.translate_lang()),
                |l| UiEvent::TranslateLang(l.to_string()),
            )
            .text_size(12)
            .width(FillLength),
        ]
        .spacing(6),
        text_input("API key (optional, in-memory)", state.translate_api_key())
            .on_input(UiEvent::TranslateApiKey)
            .padding(4)
            .size(12)
            .width(FillLength),
        row![
            button("Translate").on_press_maybe(
                (has_entries && !state.translating() && !state.running()).then_some(UiEvent::Translate)
            ),
            text(format!(
                "{} image(s), {} result(s)",
                state.images().len(),
                total_results
            ))
            .size(12),
        ]
        .spacing(6),
        text(state.status()).size(12),
        scrollable(Column::with_children(results_list).spacing(2))
            .height(FillLength)
            .width(FillLength),
    ]
    .spacing(8)
    .height(FillLength)
    .into()
}
