//! Middle section: per-entry text styling controls (bold/italic, text color,
//! stroke color/width, background color/radius) applied to exactly one OCR
//! entry: the one selected in the main area. When no entry is selected the
//! controls stay visible but are inert. Colors are edited as `#RRGGBB[AA]`
//! hex text; while a field does not parse, the last valid color stays in
//! effect and the input text turns red.

use iced::widget::text_input::Status;
use iced::widget::{checkbox, column, row, text, text_input};
use iced::{Color, Element, Fill as FillLength};

use crate::app::{parse_hex, App, Message};
use crate::ui::panel::MUTED_FG;

const LABEL_WIDTH: f32 = 84.0;
const INVALID_FG: Color = Color::from_rgb8(220, 120, 120);
const HINT: &str = "Select a text entry in the image to style it.";

fn field_row<'a>(
    label: &'a str,
    input: Element<'a, Message>,
) -> Element<'a, Message> {
    row![
        text(label).size(12).color(MUTED_FG).width(LABEL_WIDTH),
        input,
    ]
    .spacing(4)
    .into()
}

fn hex_input(value: &str, on_input: Option<fn(String) -> Message>) -> Element<'_, Message> {
    let valid = parse_hex(value).is_some();
    text_input("#RRGGBBAA", value)
        .on_input_maybe(on_input)
        .padding(4)
        .size(12)
        .width(FillLength)
        .style(move |theme, status: Status| {
            let mut style = text_input::default(theme, status);
            if !valid {
                style.value = INVALID_FG;
            }
            style
        })
        .into()
}

fn number_input(value: &str, on_input: Option<fn(String) -> Message>) -> Element<'_, Message> {
    text_input("0.0", value)
        .on_input_maybe(on_input)
        .padding(4)
        .size(12)
        .width(FillLength)
        .into()
}

pub fn view(app: &App) -> Element<'_, Message> {
    let Some((image_index, entry_id)) = app.selected else {
        let style = &app.style_working;
        return column![
            text("Styling").size(14),
            row![
                checkbox(style.bold).label("Bold").text_size(12),
                checkbox(style.italic).label("Italic").text_size(12),
            ]
            .spacing(16),
            field_row("Text color", hex_input(&app.style_text_hex, None)),
            field_row("Stroke color", hex_input(&app.style_stroke_hex, None)),
            field_row("Stroke width", number_input(&app.style_stroke_width, None)),
            field_row("Background", hex_input(&app.style_bg_hex, None)),
            field_row("Corner radius", number_input(&app.style_bg_radius, None)),
            text(HINT).size(12).color(MUTED_FG),
        ]
        .spacing(6)
        .into();
    };

    let entry = app.images[image_index].project.ocr.get(entry_id);
    let heading = entry
        .map(|e| {
            let entry_text = app.images[image_index].project.display_text(e);
            let short: String = entry_text.chars().take(24).collect();
            if entry_text.chars().count() > 24 {
                format!("Styling — \"{short}…\"")
            } else {
                format!("Styling — \"{short}\"")
            }
        })
        .unwrap_or_else(|| "Styling — entry".to_string());

    column![
        text(heading).size(14),
        row![
            checkbox(app.style_working.bold)
                .label("Bold")
                .text_size(12)
                .on_toggle(Message::StyleBold),
            checkbox(app.style_working.italic)
                .label("Italic")
                .text_size(12)
                .on_toggle(Message::StyleItalic),
        ]
        .spacing(16),
        field_row("Text color", hex_input(&app.style_text_hex, Some(Message::StyleTextHex))),
        field_row("Stroke color", hex_input(&app.style_stroke_hex, Some(Message::StyleStrokeHex))),
        field_row("Stroke width", number_input(&app.style_stroke_width, Some(Message::StyleStrokeWidth))),
        field_row("Background", hex_input(&app.style_bg_hex, Some(Message::StyleBgHex))),
        field_row("Corner radius", number_input(&app.style_bg_radius, Some(Message::StyleBgRadius))),
    ]
    .spacing(6)
    .into()
}