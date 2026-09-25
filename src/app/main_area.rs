use iced::Task;
use easyscanlate_ui::event::{MainAreaMode, ManualMode};

use super::{App, Message};

pub fn handle_toggle_overlay(app: &mut App) -> Task<Message> {
    app.active_tab_mut().show_overlay_text = !app.active_tab_mut().show_overlay_text;
    app.active_tab_mut().status = if app.active_tab_mut().show_overlay_text {
        "Overlay text shown."
    } else {
        "Overlay text hidden."
    }
    .to_string();
    Task::none()
}

pub fn handle_toggle_inpaint(app: &mut App) -> Task<Message> {
    app.active_tab_mut().show_inpaint = !app.active_tab_mut().show_inpaint;
    app.active_tab_mut().status = if app.active_tab_mut().show_inpaint {
        "Inpaint layer shown."
    } else {
        "Inpaint layer hidden."
    }
    .to_string();
    Task::none()
}

pub fn handle_mode(app: &mut App, mode: MainAreaMode) -> Task<Message> {
    if app.active_tab_mut().manual_mode != ManualMode::None {
        app.active_tab_mut().status = "Exit manual mode to switch View/Compare.".to_string();
        return Task::none();
    }
    app.active_tab_mut().view_mode = mode;
    app.active_tab_mut().status = match mode {
        MainAreaMode::View => "View mode: single column with overlay.".to_string(),
        MainAreaMode::Compare => {
            "Compare mode: original (left) vs current (right), scrolling in sync."
                .to_string()
        }
    };
    Task::none()
}

pub fn handle_viewer_scroll(app: &mut App, anchor: f32) -> Task<Message> {
    // Non-finite anchors come from degenerate frames (minimized /
    // zero-size viewport) and must not clobber the stored anchor.
    if anchor.is_finite() {
        app.active_tab_mut().viewer_scroll = anchor.clamp(0.0, 1.0);
    }
    Task::none()
}

pub fn handle_panel_resized(app: &mut App, resized: iced::widget::pane_grid::ResizeEvent) -> Task<Message> {
    let ratio = resized.ratio.clamp(0.20, 0.80);
    app.active_tab_mut().panes.resize(resized.split, ratio);
    Task::none()
}

pub fn handle_results_pane_resized(app: &mut App, resized: iced::widget::pane_grid::ResizeEvent) -> Task<Message> {
    app.active_tab_mut().results_panes.resize(resized.split, resized.ratio);
    Task::none()
}

pub fn handle_editor_resized(app: &mut App, resized: iced::widget::pane_grid::ResizeEvent) -> Task<Message> {
    let ratio = resized.ratio.clamp(0.15, 0.55);
    app.active_tab_mut().outer_panes.resize(resized.split, ratio);
    Task::none()
}
