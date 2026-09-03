use crate::app::tests::{app_with_entry, edit_action, start_edit, type_text};
use crate::app::{update, Message};
use easyscanlate_model::EntryId;
use easyscanlate_ui::event::{EditOrigin, MainAreaMode, ToolbarAction, UiEvent};
use iced::widget::text_editor;

#[test]
fn first_keystroke_forks_off_the_original_profile() {
    let (mut app, id) = app_with_entry();
    start_edit(&mut app, id);
    type_text(&mut app, "안녕하세요");
    let project = &app.active_tab().project;
    assert_eq!(project.profiles.len(), 2);
    assert_ne!(project.profiles.selected_id(), project.profiles.original_id());
    assert_eq!(project.profiles.selected().name, "Profile 1");
    let entry = project.ocr.get(id).unwrap();
    assert_eq!(project.display_text(entry), "안녕하세요");
    assert_eq!(entry.text, "안녕", "OCR source of truth must stay untouched");
}

#[test]
fn later_keystrokes_stay_on_the_forked_profile() {
    let (mut app, id) = app_with_entry();
    start_edit(&mut app, id);
    type_text(&mut app, "abc");
    let project = &app.active_tab().project;
    assert_eq!(project.profiles.len(), 2, "fork must happen exactly once");
    assert_eq!(project.profiles.selected().name, "Profile 1");
    let entry = project.ocr.get(id).unwrap();
    assert_eq!(project.display_text(entry), "abc");
}

#[test]
fn edits_on_a_non_original_profile_apply_in_place() {
    let (mut app, id) = app_with_entry();
    app.active_tab_mut()
        .project
        .profiles
        .selected_mut()
        .set_translation(id, Some("Hello".into()));
    let jp = app.active_tab_mut().project.profiles.add("JP");
    app.active_tab_mut().project.profiles.select(jp);
    start_edit(&mut app, id);
    type_text(&mut app, "Hi");
    let project = &app.active_tab().project;
    assert_eq!(project.profiles.len(), 2, "no fork on non-original profiles");
    assert_eq!(project.profiles.selected_id(), jp);
    let entry = project.ocr.get(id).unwrap();
    assert_eq!(project.display_text(entry), "Hi");
}

#[test]
fn double_click_alone_does_not_fork() {
    let (mut app, id) = app_with_entry();
    let _ = update(&mut app, Message::Ui(UiEvent::EntryDoubleClicked((0, id))));
    assert_eq!(app.active_tab().project.profiles.len(), 1);
    assert_eq!(app.active_tab().editing, Some((0, id)));
    assert_eq!(app.active_tab().editing_origin, EditOrigin::Overlay);
    assert!(app.active_tab().edit_content.is_some(), "double-click must seed the editor");
}

#[test]
fn panel_edit_forks_on_first_keystroke() {
    let (mut app, id) = app_with_entry();
    let _ = update(&mut app, Message::Ui(UiEvent::PanelEntryEdit((0, id))));
    assert_eq!(app.active_tab().editing, Some((0, id)));
    assert_eq!(app.active_tab().editing_origin, EditOrigin::Panel);
    assert_eq!(app.active_tab().selected, Some((0, id)), "panel edit must select the row");
    type_text(&mut app, "안녕하세요");
    let project = &app.active_tab().project;
    assert_eq!(project.profiles.len(), 2, "fork must happen on first keystroke");
    assert_ne!(project.profiles.selected_id(), project.profiles.original_id());
    let entry = project.ocr.get(id).unwrap();
    assert_eq!(project.display_text(entry), "안녕하세요");
    assert_eq!(entry.text, "안녕", "OCR source of truth must stay untouched");
}

#[test]
fn panel_edit_submit_clears_editing_state() {
    let (mut app, id) = app_with_entry();
    let _ = update(&mut app, Message::Ui(UiEvent::PanelEntryEdit((0, id))));
    type_text(&mut app, "hi");
    let _ = update(&mut app, Message::Ui(UiEvent::EditSubmit));
    assert_eq!(app.active_tab().editing, None);
    assert_eq!(app.active_tab().editing_origin, EditOrigin::Overlay);
    assert!(app.active_tab().edit_content.is_none());
    let entry = app.active_tab().project.ocr.get(id).unwrap();
    assert_eq!(app.active_tab().project.display_text(entry), "hi");
}

#[test]
fn moving_an_entry_updates_view_quad_but_not_the_ocr_quad() {
    use easyscanlate_model::Quad;
    let (mut app, id) = app_with_entry();
    let moved = Quad {
        points: [[20.0, 25.0], [40.0, 25.0], [40.0, 35.0], [20.0, 35.0]],
    };
    let _ = update(&mut app, Message::Ui(UiEvent::EntryMoved((0, id, moved))));
    let entry = app.active_tab().project.ocr.get(id).unwrap();
    assert_eq!(app.active_tab().project.view_quad(entry), moved);
    assert_eq!(
        entry.quad.bounds(),
        [0.0, 0.0, 10.0, 10.0],
        "dragging must never touch the OCR quad"
    );
}

#[test]
fn switching_main_area_mode_updates_state() {
    let (mut app, _id) = app_with_entry();
    assert_eq!(app.active_tab().view_mode, MainAreaMode::View);
    let _ = update(&mut app, Message::Ui(UiEvent::MainAreaMode(MainAreaMode::Compare)));
    assert_eq!(app.active_tab().view_mode, MainAreaMode::Compare);
    assert!(app.active_tab().status.contains("Compare mode"));
    let _ = update(&mut app, Message::Ui(UiEvent::MainAreaMode(MainAreaMode::View)));
    assert_eq!(app.active_tab().view_mode, MainAreaMode::View);
}

#[test]
fn enter_inserts_a_newline() {
    let (mut app, id) = app_with_entry();
    start_edit(&mut app, id);
    edit_action(&mut app, text_editor::Action::Edit(text_editor::Edit::Enter));
    let entry = app.active_tab().project.ocr.get(id).unwrap();
    assert_eq!(
        app.active_tab().project.display_text(entry),
        "\n",
        "the selected text is replaced by the newline"
    );
    let text = app.active_tab().edit_content.as_ref().unwrap().text();
    assert_eq!(text, "\n");
}

#[test]
fn submit_clears_the_editing_state() {
    let (mut app, id) = app_with_entry();
    start_edit(&mut app, id);
    type_text(&mut app, "hi");
    let _ = update(&mut app, Message::Ui(UiEvent::EditSubmit));
    assert_eq!(app.active_tab().editing, None);
    assert!(app.active_tab().edit_content.is_none());
    let entry = app.active_tab().project.ocr.get(id).unwrap();
    assert_eq!(app.active_tab().project.display_text(entry), "hi");
}

#[test]
fn toolbar_rename_starts_an_inline_edit() {
    let (mut app, id) = app_with_entry();
    start_edit(&mut app, id);
    let _ = update(&mut app, Message::Ui(UiEvent::EditSubmit));
    let _ = update(
        &mut app,
        Message::Ui(UiEvent::EntryToolbar((0, id, ToolbarAction::Rename))),
    );
    assert_eq!(app.active_tab().selected, Some((0, id)));
    assert_eq!(app.active_tab().editing, Some((0, id)));
    assert!(app.active_tab().edit_content.is_some());
    assert_eq!(app.active_tab().project.profiles.len(), 1, "rename must not fork");
}

#[test]
fn toolbar_delete_soft_deletes_and_clears_selection() {
    let (mut app, id) = app_with_entry();
    start_edit(&mut app, id);
    let _ = update(&mut app, Message::Ui(UiEvent::EditSubmit));
    let _ = update(
        &mut app,
        Message::Ui(UiEvent::EntryToolbar((0, id, ToolbarAction::Delete))),
    );
    assert_eq!(app.active_tab().selected, None);
    assert_eq!(app.active_tab().editing, None);
    assert!(app.active_tab().edit_content.is_none());
    let image_id = app.active_tab().images[0].image_id;
    assert_eq!(app.active_tab().project.ocr.visible_count_for(image_id), 0);
    assert!(app.active_tab().project.ocr.get(id).unwrap().deleted);
}

#[test]
fn toolbar_actions_on_unknown_entries_are_noops() {
    let (mut app, _id) = app_with_entry();
    let image_id = app.active_tab().images[0].image_id;
    let id = app.active_tab().project.ocr.visible_for(image_id).next().unwrap().id;
    start_edit(&mut app, id);
    let _ = update(&mut app, Message::Ui(UiEvent::EditSubmit));
    app.active_tab_mut().selected = None;
    let missing = EntryId(u64::MAX);
    let _ = update(
        &mut app,
        Message::Ui(UiEvent::EntryToolbar((0, missing, ToolbarAction::Rename))),
    );
    let _ = update(
        &mut app,
        Message::Ui(UiEvent::EntryToolbar((0, missing, ToolbarAction::Delete))),
    );
    let _ = update(
        &mut app,
        Message::Ui(UiEvent::EntryToolbar((999, missing, ToolbarAction::Delete))),
    );
    let _ = update(
        &mut app,
        Message::Ui(UiEvent::EntryToolbar((0, missing, ToolbarAction::RevertTransform))),
    );
    assert_eq!(app.active_tab().editing, None);
    assert_eq!(app.active_tab().selected, None);
    assert_eq!(app.active_tab().project.ocr.visible_count_for(image_id), 1);
}

#[test]
fn toolbar_revert_drops_the_view_quad_back_to_the_ocr_quad() {
    let (mut app, id) = app_with_entry();
    let ocr_quad = app.active_tab().project.ocr.get(id).unwrap().quad;
    let ocr_tl = ocr_quad.ordered()[0];
    let moved = ocr_quad.translate(15.0, 8.0).rotate([50.0, 50.0], 0.4);
    let moved_tl = moved.ordered()[0];
    let _ = update(&mut app, Message::Ui(UiEvent::EntryMoved((0, id, moved))));
    assert_ne!(app.active_tab().project.view_quad(app.active_tab().project.ocr.get(id).unwrap()), ocr_quad);
    app.active_tab_mut().selected = Some((0, id));
    let _ = update(
        &mut app,
        Message::Ui(UiEvent::EntryToolbar((0, id, ToolbarAction::RevertTransform))),
    );
    let view = app.active_tab().project.view_quad(app.active_tab().project.ocr.get(id).unwrap());
    let tl = view.ordered()[0];
    assert!(
        (tl[0] - moved_tl[0]).abs() < 1e-3 && (tl[1] - moved_tl[1]).abs() < 1e-3,
        "revert must keep the box's position, got {tl:?} expected {moved_tl:?}"
    );
    assert_eq!(view.translate(-(tl[0] - ocr_tl[0]), -(tl[1] - ocr_tl[1])), ocr_quad,
        "revert must restore the OCR shape/rotation/size");
    assert_eq!(app.active_tab().selected, Some((0, id)), "revert must keep the selection");
    assert!(
        !app.active_tab().project.ocr.get(id).unwrap().deleted,
        "revert must not touch the entry"
    );
}
