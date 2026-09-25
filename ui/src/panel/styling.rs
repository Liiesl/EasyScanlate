//! The styling panel, laid out like a compact typography inspector: a
//! header with the panel title, an auto-detect action and a reset button
//! (visual only), the font picker, a toolbar of bold/italic toggles next
//! to the alignment segments, then labeled sections for the text fill,
//! the stroke and the background/corner radius.
//! All controls edit exactly one OCR entry: the one selected in the main
//! area. When no entry is selected the controls stay visible but are inert.
//! Each color section is a unified `HexColorInput` (solid hex + alpha `%`,
//! or gradient + angle) with its own self-hosted floating picker.
//!
//! Below the sections a horizontally scrollable grid of style presets (in
//! memory only): one square per preset slot — checkerboard underlay, the
//! preset's background on top and "Aa" in its text color; empty slots show
//! a checkerboard dot — plus a "+" tile that fills the first empty slot
//! with the current working style. Clicking a preset applies it to the
//! selected entry; right-clicking opens a context menu to replace the
//! slot with the current style or to remove the preset (emptying it).

use iced::widget::button::Status;
use iced::widget::image::{self, Handle};
use iced::widget::{
    button, checkbox, column, container, row, scrollable, space::Space, text, text_input,
    tooltip,
};
use iced::{
    Background, Border, Color, Element, Fill as FillLength, Font, Length, Padding, Shadow,
};

use neverliie_iced_widgets::advanced_dropdown::{advanced_dropdown, Item, MenuItem};
use neverliie_iced_widgets::number_input::{NumberInput, Status as NumberStatus, Style as NumberStyle};
use neverliie_iced_widgets::split_button::split_button;
use neverliie_iced_widgets::hex_color_input::{HexColorInput, HexColorValue};
use neverliie_iced_widgets::context_menu::{ContextMenu, Menu};
use neverliie_iced_widgets::overlay::Position;

use easyscanlate_model::{CapsMode, EntryStyle, TextAlign};
use easyscanlate_settings::InpaintBackend;

use crate::event::{StyleField, UiEvent};
use crate::main_area::overlay::{font_support, preview_font, styled_font};
use crate::segmented::{segment_compact, segment_icon, segmented_group, BORDER, INPUT_BG, MUTED_FG, TEXT_MAIN};
use crate::scale;
use crate::state::UiState;
use lucide_icons::Icon;

const HINT: &str = "Select a text entry in the image to style it.";

/// Side of a preset square, in points — reduced from 56 (absurd at base) to 36.
const PRESET_SIDE: f32 = 36.0;
/// Corner radius of a preset square, in points.
const PRESET_RADIUS: f32 = 5.0;
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
    text(label).size(scale::s(11.0)).color(MUTED_FG).into()
}

/// Unified hex input for `field`: solid hex + alpha `%`, or gradient +
/// angle when the value is a gradient. The widget self-hosts its floating
/// picker (swatch opens it); typing, alpha/angle edits and picker drags
/// publish `StyleColorChanged` live, the picker OK button publishes
/// `StyleColorSubmit`. `HexColorInput` keeps in-progress text internally,
/// so no per-field string buffer lives in app state. Transparent
/// (`a == 0`, formerly `"None"`) is a solid with `0%` alpha.
fn unified_field<'a, S: UiState + ?Sized>(
    state: &'a S,
    field: StyleField,
    value: HexColorValue,
) -> Element<'a, UiEvent> {
    let selected = state.selected().is_some();
    let show_picker = selected && state.style_picker_open() == Some(field);
    let mut input = HexColorInput::new(
        value,
        move |v| UiEvent::StyleColorChanged(field, v),
        show_picker,
        UiEvent::StyleColorOpen(field),
        UiEvent::StyleColorCancel(field),
    )
    .on_submit(move |v| UiEvent::StyleColorSubmit(field, v))
    .on_tab_change(move |tab| UiEvent::StyleColorTabChanged(field, tab))
    .position(Position::BottomLeft)
    .on_dropper_capture(|| UiEvent::StyleDropperCapture)
    .width(FillLength)
    .text_size(scale::s(12.0))
    .padding(scale::s(2.0))
    .border_radius(scale::s(4.0));
    if let Some(buffer) = state.dropper_buffer() {
        input = input.dropper_buffer(buffer);
    }
    input.into()
}

/// A number input with a muted icon prefix, matching the dark input-box
/// look (`INPUT_BG` + `BORDER`). Built on the shared `NumberInput`
/// `(-|icon input|+)` pill: stepping buttons, wheel/arrow keys and
/// in-progress text (`"-"`, `"5."`) come from the widget, so the app holds
/// only the numeric value. Disabled (no selection, or fixed size while
/// auto-size fits) renders the inner field read-only with inert buttons.
fn style_number<'a>(
    icon: Icon,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    shift_step: f32,
    on_change: fn(f32) -> UiEvent,
    enabled: bool,
) -> Element<'a, UiEvent> {
    NumberInput::new(range, value, on_change)
        .step(step)
        .shift_step(shift_step)
        .prefix(
            crate::icon::lucide(icon)
                .size(scale::s(12.0))
                .color(MUTED_FG),
        )
        .width(FillLength)
        .padding(scale::s(2.0))
        .text_size(scale::s(12.0))
        .border_radius(scale::s(4.0))
        .enabled(enabled)
        .style(|_theme, _status: NumberStatus| NumberStyle {
            background: INPUT_BG.into(),
            border: Border {
                radius: scale::s(4.0).into(),
                width: scale::s(1.0),
                color: BORDER,
            },
            shadow: Shadow::default(),
        })
        .button_style(|_theme, status: button::Status| button::Style {
            background: Some(Background::Color(match status {
                button::Status::Hovered => Color::from_rgba8(255, 255, 255, 0.10),
                button::Status::Pressed => Color::from_rgba8(255, 255, 255, 0.16),
                button::Status::Disabled | button::Status::Active => Color::TRANSPARENT,
            })),
            border: Border::default(),
            shadow: Shadow::default(),
            text_color: if matches!(status, button::Status::Disabled) {
                MUTED_FG
            } else {
                TEXT_MAIN
            },
            ..button::Style::default()
        })
        .input_style(|_theme, _status| text_input::Style {
            background: Background::Color(Color::TRANSPARENT),
            border: Border::default(),
            icon: MUTED_FG,
            placeholder: MUTED_FG,
            value: TEXT_MAIN,
            selection: crate::accent::accent(),
        })
        .into()
}

fn tip(label: &str) -> container::Container<'_, UiEvent> {
    container(text(label).size(scale::s(11.0)))
        .padding(scale::s(6.0))
        .style(container::rounded_box)
}

/// The panel header: title, auto-detect action and the reset button
/// (visual only — it has no event wired up). Auto Detect is a bulk job and
/// must be disabled while any bulk job (pipeline/translate/inpaint) is active.
fn header_row<'a, S: UiState + ?Sized>(state: &'a S, selected: bool) -> Element<'a, UiEvent> {
    let can_auto = selected && !state.is_bulk_busy();
    let auto_btn = button(
        row![
            crate::icon::lucide(Icon::Sparkles).size(scale::s(14.0)).center(),
            text("Auto Detect").size(scale::s(11.0))
        ]
        .spacing(scale::s(4.0))
        .align_y(iced::Alignment::Center),
    )
    .style(crate::panel::button_style)
    .on_press_maybe(can_auto.then_some(UiEvent::StyleAutoDetect))
    .padding(scale::s(6.0));
    let auto: Element<'_, UiEvent> = tooltip(crate::button::with_disabled_cursor(auto_btn.into()), tip("Auto detect style"), tooltip::Position::Top)
        .gap(scale::s(4.0))
        .into();
    let reset_btn = button(crate::icon::lucide(Icon::RotateCcw).size(scale::s(14.0)).center())
        .style(crate::panel::button_style)
        .on_press_maybe(None::<UiEvent>)
        .padding(scale::s(6.0));
    let reset: Element<'_, UiEvent> = tooltip(crate::button::with_disabled_cursor(reset_btn.into()), tip("Reset style"), tooltip::Position::Top)
        .gap(scale::s(4.0))
        .into();
    row![
        text("Typography").size(scale::s(12.0)).color(MUTED_FG),
        Space::new().width(FillLength),
        auto,
        reset,
    ]
    .align_y(iced::Alignment::Center)
    .spacing(scale::s(6.0))
    .into()
}

/// The font picker: a searchable `advanced_dropdown` over the installed
/// fonts, grouped into a "Featured" section (the embedded families in
/// `BUNDLED_FONTS` order) and an "All fonts" section (everything else,
/// deduped against Featured). Each family keeps the current pattern:
/// a group label (family name in the UI font) alternating with one item
/// (the same name previewed in the family's own face), emitting
/// `StyleFont` on selection. The closed field stays in the UI font;
/// only the open rows preview. Font files load lazily: the first visible
/// families on open plus the hovered row (see `StyleFontPreviewOpen` /
/// `StyleFontPreviewHover`).
fn font_field<'a, S: UiState + ?Sized>(state: &'a S) -> Element<'a, UiEvent> {
    use easyscanlate_model::{ALL_FONTS_LABEL, FEATURED_FONTS_LABEL, partition_featured_fonts};
    let installed = state.installed_fonts();
    let (featured, rest) = partition_featured_fonts(installed);
    let mut entries: Vec<MenuItem<'a, String, UiEvent, iced::Theme, iced::Renderer>> =
        Vec::with_capacity(2 + featured.len() * 2 + 3 + rest.len() * 2);
    entries.push(MenuItem::Label(FEATURED_FONTS_LABEL));
    entries.push(MenuItem::Separator);
    for &index in &featured {
        let family = &installed[index];
        entries.push(MenuItem::Label(family.as_str()));
        entries.push(MenuItem::Item(
            Item::new(family.clone(), family.clone()).font(preview_font(family)),
        ));
    }
    entries.push(MenuItem::Separator);
    entries.push(MenuItem::Label(ALL_FONTS_LABEL));
    entries.push(MenuItem::Separator);
    for &index in &rest {
        let family = &installed[index];
        entries.push(MenuItem::Label(family.as_str()));
        entries.push(MenuItem::Item(
            Item::new(family.clone(), family.clone()).font(preview_font(family)),
        ));
    }
    advanced_dropdown(
        entries,
        state.style_working().font_family.clone(),
        UiEvent::StyleFont,
    )
    .searchable(true)
    .text_size(scale::s(12.0))
    .width(FillLength)
    .menu_max_height(300.0)
    .on_open(UiEvent::StyleFontPreviewOpen)
    .on_option_hovered(|name: String| UiEvent::StyleFontPreviewHover(name))
    .into()
}

/// The bold/italic toggles next to the alignment segments.
///
/// Availability is per selected font (see `font_support`): a toggle is
/// disabled when the family has no such face. The stored flag is kept, so an
/// auto-detected Bold/Italic on an unsupported family stays visually selected
/// (accent on disabled fill, see `segment_icon`) with a tooltip explaining
/// the missing face. Families with Bold + Italic but no BoldItalic act as an
/// exclusive switcher — enforced in `handle_bold`/`handle_italic`.
fn format_align_row<'a>(
    style: &EntryStyle,
    selected: bool,
) -> Element<'a, UiEvent> {
    let support = font_support(style.font_family.as_deref());
    let family = style.font_family.as_deref().unwrap_or("Default font");
    let can_bold = selected && support.has_bold;
    let can_italic = selected && support.has_italic;
    let bold_btn: Element<'a, UiEvent> = segment_icon(
        style.bold,
        Icon::Bold,
        can_bold.then_some(UiEvent::StyleBold(!style.bold)),
    );
    let italic_btn: Element<'a, UiEvent> = segment_icon(
        style.italic,
        Icon::Italic,
        can_italic.then_some(UiEvent::StyleItalic(!style.italic)),
    );
    // Only explain missing faces when an entry is selected; with no selection
    // both toggles are inert anyway. Tooltip content owns its `String` so the
    // returned element does not borrow locals.
    let bold_btn: Element<'a, UiEvent> = if selected && !support.has_bold {
        let msg = if style.bold {
            format!("Bold not available in {family} — showing detected value")
        } else {
            format!("Bold not available in {family}")
        };
        tooltip(
            bold_btn,
            container(text(msg).size(scale::s(11.0)))
                .padding(scale::s(6.0))
                .style(container::rounded_box),
            tooltip::Position::Top,
        )
        .gap(scale::s(4.0))
        .into()
    } else {
        bold_btn
    };
    let italic_btn: Element<'a, UiEvent> = if selected && !support.has_italic {
        let msg = if style.italic {
            format!("Italic not available in {family} — showing detected value")
        } else {
            format!("Italic not available in {family}")
        };
        tooltip(
            italic_btn,
            container(text(msg).size(scale::s(11.0)))
                .padding(scale::s(6.0))
                .style(container::rounded_box),
            tooltip::Position::Top,
        )
        .gap(scale::s(4.0))
        .into()
    } else {
        italic_btn
    };
    row![
        container(segmented_group(vec![bold_btn, italic_btn]))
        .width(Length::FillPortion(1)),
        container(segmented_group(vec![
            segment_icon(
                style.text_align == TextAlign::Left,
                Icon::AlignLeft,
                selected.then_some(UiEvent::StyleTextAlign(TextAlign::Left)),
            ),
            segment_icon(
                style.text_align == TextAlign::Center,
                Icon::AlignCenter,
                selected.then_some(UiEvent::StyleTextAlign(TextAlign::Center)),
            ),
            segment_icon(
                style.text_align == TextAlign::Right,
                Icon::AlignRight,
                selected.then_some(UiEvent::StyleTextAlign(TextAlign::Right)),
            ),
            segment_icon(
                style.text_align == TextAlign::Circular,
                Icon::Ellipse,
                selected.then_some(UiEvent::StyleTextAlign(TextAlign::Circular)),
            ),
        ]))
        .width(Length::FillPortion(2)),
    ]
    .spacing(scale::s(8.0))
    .into()
}

/// A small muted caption above a paired number input ("Line height", ...).
fn caption<'a>(label: &'a str) -> Element<'a, UiEvent> {
    text(label).size(scale::s(10.0)).color(MUTED_FG).into()
}

/// The typography size/spacing controls: an `Auto size` checkbox (default on:
/// the text is fitted to its box) plus a fixed font-size input used only
/// while auto-size is off with the mutually exclusive All Caps / Small Caps
/// toggle on its right, then one row of line-height and letter-spacing
/// (image px).
fn typography_section<'a, S: UiState + ?Sized>(
    _state: &'a S,
    style: &EntryStyle,
    selected: bool,
) -> Element<'a, UiEvent> {
    let auto: Element<'a, UiEvent> = checkbox(style.auto_size)
        .label("Auto size")
        .text_size(scale::s(12.0))
        .on_toggle_maybe(selected.then_some(UiEvent::StyleAutoSize))
        .into();
    // Fixed size is inert while auto-size fits the box.
    let fixed_input = style_number(
        Icon::Type,
        style.font_size,
        1.0..=500.0,
        1.0,
        10.0,
        UiEvent::StyleFontSize,
        selected && !style.auto_size,
    );
    let caps_aa = segment_compact(
        style.caps == CapsMode::AllCaps,
        "AA",
        selected.then_some(UiEvent::StyleCaps(if style.caps == CapsMode::AllCaps {
            CapsMode::None
        } else {
            CapsMode::AllCaps
        })),
        Font::DEFAULT,
    );
    let caps_sc = segment_compact(
        style.caps == CapsMode::SmallCaps,
        "aA",
        selected.then_some(UiEvent::StyleCaps(if style.caps == CapsMode::SmallCaps {
            CapsMode::None
        } else {
            CapsMode::SmallCaps
        })),
        Font::DEFAULT,
    );
    column![
        row![
            container(auto).width(Length::FillPortion(1)),
            container(
                column![caption("Size"), fixed_input,]
                    .spacing(scale::s(4.0))
            )
            .width(Length::FillPortion(1)),
            container(
                column![
                    caption("Case"),
                    segmented_group(vec![caps_aa, caps_sc]),
                ]
                .spacing(scale::s(4.0))
            )
            .width(Length::FillPortion(1)),
        ]
        .spacing(scale::s(8.0))
        .align_y(iced::Alignment::End),
        row![
            container(
                column![
                    caption("Line height"),
                    style_number(
                        Icon::AlignCenter,
                        style.line_height,
                        0.5..=3.0,
                        0.1,
                        0.5,
                        UiEvent::StyleLineHeight,
                        selected,
                    ),
                ]
                .spacing(scale::s(4.0))
            )
            .width(Length::FillPortion(1)),
            container(
                column![
                    caption("Letter spacing"),
                    style_number(
                        Icon::Minus,
                        style.letter_spacing,
                        0.0..=20.0,
                        0.5,
                        2.0,
                        UiEvent::StyleLetterSpacing,
                        selected,
                    ),
                ]
                .spacing(scale::s(4.0))
            )
            .width(Length::FillPortion(1)),
        ]
        .spacing(scale::s(8.0))
        .align_y(iced::Alignment::End),
    ]
    .spacing(scale::s(8.0))
    .into()
}

/// The "Fill" section: a single unified hex input (solid or gradient with
/// angle). No Solid|Gradient tabs: the widget switches modes itself via the
/// picker / hex field.
fn fill_section<'a, S: UiState + ?Sized>(state: &'a S) -> Element<'a, UiEvent> {
    column![
        section_title("Fill"),
        unified_field(state, StyleField::Fill, state.style_fill_value()),
    ]
    .spacing(scale::s(8.0))
    .into()
}

/// The "Stroke" section: unified solid-or-gradient input plus width.
fn stroke_section<'a, S: UiState + ?Sized>(
    state: &'a S,
    selected: bool,
) -> Element<'a, UiEvent> {
    column![
        section_title("Stroke"),
        row![
            container(unified_field(
                state,
                StyleField::Stroke,
                state.style_stroke_value(),
            ))
            .width(Length::FillPortion(3)),
            container(style_number(
                Icon::Minus,
                state.style_stroke_width(),
                0.0..=50.0,
                0.5,
                2.0,
                UiEvent::StyleStrokeWidth,
                selected,
            ))
            .width(Length::FillPortion(2)),
        ]
        .spacing(scale::s(8.0)),
    ]
    .spacing(scale::s(8.0))
    .into()
}

/// Split-button value for the "Inpaint Background" control. `SplitButton`
/// renders the selected *value's* `Display` on its face (item labels only
/// appear in the menu rows), so this wrapper carries the full face text
/// (`Inpaint Background (<backend>)`) while the menu rows use the short
/// backend names from the item labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PanelBackend(InpaintBackend);

impl std::fmt::Display for PanelBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let short = match self.0 {
            InpaintBackend::Harmonic => "Harmonic",
            InpaintBackend::Telea => "Telea",
            InpaintBackend::Lama => "LaMa",
            InpaintBackend::Aot => "AOT-GAN",
            InpaintBackend::ShiftMap => "ShiftMap",
        };
        write!(f, "Inpaint Background ({short})")
    }
}

/// The background section: background color plus corner radius, each with
/// its own caption, with an "Inpaint Background" split button. The main area makes the bg
/// transparent and inpaints the *current* view quad (not the original OCR
/// quad) — i.e. the box's present position/size after any
/// move/resize/rotate/distort — with the default backend; the arrow zone
/// opens a menu to switch the default inpaint backend directly (select-only:
/// it persists `inpaint_backend` without running anything).
/// While nothing is selected or a bulk job is busy the whole control falls
/// back to a plain inert button, since `SplitButton` has no disabled state
/// (omitting `on_press` alone would still leave the arrow menu live).
fn background_section<'a, S: UiState + ?Sized>(
    state: &'a S,
    selected: bool,
) -> Element<'a, UiEvent> {
    let backend = easyscanlate_settings::get(|s| s.inpaint_backend);
    let action: Element<'a, UiEvent> = if selected && !state.is_bulk_busy() {
        let options = [
            MenuItem::Item(
                Item::new(PanelBackend(InpaintBackend::Harmonic), "Harmonic").icon(
                    crate::icon::lucide(Icon::Eraser).size(scale::s(14.0)).center(),
                ),
            ),
            MenuItem::Item(
                Item::new(PanelBackend(InpaintBackend::Telea), "Telea").icon(
                    crate::icon::lucide(Icon::Eraser).size(scale::s(14.0)).center(),
                ),
            ),
            MenuItem::Item(
                Item::new(PanelBackend(InpaintBackend::Lama), "LaMa").icon(
                    crate::icon::lucide(Icon::Eraser).size(scale::s(14.0)).center(),
                ),
            ),
            MenuItem::Item(
                Item::new(PanelBackend(InpaintBackend::Aot), "AOT-GAN").icon(
                    crate::icon::lucide(Icon::Eraser).size(scale::s(14.0)).center(),
                ),
            ),
            MenuItem::Item(
                Item::new(PanelBackend(InpaintBackend::ShiftMap), "ShiftMap").icon(
                    crate::icon::lucide(Icon::Eraser).size(scale::s(14.0)).center(),
                ),
            ),
        ];
        split_button(
            options,
            Some(PanelBackend(backend)),
            |pick: PanelBackend| UiEvent::StyleInpaintBackendSelected(pick.0),
        )
            .on_press(|_| UiEvent::StyleInpaintBackground)
            .width(FillLength)
            .padding(scale::s(6.0))
            .text_size(scale::s(11.0))
            .style(
                |_theme, status: neverliie_iced_widgets::split_button::Status| {
                    use neverliie_iced_widgets::split_button::Status as SplitStatus;
                    let (bg, txt) = match status {
                        SplitStatus::Active => (crate::panel::PANEL_BG, TEXT_MAIN),
                        SplitStatus::Hovered => {
                            (Color::from_rgba8(46, 48, 62, 0.82), TEXT_MAIN)
                        }
                        SplitStatus::Opened { .. } => {
                            (Color::from_rgba8(55, 57, 72, 0.87), TEXT_MAIN)
                        }
                        SplitStatus::Disabled => {
                            (Color::from_rgba8(34, 36, 44, 0.35), MUTED_FG)
                        }
                    };
                    neverliie_iced_widgets::split_button::Style {
                        text_color: txt,
                        placeholder_color: txt,
                        handle_color: txt,
                        background: Background::Color(bg),
                        border: Border {
                            radius: scale::s(4.0).into(),
                            // Width stays 0 (no outline drawn), but the color
                            // must be visible: SplitButton reuses it as the
                            // source for the main|arrow divider (0.35 alpha)
                            // and the per-zone hover tints (0.10 main, 0.25
                            // arrow). TRANSPARENT here would hide both.
                            width: 0.0,
                            color: TEXT_MAIN,
                        },
                        shadow: Shadow::default(),
                    }
                },
            )
            .into()
    } else {
        button(
            row![
                crate::icon::lucide(Icon::Eraser).size(scale::s(14.0)).center(),
                text(PanelBackend(backend).to_string()).size(scale::s(11.0))
            ]
            .spacing(scale::s(4.0))
            .align_y(iced::Alignment::Center),
        )
        .width(FillLength)
        .padding(scale::s(6.0))
        .style(crate::panel::button_style)
        .on_press_maybe(None::<UiEvent>)
        .into()
    };
    column![
        row![
            container(
                column![
                    caption("Background"),
                    unified_field(
                        state,
                        StyleField::Background,
                        state.style_bg_value(),
                    ),
                ]
                .spacing(scale::s(4.0))
            )
            .width(Length::FillPortion(3)),
            container(
                column![
                    caption("Corner"),
                    style_number(
                        Icon::SquareRoundCorner,
                        state.style_bg_radius(),
                        0.0..=100.0,
                        1.0,
                        10.0,
                        UiEvent::StyleBgRadius,
                        selected,
                    ),
                ]
                .spacing(scale::s(4.0))
            )
            .width(Length::FillPortion(2)),
        ]
        .spacing(scale::s(8.0))
        .align_y(iced::Alignment::End),
        crate::button::with_disabled_cursor(action),
    ]
    .spacing(scale::s(8.0))
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
                let color = if (x / tile + y / tile).is_multiple_of(2) {
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
    let button = crate::button::with_disabled_cursor(
        button(glyph)
            .width(FillLength)
            .height(FillLength)
            .padding(Padding::ZERO)
            .style(crate::panel::button_style)
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
                        radius: scale::s(PRESET_RADIUS).into(),
                        width: scale::s(1.0),
                        color: border_color,
                    },
                    shadow: Shadow::default(),
                    ..button::Style::default()
                }
            })
            .into(),
    );
    iced::widget::stack![underlay, button]
        .width(iced::Length::Fixed(scale::s(PRESET_SIDE)))
        .height(iced::Length::Fixed(scale::s(PRESET_SIDE)))
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
        crate::icon::lucide(Icon::Plus)
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

/// An empty preset slot: checkerboard underlay and a muted ellipse, inert to
/// clicks but right-clickable to fill it via the context menu.
fn empty_square<'a>() -> Element<'a, UiEvent> {
    square_tile(
        image::Image::new(checker_handle().clone())
            .width(FillLength)
            .height(FillLength)
            .border_radius(PRESET_RADIUS)
            .into(),
        crate::icon::lucide(Icon::Ellipse)
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
                    .text_size(scale::s(12.0))
                    .into();
            };
            let tile = preset_square(preset.clone(), can_apply.then_some(UiEvent::StylePresetApply(index)));
            ContextMenu::new(tile, preset_menu(index, true))
                .on_dismiss(UiEvent::StylePresetMenuDismiss)
                .text_size(scale::s(12.0))
                .into()
        })
        .collect();
    tiles.push(add_square(Some(UiEvent::StylePresetAdd)));
    let mut columns: Vec<Element<'a, UiEvent>> = Vec::with_capacity(tiles.len().div_ceil(2));
    while !tiles.is_empty() {
        let top = tiles.remove(0);
        let bottom = if tiles.is_empty() {
            Space::new().into()
        } else {
            tiles.remove(0)
        };
        columns.push(column![top, bottom].spacing(scale::s(4.0)).into());
    }
    let strip = scrollable::Scrollable::with_direction(
        row(columns).spacing(scale::s(4.0)),
        scrollable::Direction::Horizontal(crate::scroll::horizontal()),
    )
    .width(FillLength)
    .height(scale::s(PRESET_SIDE * 2.0 + 4.0 + 10.0));
    column![
        text("Presets").size(scale::s(12.0)).color(MUTED_FG),
        strip,
    ]
    .spacing(scale::s(4.0))
    .into()
}

pub fn view<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    let style = state.style_working();
    let selected = state.selected().is_some();

    scrollable(column![
        header_row(state, selected),
        font_field(state),
        format_align_row(style, selected),
        typography_section(state, style, selected),
        fill_section(state),
        stroke_section(state, selected),
        background_section(state, selected),
        presets_grid(state),
        text(HINT).size(scale::s(12.0)).color(MUTED_FG),
    ]
    .spacing(scale::s(10.0)))
    .spacing(scale::s(crate::scroll::EMBEDDED_SPACING))
    .width(FillLength)
    .height(FillLength)
    .into()
}

/// Compatibility shim: each [`unified_field`] now self-hosts its own
/// floating picker, so no shell-root host is needed. Kept so `shell`
/// keeps compiling while callers migrate; just returns `base`.
pub fn overlay_host<'a, S: UiState + ?Sized>(
    _state: &'a S,
    base: Element<'a, UiEvent>,
) -> Element<'a, UiEvent> {
    base
}