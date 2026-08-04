pub mod edit_tests;
pub mod preset_tests;
pub mod retranslate_tests;

pub use super::*;
use scanlateit_model::INITIAL_PRESET_SLOTS;

pub(crate) fn app_with_entry() -> (App, EntryId) {
    use scanlateit_model::{EntrySource, NewEntry, Quad};
    let mut app = App::new(NativeFrame::default());
    let image_id = app.project.add_image("x.png", 100.0, 100.0);
    let id = app.project.ocr.append_for_image(
        image_id,
        NewEntry {
            source: EntrySource::AutoOcr,
            text: "안녕".to_string(),
            score: 0.9,
            quad: Quad {
                points: [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
            },
        },
    );
    app.images.push(LoadedImage {
        image_id,
        decode: PageDecode::default(),
        inpaint: Vec::new(),
    });
    (app, id)
}

pub(crate) fn start_edit(app: &mut App, id: EntryId) {
    app.selected = Some((0, id));
    app.editing = Some((0, id));
    app.editing_dirty = false;
    let text = app
        .project
        .display_text(app.project.ocr.get(id).unwrap())
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
