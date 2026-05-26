//! Middle section: global text styling controls (bold/italic, text color,
//! stroke color/width, background color/radius) applied live to every
//! overlay entry. Colors are edited as `#RRGGBB[AA]` hex text; while a field
//! does not parse, the last valid color stays in effect and the input text
//! turns red.

use iced::widget::text_input::Status;
use iced::widget::{checkbox, column, row, text, text_input};
use iced::{Color, Element, Fill as FillLength};

use crate::app::{parse_hex, App, Message};
use crate::ui::panel::MUTED_FG;

const LABEL_WIDTH: f32 = 84.0;
const INVALID_FG: Color = Color::from_rgb8(220, 120, 120);

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

fn hex_input(value: &str, on_input: fn(String) -> Message) -> Element<'_, Message> {
    let valid = parse_hex(value).is_some();
    text_input("#RRGGBBAA", value)
        .on_input(on_input)
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

fn number_input(value: &str, on_input: fn(String) -> Message) -> Element<'_, Message> {
    text_input("0.0", value)
        .on_input(on_input)
        .padding(4)
        .size(12)
        .width(FillLength)
        .into()
}

pub fn view(app: &App) -> Element<'_, Message> {
    column![
        text("Styling").size(14),
        row![
            checkbox(app.style.bold)
                .label("Bold")
                .text_size(12)
                .on_toggle(Message::StyleBold),
            checkbox(app.style.italic)
                .label("Italic")
                .text_size(12)
                .on_toggle(Message::StyleItalic),
        ]
        .spacing(16),
        field_row("Text color", hex_input(&app.style_text_hex, Message::StyleTextHex)),
        field_row("Stroke color", hex_input(&app.style_stroke_hex, Message::StyleStrokeHex)),
        field_row("Stroke width", number_input(&app.style_stroke_width, Message::StyleStrokeWidth)),
        field_row("Background", hex_input(&app.style_bg_hex, Message::StyleBgHex)),
        field_row("Corner radius", number_input(&app.style_bg_radius, Message::StyleBgRadius)),
    ]
    .spacing(6)
    .into()
}