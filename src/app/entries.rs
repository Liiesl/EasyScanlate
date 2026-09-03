use easyscanlate_ui::UiState;
use iced::Task;

use super::{App, Message};

pub fn handle_reorder(app: &mut App) -> Task<Message> {
    if app.active_state().is_bulk_busy() {
        app.active_tab_mut().status = "Wait for current task to finish.".to_string();
        return Task::none();
    }
    if app.active_tab_mut().images.is_empty() {
        app.active_tab_mut().status = "No images to reorder.".to_string();
        return Task::none();
    }
    let ids: Vec<_> = app.active_tab().project.images().iter().map(|m| m.id).collect();
    if ids.is_empty() {
        let ev = app.active_tab_mut().project.reorder_entries_for_image_with_event(easyscanlate_model::ImageId(0));
        crate::app::state::handle_model_event(app.active_tab_mut(), ev);
    } else {
        for image_id in ids {
            let ev = app.active_tab_mut().project.reorder_entries_for_image_with_event(image_id);
            crate::app::state::handle_model_event(app.active_tab_mut(), ev);
        }
    }
    app.active_tab_mut().status = format!(
        "Reordered {} image(s) by position (higher first, left to right).",
        app.active_tab_mut().images.len()
    );
    Task::none()
}
