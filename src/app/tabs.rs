//! Titlebar tab strip — 32px custom frame (`NativeFrame` fill row).
//!
//! Renders `easyscanlate` (pinned, no ×, with icon) + project tabs (`title • ×`) + `+`
//! immediately after last tab. Scroll area grows with tabs until 80% of titlebar
//! then becomes fixed and scrollable. Inactive bg = transparent, active bg =
//! `PANEL_BG` (`34,36,44,0.78`) with top-only radius 6, no border/underline.
//! Trailing `Fill` gap falls through to `draggable` in
//! `NeverLiieIcedWidgets/src/title_bar/mod.rs:628,653`.
//!
//! Overflow via `scrollable` horizontal with `Scrollbar::new()`.

use iced::border::Radius;
use iced::widget::{Responsive, button, container, row, scrollable, space, text};
use iced::{Background, Border, Color, Element, Length, Shadow, Theme};

use iced::Task;

use super::tab::TabId;
use super::{App, Message, TabMessage};
use easyscanlate_ui::event::UiEvent;

const ACCENT: Color = Color::from_rgb8(92, 190, 255);
const PANEL_BG: Color = Color::from_rgba8(34, 36, 44, 0.78);
const PANEL_HOVER: Color = Color::from_rgba8(46, 48, 62, 0.90);
const PANEL_PRESSED: Color = Color::from_rgba8(55, 57, 72, 0.95);

fn chip_button_style(active: bool, is_dark: bool) -> impl Fn(&Theme, button::Status) -> button::Style + Clone {
    move |_theme: &Theme, status: button::Status| {
        // Active bg = PANEL_BG, inactive = TRANSPARENT (same for dark/light per spec Q3).
        // Text stays theme-aware for readability on transparent.
        let (base_bg, text_color) = if active {
            (PANEL_BG, Color::WHITE)
        } else if is_dark {
            (Color::TRANSPARENT, Color::from_rgb8(220, 220, 225))
        } else {
            (Color::TRANSPARENT, Color::from_rgb8(40, 40, 45))
        };

        let bg = match status {
            button::Status::Hovered => {
                if active {
                    PANEL_HOVER
                } else if is_dark {
                    Color::from_rgba8(255, 255, 255, 0.08)
                } else {
                    Color::from_rgba8(0, 0, 0, 0.06)
                }
            }
            button::Status::Pressed => {
                if active {
                    PANEL_PRESSED
                } else if is_dark {
                    Color::from_rgba8(255, 255, 255, 0.12)
                } else {
                    Color::from_rgba8(0, 0, 0, 0.10)
                }
            }
            button::Status::Disabled => Color::from_rgba8(60, 60, 65, 0.35),
            button::Status::Active => base_bg,
        };

        let border = Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: Radius::default().top(6.0),
        };

        button::Style {
            background: Some(Background::Color(bg)),
            border,
            shadow: Shadow::default(),
            text_color,
            ..button::Style::default()
        }
    }
}

fn ghost_close_style(_theme: &Theme, status: button::Status) -> button::Style {
    let (bg, txt) = match status {
        button::Status::Hovered => (Color::from_rgba8(255, 70, 70, 0.18), Color::from_rgb8(255, 120, 120)),
        button::Status::Pressed => (Color::from_rgba8(180, 40, 40, 0.25), Color::WHITE),
        button::Status::Active => (Color::TRANSPARENT, Color::from_rgb8(180, 180, 185)),
        button::Status::Disabled => (Color::TRANSPARENT, Color::from_rgba8(100, 100, 100, 0.5)),
    };
    button::Style {
        background: Some(Background::Color(bg)),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: Radius::default().top(4.0),
        },
        shadow: Shadow::default(),
        text_color: txt,
        ..button::Style::default()
    }
}

fn ghost_add_style(is_dark: bool) -> impl Fn(&Theme, button::Status) -> button::Style + Clone {
    move |_theme: &Theme, status: button::Status| {
        let bg = match status {
            button::Status::Hovered => {
                if is_dark {
                    Color::from_rgba8(255, 255, 255, 0.08)
                } else {
                    Color::from_rgba8(0, 0, 0, 0.06)
                }
            }
            button::Status::Pressed => {
                if is_dark {
                    Color::from_rgba8(255, 255, 255, 0.12)
                } else {
                    Color::from_rgba8(0, 0, 0, 0.10)
                }
            }
            _ => Color::TRANSPARENT,
        };
        let txt = if is_dark { Color::from_rgb8(200, 200, 210) } else { Color::from_rgb8(60, 60, 70) };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: if matches!(status, button::Status::Hovered) {
                    if is_dark { Color::from_rgba8(255, 255, 255, 0.10) } else { Color::from_rgba8(0,0,0,0.08) }
                } else { Color::TRANSPARENT },
                width: if matches!(status, button::Status::Hovered) { 1.0 } else { 0.0 },
                radius: Radius::default().top(6.0),
            },
            shadow: Shadow::default(),
            text_color: txt,
            ..button::Style::default()
        }
    }
}

/// 32px titlebar tab strip — `easyscanlate | proj • × | … + | Fill(drag gap)`.
///
/// `+` is inside the scrollable row immediately after last chip, so it sits
/// right on the side of rightmost tab. The scroll viewport grows with tab
/// count until 80% of titlebar width, then becomes fixed and scrollable.
/// Called from `crate::app::view::view` as `Some(titlebar_view(app))` fed to
/// `NativeFrame::view(..., title_content)`. Trailing `Fill` gap falls through
/// to `draggable` in `NeverLiieIcedWidgets/src/title_bar/mod.rs:653`.
pub(crate) fn close_tab_immediate(app: &mut App, id: TabId) -> Task<Message> {
    if let Some(idx) = app.tabs.iter().position(|t| t.id == id) {
        if app.tabs[idx].is_home() {
            return Task::none();
        }
        app.engines.queue.cancel_pending_for_tab(id);
        let freed = !app.engines.queue.cancel_running_for_tab(id).is_empty();
        let promote = if freed {
            crate::app::queue::dispatch_pending(app)
        } else {
            Task::none()
        };
        if freed {
            crate::app::queue::refresh_queued_statuses(app);
        }
        app.tabs.remove(idx);
        if app.active >= app.tabs.len() {
            app.active = app.tabs.len().saturating_sub(1);
        } else if idx < app.active {
            app.active -= 1;
        }
        if app.active >= app.tabs.len() && !app.tabs.is_empty() {
            app.active = app.tabs.len() - 1;
        }
        if app.pending_close == Some(id) {
            app.pending_close = None;
        }
        return promote;
    }
    Task::none()
}

fn cleanup_queue_for_tabs(app: &mut App, ids: &[TabId]) {
    for rid in ids {
        app.engines.queue.cancel_pending_for_tab(*rid);
        app.engines.queue.cancel_running_for_tab(*rid);
    }
}

pub fn handle_close(app: &mut App, raw: u64) -> Task<Message> {
    let id = TabId(raw);
    if let Some(idx) = app.tabs.iter().position(|t| t.id == id) {
        if app.tabs[idx].is_home() {
            return Task::none();
        }
        if app.tabs[idx].dirty {
            app.pending_close = Some(id);
        } else {
            return close_tab_immediate(app, id);
        }
    }
    Task::none()
}

pub fn handle_close_confirmed(app: &mut App, raw: u64, save: bool) -> Task<Message> {
    let id = TabId(raw);
    let Some(idx) = app.tabs.iter().position(|t| t.id == id) else {
        app.pending_close = None;
        return Task::none();
    };
    if app.tabs[idx].is_home() {
        app.pending_close = None;
        return Task::none();
    }
    if save {
        app.pending_close = Some(id);
        let path_opt = app.tabs[idx].mmtl_path.clone();
        if let Some(path) = path_opt {
            let project = app.tabs[idx].project.clone();
            let tid = id;
            let inpaint = {
                let tab = &app.tabs[idx];
                let mut out = Vec::new();
                for loaded in &tab.images {
                    let image_id = loaded.image_id;
                    for layer in &loaded.inpaint {
                        let (width, height, pixels) = match &layer.handle {
                            iced::widget::image::Handle::Rgba { width, height, pixels, .. } => (*width, *height, pixels.to_vec()),
                            iced::widget::image::Handle::Bytes(_id, bytes) => {
                                if let Ok(img) = image::load_from_memory(bytes) {
                                    let rgba = img.to_rgba8();
                                    let (w, h) = (rgba.width(), rgba.height());
                                    (w, h, rgba.into_raw())
                                } else { continue; }
                            }
                            _ => continue,
                        };
                        out.push(easyscanlate_mmtl::InpaintImageData { image_id, bounds: layer.bounds, width, height, rgba: pixels });
                    }
                }
                out
            };
            return Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        easyscanlate_mmtl::save_mmtl(&project, &inpaint, &path).map(|_| path.to_string_lossy().to_string()).map_err(|e| e.to_string())
                    }).await.unwrap_or_else(|e| Err(format!("save task failed: {e}")))
                },
                move |res| Message::Tab(tid, TabMessage::MmtlSaved(res)),
            );
        } else {
            let tid = id;
            return Task::perform(
                async move {
                    let file = rfd::AsyncFileDialog::new()
                        .add_filter("Manga Translation (.mmtl)", &["mmtl"])
                        .set_file_name("project.mmtl")
                        .save_file()
                        .await;
                    file.map(|f| f.path().to_string_lossy().to_string())
                },
                move |picked| Message::Tab(tid, TabMessage::MmtlSavePicked(picked)),
            );
        }
    } else {
        return close_tab_immediate(app, id);
    }
}

pub fn handle_close_cancel(app: &mut App) -> Task<Message> {
    app.pending_close = None;
    Task::none()
}

pub fn handle_close_others(app: &mut App, raw: u64) -> Task<Message> {
    let keep = TabId(raw);
    if let Some(dirty) = app.tabs.iter().find(|t| t.is_project() && t.id != keep && t.dirty).map(|t| t.id) {
        app.pending_close = Some(dirty);
        return Task::none();
    }
    let remove_ids: Vec<TabId> = app.tabs.iter().filter(|t| t.id != keep && t.is_project()).map(|t| t.id).collect();
    cleanup_queue_for_tabs(app, &remove_ids);
    let keep_idx = app.tabs.iter().position(|t| t.id == keep);
    if let Some(kidx) = keep_idx {
        let mut i = app.tabs.len();
        while i > 0 {
            i -= 1;
            if i == 0 { continue; }
            if app.tabs[i].id == keep { continue; }
            app.tabs.remove(i);
            if app.active > i { app.active -= 1; }
            else if app.active == i { app.active = kidx.min(app.tabs.len().saturating_sub(1)); }
        }
        if let Some(new_k) = app.tabs.iter().position(|t| t.id == keep) {
            app.active = new_k;
        }
    }
    let promote = crate::app::queue::dispatch_pending(app);
    crate::app::queue::refresh_queued_statuses(app);
    promote
}

pub fn handle_close_all(app: &mut App) -> Task<Message> {
    if let Some(dirty) = app.tabs.iter().find(|t| t.is_project() && t.dirty).map(|t| t.id) {
        app.pending_close = Some(dirty);
        return Task::none();
    }
    let remove_ids: Vec<TabId> = app.tabs.iter().filter(|t| t.is_project()).map(|t| t.id).collect();
    cleanup_queue_for_tabs(app, &remove_ids);
    app.tabs.retain(|t| t.is_home());
    app.active = 0;
    app.pending_close = None;
    let promote = crate::app::queue::dispatch_pending(app);
    crate::app::queue::refresh_queued_statuses(app);
    promote
}

pub fn handle_selected(app: &mut App, raw: u64) -> Task<Message> {
    if let Some(idx) = app.tabs.iter().position(|t| t.id.0 == raw) {
        app.active = idx;
    }
    Task::none()
}

pub fn titlebar_view(app: &App) -> Element<'_, Message> {
    let h = app.frame.config().title_bar_height;

    // Use `Responsive` to get available titlebar width and cap scroll viewport
    // at 80% (Q1). `Responsive` fills the title_content gap between leading
    // and caption buttons (`NeverLiieIcedWidgets/src/title_bar/mod.rs:616-628`).
    Responsive::new(move |size| {
        let is_dark = easyscanlate_settings::get(|s| s.aurora_is_dark);
        let max_w = (size.width * 0.80).max(160.0);

        // Content width if all chips + `+` were laid out without clipping.
        // chip 160×n + add 28 + spacing 4 between each element (n chips + plus => n gaps)
        let n = app.tabs.len() as f32;
        let content_w = n * 160.0 + 28.0 + n * 4.0;
        let viewport_w = content_w.min(max_w);

        let mut chip_plus: Vec<Element<'_, Message>> = Vec::with_capacity(app.tabs.len() + 1);

        for (idx, tab) in app.tabs.iter().enumerate() {
            let is_active = idx == app.active;
            let is_home = tab.is_home() && idx == 0;

            // Title: Home shows `easyscanlate` with icon, others show tab.title.
            let title: Element<'_, Message> = if is_home {
                let icon_elem: Element<'_, Message> =
                    crate::app::chrome::title_icon_handle()
                        .map(|h| iced::widget::image(h).width(14).height(14).into())
                        .unwrap_or_else(|| space::horizontal().width(Length::Fixed(0.0)).into());
                let label: Element<'_, Message> =
                    text("easyscanlate").size(12).width(Length::Fill).into();
                row![icon_elem, label]
                    .spacing(6)
                    .align_y(iced::Alignment::Center)
                    .width(Length::Fill)
                    .into()
            } else {
                text(tab.title.clone())
                    .size(12)
                    .width(Length::Fill)
                    .into()
            };

            let dirty: Element<'_, Message> = if tab.dirty {
                container(text("•").size(11).color(ACCENT))
                    .width(Length::Fixed(8.0))
                    .center_x(Length::Fixed(8.0))
                    .center_y(Length::Fixed(14.0))
                    .into()
            } else {
                space::horizontal().width(Length::Fixed(0.0)).into()
            };

            let close: Element<'_, Message> = if is_home {
                space::horizontal()
                    .width(Length::Fixed(14.0))
                    .height(Length::Fixed(14.0))
                    .into()
            } else {
                let id = tab.id.0;
                button(
                    text("×")
                        .size(11)
                        .width(Length::Fixed(14.0))
                        .height(Length::Fixed(14.0))
                        .center(),
                )
                .width(Length::Fixed(14.0))
                .height(Length::Fixed(14.0))
                .padding(0)
                .style(ghost_close_style)
                .on_press(Message::Ui(UiEvent::TabClose(id)))
                .into()
            };

            let inner_row: Element<'_, Message> = row![title, dirty, close]
                .spacing(4)
                .align_y(iced::Alignment::Center)
                .width(Length::Fill)
                .height(Length::Fixed(24.0))
                .into();

            let chip_btn: Element<'_, Message> = button(inner_row)
                .width(Length::Fixed(160.0))
                .height(Length::Fixed(24.0))
                .padding([2, 6])
                .style(chip_button_style(is_active, is_dark))
                .on_press(Message::Ui(UiEvent::TabSelected(tab.id.0)))
                .into();

            chip_plus.push(chip_btn);
        }

        let add_btn: Element<'_, Message> =
            button(text("+").size(14).width(Length::Fixed(24.0)).height(Length::Fixed(24.0)).center())
                .width(Length::Fixed(28.0))
                .height(Length::Fixed(24.0))
                .padding([2, 6])
                .style(ghost_add_style(is_dark))
                .on_press(Message::Ui(UiEvent::TabNew))
                .into();
        chip_plus.push(add_btn);

        let chips_row: Element<'_, Message> = row(chip_plus)
            .spacing(4)
            .align_y(iced::Alignment::Center)
            .width(Length::Shrink)
            .height(Length::Fixed(h))
            .into();

        // Viewport grows with content until 80% then scrolls.
        let chips_scroll: Element<'_, Message> = scrollable::Scrollable::with_direction(
            chips_row,
            scrollable::Direction::Horizontal(scrollable::Scrollbar::new()),
        )
        .width(Length::Fixed(viewport_w))
        .height(Length::Fixed(h))
        .into();

        let scroll_container: Element<'_, Message> = container(chips_scroll)
            .width(Length::Fixed(viewport_w))
            .height(Length::Fixed(h))
            .center_y(Length::Fixed(h))
            .into();

        let drag_gap: Element<'_, Message> = space::horizontal()
            .width(Length::Fill)
            .height(Length::Fixed(h))
            .into();

        row![scroll_container, drag_gap]
            .spacing(4)
            .align_y(iced::Alignment::Center)
            .width(Length::Fill)
            .height(Length::Fixed(h))
            .padding([0, 8])
            .into()
    })
    .width(Length::Fill)
    .height(Length::Fixed(h))
    .into()
}
