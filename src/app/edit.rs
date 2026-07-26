use iced::widget::text_editor;
use iced::{Rectangle, Task};
use scanlateit_model::EntryId;
use scanlateit_ui::event::{EditOrigin, ToolbarAction};
use scanlateit_ui::panel::results::scroll_to_row;
use scanlateit_ui::event::UiEvent;

use super::layout::{EDIT_INPUT_ID, PANEL_EDIT_INPUT_ID};
use super::{App, Message};

/// Starts an inline edit of `(index, id)`: selects the entry, seeds the
/// editor buffer with its displayed text and selects it all so the first
/// keystroke replaces it, then focuses the editor of `origin` (the floating
/// overlay editor or the panel's results-row editor). Shared by the
/// double-click action, the toolbar's "Rename" button and the panel rows.
pub fn start_inline_edit(app: &mut App, index: usize, id: EntryId, origin: EditOrigin) -> Task<Message> {
    let Some(image) = app.images.get(index) else {
        return Task::none();
    };
    let Some(entry) = image.project.ocr.get(id) else {
        return Task::none();
    };
    let text = image.project.display_text(entry).to_string();
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
    seed_style_inputs(app, app.images[index].project.entry_style(id));
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
        Some((index, id))
            if app
                .images
                .get(index)
                .is_some_and(|image| image.project.ocr.get(id).is_some()) =>
        {
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
            let Some(image) = app.images.get_mut(index) else {
                return Task::none();
            };
            if !image.project.delete_entry(id) {
                return Task::none();
            }
            app.selected = None;
            clear_editing(app);
            app.status = "Deleted entry.".to_string();
            Task::none()
        }
        ToolbarAction::RevertTransform => {
            let Some(image) = app.images.get_mut(index) else {
                return Task::none();
            };
            if !image.project.has_view_quad(id) {
                return Task::none();
            }
            image.project.revert_transform(id);
            app.status = "Reverted transform.".to_string();
            Task::none()
        }
    }
}

pub fn handle_entry_moved(app: &mut App, index: usize, id: EntryId, quad: scanlateit_model::Quad) -> Task<Message> {
    if let Some(image) = app.images.get_mut(index) {
        image.project.set_view_quad(id, quad);
    }
    Task::none()
}

pub fn handle_edit_action(app: &mut App, action: text_editor::Action) -> Task<Message> {
    let Some(content) = app.edit_content.as_mut() else {
        return Task::none();
    };
    content.perform(action);
    let text = content.text();
    let Some((index, id)) = app.editing else {
        return Task::none();
    };
    if !app.editing_dirty {
        app.editing_dirty = true;
        // Fork the edited image's profile and propagate the new profile to all images
        let forked_name = {
            let project = &mut app.images[index].project;
            project.profiles.fork_for_edit()
        };
        if let Some(name) = forked_name {
            for (i, img) in app.images.iter_mut().enumerate() {
                if i == index {
                    continue;
                }
                if let Some(existing) = img.project.profiles.find_by_name(&name) {
                    img.project.profiles.select(existing);
                } else {
                    let nid = img.project.profiles.add(name.clone());
                    img.project.profiles.select(nid);
                }
            }
            // Keep translate base in sync when it was the original profile
            // (so base can be the forked profile if needed)
            app.status = format!(
                "Edit forked into '{name}': the OCR text stays untouched."
            );
        }
    }
    let project = &mut app.images[index].project;
    let target_text = text.clone();
    project
        .profiles
        .selected_mut()
        .set_translation(id, Some(target_text.clone()));
    // Propagate the same translation to the forked profile of other images if they were synced
    // (other images don't have this entry id, but keep profiles in sync for consistency;
    // only the edited image's entry matters)
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
