//! Middle section: per-entry text styling controls (bold/italic, text color,
//! stroke color/width, background color/radius) applied to exactly one OCR
//! entry: the one selected in the main area. When no entry is selected the
//! controls stay visible but are inert. Colors are picked with the
//! `neverliie_iced_widgets` `ColorPicker`; its button underlay is a flat
//! rectangle filled with the entry's current color.

use iced::widget::button::Status;
use iced::widget::{button, checkbox, column, row, space::Space, text, text_input};
use iced::{Background, Border, Color, Element, Fill as FillLength, Padding, Shadow, Vector};

use neverliie_iced_widgets::color_picker::ColorPicker;
use neverliie_iced_widgets::overlay::{Anchor, Position};

use crate::event::{StyleField, UiEvent};
use crate::panel::MUTED_FG;
use crate::state::UiState;

const LABEL_WIDTH: f32 = 84.0;
const SWATCH_HEIGHT: f32 = 20.0;
const HINT: &str = "Select a text entry in the image to style it.";

fn field_row<'a>(
    label: &'a str,
    input: Element<'a, UiEvent>,
) -> Element<'a, UiEvent> {
    row![
        text(label).size(12).color(MUTED_FG).width(LABEL_WIDTH),
        input,
    ]
    .spacing(4)
    .into()
}

/// A flat rectangle button filled with `color`; the underlay of the color
/// picker for `field`. `on_open` is `None` (button disabled) while no entry
/// is selected.
fn swatch_button(color: Color, on_open: Option<UiEvent>) -> Element<'static, UiEvent> {
    button(Space::new())
        .width(FillLength)
        .height(SWATCH_HEIGHT)
        .padding(Padding::ZERO)
        .on_press_maybe(on_open)
        .style(move |_theme, status: Status| {
            let border_color = if matches!(status, Status::Hovered | Status::Pressed) {
                Color::from_rgb8(230, 230, 230)
            } else {
                Color::from_rgb8(90, 90, 90)
            };
            button::Style {
                background: Some(Background::Color(color)),
                border: Border {
                    radius: 3.0.into(),
                    width: 1.0,
                    color: border_color,
                },
                shadow: Shadow::default(),
                ..button::Style::default()
            }
        })
        .into()
}

/// A color field for `field`: a `ColorPicker` whose underlay is a rectangle
/// filled with the current value. The picker opens anchored to the bottom-
/// right corner of the swatch button (the click target) and applies on OK.
fn color_field<'a, S: UiState + ?Sized>(
    state: &'a S,
    field: StyleField,
    color: Color,
) -> Element<'a, UiEvent> {
    let show_picker = state.style_picker_open() == Some(field);
    let on_open = state.selected().map(|_| UiEvent::StyleColorOpen(field));
    ColorPicker::new(
        show_picker,
        color,
        swatch_button(color, on_open),
        UiEvent::StyleColorCancel(field),
        move |picked| UiEvent::StyleColorSubmit(field, picked),
    )
    .position(Position::Parent {
        anchor: Anchor::BottomRight,
        offset: Vector::new(0.0, 4.0),
    })
    .into()
}

fn number_input(value: &str, on_input: Option<fn(String) -> UiEvent>) -> Element<'_, UiEvent> {
    text_input("0.0", value)
        .on_input_maybe(on_input)
        .padding(4)
        .size(12)
        .width(FillLength)
        .into()
}

pub fn view<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    let style = state.style_working();
    let Some((image_index, entry_id)) = state.selected() else {
        return column![
            text("Styling").size(14),
            row![
                checkbox(style.bold).label("Bold").text_size(12),
                checkbox(style.italic).label("Italic").text_size(12),
            ]
            .spacing(16),
            field_row(
                "Text color",
                color_field(state, StyleField::Text, state.style_text_color()),
            ),
            field_row(
                "Stroke color",
                color_field(state, StyleField::Stroke, state.style_stroke_color()),
            ),
            field_row("Stroke width", number_input(state.style_stroke_width(), None)),
            field_row(
                "Background",
                color_field(state, StyleField::Background, state.style_bg_color()),
            ),
            field_row("Corner radius", number_input(state.style_bg_radius(), None)),
            text(HINT).size(12).color(MUTED_FG),
        ]
        .spacing(6)
        .into();
    };

    let entry = state.images()[image_index].project.ocr.get(entry_id);
    let heading = entry
        .map(|e| {
            let entry_text = state.images()[image_index].project.display_text(e);
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
            checkbox(style.bold)
                .label("Bold")
                .text_size(12)
                .on_toggle(UiEvent::StyleBold),
            checkbox(style.italic)
                .label("Italic")
                .text_size(12)
                .on_toggle(UiEvent::StyleItalic),
        ]
        .spacing(16),
        field_row(
            "Text color",
            color_field(state, StyleField::Text, state.style_text_color()),
        ),
        field_row(
            "Stroke color",
            color_field(state, StyleField::Stroke, state.style_stroke_color()),
        ),
        field_row("Stroke width", number_input(state.style_stroke_width(), Some(UiEvent::StyleStrokeWidth))),
        field_row(
            "Background",
            color_field(state, StyleField::Background, state.style_bg_color()),
        ),
        field_row("Corner radius", number_input(state.style_bg_radius(), Some(UiEvent::StyleBgRadius))),
    ]
    .spacing(6)
    .into()
}