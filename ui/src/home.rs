use std::collections::BTreeSet;

use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Color, Element, Length, Fill as FillLength};
use lucide_icons::Icon;
use neverliie_iced_widgets::ellipsis_text::EllipsisText;

use crate::event::UiEvent;
use crate::panel::PANEL_BG;
use crate::scale;
use crate::state::HomeSelection;

const SIDEBAR_WIDTH: f32 = 200.0;

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

fn project_row<'a>(
    name: String,
    size: String,
    path: String,
    series: Option<String>,
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
    // `None` hides the Series column (series-filtered view: implied).
    if let Some(s) = series {
        cells = cells.push(ellipsis_cell(
            s,
            12.0,
            muted,
            Length::Fixed(scale::s(SERIES_W)),
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
    .style(|_, status| {
        let bg = if status == iced::widget::button::Status::Hovered {
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
    .on_press(UiEvent::HomeRecentClicked(open_path))
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
    let mut nav = column![
        nav_button(
            recent_label,
            selection == HomeSelection::Recent,
            UiEvent::HomeSelectRecent,
        ),
    ]
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
            crate::icon::lucide(Icon::Plus).size(scale::s(14.0)).center(),
            text("New Project").size(scale::s(13.0)).width(FillLength).center(),
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
            crate::icon::lucide(Icon::FolderOpen).size(scale::s(14.0)).center(),
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
                let series = series_items
                    .iter()
                    .find(|t| t.path == rp.path)
                    .and_then(|t| t.series.clone())
                    .unwrap_or_else(|| "—".to_string());
                col = col.push(project_row(
                    rp.name.clone(),
                    file_size_display(&rp.path),
                    rp.path.clone(),
                    Some(series),
                    rel,
                ));
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
                    col = col.push(project_row(
                        t.name.clone(),
                        file_size_display(&t.path),
                        t.path.clone(),
                        None,
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
        text("Name").size(scale::s(12.0)).color(Color::WHITE).width(FillLength),
        text("Size").size(scale::s(12.0)).color(Color::WHITE).width(Length::Fixed(scale::s(SIZE_W))),
        text("Path").size(scale::s(12.0)).color(Color::WHITE).width(Length::FillPortion(2)),
    ]
    .spacing(scale::s(8.0));
    if show_series {
        header_cells = header_cells.push(
            text("Series").size(scale::s(12.0)).color(Color::WHITE).width(Length::Fixed(scale::s(SERIES_W))),
        );
    }
    header_cells = header_cells.push(
        text("Last Opened").size(scale::s(12.0)).color(Color::WHITE).width(Length::Fixed(scale::s(REL_W))),
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

    let content = row![sidebar, main].spacing(scale::s(12.0)).height(Length::Fill);

    container(content)
        .padding(scale::s(16.0))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
