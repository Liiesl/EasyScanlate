use crate::app::tests::app_with_entry;
use crate::app::translation as translation;
use crate::app::{update, Message, TabMessage};
use easyscanlate_model::EntryId;
use easyscanlate_ui::event::UiEvent;
use std::collections::BTreeMap;

#[cfg(feature = "translation")]
#[test]
fn retranslate_without_connection_is_rejected() {
    let (mut app, id) = app_with_entry();
    let _ = update(&mut app, Message::Ui(UiEvent::RetranslateEntry((0, id))));
    assert!(!app.active_tab().translating);
    assert!(app.active_tab().status.contains("Connect a translation service"));
}

#[cfg(feature = "translation")]
#[test]
fn retranslate_missing_entry_is_rejected() {
    let (mut app, _) = app_with_entry();
    let _ = update(
        &mut app,
        Message::Ui(UiEvent::RetranslateEntry((0, EntryId(999)))),
    );
    assert!(!app.active_tab().translating);
    assert!(app.active_tab().status.contains("no longer exists"));
}

#[cfg(feature = "translation")]
#[test]
fn retranslate_starts_translation_when_connected() {
    let (mut app, id) = app_with_entry();
    app.tx = translation::Session::new(
        BTreeMap::from([(
            "openai".to_string(),
            translation::Connection {
                api_key: "sk-o".to_string(),
                base_url: None,
                model: None,
            },
        )]),
        None,
    );
    let _ = update(&mut app, Message::Ui(UiEvent::RetranslateEntry((0, id))));
    assert!(app.active_tab().translating, "retranslate must set the translating flag");
    assert!(app.active_tab().status.starts_with("Retranslating"));
}

#[cfg(feature = "translation")]
#[test]
fn retranslate_finished_forks_a_profile_off_the_default() {
    let (mut app, id) = app_with_entry();
    let tid = app.active_tab().id;
    let _ = update(
        &mut app,
        Message::Tab(tid, TabMessage::RetranslateFinished((0, id), Ok("Hello".to_string()))),
    );
    let project = &app.active_tab().project;
    assert_eq!(project.profiles.len(), 2, "a fork must be created");
    assert_eq!(project.profiles.selected().name, "Profile 1");
    assert_eq!(project.profiles.selected().translation_of(id), Some("Hello"));
    let original = project
        .profiles
        .iter()
        .find(|p| p.id == project.profiles.original_id())
        .expect("the Default profile always exists");
    assert_eq!(original.translation_of(id), None, "Default keeps no delta");
}

#[cfg(feature = "translation")]
#[test]
fn retranslate_finished_writes_into_the_selected_profile_in_place() {
    let (mut app, id) = app_with_entry();
    app.active_tab_mut()
        .project
        .profiles
        .selected_mut()
        .set_translation(id, Some("Hello".into()));
    let jp = app.active_tab_mut().project.profiles.add("JP");
    app.active_tab_mut().project.profiles.select(jp);
    let tid = app.active_tab().id;
    let _ = update(
        &mut app,
        Message::Tab(tid, TabMessage::RetranslateFinished((0, id), Ok("Hola".to_string()))),
    );
    let project = &app.active_tab().project;
    assert_eq!(project.profiles.len(), 2, "no fork on non-original profiles");
    assert_eq!(project.profiles.selected_id(), jp);
    assert_eq!(project.profiles.selected().translation_of(id), Some("Hola"));
}

#[cfg(feature = "translation")]
#[test]
fn retranslate_finished_error_leaves_the_profile_untouched() {
    let (mut app, id) = app_with_entry();
    app.active_tab_mut()
        .project
        .profiles
        .selected_mut()
        .set_translation(id, Some("Hello".into()));
    let tid = app.active_tab().id;
    let _ = update(
        &mut app,
        Message::Tab(tid, TabMessage::RetranslateFinished((0, id), Err("boom".to_string()))),
    );
    assert!(!app.active_tab().translating);
    assert_eq!(app.active_tab().status, "boom");
    let project = &app.active_tab().project;
    assert_eq!(project.profiles.len(), 1);
    assert_eq!(project.profiles.selected().translation_of(id), Some("Hello"));
}

#[cfg(feature = "translation")]
#[test]
fn retranslate_finished_strips_quotes_and_clears_when_identical_to_ocr() {
    let (mut app, id) = app_with_entry();
    let tid = app.active_tab().id;
    let _ = update(
        &mut app,
        Message::Tab(tid, TabMessage::RetranslateFinished((0, id), Ok("\"안녕\"".to_string()))),
    );
    let project = &app.active_tab().project;
    assert_eq!(project.profiles.len(), 2, "the fork still happens");
    assert_eq!(
        project.profiles.selected().translation_of(id),
        None,
        "same as the OCR text: delta cleared, original shown"
    );
    assert_eq!(project.display_text(project.ocr.get(id).unwrap()), "안녕");
}
