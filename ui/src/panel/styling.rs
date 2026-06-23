//! The styling panel, laid out like a compact typography inspector: a
//! header with the panel title, an auto-detect action and a reset button
//! (visual only), the font picker, a toolbar of bold/italic toggles next
//! to the alignment segments, then labeled sections for the text fill
//! (solid vs gradient tabs), the stroke and the background/corner radius.
//! All controls edit exactly one OCR entry: the one selected in the main
//! area. When no entry is selected the controls stay visible but are inert.
//! Colors are picked with the `neverliie_iced_widgets` `ColorPicker`; its
//! button underlay is a flat rectangle filled with the entry's current
//! color, shown next to its hex value.
//!
//! Below the sections a horizontally scrollable grid of style presets (in
//! memory only): one square per preset slot — checkerboard underlay, the
//! preset's background on top and "Aa" in its text color; empty slots show
//! a checkerboard dot — plus a "+" tile that fills the first empty slot
//! with the current working style. Clicking a preset applies it to the
//! selected entry; right-clicking opens a context menu to replace the
//! slot with the current style or to remove the preset (emptying it).

use iced::font::Weight;
use iced::widget::button::Status;
use iced::widget::image::{self, Handle};
use iced::widget::{
    button, column, container, pick_list, row, rule, scrollable, space::Space, text, text_input,
};
use iced::{
    Background, Border, Color, Element, Fill as FillLength, Font, Length, Padding, Shadow, Vector,
};

use neverliie_iced_widgets::advanced_dropdown::{advanced_dropdown, Item, MenuItem};
use neverliie_iced_widgets::color_picker::ColorPicker;
use neverliie_iced_widgets::context_menu::{ContextMenu, Menu};
use neverliie_iced_widgets::overlay::{Anchor, Position};

use scanlateit_model::{EntryStyle, TextAlign, TextGradientDir};

use crate::event::{StyleField, UiEvent};
use crate::main_area::overlay::styled_font;
use crate::segmented::{segment, segmented_group, ACCENT, BORDER, INPUT_BG, MUTED_FG, TEXT_MAIN};
use crate::state::UiState;

const SWATCH_SIDE: f32 = 20.0;
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

/// A muted, uppercase section label ("Fill", "Stroke", ...).
fn section_title<'a>(label: &'a str) -> Element<'a, UiEvent> {
    text(label).size(11).color(MUTED_FG).into()
}

/// A dark, bordered wrapper for inputs and swatch rows.
fn field_wrap<'a>(content: Element<'a, UiEvent>, padding: Padding) -> Element<'a, UiEvent> {
    container(content)
        .padding(padding)
        .width(FillLength)
        .style(|_theme| container::Style {
            background: Some(INPUT_BG.into()),
            border: Border {
                radius: 4.0.into(),
                width: 1.0,
                color: BORDER,
            },
            ..container::Style::default()
        })
        .into()
}

/// One tab of the fill tabs: an underline (accent when active) under the
/// label, like the mockup's bottom-border tab bar.
fn tab<'a>(label: &'a str, active: bool, on_press: Option<UiEvent>) -> Element<'a, UiEvent> {
    let underline: Element<'a, UiEvent> = if active {
        rule::horizontal(2)
            .style(|_theme: &iced::Theme| rule::Style {
                color: ACCENT,
                radius: 0.0.into(),
                fill_mode: rule::FillMode::Full,
                snap: true,
            })
            .into()
    } else {
        Space::new().height(2.0).into()
    };
    column![
        button(text(label).size(11))
            .width(FillLength)
            .padding([5, 0])
            .on_press_maybe(on_press)
            .style(move |_theme, status: Status| {
                let hovered = matches!(status, Status::Hovered | Status::Pressed);
                button::Style {
                    background: None,
                    border: Border::default(),
                    shadow: Shadow::default(),
                    text_color: if active || hovered { TEXT_MAIN } else { MUTED_FG },
                    ..button::Style::default()
                }
            }),
        underline,
    ]
    .width(FillLength)
    .into()
}

/// The "Solid | Gradient" tab bar; the tabs mirror `style.text_gradient`.
fn fill_tabs<'a>(gradient: bool, selected: bool) -> Element<'a, UiEvent> {
    row![
        tab("Solid", !gradient, selected.then_some(UiEvent::StyleGradientToggle(false))),
        tab("Gradient", gradient, selected.then_some(UiEvent::StyleGradientToggle(true))),
    ]
    .spacing(4)
    .into()
}

/// The uppercase hex value of `color`, or "None" for fully transparent
/// colors (matching the mockup's background swatch).
fn hex_label(color: Color) -> String {
    let [r, g, b, a] = color.into_rgba8();
    if a == 0 {
        "None".to_string()
    } else if a == 255 {
        format!("#{r:02X}{g:02X}{b:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

/// A flat rectangle button filled with `color`; the underlay of the color
/// picker for `field`. `on_open` is `None` (button disabled) while no entry
/// is selected.
fn swatch_button(color: Color, on_open: Option<UiEvent>) -> Element<'static, UiEvent> {
    button(Space::new())
        .width(SWATCH_SIDE)
        .height(SWATCH_SIDE)
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

/// A color field for `field`: a swatch with its hex value, wrapped in an
/// input-style box. The picker opens anchored to the bottom-right corner
/// of the swatch (the click target) and applies on OK.
fn color_field<'a, S: UiState + ?Sized>(
    state: &'a S,
    field: StyleField,
    color: Color,
) -> Element<'a, UiEvent> {
    let show_picker = state.style_picker_open() == Some(field);
    let on_open = state.selected().map(|_| UiEvent::StyleColorOpen(field));
    field_wrap(
        row![
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
            }),
            text(hex_label(color))
                .size(11)
                .font(Font::MONOSPACE)
                .color(TEXT_MAIN),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into(),
        Padding {
            top: 3.0,
            right: 8.0,
            bottom: 3.0,
            left: 4.0,
        },
    )
}

/// A number input with a muted glyph prefix, wrapped in an input-style box.
fn number_field<'a>(
    prefix: &'a str,
    value: &'a str,
    on_input: Option<fn(String) -> UiEvent>,
) -> Element<'a, UiEvent> {
    field_wrap(
        row![
            text(prefix).size(12).color(MUTED_FG),
            text_input("0", value)
                .on_input_maybe(on_input)
                .padding(0)
                .size(12)
                .width(FillLength)
                .style(|_theme, _status| text_input::Style {
                    background: Background::Color(Color::TRANSPARENT),
                    border: Border::default(),
                    icon: MUTED_FG,
                    placeholder: MUTED_FG,
                    value: TEXT_MAIN,
                    selection: ACCENT,
                }),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into(),
        Padding {
            top: 4.0,
            right: 8.0,
            bottom: 4.0,
            left: 8.0,
        },
    )
}

/// The panel header: title, auto-detect action and the reset button
/// (visual only — it has no event wired up).
fn header_row<'a>(selected: bool) -> Element<'a, UiEvent> {
    row![
        text("Typography").size(12).color(MUTED_FG),
        Space::new().width(FillLength),
        button(text("Auto-detect").size(11))
            .on_press_maybe(selected.then_some(UiEvent::StyleAutoDetect))
            .padding([4, 8])
            .style(|_theme, status: Status| {
                let hovered = matches!(status, Status::Hovered | Status::Pressed);
                button::Style {
                    background: Some(Background::Color(INPUT_BG)),
                    border: Border {
                        radius: 4.0.into(),
                        width: 1.0,
                        color: BORDER,
                    },
                    shadow: Shadow::default(),
                    text_color: if hovered { TEXT_MAIN } else { MUTED_FG },
                    ..button::Style::default()
                }
            }),
        button(text("↺").size(13))
            .on_press_maybe(None::<UiEvent>)
            .padding([4, 6])
            .style(|_theme, _status| button::Style {
                background: None,
                border: Border::default(),
                shadow: Shadow::default(),
                text_color: MUTED_FG,
                ..button::Style::default()
            }),
    ]
    .align_y(iced::Alignment::Center)
    .spacing(6)
    .into()
}

/// The font picker: a searchable `advanced_dropdown` over the installed
/// fonts, emitting `StyleFont` on selection.
fn font_field<'a, S: UiState + ?Sized>(state: &'a S) -> Element<'a, UiEvent> {
    let entries: Vec<MenuItem<'a, String, UiEvent, iced::Theme, iced::Renderer>> = state
        .installed_fonts()
        .iter()
        .map(|font| MenuItem::Item(Item::new(font.clone(), font.clone())))
        .collect();
    advanced_dropdown(
        entries,
        state.style_working().font_family.clone(),
        UiEvent::StyleFont,
    )
    .searchable(true)
    .text_size(12)
    .width(FillLength)
    .into()
}

/// The bold/italic toggles next to the alignment segments.
fn format_align_row<'a>(
    style: &EntryStyle,
    selected: bool,
) -> Element<'a, UiEvent> {
    let bold = Font {
        weight: Weight::Bold,
        ..Font::DEFAULT
    };
    let italic = Font {
        style: iced::font::Style::Italic,
        ..Font::DEFAULT
    };
    row![
        container(segmented_group(vec![
            segment(style.bold, "B", selected.then_some(UiEvent::StyleBold(!style.bold)), bold),
            segment(style.italic, "I", selected.then_some(UiEvent::StyleItalic(!style.italic)), italic),
        ]))
        .width(Length::FillPortion(1)),
        container(segmented_group(vec![
            segment(
                style.text_align == TextAlign::Left,
                "L",
                selected.then_some(UiEvent::StyleTextAlign(TextAlign::Left)),
                Font::DEFAULT,
            ),
            segment(
                style.text_align == TextAlign::Center,
                "C",
                selected.then_some(UiEvent::StyleTextAlign(TextAlign::Center)),
                Font::DEFAULT,
            ),
            segment(
                style.text_align == TextAlign::Right,
                "R",
                selected.then_some(UiEvent::StyleTextAlign(TextAlign::Right)),
                Font::DEFAULT,
            ),
            segment(
                style.text_align == TextAlign::Circular,
                "◎",
                selected.then_some(UiEvent::StyleTextAlign(TextAlign::Circular)),
                Font::DEFAULT,
            ),
        ]))
        .width(Length::FillPortion(2)),
    ]
    .spacing(8)
    .into()
}

/// The "Fill" section: solid (text color) vs gradient (two colors plus
/// direction) tabs.
fn fill_section<'a, S: UiState + ?Sized>(
    state: &'a S,
    style: &EntryStyle,
    selected: bool,
) -> Element<'a, UiEvent> {
    let gradient = style.text_gradient;
    column![
        section_title("Fill"),
        fill_tabs(gradient, selected),
        if gradient {
            column![
                row![
                    color_field(state, StyleField::GradientA, state.style_gradient_a()),
                    color_field(state, StyleField::GradientB, state.style_gradient_b()),
                ]
                .spacing(8),
                pick_list(TextGradientDir::LABELS, Some(style.gradient_dir.label()), |l| {
                    UiEvent::StyleGradientDir(TextGradientDir::from_label(&l))
                })
                .text_size(12)
                .width(FillLength),
            ]
            .spacing(8)
            .into()
        } else {
            color_field(state, StyleField::Text, state.style_text_color())
        },
    ]
    .spacing(8)
    .into()
}

/// The "Stroke" section: color plus width.
fn stroke_section<'a, S: UiState + ?Sized>(
    state: &'a S,
    selected: bool,
) -> Element<'a, UiEvent> {
    column![
        section_title("Stroke"),
        row![
            container(color_field(
                state,
                StyleField::Stroke,
                state.style_stroke_color(),
            ))
            .width(Length::FillPortion(2)),
            container(number_field(
                "─",
                state.style_stroke_width(),
                selected.then_some(UiEvent::StyleStrokeWidth),
            ))
            .width(Length::FillPortion(1)),
        ]
        .spacing(8),
    ]
    .spacing(8)
    .into()
}

/// The "Background & Corner" section: background color plus corner radius.
fn background_section<'a, S: UiState + ?Sized>(
    state: &'a S,
    selected: bool,
) -> Element<'a, UiEvent> {
    column![
        section_title("Background & Corner"),
        row![
            container(color_field(
                state,
                StyleField::Background,
                state.style_bg_color(),
            ))
            .width(Length::FillPortion(2)),
            container(number_field(
                "▣",
                state.style_bg_radius(),
                selected.then_some(UiEvent::StyleBgRadius),
            ))
            .width(Length::FillPortion(1)),
        ]
        .spacing(8),
    ]
    .spacing(8)
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
            let tile = preset_square(preset.clone(), can_apply.then_some(UiEvent::StylePresetApply(index)));
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
    let selected = state.selected().is_some();

    scrollable(column![
        header_row(selected),
        font_field(state),
        format_align_row(style, selected),
        fill_section(state, style, selected),
        stroke_section(state, selected),
        background_section(state, selected),
        presets_grid(state),
        text(HINT).size(12).color(MUTED_FG),
    ]
    .spacing(10))
    .width(FillLength)
    .height(FillLength)
    .into()
}