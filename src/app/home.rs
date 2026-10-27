//! Home table interactions: single-click selection, double-click opening and
//! inline series reassignment.

use std::path::Path;
use std::time::Instant;

use iced::Task;

use easyscanlate_ui::home::DOUBLE_CLICK_DELAY;

use super::{App, Message};

fn normalize_series(series: Option<String>) -> Option<String> {
    series.and_then(|s| {
        let t = s.trim().to_string();
        if t.is_empty() { None } else { Some(t) }
    })
}

/// Clears the Home selection, double-click timer and inline series editor.
pub fn clear_interaction(app: &mut App) {
    app.home_selected_project = None;
    app.home_last_click = None;
    app.home_series_creating = None;
    app.home_series_name.clear();
}

/// A Home project row was clicked. The first press selects it; a second press
/// on the same row within [`DOUBLE_CLICK_DELAY`] opens the project.
pub fn handle_project_click(app: &mut App, path: String) -> Task<Message> {
    let now = Instant::now();
    let is_double = app.home_last_click.as_ref().is_some_and(|(at, prev)| {
        prev == &path && now.duration_since(*at) <= DOUBLE_CLICK_DELAY
    });

    app.home_series_creating = None;
    app.home_series_name.clear();
    app.home_selected_project = Some(path.clone());

    if is_double {
        app.home_last_click = None;
        super::backdrop::begin_load(app, super::backdrop::PendingLoad::Recent(path))
    } else {
        app.home_last_click = Some((now, path));
        Task::none()
    }
}

pub fn handle_series_create_start(app: &mut App, path: String) -> Task<Message> {
    app.home_last_click = None;
    app.home_series_creating = Some(path);
    app.home_series_name.clear();
    Task::none()
}

pub fn handle_series_name(app: &mut App, name: String) -> Task<Message> {
    app.home_series_name = name;
    Task::none()
}

pub fn handle_series_cancel(app: &mut App) -> Task<Message> {
    app.home_series_creating = None;
    app.home_series_name.clear();
    Task::none()
}

pub fn handle_series_create_confirm(app: &mut App) -> Task<Message> {
    let Some(path) = app.home_series_creating.clone() else {
        return Task::none();
    };
    let name = app.home_series_name.trim().to_string();
    if name.is_empty() {
        return Task::none();
    }
    app.home_series_creating = None;
    app.home_series_name.clear();
    apply_series_change(app, path, Some(name))
}

/// Reassigns `path` to `series` (`None` = standalone). The series index is a
/// derived cache: update it immediately for the Home UI, and persist the tag
/// into the `.mmtl` when the project is not currently open.
pub fn handle_series_changed(
    app: &mut App,
    path: String,
    series: Option<String>,
) -> Task<Message> {
    app.home_last_click = None;
    app.home_series_creating = None;
    app.home_series_name.clear();
    apply_series_change(app, path, series)
}

fn apply_series_change(app: &mut App, path: String, series: Option<String>) -> Task<Message> {
    let series = normalize_series(series);
    easyscanlate_settings::series::reassign(&path, series.clone());
    app.series_items = easyscanlate_settings::series::load_series().items;

    if let Some(idx) = app
        .tabs
        .iter()
        .position(|t| t.mmtl_path.as_deref() == Some(Path::new(&path)))
    {
        let label = match series.as_deref() {
            Some(name) => format!("Series set to “{name}” — save the project to keep it."),
            None => "Series removed — save the project to keep the change.".to_string(),
        };
        let tab = &mut app.tabs[idx];
        tab.project.set_series(series);
        tab.dirty = true;
        tab.status = label;
        return Task::none();
    }

    let task_path = path.clone();
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                easyscanlate_mmtl::update_project_series(Path::new(&task_path), series)
            })
            .await
            .unwrap_or_else(|e| Err(format!("update task failed: {e}")))
        },
        move |result| Message::HomeSeriesPersisted { path, result },
    )
}

pub fn handle_series_persisted(
    _app: &mut App,
    path: String,
    result: Result<(), String>,
) -> Task<Message> {
    match result {
        Ok(()) => Task::none(),
        Err(e) => super::error::show(
            "Couldn't save the series tag",
            format!("{path}\n{e}"),
        ),
    }
}
