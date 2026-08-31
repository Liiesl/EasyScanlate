pub mod edit_tests;
pub mod preset_tests;
pub mod retranslate_tests;

pub use super::*;
use iced::widget::text_editor;
use scanlateit_settings::INITIAL_PRESET_SLOTS;

pub(crate) fn app_with_entry() -> (App, EntryId) {
    use scanlateit_model::{EntrySource, NewEntry, Quad};
    let mut app = App::new(NativeFrame::default());
    {
        let tab = app.active_tab_mut();
        tab.kind = crate::app::tab::TabKind::Project;
        tab.title = "test".to_string();
    }
    let image_id = {
        let tab = app.active_tab_mut();
        tab.project.add_image("x.png", 100.0, 100.0)
    };
    let id = {
        let tab = app.active_tab_mut();
        tab.project.ocr.append_for_image(
            image_id,
            NewEntry {
                source: EntrySource::AutoOcr,
                text: "안녕".to_string(),
                score: 0.9,
                quad: Quad {
                    points: [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
                },
            },
        )
    };
    app.active_tab_mut().images.push(LoadedImage {
        image_id,
        decode: PageDecode::default(),
        inpaint: Vec::new(),
    });
    (app, id)
}

pub(crate) fn start_edit(app: &mut App, id: EntryId) {
    {
        let tab = app.active_tab_mut();
        tab.selected = Some((0, id));
        tab.editing = Some((0, id));
        tab.editing_dirty = false;
    }
    let text = {
        let tab = app.active_tab();
        tab.project
            .display_text(tab.project.ocr.get(id).unwrap())
            .to_string()
    };
    let mut content = text_editor::Content::with_text(&text);
    content.perform(text_editor::Action::SelectAll);
    app.active_tab_mut().edit_content = Some(content);
}

pub(crate) fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        let _ = update(
            app,
            Message::Ui(UiEvent::EditAction(text_editor::Action::Edit(
                text_editor::Edit::Insert(c),
            ))),
        );
    }
}

pub(crate) fn edit_action(app: &mut App, action: text_editor::Action) {
    let _ = update(app, Message::Ui(UiEvent::EditAction(action)));
}
