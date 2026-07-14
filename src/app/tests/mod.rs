pub mod edit_tests;
pub mod preset_tests;
pub mod retranslate_tests;

pub use super::*;
use scanlateit_model::INITIAL_PRESET_SLOTS;

pub(crate) fn app_with_entry() -> (App, EntryId) {
    use scanlateit_model::{EntrySource, NewEntry, Quad};
    let mut app = App::new(NativeFrame::default());
    let mut project = Project::new();
    let id = project.ocr.append(NewEntry {
        source: EntrySource::AutoOcr,
        text: "안녕".to_string(),
        score: 0.9,
        quad: Quad {
            points: [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
        },
    });
    app.images.push(LoadedImage {
        width: 100.0,
        height: 100.0,
        path: "x.png".to_string(),
        project,
        decode: PageDecode::default(),
        inpaint: Vec::new(),
    });
    (app, id)
}

pub(crate) fn start_edit(app: &mut App, id: EntryId) {
    app.selected = Some((0, id));
    app.editing = Some((0, id));
    app.editing_dirty = false;
    let text = app.images[0]
        .project
        .display_text(app.images[0].project.ocr.get(id).unwrap())
        .to_string();
    let mut content = text_editor::Content::with_text(&text);
    content.perform(text_editor::Action::SelectAll);
    app.edit_content = Some(content);
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
