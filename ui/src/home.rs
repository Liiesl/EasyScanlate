use std::collections::BTreeSet;
use std::time::Duration;

use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Border, Color, Element, Fill as FillLength, Length};
use lucide_icons::Icon;
use neverliie_iced_widgets::advanced_dropdown::{
    Footer, Handle, Item, MenuItem, Status as DropdownStatus, advanced_dropdown,
};
use neverliie_iced_widgets::ellipsis_text::EllipsisText;

use crate::event::UiEvent;
use crate::panel::PANEL_BG;
use crate::scale;
use crate::segmented::{BORDER, INPUT_BG, MUTED_FG, TEXT_MAIN};
use crate::state::HomeSelection;

const SIDEBAR_WIDTH: f32 = 200.0;

/// Maximum gap between two presses on the same Home row to count as a
/// double-click (opens the project).
pub const DOUBLE_CLICK_DELAY: Duration = Duration::from_millis(400);

/// Value/label pair for the Series column dropdown. The Display form is
/// shortened so the closed cell never overflows the fixed column; menu items
/// keep the full name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct HomeSeriesOption {
    name: Option<String>,
}

impl std::fmt::Display for HomeSeriesOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.name {
            Some(name) => f.write_str(&truncate_series_label(name)),
            None => f.write_str("—"),
        }
    }
}

fn truncate_series_label(name: &str) -> String {
    const MAX_CHARS: usize = 18;
    if name.chars().count() <= MAX_CHARS {
        return name.to_string();
    }
    let mut out: String = name.chars().take(MAX_CHARS - 1).collect();
    out.push('…');
    out
}

/// Fixed widths of the main-panel table columns (shared by rows + header).
/// Name and Path are flexible (`Fill` / `FillPortion(2)`): they split all
/// leftover space with no dead pad, ellipsizing only what truly overflows.
const SIZE_W: f32 = 80.0;
const SERIES_W: f32 = 150.0;
const REL_W: f32 = 140.0;

/// Highlight behind a selected nav item — mirrors the results list
/// (`panel/results.rs::SELECTED_BG`): slightly lighter and more opaque than
/// `PANEL_BG`, flat in every status, no border.
const SELECTED_BG: Color = Color::from_rgba8(52, 58, 76, 0.90);

/// Distinct series names in the sidebar, most-recently-touched first.
fn series_names(items: &[easyscanlate_settings::series::TrackedProject]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for item in items {
        if let Some(s) = item.series.as_deref()
            && seen.insert(s.to_string())
        {
            out.push(s.to_string());
        }
    }
    out
}

/// Sidebar series row: bare name only (no count), clamped to a single line
/// with `…`. Color is left unset so it inherits the button's `text_color`
/// (selected / hover states).
fn series_button<'a>(name: String, selected: bool, event: UiEvent) -> Element<'a, UiEvent> {
    button(EllipsisText::new(name).size(scale::s(13.0)).max_lines(1))
        .width(Length::Fill)
        .padding(scale::s(10.0))
        .style(move |theme, status| {
            if selected {
                iced::widget::button::Style {
                    background: Some(SELECTED_BG.into()),
                    border: iced::Border::default().rounded(scale::s(4.0)),
                    text_color: crate::segmented::TEXT_MAIN,
                    ..Default::default()
                }
            } else {
                crate::panel::button_style(theme, status)
            }
        })
        .on_press(event)
        .into()
}

/// Style for the `Series` group header: a chrome-less clickable label — no
/// background or border in any status, only the text (and chevron, which is
/// also text) brightens on hover/press.
fn group_header_style(
    _theme: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    use iced::widget::button::Status;
    let text_color = match status {
        Status::Hovered | Status::Pressed => crate::segmented::TEXT_MAIN,
        Status::Disabled => crate::segmented::MUTED_FG,
        Status::Active => crate::segmented::MUTED_FG,
    };
    iced::widget::button::Style {
        background: None,
        border: iced::Border::default(),
        shadow: iced::Shadow::default(),
        text_color,
        ..iced::widget::button::Style::default()
    }
}
/// Sidebar nav button; `selected` renders the active filter distinctly.
fn nav_button<'a>(label: String, selected: bool, event: UiEvent) -> Element<'a, UiEvent> {
    button(text(label).size(scale::s(13.0)).width(FillLength))
        .width(Length::Fill)
        .padding(scale::s(10.0))
        .style(move |theme, status| {
            if selected {
                iced::widget::button::Style {
                    background: Some(SELECTED_BG.into()),
                    border: iced::Border::default().rounded(scale::s(4.0)),
                    text_color: crate::segmented::TEXT_MAIN,
                    ..Default::default()
                }
            } else {
                crate::panel::button_style(theme, status)
            }
        })
        .on_press(event)
        .into()
}

fn icon_button<'a>(icon: Icon, size: f32, event: UiEvent) -> Element<'a, UiEvent> {
    button(crate::icon::lucide(icon).size(scale::s(size)).center())
        .padding(scale::s(10.0))
        .style(crate::panel::button_style)
        .on_press(event)
        .into()
}

/// File size of `path` as `1.2 MB` / `340 KB` / `12 B`; `—` when the file
/// cannot be statted. Resolved at view time so it is always accurate — only
/// the visible rows pay one `metadata` call each.
fn file_size_display(path: &str) -> String {
    match std::fs::metadata(path).map(|m| m.len()) {
        Ok(len) => format_size(len),
        Err(_) => "—".to_string(),
    }
}

fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// Single-line ellipsis table cell with an explicit color (table rows use a
/// transparent button style, so no text color is inherited from it).
fn ellipsis_cell<'a>(
    content: String,
    size: f32,
    color: Color,
    width: Length,
) -> Element<'a, UiEvent> {
    container(
        EllipsisText::new(content)
            .size(scale::s(size))
            .max_lines(1)
            .color(color),
    )
    .width(width)
    .into()
}

/// Compact inline input used by the home "+ New series" editor.
fn home_input_style(_theme: &iced::Theme, _status: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: INPUT_BG.into(),
        border: Border {
            color: BORDER,
            width: scale::s(1.0),
            radius: scale::s(4.0).into(),
        },
        placeholder: MUTED_FG,
        value: TEXT_MAIN,
        selection: crate::accent::accent(),
        icon: MUTED_FG,
    }
}

/// Closed field of the series dropdown: invisible (plain text) at rest, a
/// subtle rounded highlight + brighter text on hover/open.
fn series_dropdown_style(
    _theme: &iced::Theme,
    status: DropdownStatus,
) -> neverliie_iced_widgets::advanced_dropdown::Style {
    let (text_color, background, border): (Color, iced::Background, Border) = match status {
        DropdownStatus::Active => (MUTED_FG, Color::TRANSPARENT.into(), Border::default()),
        DropdownStatus::Hovered | DropdownStatus::Opened { .. } => (
            TEXT_MAIN,
            Color::from_rgba8(255, 255, 255, 0.10).into(),
            Border::default().rounded(scale::s(4.0)),
        ),
    };
    neverliie_iced_widgets::advanced_dropdown::Style {
        text_color,
        placeholder_color: MUTED_FG,
        handle_color: MUTED_FG,
        background,
        border,
    }
}

/// The Series table cell: a text-looking dropdown that reassigns the row's
/// project without opening it.
fn series_dropdown_cell<'a>(
    path: String,
    current: Option<String>,
    available: &[String],
) -> Element<'a, UiEvent> {
    let mut entries: Vec<MenuItem<'a, HomeSeriesOption, UiEvent, iced::Theme, iced::Renderer>> =
        Vec::with_capacity(available.len() + 3);
    entries.push(MenuItem::Item(Item::new(
        HomeSeriesOption { name: None },
        "No series",
    )));
    if !available.is_empty() {
        entries.push(MenuItem::Separator);
        entries.push(MenuItem::Label("Series"));
        for name in available {
            entries.push(MenuItem::Item(Item::new(
                HomeSeriesOption {
                    name: Some(name.clone()),
                },
                name.clone(),
            )));
        }
    }
    let select_path = path.clone();
    let selected = Some(HomeSeriesOption { name: current });
    advanced_dropdown(entries, selected, move |opt: HomeSeriesOption| {
        UiEvent::HomeSeriesChanged {
            path: select_path.clone(),
            series: opt.name,
        }
    })
    .placeholder("—")
    .text_size(scale::s(12.0))
    .width(Length::Fixed(scale::s(SERIES_W)))
    .padding([0.0, scale::s(4.0)])
    .menu_max_height(240.0)
    .handle(Handle::None)
    .style(series_dropdown_style)
    .footer(Footer::new(
        "+ New series",
        UiEvent::HomeSeriesCreateStart(path),
    ))
    .into()
}

/// Inline editor shown under a Home row while creating/assigning a new series.
fn home_series_editor<'a>(name: &'a str) -> Element<'a, UiEvent> {
    container(
        row![
            text("New series").size(scale::s(11.0)).color(MUTED_FG),
            text_input("New series name…", name)
                .on_input(UiEvent::HomeSeriesName)
                .on_submit(UiEvent::HomeSeriesCreateConfirm)
                .padding(scale::s(6.0))
                .size(scale::s(12.0))
                .width(FillLength)
                .style(home_input_style),
            button(
                text("Add")
                    .size(scale::s(12.0))
                    .width(FillLength)
                    .center()
            )
            .padding(scale::s(6.0))
            .width(Length::Fixed(scale::s(64.0)))
            .style(crate::panel::button_style)
            .on_press(UiEvent::HomeSeriesCreateConfirm),
            button(
                text("Cancel")
                    .size(scale::s(12.0))
                    .width(FillLength)
                    .center()
            )
            .padding(scale::s(6.0))
            .width(Length::Fixed(scale::s(76.0)))
            .style(crate::panel::button_style)
            .on_press(UiEvent::HomeSeriesCancel),
        ]
        .spacing(scale::s(8.0))
        .align_y(iced::Alignment::Center)
        .width(FillLength),
    )
    .padding(scale::s(8.0))
    .width(FillLength)
    .style(|_| container::Style {
        background: Some(Color::from_rgba8(43, 46, 58, 0.65).into()),
        border: Border::default().rounded(scale::s(6.0)),
        ..Default::default()
    })
    .into()
}

fn project_row<'a>(
    name: String,
    size: String,
    path: String,
    show_series: bool,
    current_series: Option<String>,
    series_choices: &[String],
    selected: bool,
    rel: String,
) -> Element<'a, UiEvent> {
    let open_path = path.clone();
    let muted = Color::from_rgb(0.6, 0.6, 0.6);
    let mut cells = row![
        ellipsis_cell(name, 13.0, Color::WHITE, FillLength),
        ellipsis_cell(size, 12.0, muted, Length::Fixed(scale::s(SIZE_W))),
        ellipsis_cell(path, 12.0, muted, Length::FillPortion(2)),
    ]
    .spacing(scale::s(8.0))
    .align_y(iced::Alignment::Center);
    // `show_series` is false in the series-filtered view (implied by the
    // selection); otherwise the cell is an inline reassignment dropdown.
    if show_series {
        cells = cells.push(series_dropdown_cell(
            open_path.clone(),
            current_series,
            series_choices,
        ));
    }
    cells = cells.push(
        text(rel)
            .size(scale::s(12.0))
            .color(muted)
            .width(Length::Fixed(scale::s(REL_W))),
    );
    button(cells)
        .width(Length::Fill)
        .padding(scale::s(10.0))
        .style(move |_, status| {
            let hovered = status == iced::widget::button::Status::Hovered;
            let bg = if selected {
                SELECTED_BG
            } else if hovered {
                Color::from_rgba8(255, 255, 255, 0.08)
            } else {
                Color::TRANSPARENT
            };
            iced::widget::button::Style {
                background: Some(bg.into()),
                border: iced::Border::default().rounded(scale::s(6.0)),
                ..Default::default()
            }
        })
        .on_press(UiEvent::HomeProjectClicked(open_path))
        .into()
}

fn empty_hint(msg: &str) -> Element<'_, UiEvent> {
    container(
        text(msg)
            .size(scale::s(13.0))
            .color(Color::from_rgb(0.6, 0.6, 0.6)),
    )
    .padding(scale::s(20.0))
    .width(Length::Fill)
    .center_x(Length::Fill)
    .into()
}

pub fn view<'a, S: crate::state::UiState + ?Sized>(state: &'a S) -> Element<'a, UiEvent> {
    let selection = state.home_selection();
    let series_items = state.series_items();
    let names = series_names(series_items);
    let selected_project = state.home_selected_project();

    // --- Sidebar top: Recent (standalone) nav + one collapsible Series group ---
    // The group lists clickable series names only (bare, single-line
    // ellipsis); the `.mmtl` files of the selected series live in the main
    // panel.
    let recent_count = state.recent_projects().len();
    let recent_label = if recent_count > 0 {
        format!("Recent ({recent_count})")
    } else {
        "Recent".to_string()
    };
    let mut nav = column![nav_button(
        recent_label,
        selection == HomeSelection::Recent,
        UiEvent::HomeSelectRecent,
    ),]
    .spacing(scale::s(4.0))
    .width(Length::Fill);

    // Series group: clickable names only; `.mmtl` files show in the main panel.
    let group_collapsed = state.is_series_group_collapsed();
    if !names.is_empty() {
        let group_header: Element<'_, UiEvent> = button(
            row![
                crate::icon::lucide(if group_collapsed {
                    Icon::ChevronRight
                } else {
                    Icon::ChevronDown
                })
                .size(scale::s(14.0))
                .center(),
                text("Series").size(scale::s(13.0)),
            ]
            .spacing(scale::s(4.0))
            .align_y(iced::Alignment::Center)
            .width(Length::Fill),
        )
        .width(Length::Fill)
        .padding(scale::s(8.0))
        .style(group_header_style)
        .on_press(UiEvent::HomeToggleSeriesGroup)
        .into();
        nav = nav.push(group_header);
        if !group_collapsed {
            for name in &names {
                let selected = selection == HomeSelection::Series(name.clone());
                nav = nav.push(series_button(
                    name.clone(),
                    selected,
                    UiEvent::HomeSelectSeries(name.clone()),
                ));
            }
        }
    }
    let nav_scroll: Element<'_, UiEvent> = scrollable(nav)
        .spacing(scale::s(crate::scroll::EMBEDDED_SPACING))
        .height(Length::Fill)
        .width(Length::Fill)
        .into();

    // --- Sidebar bottom: New Project (full width + icon), then Open + cog ---
    let new_btn = button(
        row![
            crate::icon::lucide(Icon::Plus)
                .size(scale::s(14.0))
                .center(),
            text("New Project")
                .size(scale::s(13.0))
                .width(FillLength)
                .center(),
        ]
        .spacing(scale::s(8.0))
        .align_y(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .padding(scale::s(12.0))
    .style(crate::panel::button_style)
    .on_press(UiEvent::HomeNewProject);
    let open_btn = button(
        row![
            crate::icon::lucide(Icon::FolderOpen)
                .size(scale::s(14.0))
                .center(),
            text("Open Project").size(scale::s(13.0)),
        ]
        .spacing(scale::s(8.0))
        .align_y(iced::Alignment::Center),
    )
    .width(Length::Fill)
    .padding(scale::s(12.0))
    .style(crate::panel::button_style)
    .on_press(UiEvent::HomeOpenProject);
    let bottom = column![
        new_btn,
        row![
            open_btn,
            icon_button(Icon::Settings, 16.0, UiEvent::HomeSettings),
        ]
        .spacing(scale::s(8.0))
        .align_y(iced::Alignment::Center)
        .width(Length::Fill),
    ]
    .spacing(scale::s(8.0))
    .width(Length::Fill);

    let sidebar = container(
        column![nav_scroll, bottom]
            .spacing(scale::s(12.0))
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::Fixed(scale::s(SIDEBAR_WIDTH)))
    .height(Length::Fill)
    .padding(scale::s(12.0))
    .style(|_| container::Style {
        background: Some(PANEL_BG.into()),
        border: iced::Border::default().rounded(scale::s(12.0)),
        ..Default::default()
    });

    // --- Main panel: filtered by the sidebar selection ---
    let (title, rows): (String, Element<'_, UiEvent>) = match &selection {
        HomeSelection::Recent => {
            let mut col = column![].spacing(scale::s(2.0));
            let mut n = 0;
            for rp in state.recent_projects().iter().take(10) {
                n += 1;
                let rel = easyscanlate_settings::format_relative(rp.last_opened);
                let current_series = series_items
                    .iter()
                    .find(|t| t.path == rp.path)
                    .and_then(|t| t.series.clone());
                let path = rp.path.clone();
                let selected = selected_project == Some(path.as_str());
                col = col.push(project_row(
                    rp.name.clone(),
                    file_size_display(&rp.path),
                    path.clone(),
                    true,
                    current_series,
                    &names,
                    selected,
                    rel,
                ));
                if state.home_series_creating() == Some(path.as_str()) {
                    col = col.push(home_series_editor(state.home_series_name()));
                }
            }
            let body: Element<'_, UiEvent> = if n == 0 {
                empty_hint("No recent projects. Create or open one to begin.")
            } else {
                scrollable(col)
                    .spacing(scale::s(crate::scroll::EMBEDDED_SPACING))
                    .height(Length::Fill)
                    .into()
            };
            ("Recent Projects".to_string(), body)
        }
        HomeSelection::Series(name) => {
            let items: Vec<&easyscanlate_settings::series::TrackedProject> = series_items
                .iter()
                .filter(|t| t.series.as_deref() == Some(name.as_str()))
                .take(20)
                .collect();
            let body: Element<'_, UiEvent> = if items.is_empty() {
                empty_hint("No projects in this series yet.")
            } else {
                let mut col = column![].spacing(scale::s(2.0));
                for t in items {
                    let rel = easyscanlate_settings::format_relative(t.last_opened);
                    let selected = selected_project == Some(t.path.as_str());
                    col = col.push(project_row(
                        t.name.clone(),
                        file_size_display(&t.path),
                        t.path.clone(),
                        false,
                        None,
                        &names,
                        selected,
                        rel,
                    ));
                }
                scrollable(col)
                    .spacing(scale::s(crate::scroll::EMBEDDED_SPACING))
                    .height(Length::Fill)
                    .into()
            };
            (format!("Series — {name}"), body)
        }
    };

    // Table header: mirrors the row columns; Series is hidden in the
    // series-filtered view (implied by the selection).
    let show_series = matches!(&selection, HomeSelection::Recent);
    let mut header_cells = row![
        text("Name")
            .size(scale::s(12.0))
            .color(Color::WHITE)
            .width(FillLength),
        text("Size")
            .size(scale::s(12.0))
            .color(Color::WHITE)
            .width(Length::Fixed(scale::s(SIZE_W))),
        text("Path")
            .size(scale::s(12.0))
            .color(Color::WHITE)
            .width(Length::FillPortion(2)),
    ]
    .spacing(scale::s(8.0));
    if show_series {
        header_cells = header_cells.push(
            text("Series")
                .size(scale::s(12.0))
                .color(Color::WHITE)
                .width(Length::Fixed(scale::s(SERIES_W))),
        );
    }
    header_cells = header_cells.push(
        text("Last Opened")
            .size(scale::s(12.0))
            .color(Color::WHITE)
            .width(Length::Fixed(scale::s(REL_W))),
    );
    let header = container(header_cells)
        .padding(scale::s(12.0))
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Color::from_rgba8(60, 60, 65, 0.9).into()),
            border: iced::Border::default().rounded(scale::s(8.0)),
            ..Default::default()
        });

    let main = container(
        column![
            text(title).size(scale::s(22.0)).color(Color::WHITE),
            column![header, rows].spacing(scale::s(6.0)),
        ]
        .spacing(scale::s(16.0)),
    )
    .padding(scale::s(16.0))
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|_| container::Style {
        background: Some(PANEL_BG.into()),
        border: iced::Border::default().rounded(scale::s(12.0)),
        ..Default::default()
    });

    let content = row![sidebar, main]
        .spacing(scale::s(12.0))
        .height(Length::Fill);

    container(content)
        .padding(scale::s(16.0))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
