use iced::widget::text_editor;
use iced::{Rectangle, Task};
use easyscanlate_model::EntryId;
use easyscanlate_ui::event::{EditOrigin, ToolbarAction};
use easyscanlate_ui::panel::results::scroll_to_row;

use super::layout::{EDIT_INPUT_ID, PANEL_EDIT_INPUT_ID};
use super::{App, Message};

/// Starts an inline edit of `(index, id)`: selects the entry, seeds the
/// editor buffer with its displayed text and selects it all so the first
/// keystroke replaces it, then focuses the editor of `origin` (the floating
/// overlay editor or the panel's results-row editor). Shared by the
/// double-click action, the toolbar's "Rename" button and the panel rows.
pub fn start_inline_edit(app: &mut App, index: usize, id: EntryId, origin: EditOrigin) -> Task<Message> {
    let tab = app.active_tab_mut();
    if index >= tab.images.len() {
        return Task::none();
    }
    let Some(entry) = tab.project.entry(id) else {
        return Task::none();
    };
    let image_id = tab.images[index].image_id;
    if entry.image_id != image_id || tab.project.image(image_id).is_none() {
        return Task::none();
    }
    let text = tab.project.display_text(entry).to_string();
    clear_editing(app);
    let mut tasks = vec![select_entry(app, index, id)];
    {
        let tab = app.active_tab_mut();
        tab.editing = Some((index, id));
        tab.editing_origin = origin;
        tab.editing_dirty = false;
        tab.editing_rect = None;
        let mut content = text_editor::Content::with_text(&text);
        content.perform(text_editor::Action::SelectAll);
        tab.edit_content = Some(content);
        tab.status = format!("Editing \"{text}\" in the overlay.");
    }
    let focus_id = match origin {
        EditOrigin::Overlay => EDIT_INPUT_ID,
        EditOrigin::Panel => PANEL_EDIT_INPUT_ID,
    };
    tasks.push(iced::widget::operation::focus(focus_id));
    if origin == EditOrigin::Overlay {
        tasks.push(scroll_to_row::<Message>(index, id));
    }
    Task::batch(tasks)
}

/// Clears every piece of inline-editing state in one place (App wrapper).
pub fn clear_editing(app: &mut App) {
    clear_editing_tab(app.active_tab_mut())
}

/// Clears editing on a Tab directly (used by handle_model_event).
pub fn clear_editing_tab(tab: &mut super::tab::Tab) {
    tab.editing = None;
    tab.editing_origin = EditOrigin::Overlay;
    tab.edit_content = None;
    tab.editing_dirty = false;
    tab.editing_rect = None;
}

/// Reseeds the style panel inputs from `style`, closing any open picker and
/// keeping the raw number strings in sync with the resolved values. Also
/// clears any hex text buffers so the hex inputs show the canonical value.
pub fn seed_style_inputs(app: &mut App, style: easyscanlate_model::EntryStyle) {
    let tab = app.active_tab_mut();
    tab.style_stroke_width = style.stroke_width.to_string();
    tab.style_bg_radius = style.bg_radius.to_string();
    tab.style_working = style;
    tab.style_picker = None;
    tab.style_hex_overrides.clear();
}

/// Selects `(index, id)`: seeds the style inputs and, when the entry's page
/// is outside the currently settled decode window (a panel-driven reveal
/// moved the viewport without a `TilesVisible` event), schedules a full-res
/// settle for that page.
pub fn select_entry(app: &mut App, index: usize, id: EntryId) -> Task<Message> {
    {
        let tab = app.active_tab_mut();
        tab.selected_inpaint = None;
        tab.selected = Some((index, id));
        let style = tab.project.entry_style(id);
        // inline seed to avoid double borrow
        tab.style_stroke_width = style.stroke_width.to_string();
        tab.style_bg_radius = style.bg_radius.to_string();
        tab.style_working = style;
        tab.style_picker = None;
        tab.style_hex_overrides.clear();
        if tab.scheduler.needs_settle(index, tab.images.len()) {
            let tid = tab.id;
            return tab.scheduler
                .schedule(index..index + 1, move |seq| Message::Tab(tid, crate::app::TabMessage::SettleElapsed(seq)));
        }
    }
    Task::none()
}

// ---------------------------------------------------------------------------
// UiEvent handlers that belong to editing
// ---------------------------------------------------------------------------

pub fn handle_entry_clicked(app: &mut App, selection: Option<(usize, EntryId)>) -> Task<Message> {
    clear_editing(app);
    {
        let tab = app.active_tab_mut();
        if tab.selected_inpaint.is_some() {
            tab.selected_inpaint = None;
        }
    }
    match selection {
        Some((index, id)) => {
            let ok = {
                let tab = app.active_tab();
                index < tab.images.len() && tab.project.entry(id).is_some()
            };
            if ok {
                Task::batch([select_entry(app, index, id), scroll_to_row::<Message>(index, id)])
            } else {
                app.active_tab_mut().selected = None;
                Task::none()
            }
        }
        _ => {
            app.active_tab_mut().selected = None;
            Task::none()
        }
    }
}

pub fn handle_entry_double_clicked(app: &mut App, pair: (usize, EntryId)) -> Task<Message> {
    app.active_tab_mut().selected_inpaint = None;
    start_inline_edit(app, pair.0, pair.1, EditOrigin::Overlay)
}

pub fn handle_panel_entry_edit(app: &mut App, pair: (usize, EntryId)) -> Task<Message> {
    if app.active_tab().translation_panel_mode == easyscanlate_ui::event::TranslationPanelMode::Translate {
        return handle_entry_clicked(app, Some(pair));
    }
    app.active_tab_mut().selected_inpaint = None;
    start_inline_edit(app, pair.0, pair.1, EditOrigin::Panel)
}

pub fn handle_entry_toolbar(app: &mut App, index: usize, id: EntryId, action: ToolbarAction) -> Task<Message> {
    match action {
        ToolbarAction::Rename => start_inline_edit(app, index, id, EditOrigin::Overlay),
        ToolbarAction::Delete => {
            let ok = {
                let tab = app.active_tab();
                index < tab.images.len()
            };
            if !ok {
                return Task::none();
            }
            let ev = {
                let tab = app.active_tab_mut();
                tab.project.delete_entry_with_event(id)
            };
            let Some(ev) = ev else {
                return Task::none();
            };
            {
                let tab = app.active_tab_mut();
                crate::app::handle_model_event(tab, ev);
                tab.status = "Deleted entry.".to_string();
            }
            Task::none()
        }
        ToolbarAction::RevertTransform => {
            let ok = {
                let tab = app.active_tab();
                index < tab.images.len() && tab.project.has_view_quad(id)
            };
            if !ok {
                return Task::none();
            }
            let ev = {
                let tab = app.active_tab_mut();
                tab.project.revert_transform_with_event(id)
            };
            if let Some(ev) = ev {
                let tab = app.active_tab_mut();
                crate::app::handle_model_event(tab, ev);
            }
            app.active_tab_mut().status = "Reverted transform.".to_string();
            Task::none()
        }
    }
}

pub fn handle_entry_moved(app: &mut App, index: usize, id: EntryId, quad: easyscanlate_model::Quad) -> Task<Message> {
    let ok = {
        let tab = app.active_tab();
        index < tab.images.len()
    };
    if ok {
        let ev = {
            let tab = app.active_tab_mut();
            tab.project.set_view_quad_with_event(id, quad)
        };
        crate::app::handle_model_event(app.active_tab_mut(), ev);
    }
    Task::none()
}

pub fn handle_edit_action(app: &mut App, action: text_editor::Action) -> Task<Message> {
    // Need to handle mutable borrow of edit_content
    let has_edit = app.active_tab().edit_content.is_some();
    if !has_edit {
        return Task::none();
    }
    // Perform action on content
    let (text, editing, editing_dirty) = {
        let tab = app.active_tab_mut();
        let content = tab.edit_content.as_mut().unwrap();
        content.perform(action);
        let text = content.text();
        (text, tab.editing, tab.editing_dirty)
    };
    let Some((_index, id)) = editing else {
        return Task::none();
    };
    if !editing_dirty {
        {
            let tab = app.active_tab_mut();
            tab.editing_dirty = true;
            if let Some((name, evs)) = tab.project.fork_for_edit_with_event() {
                for ev in evs {
                    crate::app::handle_model_event(tab, ev);
                }
                tab.status = format!(
                    "Edit forked into '{name}': the OCR text stays untouched."
                );
            }
        }
    }
    let ev = {
        let tab = app.active_tab_mut();
        tab.project.set_translation_with_event(id, Some(text.clone()))
    };
    crate::app::handle_model_event(app.active_tab_mut(), ev);
    Task::none()
}

pub fn handle_edit_rect(app: &mut App, rect: Rectangle) -> Task<Message> {
    if app.active_tab().editing.is_some() {
        app.active_tab_mut().editing_rect = Some(rect);
    }
    Task::none()
}

pub fn handle_edit_submit(app: &mut App) -> Task<Message> {
    clear_editing(app);
    Task::none()
}
