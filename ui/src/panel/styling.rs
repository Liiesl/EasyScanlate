//! Middle section: per-entry text styling controls (bold/italic, text color,
//! stroke color/width, background color/radius) applied to exactly one OCR
//! entry: the one selected in the main area. When no entry is selected the
//! controls stay visible but are inert. Colors are picked with the
//! `neverliie_iced_widgets` `ColorPicker`; its button underlay is a flat
//! rectangle filled with the entry's current color.
//!
//! Below the controls a horizontally scrollable grid of style presets (in
//! memory only): one square per preset slot — checkerboard underlay, the
//! preset's background on top and "Aa" in its text color; empty slots show
//! a checkerboard dot — plus a "+" tile that fills the first empty slot
//! with the current working style. Clicking a preset applies it to the
//! selected entry; right-clicking opens a context menu to replace the
//! slot with the current style or to remove the preset (emptying it).

use iced::widget::button::Status;
use iced::widget::image::{self, Handle};
use iced::widget::{
    button, checkbox, column, row, scrollable, space::Space, text, text_input,
};
use iced::{
    Background, Border, Color, Element, Fill as FillLength, Font, Padding, Shadow, Vector,
};

use neverliie_iced_widgets::color_picker::ColorPicker;
use neverliie_iced_widgets::context_menu::{ContextMenu, Menu};
use neverliie_iced_widgets::overlay::{Anchor, Position};

use scanlateit_model::EntryStyle;

use crate::event::{StyleField, UiEvent};
use crate::main_area::overlay::styled_font;
use crate::panel::MUTED_FG;
use crate::state::UiState;

const LABEL_WIDTH: f32 = 84.0;
const SWATCH_HEIGHT: f32 = 20.0;
const HINT: &str = "Select a text entry in the image to style it.";

/// Side of a preset square, in points.
const PRESET_SIDE: f32 = 56.0;
/// Corner radius of a preset square, in points.
const PRESET_RADIUS: f32 = 6.0;
/// Checkerboard tiles behind a preset's background: the color picker's
/// light/dark pair (`#E6E6E6` / `#C8C8C8`) at half alpha, so the panel
/// shows through like in the color picker's swatches.
const CHECKER_LIGHT: Color = Color::from_rgba8(230, 230, 230, 0.5);
const CHECKER_DARK: Color = Color::from_rgba8(200, 200, 200, 0.5);
/// Fill of the "+" add tile, a step lighter than the panel background.
const ADD_TILE_BG: Color = Color::from_rgb8(43, 46, 56);

fn to_color(rgba: [u8; 4]) -> Color {
    Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3] as f32 / 255.0)
}

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

/// The checkerboard image shared by every preset square: a 64px RGBA bitmap
/// of 8px light/dark tiles, like the color picker's swatches. Built once
/// (lazily) so the renderer's image cache keeps a single upload.
fn checker_handle() -> &'static Handle {
    static CHECKER: std::sync::OnceLock<Handle> = std::sync::OnceLock::new();
    CHECKER.get_or_init(|| {
        let side = 64u32;
        let tile = 8u32;
        let mut pixels = Vec::with_capacity((side * side * 4) as usize);
        for y in 0..side {
            for x in 0..side {
                let color = if (x / tile + y / tile) % 2 == 0 {
                    CHECKER_LIGHT
                } else {
                    CHECKER_DARK
                };
                let [r, g, b, a] = color.into_rgba8();
                pixels.extend_from_slice(&[r, g, b, a]);
            }
        }
        Handle::from_rgba(side, side, pixels)
    })
}

/// A bordered square button of [`PRESET_SIDE`] pixels: `underlay` drawn
/// first, `fill` composited over it, and the `glyph` centered on top — the
/// same layered look as the color picker's swatches, built from regular
/// widgets. `on_press` is `None` (button inert) while nothing can be done.
fn square_tile<'a>(
    underlay: Element<'a, UiEvent>,
    glyph: Element<'a, UiEvent>,
    fill: Option<Color>,
    on_press: Option<UiEvent>,
) -> Element<'a, UiEvent> {
    let button = button(glyph)
        .width(FillLength)
        .height(FillLength)
        .padding(Padding::ZERO)
        .on_press_maybe(on_press)
        .style(move |_theme, status: Status| {
            let border_color = if matches!(status, Status::Hovered | Status::Pressed) {
                Color::from_rgb8(230, 230, 230)
            } else {
                Color::from_rgb8(90, 90, 90)
            };
            button::Style {
                background: fill.map(Background::Color),
                border: Border {
                    radius: PRESET_RADIUS.into(),
                    width: 1.0,
                    color: border_color,
                },
                shadow: Shadow::default(),
                ..button::Style::default()
            }
        });
    iced::widget::stack![underlay, button]
        .width(iced::Length::Fixed(PRESET_SIDE))
        .height(iced::Length::Fixed(PRESET_SIDE))
        .into()
}

/// One preset swatch: checkerboard underlay, the preset's background on
/// top, "Aa" centered in the preset's text color (with its bold/italic).
fn preset_square<'a>(style: EntryStyle, on_press: Option<UiEvent>) -> Element<'a, UiEvent> {
    square_tile(
        image::Image::new(checker_handle().clone())
            .width(FillLength)
            .height(FillLength)
            .border_radius(PRESET_RADIUS)
            .into(),
        text("Aa")
            .size(PRESET_SIDE * 0.36)
            .color(to_color(style.text_color))
            .font(styled_font(Font::DEFAULT, &style))
            .width(FillLength)
            .height(FillLength)
            .center()
            .into(),
        Some(to_color(style.bg_color)),
        on_press,
    )
}

/// The "+" add tile: plain fill and a muted plus sign.
fn add_square<'a>(on_press: Option<UiEvent>) -> Element<'a, UiEvent> {
    square_tile(
        Space::new().into(),
        text("+")
            .size(PRESET_SIDE * 0.5)
            .color(MUTED_FG)
            .width(FillLength)
            .height(FillLength)
            .center()
            .into(),
        Some(ADD_TILE_BG),
        on_press,
    )
}

/// An empty preset slot: checkerboard underlay and a muted dot, inert to
/// clicks but right-clickable to fill it via the context menu.
fn empty_square<'a>() -> Element<'a, UiEvent> {
    square_tile(
        image::Image::new(checker_handle().clone())
            .width(FillLength)
            .height(FillLength)
            .border_radius(PRESET_RADIUS)
            .into(),
        text("·")
            .size(PRESET_SIDE * 0.3)
            .color(MUTED_FG)
            .width(FillLength)
            .height(FillLength)
            .center()
            .into(),
        None,
        None,
    )
}

/// The right-click menu for preset slot `index`: filled slots offer
/// replacing the style or removing the preset; empty slots only fill.
fn preset_menu<'a>(
    index: usize,
    filled: bool,
) -> Menu<'a, UiEvent, iced::Theme, iced::Renderer> {
    let mut menu = Menu::new().item("Replace with current style", UiEvent::StylePresetReplace(index));
    if filled {
        menu = menu.item("Remove preset", UiEvent::StylePresetRemove(index));
    }
    menu
}

/// The style-preset grid: two rows — one square per preset slot (empty
/// slots shown as checkerboard dots) plus the "+" add tile, stacked in
/// pairs that flow rightward, inside a horizontal scrollable. Presets
/// apply to the selected entry (disabled while none is selected); the
/// "+" fills the first empty slot or appends a new preset.
fn presets_grid<'a, S: UiState + ?Sized>(state: &'a S) -> Element<'a, UiEvent> {
    let can_apply = state.selected().is_some();
    let mut tiles: Vec<Element<'a, UiEvent>> = state
        .style_presets()
        .iter()
        .enumerate()
        .map(|(index, slot)| {
            let Some(preset) = slot else {
                let tile = empty_square();
                return ContextMenu::new(tile, preset_menu(index, false))
                    .on_dismiss(UiEvent::StylePresetMenuDismiss)
                    .text_size(12.0)
                    .into();
            };
            let tile = preset_square(*preset, can_apply.then_some(UiEvent::StylePresetApply(index)));
            ContextMenu::new(tile, preset_menu(index, true))
                .on_dismiss(UiEvent::StylePresetMenuDismiss)
                .text_size(12.0)
                .into()
        })
        .collect();
    tiles.push(add_square(Some(UiEvent::StylePresetAdd)));
    let mut columns: Vec<Element<'a, UiEvent>> = Vec::with_capacity((tiles.len() + 1) / 2);
    while !tiles.is_empty() {
        let top = tiles.remove(0);
        let bottom = if tiles.is_empty() {
            Space::new().into()
        } else {
            tiles.remove(0)
        };
        columns.push(column![top, bottom].spacing(6).into());
    }
    let strip = scrollable::Scrollable::with_direction(
        row(columns).spacing(6),
        scrollable::Direction::Horizontal(scrollable::Scrollbar::new()),
    )
    .width(FillLength)
    .height(PRESET_SIDE * 2.0 + 6.0 + 14.0);
    column![
        text("Presets").size(12).color(MUTED_FG),
        strip,
    ]
    .spacing(6)
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
            presets_grid(state),
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
        button(text("Auto-detect style").size(12))
            .on_press(UiEvent::StyleAutoDetect)
            .padding(6)
            .width(FillLength),
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
        presets_grid(state),
    ]
    .spacing(6)
    .into()
}