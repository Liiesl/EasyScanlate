//! Photoshop-like layer list for inpaint patches (and future notes).
//! Reads `Project.extras.inpaint_patches` plus `LoadedImage.inpaint` layers.
//! Clicking a row selects the patch: the main area highlights its bbox (like
//! result selection) with a static border (no move/resize) and a floating
//! Delete / Repaint toolbar. While an inpaint is selected OCR overlays are
//! hidden. The row itself shows Delete / Repaint when selected.

use iced::widget::image::{self, Handle};
use iced::widget::{button, column, container, mouse_area, row, scrollable, space, text};
use iced::{Border, Color, Element, Fill as FillLength, Length, Padding};

use crate::event::UiEvent;
use crate::panel::{MUTED_FG, PANEL_BG};
use crate::scale;
use crate::state::UiState;

/// Background of a layer row – intentionally translucent like the outer
/// `PANEL_BG` card so the aurora shows through in a satisfying stack:
/// aurora → outer PANEL_BG (0.78) → inner list (0.32) → row (0.48).
const ROW_BG: Color = Color::from_rgba8(34, 36, 44, 0.48);
const ROW_BG_DIMMED: Color = Color::from_rgba8(34, 36, 44, 0.26);
const ROW_BORDER: Color = Color::from_rgba8(255, 255, 255, 0.10);
const SELECTED_BORDER: Color = Color::from_rgba8(92, 190, 255, 0.9);
const SELECTED_BG: Color = Color::from_rgba8(92, 190, 255, 0.08);
/// Inner scrollable inset – even more transparent than the row so the
/// row floats over it and the aurora layers clearly.
const INNER_BG: Color = Color::from_rgba8(34, 36, 44, 0.24);

fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

fn header_row<'a, S: UiState + ?Sized>(state: &'a S, total: usize) -> Element<'a, UiEvent> {
    let count_label = if total == 0 {
        "0".to_string()
    } else {
        total.to_string()
    };
    row![
        text("LAYERS").size(scale::s(11.0)).color(MUTED_FG),
        container(
            text(count_label)
                .size(scale::s(10.0))
                .color(Color::from_rgb8(180, 180, 180))
        )
        .padding([scale::s(1.0), scale::s(6.0)])
        .style(|_theme| container::Style {
            background: Some(Color::from_rgb8(38, 40, 50).into()),
            border: Border::default().rounded(scale::s(10.0)),
            ..container::Style::default()
        }),
        space::horizontal(),
        button(
            text(if state.show_inpaint() { "👁" } else { "🚫" }).size(scale::s(12.0))
        )
        .padding([scale::s(2.0), scale::s(6.0)])
        .on_press(UiEvent::ToggleInpaintLayer)
        .style(|_theme, status| {
            use iced::widget::button::Status;
            let hovered = matches!(status, Status::Hovered | Status::Pressed);
            button::Style {
                background: Some(Color::from_rgb8(42, 44, 54).into()),
                border: Border::default().rounded(scale::s(4.0)),
                text_color: if hovered {
                    Color::WHITE
                } else {
                    MUTED_FG
                },
                ..button::Style::default()
            }
        }),
    ]
    .spacing(scale::s(6.0))
    .align_y(iced::Alignment::Center)
    .into()
}

/// One layer row: eye, 28px thumbnail, title/subtitle and, when selected,
/// Delete / Repaint actions. The whole row is a click target that toggles
/// inpaint selection (like the results list highlights its overlay).
fn layer_row<'a>(
    image_name: &'a str,
    image_index: usize,
    index_in_image: usize,
    bounds: [f32; 4],
    handle: Option<Handle>,
    global_visible: bool,
    is_selected: bool,
) -> Element<'a, UiEvent> {
    let [x, y, w, h] = bounds;
    let title = format!("Inpaint  {}", index_in_image + 1);
    let subtitle = format!("{}  •  {:.0}×{:.0}  @ {:.0},{:.0}", image_name, w, h, x, y);

    // Thumbnail: actual crop preview. When no handle is available (model-only
    // patch without runtime pixels) we fall back to a muted placeholder.
    let thumb: Element<'a, UiEvent> = if let Some(hdl) = handle {
        // Dim the preview when the global inpaint layer is hidden, like PS eye off.
        let opacity = if global_visible { 1.0 } else { 0.45 };
        container(
            image::Image::new(hdl)
                .width(FillLength)
                .height(FillLength)
                .border_radius(3.0)
                .opacity(opacity),
        )
        .width(Length::Fixed(scale::s(28.0)))
        .height(Length::Fixed(scale::s(28.0)))
        .style(|_theme| container::Style {
            background: Some(Color::from_rgba8(20, 20, 25, 0.9).into()),
            border: Border {
                color: Color::from_rgba8(255, 255, 255, 0.18),
                width: scale::s(1.0),
                radius: scale::s(3.0).into(),
            },
            ..container::Style::default()
        })
        .into()
    } else {
        container(space::horizontal())
            .width(Length::Fixed(scale::s(28.0)))
            .height(Length::Fixed(scale::s(28.0)))
            .style(move |_theme| container::Style {
                background: Some(if global_visible {
                    Color::from_rgba8(90, 110, 140, 0.85).into()
                } else {
                    Color::from_rgba8(48, 48, 54, 0.6).into()
                }),
                border: Border {
                    color: Color::from_rgba8(255, 255, 255, 0.18),
                    width: scale::s(1.0),
                    radius: scale::s(3.0).into(),
                },
                ..container::Style::default()
            })
            .into()
    };

    let eye = text(if global_visible { "👁" } else { "·" })
        .size(scale::s(11.0))
        .color(if global_visible { MUTED_FG } else { Color::from_rgba8(120, 120, 120, 0.9) })
        .width(Length::Fixed(scale::s(16.0)))
        .center();

    let row_bg = if global_visible { ROW_BG } else { ROW_BG_DIMMED };

    // Right-side actions: only when selected, mirroring the results row's
    // Delete / Retranslate but for inpaint: Delete / Repaint (exact rect).
    let actions: Element<'a, UiEvent> = if is_selected {
        row![
            button(text("Delete").size(scale::s(10.0)))
                .padding([scale::s(2.0), scale::s(6.0)])
                .on_press(UiEvent::InpaintDelete((image_index, index_in_image))),
            button(text("Repaint").size(scale::s(10.0)))
                .padding([scale::s(2.0), scale::s(6.0)])
                .on_press(UiEvent::InpaintRepaint((image_index, index_in_image))),
        ]
        .spacing(scale::s(4.0))
        .into()
    } else {
        text("⋮").size(scale::s(12.0)).color(MUTED_FG).into()
    };

    let inner = row![
        eye,
        thumb,
        column![
            text(title).size(scale::s(11.0)).color(Color::from_rgb8(230, 230, 230)),
            text(subtitle).size(scale::s(10.0)).color(MUTED_FG),
        ]
        .spacing(scale::s(1.0))
        .width(FillLength),
        actions,
    ]
    .spacing(scale::s(8.0))
    .align_y(iced::Alignment::Center);

    let styled = container(inner)
        .width(FillLength)
        .padding(Padding {
            top: scale::s(6.0),
            right: scale::s(8.0),
            bottom: scale::s(6.0),
            left: scale::s(8.0),
        })
        .style(move |_theme| container::Style {
            background: Some(if is_selected { SELECTED_BG } else { row_bg }.into()),
            border: Border::default()
                .width(scale::s(1.0))
                .color(if is_selected { SELECTED_BORDER } else { ROW_BORDER })
                .rounded(scale::s(6.0)),
            ..container::Style::default()
        });

    // Clicking the row selects the inpaint (like result item highlight on main
    // area). Clicking the already-selected row deselects it. Buttons inside
    // capture clicks before the outer mouse_area.
    let target = if is_selected {
        UiEvent::InpaintClicked(None)
    } else {
        UiEvent::InpaintClicked(Some((image_index, index_in_image)))
    };
    mouse_area(styled).on_press(target).into()
}

fn image_header<'a>(name: &'a str, count: usize) -> Element<'a, UiEvent> {
    row![
        text(name).size(scale::s(11.0)).color(MUTED_FG),
        space::horizontal(),
        text(format!("{} patch{}", count, if count == 1 { "" } else { "es" }))
            .size(scale::s(10.0))
            .color(Color::from_rgba8(140, 140, 150, 1.0)),
    ]
    .spacing(scale::s(6.0))
    .align_y(iced::Alignment::Center)
    .into()
}

pub fn view<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    let images = state.images();
    let global_visible = state.show_inpaint();
    let selected_inpaint = state.selected_inpaint();

    // Aggregate patches per image. For a proper preview we need the GPU `Handle`
    // stored in `LoadedImage.inpaint` (the actual crop pixels). The model
    // `extras.inpaint_patches` only carries bounds, so we prefer the runtime
    // layers when they exist; otherwise we fall back to extras with a placeholder thumb.
    let mut total = 0usize;
    let mut sections: Vec<Element<'_, UiEvent>> = Vec::new();

    for (image_index, img) in images.iter().enumerate() {
        let name = file_name(&img.path);

        // Build (bounds, Option<Handle>) list per image.
        let entries: Vec<([f32; 4], Option<Handle>)> = if !img.inpaint.is_empty() {
            img.inpaint
                .iter()
                .map(|layer| (layer.bounds, Some(layer.handle.clone())))
                .collect()
        } else if !img.project.extras.inpaint_patches.is_empty() {
            img.project
                .extras
                .inpaint_patches
                .iter()
                .map(|p| (p.bounds, None))
                .collect()
        } else {
            Vec::new()
        };

        if entries.is_empty() {
            continue;
        }
        total += entries.len();
        sections.push(image_header(name, entries.len()));
        for (i, (bounds, handle)) in entries.into_iter().enumerate() {
            let is_selected = selected_inpaint == Some((image_index, i));
            sections.push(layer_row(name, image_index, i, bounds, handle, global_visible, is_selected));
        }
    }

    // Empty / no patches
    let list: Element<'_, UiEvent> = if sections.is_empty() {
        if images.is_empty() {
            column![
                text("No images loaded.")
                    .size(scale::s(11.0))
                    .color(MUTED_FG),
                text("Open images to see inpaint layers here.")
                    .size(scale::s(10.0))
                    .color(Color::from_rgba8(140, 140, 150, 1.0)),
            ]
            .spacing(scale::s(4.0))
            .into()
        } else {
            column![
                text("No inpaint layers yet.")
                    .size(scale::s(11.0))
                    .color(Color::from_rgb8(210, 210, 210)),
                text("Drag on the image in Inpaint mode to create a patch.")
                    .size(scale::s(10.0))
                    .color(MUTED_FG),
                container(
                    text("Tip: click a layer to highlight its box on the image. Delete / Repaint appear when selected.")
                        .size(scale::s(10.0))
                        .color(Color::from_rgba8(130, 130, 140, 1.0))
                )
                .padding([scale::s(6.0), scale::s(0.0)])
            ]
            .spacing(scale::s(6.0))
            .into()
        }
    } else {
        column(sections).spacing(scale::s(6.0)).into()
    };

    let header = header_row(state, total);

    // Inner inset is now 0.24 – rows at 0.48 float over it, both over
    // outer PANEL_BG 0.78, so the aurora layers satisfyingly.
    column![
        header,
        container(
            scrollable(list).width(FillLength).height(FillLength)
        )
        .width(FillLength)
        .height(FillLength)
        .padding(scale::s(4.0))
        .style(|_theme| container::Style {
            background: Some(INNER_BG.into()),
            border: Border {
                color: Color::from_rgba8(255, 255, 255, 0.05),
                width: scale::s(1.0),
                radius: scale::s(6.0).into(),
            },
            ..container::Style::default()
        }),
        // Footer hint, subtle
        text("Inpaint • Notes (layers)").size(scale::s(9.0)).color(Color::from_rgba8(110, 110, 120, 1.0)),
    ]
    .spacing(scale::s(8.0))
    .height(FillLength)
    .into()
}
