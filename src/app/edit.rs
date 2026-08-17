use iced::widget::text_editor;
use iced::{Rectangle, Task};
use scanlateit_model::EntryId;
use scanlateit_ui::event::{EditOrigin, ToolbarAction};
use scanlateit_ui::panel::results::scroll_to_row;

use super::layout::{EDIT_INPUT_ID, PANEL_EDIT_INPUT_ID};
use super::{App, Message};

/// Starts an inline edit of `(index, id)`: selects the entry, seeds the
/// editor buffer with its displayed text and selects it all so the first
/// keystroke replaces it, then focuses the editor of `origin` (the floating
/// overlay editor or the panel's results-row editor). Shared by the
/// double-click action, the toolbar's "Rename" button and the panel rows.
pub fn start_inline_edit(app: &mut App, index: usize, id: EntryId, origin: EditOrigin) -> Task<Message> {
    if index >= app.images.len() {
        return Task::none();
    }
    // Image existence is source of truth via Project, not images[index] alone
    let Some(entry) = app.project.entry(id) else {
        return Task::none();
    };
    // Validate entry belongs to requested image (and image exists in Project)
    let image_id = app.images[index].image_id;
    if entry.image_id != image_id || app.project.image(image_id).is_none() {
        return Task::none();
    }
    let text = app.project.display_text(entry).to_string();
    clear_editing(app);
    let mut tasks = vec![select_entry(app, index, id)];
    app.editing = Some((index, id));
    app.editing_origin = origin;
    app.editing_dirty = false;
    app.editing_rect = None;
    let mut content = text_editor::Content::with_text(&text);
    content.perform(text_editor::Action::SelectAll);
    app.edit_content = Some(content);
    app.status = format!("Editing \"{text}\" in the overlay.");
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

/// Clears every piece of inline-editing state in one place.
pub fn clear_editing(app: &mut App) {
    app.editing = None;
    app.editing_origin = EditOrigin::Overlay;
    app.edit_content = None;
    app.editing_dirty = false;
    app.editing_rect = None;
}

/// Reseeds the style panel inputs from `style`, closing any open picker and
/// keeping the raw number strings in sync with the resolved values. Also
/// clears any hex text buffers so the hex inputs show the canonical value.
pub fn seed_style_inputs(app: &mut App, style: scanlateit_model::EntryStyle) {
    app.style_stroke_width = style.stroke_width.to_string();
    app.style_bg_radius = style.bg_radius.to_string();
    app.style_working = style;
    app.style_picker = None;
    app.style_hex_overrides.clear();
}

/// Selects `(index, id)`: seeds the style inputs and, when the entry's page
/// is outside the currently settled decode window (a panel-driven reveal
/// moved the viewport without a `TilesVisible` event), schedules a full-res
/// settle for that page.
pub fn select_entry(app: &mut App, index: usize, id: EntryId) -> Task<Message> {
    app.selected_inpaint = None;
    app.selected = Some((index, id));
    seed_style_inputs(app, app.project.entry_style(id));
    if app.scheduler.needs_settle(index, app.images.len()) {
        app.scheduler
            .schedule(index..index + 1, Message::SettleElapsed)
    } else {
        Task::none()
    }
}

// ---------------------------------------------------------------------------
// UiEvent handlers that belong to editing
// ---------------------------------------------------------------------------

pub fn handle_entry_clicked(app: &mut App, selection: Option<(usize, EntryId)>) -> Task<Message> {
    clear_editing(app);
    if app.selected_inpaint.is_some() {
        app.selected_inpaint = None;
    }
    match selection {
        Some((index, id)) if index < app.images.len() && app.project.entry(id).is_some() => {
            Task::batch([select_entry(app, index, id), scroll_to_row::<Message>(index, id)])
        }
        _ => {
            app.selected = None;
            Task::none()
        }
    }
}

pub fn handle_entry_double_clicked(app: &mut App, pair: (usize, EntryId)) -> Task<Message> {
    if app.selected_inpaint.is_some() {
        app.selected_inpaint = None;
    }
    start_inline_edit(app, pair.0, pair.1, EditOrigin::Overlay)
}

pub fn handle_panel_entry_edit(app: &mut App, pair: (usize, EntryId)) -> Task<Message> {
    if app.translation_panel_mode == scanlateit_ui::event::TranslationPanelMode::Translate {
        // Editing via panel is forbidden in Translate mode: treat as selection.
        return handle_entry_clicked(app, Some(pair));
    }
    if app.selected_inpaint.is_some() {
        app.selected_inpaint = None;
    }
    start_inline_edit(app, pair.0, pair.1, EditOrigin::Panel)
}

pub fn handle_entry_toolbar(app: &mut App, index: usize, id: EntryId, action: ToolbarAction) -> Task<Message> {
    match action {
        ToolbarAction::Rename => start_inline_edit(app, index, id, EditOrigin::Overlay),
        ToolbarAction::Delete => {
            if index >= app.images.len() {
                return Task::none();
            }
            let Some(ev) = app.project.delete_entry_with_event(id) else {
                return Task::none();
            };
            crate::app::handle_model_event(app, ev);
            app.status = "Deleted entry.".to_string();
            Task::none()
        }
        ToolbarAction::RevertTransform => {
            if index >= app.images.len() {
                return Task::none();
            }
            if !app.project.has_view_quad(id) {
                return Task::none();
            }
            if let Some(ev) = app.project.revert_transform_with_event(id) {
                crate::app::handle_model_event(app, ev);
            }
            app.status = "Reverted transform.".to_string();
            Task::none()
        }
    }
}

pub fn handle_entry_moved(app: &mut App, index: usize, id: EntryId, quad: scanlateit_model::Quad) -> Task<Message> {
    if index < app.images.len() {
        let ev = app.project.set_view_quad_with_event(id, quad);
        crate::app::handle_model_event(app, ev);
    }
    Task::none()
}

pub fn handle_edit_action(app: &mut App, action: text_editor::Action) -> Task<Message> {
    let Some(content) = app.edit_content.as_mut() else {
        return Task::none();
    };
    content.perform(action);
    let text = content.text();
    let Some((_index, id)) = app.editing else {
        return Task::none();
    };
    if !app.editing_dirty {
        app.editing_dirty = true;
        // Fork the chapter-wide profile if editing Default — go through the
        // live DB so callers get granular ModelEvents (ProfileCreated/Selected)
        // via the single Message::Model hub.
        if let Some((name, evs)) = app.project.fork_for_edit_with_event() {
            for ev in evs {
                crate::app::handle_model_event(app, ev);
            }
            app.status = format!(
                "Edit forked into '{name}': the OCR text stays untouched."
            );
        }
    }
    let target_text = text.clone();
    let ev = app.project.set_translation_with_event(id, Some(target_text.clone()));
    crate::app::handle_model_event(app, ev);
    Task::none()
}

pub fn handle_edit_rect(app: &mut App, rect: Rectangle) -> Task<Message> {
    if app.editing.is_some() {
        app.editing_rect = Some(rect);
    }
    Task::none()
}

pub fn handle_edit_submit(app: &mut App) -> Task<Message> {
    clear_editing(app);
    Task::none()
}
