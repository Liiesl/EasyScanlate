use iced::Task;

use super::{App, Message};

pub fn handle_select(app: &mut App, id: easyscanlate_model::ProfileId) -> Task<Message> {
    if app.active_tab_mut().images.is_empty() {
        return Task::none();
    }
    if let Some(ev) = app.active_tab_mut().project.select_profile_with_event(id) {
        crate::app::state::handle_model_event(app.active_tab_mut(), ev);
    } else {
        return Task::none();
    }
    let name = app.active_tab_mut().project.profiles.selected().name.clone();
    app.active_tab_mut().status = format!("Profile: {name}");
    Task::none()
}

pub fn handle_create(app: &mut App) -> Task<Message> {
    if app.active_tab_mut().images.is_empty() {
        return Task::none();
    }
    let name = app.active_tab_mut().project.profiles.next_available_name();
    let (id, ev) = app.active_tab_mut().project.create_profile_with_event(name);
    crate::app::state::handle_model_event(app.active_tab_mut(), ev);
    if let Some(sel_ev) = app.active_tab_mut().project.select_profile_with_event(id) {
        crate::app::state::handle_model_event(app.active_tab_mut(), sel_ev);
    }
    let name = app.active_tab_mut().project.profiles.selected().name.clone();
    app.active_tab_mut().status = format!("Profile: {name} (created)");
    Task::none()
}
