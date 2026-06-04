use std::sync::Arc;

use iced::widget::{row, text_editor};
use iced::{Element, Font, Length, Rectangle, Task};

use scanlateit_model::{EntryId, EntryStyle, NewEntry, ProfileId, Project};
use scanlateit_ocr::{self as ocr, Engine, OcrCancellationToken};
use scanlateit_translation as translation;
use scanlateit_ui::main_area::decode::{decode_page, DecodedPage, PageDecode, MAX_DECODE_EDGE};
use scanlateit_ui::{
    event::{ToolbarAction, UiEvent},
    main_area, panel, KOREAN_FONT_NAME, KOREAN_FONT_PATH, LoadedImage, UiState,
};
use scanlateit_ui::parse_hex;

const DECODE_PRELOAD: usize = 2;

/// Widget id of the floating inline editor shown over a double-clicked entry.
const EDIT_INPUT_ID: &'static str = "overlay-editor";

const IMAGE_FILTERS: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "webp", "tiff", "avif"];

#[derive(Debug, Clone)]
pub enum Message {
    /// A widget-level event from the ui crate.
    Ui(UiEvent),
    ImagesPicked(Result<Vec<(String, u32, u32)>, String>),
    EngineReady(Result<Engine, String>),
    OcrFinished(usize, Result<Vec<NewEntry>, String>),
    FontLoaded,
    TileDecoded(usize, Result<Arc<DecodedPage>, String>),
    TranslateFinished(Vec<(usize, EntryId, String)>, Result<Vec<String>, String>),
}

impl From<UiEvent> for Message {
    fn from(event: UiEvent) -> Self {
        Message::Ui(event)
    }
}

/// Session state: one loaded image plus everything iced/OCR related that the
/// model doesn't know about (engine handle, per-image canvas cache).
pub struct App {
    pub(crate) images: Vec<LoadedImage>,
    engine: Option<Engine>,
    cancel: Option<OcrCancellationToken>,
    pub(crate) running: bool,
    pub(crate) font: Option<Font>,
    pub(crate) status: String,
    pending: usize,
    ocr_total: usize,
    ocr_failed: usize,
    ocr_cancelled: bool,
    ocr_index: usize,
    pub(crate) translating: bool,
    pub(crate) translate_model: String,
    pub(crate) translate_lang: String,
    pub(crate) translate_api_key: String,
    /// The currently selected overlay entry as `(image index, entry id)`;
    /// the style panel edits exactly this entry and nothing else.
    pub(crate) selected: Option<(usize, EntryId)>,
    /// The entry being edited inline as `(image index, entry id)`; `None`
    /// when no inline edit is active.
    pub(crate) editing: Option<(usize, EntryId)>,
    /// The multi-line editor buffer backing the inline edit; always `Some`
    /// while `editing` is. Owned here so the widget can mutate it in place.
    pub(crate) edit_content: Option<text_editor::Content>,
    /// True once a keystroke actually changed the edited text. The fork off
    /// the original profile happens exactly once, on this first change: the
    /// double-click itself never forks anything.
    pub(crate) editing_dirty: bool,
    /// Latest viewport rect of the edited entry, in tile viewer coordinates.
    pub(crate) editing_rect: Option<Rectangle>,
    /// Staged style of the selected entry. Mirrors the entry's stored style
    /// on selection; mutations are written back to that entry only.
    pub(crate) style_working: EntryStyle,
    /// Raw hex text of the styling inputs; kept as typed, only applied to
    /// `style_working` while it parses.
    pub(crate) style_text_hex: String,
    pub(crate) style_stroke_hex: String,
    pub(crate) style_bg_hex: String,
    pub(crate) style_stroke_width: String,
    pub(crate) style_bg_radius: String,
}
impl App {
    fn new() -> Self {
        let style = EntryStyle::default();
        Self {
            images: Vec::new(),
            engine: None,
            cancel: None,
            running: false,
            font: None,
            status: "Idle - open images to begin.".to_string(),
            pending: 0,
            ocr_total: 0,
            ocr_failed: 0,
            ocr_cancelled: false,
            ocr_index: 0,
            translating: false,
            translate_model: translation::MODELS[0].to_string(),
            translate_lang: translation::LANGUAGES[0].to_string(),
            translate_api_key: String::new(),
            selected: None,
            editing: None,
            edit_content: None,
            editing_dirty: false,
            editing_rect: None,
            style_working: style,
            style_text_hex: hex_to_string(style.text_color),
            style_stroke_hex: hex_to_string(style.stroke_color),
            style_bg_hex: hex_to_string(style.bg_color),
            style_stroke_width: style.stroke_width.to_string(),
            style_bg_radius: style.bg_radius.to_string(),
        }
    }
}

/// Starts an inline edit of `(index, id)`: selects the entry, seeds the
/// editor buffer with its displayed text and selects it all so the first
/// keystroke replaces it, then focuses the floating editor. Shared by the
/// double-click action and the toolbar's "Rename" button.
fn start_inline_edit(app: &mut App, index: usize, id: EntryId) -> Task<Message> {
    let Some(image) = app.images.get(index) else {
        return Task::none();
    };
    let Some(entry) = image.project.ocr.get(id) else {
        return Task::none();
    };
    let text = image.project.display_text(entry).to_string();
    app.selected = Some((index, id));
    seed_style_inputs(app, app.images[index].project.entry_style(id));
    app.editing = Some((index, id));
    app.editing_dirty = false;
    app.editing_rect = None;
    let mut content = text_editor::Content::with_text(&text);
    content.perform(text_editor::Action::SelectAll);
    app.edit_content = Some(content);
    app.status = format!("Editing \"{text}\" in the overlay.");
    Task::batch([iced::widget::operation::focus(EDIT_INPUT_ID)])
}

/// Clears every piece of inline-editing state in one place.
fn clear_editing(app: &mut App) {
    app.editing = None;
    app.edit_content = None;
    app.editing_dirty = false;
    app.editing_rect = None;
}

/// Reseeds the style panel inputs from `style`, keeping raw strings in sync
/// with the resolved values.
fn seed_style_inputs(app: &mut App, style: EntryStyle) {
    app.style_working = style;
    app.style_text_hex = hex_to_string(style.text_color);
    app.style_stroke_hex = hex_to_string(style.stroke_color);
    app.style_bg_hex = hex_to_string(style.bg_color);
    app.style_stroke_width = style.stroke_width.to_string();
    app.style_bg_radius = style.bg_radius.to_string();
}

/// Formats an RGBA color as `#RRGGBBAA`.
fn hex_to_string(rgba: [u8; 4]) -> String {
    format!("#{:02X}{:02X}{:02X}{:02X}", rgba[0], rgba[1], rgba[2], rgba[3])
}

impl UiState for App {
    fn images(&self) -> &[LoadedImage] {
        &self.images
    }

    fn running(&self) -> bool {
        self.running
    }

    fn translating(&self) -> bool {
        self.translating
    }

    fn status(&self) -> &str {
        &self.status
    }

    fn translate_model(&self) -> &str {
        &self.translate_model
    }

    fn translate_lang(&self) -> &str {
        &self.translate_lang
    }

    fn translate_api_key(&self) -> &str {
        &self.translate_api_key
    }

    fn selected(&self) -> Option<(usize, EntryId)> {
        self.selected
    }

    fn style_working(&self) -> &EntryStyle {
        &self.style_working
    }

    fn style_text_hex(&self) -> &str {
        &self.style_text_hex
    }

    fn style_stroke_hex(&self) -> &str {
        &self.style_stroke_hex
    }

    fn style_bg_hex(&self) -> &str {
        &self.style_bg_hex
    }

    fn style_stroke_width(&self) -> &str {
        &self.style_stroke_width
    }

    fn style_bg_radius(&self) -> &str {
        &self.style_bg_radius
    }

    fn editing(&self) -> Option<(usize, EntryId)> {
        self.editing
    }

    fn editing_rect(&self) -> Option<Rectangle> {
        self.editing_rect
    }

    fn edit_content(&self) -> Option<&text_editor::Content> {
        self.edit_content.as_ref()
    }

    fn font(&self) -> Option<Font> {
        self.font
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_style_round_trips_all_fields() {
        let style = EntryStyle::default();
        assert_eq!(style.bold, false);
        assert_eq!(style.italic, false);
        assert_eq!(style.stroke_color, [0, 0, 0, 255]);
        assert_eq!(style.stroke_width, 0.0);
        assert_eq!(style.bg_radius, 0.0);
    }

    fn app_with_entry() -> (App, EntryId) {
        use scanlateit_model::{EntrySource, NewEntry, Quad};
        let mut app = App::new();
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
            decode: PageDecode::Pending,
        });
        (app, id)
    }

    /// Starts an inline edit exactly like a double-click on the entry:
    /// selects it, seeds the editor buffer with its displayed text and
    /// selects it all so the first keystroke replaces it.
    fn start_edit(app: &mut App, id: EntryId) {
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

    /// Types `text` into the inline editor one character at a time, the way
    /// the editor widget reports it.
    fn type_text(app: &mut App, text: &str) {
for c in text.chars() {
            let _ = update(
                app,
                Message::Ui(UiEvent::EditAction(text_editor::Action::Edit(
                    text_editor::Edit::Insert(c),
                ))),
            );
        }
    }

    fn edit_action(app: &mut App, action: text_editor::Action) {
        let _ = update(app, Message::Ui(UiEvent::EditAction(action)));
    }

    #[test]
    fn first_keystroke_forks_off_the_original_profile() {
        let (mut app, id) = app_with_entry();
        start_edit(&mut app, id);

        type_text(&mut app, "안녕하세요");

        let project = &app.images[0].project;
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

        let project = &app.images[0].project;
        assert_eq!(project.profiles.len(), 2, "fork must happen exactly once");
        assert_eq!(project.profiles.selected().name, "Profile 1");
        let entry = project.ocr.get(id).unwrap();
        assert_eq!(project.display_text(entry), "abc");
    }

    #[test]
    fn edits_on_a_non_original_profile_apply_in_place() {
        let (mut app, id) = app_with_entry();
        app.images[0]
            .project
            .profiles
            .selected_mut()
            .set_translation(id, Some("Hello".into()));
        let jp = app.images[0].project.profiles.add("JP");
        app.images[0].project.profiles.select(jp);
        start_edit(&mut app, id);

        type_text(&mut app, "Hi");

        let project = &app.images[0].project;
        assert_eq!(project.profiles.len(), 2, "no fork on non-original profiles");
        assert_eq!(project.profiles.selected_id(), jp);
        let entry = project.ocr.get(id).unwrap();
        assert_eq!(project.display_text(entry), "Hi");
    }

    #[test]
    fn double_click_alone_does_not_fork() {
        let (mut app, id) = app_with_entry();
        let _ = update(&mut app, Message::Ui(UiEvent::EntryDoubleClicked((0, id))));
        assert_eq!(app.images[0].project.profiles.len(), 1);
        assert_eq!(app.editing, Some((0, id)));
        assert!(app.edit_content.is_some(), "double-click must seed the editor");
    }

    #[test]
    fn moving_an_entry_updates_view_bounds_but_not_the_quad() {
        let (mut app, id) = app_with_entry();
        let _ = update(&mut app, Message::Ui(UiEvent::EntryMoved((0, id, [20.0, 25.0, 40.0, 35.0]))));

        let image = &app.images[0];
        let entry = image.project.ocr.get(id).unwrap();
        assert_eq!(image.project.view_bounds(entry), [20.0, 25.0, 40.0, 35.0]);
        assert_eq!(
            entry.quad.bounds(),
            [0.0, 0.0, 10.0, 10.0],
            "dragging must never touch the OCR quad"
        );
    }

    #[test]
    fn enter_inserts_a_newline() {
        let (mut app, id) = app_with_entry();
        start_edit(&mut app, id);

        edit_action(&mut app, text_editor::Action::Edit(text_editor::Edit::Enter));

        let entry = app.images[0].project.ocr.get(id).unwrap();
        assert_eq!(
            app.images[0].project.display_text(entry),
            "\n",
            "the selected text is replaced by the newline"
        );
        let text = app.edit_content.as_ref().unwrap().text();
        assert_eq!(text, "\n");
    }

    #[test]
    fn submit_clears_the_editing_state() {
        let (mut app, id) = app_with_entry();
        start_edit(&mut app, id);
        type_text(&mut app, "hi");

        let _ = update(&mut app, Message::Ui(UiEvent::EditSubmit));

        assert_eq!(app.editing, None);
        assert!(app.edit_content.is_none());
        let entry = app.images[0].project.ocr.get(id).unwrap();
        assert_eq!(app.images[0].project.display_text(entry), "hi");
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

        assert_eq!(app.selected, Some((0, id)));
        assert_eq!(app.editing, Some((0, id)));
        assert!(app.edit_content.is_some());
        assert_eq!(app.images[0].project.profiles.len(), 1, "rename must not fork");
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

        assert_eq!(app.selected, None);
        assert_eq!(app.editing, None);
        assert!(app.edit_content.is_none());
        assert_eq!(app.images[0].project.ocr.visible_count(), 0);
        assert!(app.images[0].project.ocr.get(id).unwrap().deleted);
    }

    #[test]
    fn toolbar_actions_on_unknown_entries_are_noops() {
        let (mut app, _id) = app_with_entry();
        let id = app.images[0].project.ocr.visible().next().unwrap().id;
        start_edit(&mut app, id);
        let _ = update(&mut app, Message::Ui(UiEvent::EditSubmit));

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

        assert_eq!(app.editing, None);
        assert_eq!(app.selected, None);
        assert_eq!(app.images[0].project.ocr.visible_count(), 1);
    }
}

pub fn boot() -> (App, Task<Message>) {
    let font_task = match std::fs::read(KOREAN_FONT_PATH) {
        Ok(bytes) => iced::font::load(bytes).map(|_| Message::FontLoaded),
        Err(_) => Task::none(),
    };
    (App::new(), font_task)
}

/// Spawns OCR for exactly one image (the next in the queue). At most one task
/// is in flight at a time: the next image is only scheduled from inside the
/// `OcrFinished` handler, so each result reaches the UI before the next OCR
/// starts. The shared token is created once per run in the `StartOcr` arm.
fn start_ocr_run(app: &mut App, engine: Engine) -> Task<Message> {
    let index = app.ocr_index;
    app.ocr_index += 1;
    let path = app.images[index].path.clone();
    let token = app
        .cancel
        .as_ref()
        .expect("cancellation token set before run")
        .clone();
    Task::perform(
        async move {
            let result = engine
                .run_path_cancellable(&path, &token)
                .map(ocr::to_entries);
            (index, result)
        },
        |(index, result)| Message::OcrFinished(index, result),
    )
}

fn finalize_run(app: &mut App) {
    app.running = false;
    app.cancel = None;
    app.status = if app.ocr_cancelled {
        "OCR cancelled.".to_string()
    } else if app.ocr_failed > 0 {
        format!(
            "OCR done: {} line(s), {} image(s) failed.",
            app.ocr_total, app.ocr_failed
        )
    } else {
        format!("OCR done: {} line(s).", app.ocr_total)
    };
}

pub fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::Ui(UiEvent::OpenImages) => Task::perform(
            async {
                let files = rfd::AsyncFileDialog::new()
                    .add_filter("Images", IMAGE_FILTERS)
                    .pick_files()
                    .await;
                match files {
                    Some(files) => {
                        let mut out = Vec::with_capacity(files.len());
                        for file in files {
                            let path = file.path().to_string_lossy().into_owned();
                            let dims = image::ImageReader::open(&path)
                                .map_err(|e| format!("Failed to open {path}: {e}"))?
                                .into_dimensions()
                                .map_err(|e| format!("Failed to decode {path}: {e}"));
                            match dims {
                                Ok((width, height)) => out.push((path, width, height)),
                                Err(e) => return Err(e),
                            }
                        }
                        Ok(out)
                    }
                    None => Ok(Vec::new()),
                }
            },
            Message::ImagesPicked,
        ),
        Message::ImagesPicked(result) => match result {
            Ok(images) => {
                if images.is_empty() {
                    app.status = "No images selected.".to_string();
                    return Task::none();
                }
                for (path, width, height) in images {
                    app.images.push(LoadedImage {
                        width: width as f32,
                        height: height as f32,
                        path,
                        project: Project::new(),
                        decode: PageDecode::Pending,
                    });
                }
                app.status = format!("Decoding {} image(s)...", app.images.len());
                let tasks: Vec<Task<Message>> = app
                    .images
                    .iter()
                    .enumerate()
                    .map(|(index, image)| {
                        let path = image.path.clone();
                        Task::perform(
                            async move { decode_page(&path, MAX_DECODE_EDGE).map(Arc::new) },
                            move |result| Message::TileDecoded(index, result),
                        )
                    })
                    .collect();
                Task::batch(tasks)
            }
            Err(e) => {
                app.status = e;
                Task::none()
            }
        },
        Message::Ui(UiEvent::StartOcr) => {
            if app.images.is_empty() {
                app.status = "Open images first.".to_string();
                return Task::none();
            }
            if app.running {
                return Task::none();
            }
            app.cancel = Some(OcrCancellationToken::new());
            app.running = true;
            app.pending = app.images.len();
            app.ocr_total = 0;
            app.ocr_failed = 0;
            app.ocr_cancelled = false;
            app.ocr_index = 0;
            app.status = format!("Running OCR on {} image(s)...", app.images.len());
            match app.engine.clone() {
                Some(engine) => start_ocr_run(app, engine),
                None => Task::perform(async move { Engine::build() }, Message::EngineReady),
            }
        }
        Message::EngineReady(result) => match result {
            Ok(engine) => {
                app.engine = Some(engine.clone());
                if app.running {
                    start_ocr_run(app, engine)
                } else {
                    Task::none()
                }
            }
            Err(e) => {
                app.running = false;
                app.status = e;
                Task::none()
            }
        },
        Message::Ui(UiEvent::StopOcr) => {
            if let Some(token) = &app.cancel {
                token.cancel();
            }
            app.running = false;
            app.status = "Cancelling OCR...".to_string();
            Task::none()
        }
        Message::OcrFinished(index, result) => {
            app.pending = app.pending.saturating_sub(1);
            match result {
                Ok(entries) => {
                    let count = app.images[index].project.append_ocr(entries);
                    app.ocr_total += count;
                }
                Err(e) => {
                    app.ocr_failed += 1;
                    if e == "cancelled" {
                        app.ocr_cancelled = true;
                    }
                }
            }
            if app.pending == 0 || app.ocr_cancelled {
                finalize_run(app);
                return Task::none();
            }
            app.status = format!(
                "OCR in progress: {} of {} image(s) done ({} line(s)).",
                app.images.len() - app.pending,
                app.images.len(),
                app.ocr_total
            );
            match app.engine.clone() {
                Some(engine) => start_ocr_run(app, engine),
                None => {
                    app.ocr_failed += 1;
                    finalize_run(app);
                    Task::none()
                }
            }
        }
        Message::FontLoaded => {
            app.font = Some(Font::with_name(KOREAN_FONT_NAME));
            app.status = format!(
                "{} font ready. {}",
                KOREAN_FONT_NAME,
                if app.images.is_empty() {
                    "Open images to begin."
                } else {
                    ""
                }
            );
            Task::none()
        }
        Message::Ui(UiEvent::CycleProfile) => {
            let Some(first) = app.images.first() else {
                return Task::none();
            };
            let ids: Vec<ProfileId> = first.project.profiles.iter().map(|p| p.id).collect();
            if ids.len() > 1 {
                let current = first.project.profiles.selected_id();
                let next = ids
                    .iter()
                    .position(|id| *id == current)
                    .map(|i| ids[(i + 1) % ids.len()])
                    .unwrap_or(ids[0]);
                for img in &mut app.images {
                    img.project.profiles.select(next);
                }
                let name = app.images[0].project.profiles.selected().name.clone();
                app.status = format!("Profile: {name}");
            }
            Task::none()
        }
        Message::Ui(UiEvent::TilesVisible(range)) => {
            let start = range.start.saturating_sub(DECODE_PRELOAD);
            let end = range.end.saturating_add(DECODE_PRELOAD).min(app.images.len());
            let mut tasks = Vec::new();
            for index in start..end {
                let image = &mut app.images[index];
                if matches!(&image.decode, PageDecode::Pending) {
                    image.decode = PageDecode::Decoding;
                    let path = image.path.clone();
                    tasks.push(Task::perform(
                        async move { decode_page(&path, MAX_DECODE_EDGE).map(Arc::new) },
                        move |result| Message::TileDecoded(index, result),
                    ));
                }
            }
            if tasks.is_empty() {
                Task::none()
            } else {
                Task::batch(tasks)
            }
        }
        Message::TileDecoded(index, result) => {
            if index < app.images.len() {
                app.images[index].decode = match result {
                    Ok(decoded) => PageDecode::Ready(decoded),
                    Err(_) => PageDecode::Failed,
                };
            }
            Task::none()
        }
        Message::Ui(UiEvent::Translate) => {
            if app.translating || app.running {
                return Task::none();
            }
            let jobs: Vec<(usize, EntryId, String)> = app
                .images
                .iter()
                .enumerate()
                .flat_map(|(index, image)| {
                    image
                        .project
                        .ocr
                        .visible()
                        .map(move |entry| (index, entry.id, entry.text.clone()))
                })
                .collect();
            if jobs.is_empty() {
                app.status = "Run OCR first.".to_string();
                return Task::none();
            }
            app.translating = true;
            let texts: Vec<String> = jobs.iter().map(|(_, _, text)| text.clone()).collect();
            let target = app.translate_lang.clone();
            let model = app.translate_model.clone();
            let api_key = (!app.translate_api_key.is_empty())
                .then(|| app.translate_api_key.clone());
            app.status = format!(
                "Translating {} line(s) to {} via {model}...",
                jobs.len(),
                app.translate_lang
            );
            Task::perform(
                async move {
                    let result = translation::translate_all(&texts, &target, &model, api_key).await;
                    (jobs, result)
                },
                |(jobs, result)| Message::TranslateFinished(jobs, result),
            )
        }
        Message::Ui(UiEvent::TranslateModel(model)) => {
            app.translate_model = model;
            Task::none()
        }
        Message::Ui(UiEvent::TranslateLang(lang)) => {
            app.translate_lang = lang;
            Task::none()
        }
        Message::Ui(UiEvent::TranslateApiKey(key)) => {
            app.translate_api_key = key;
            Task::none()
        }
        Message::Ui(UiEvent::EntryClicked(selection)) => {
            clear_editing(app);
            app.selected = selection.filter(|(index, id)| {
                app.images
                    .get(*index)
                    .is_some_and(|image| image.project.ocr.get(*id).is_some())
            });
            if let Some((index, id)) = app.selected {
                seed_style_inputs(app, app.images[index].project.entry_style(id));
            }
            Task::none()
        }
        Message::Ui(UiEvent::EntryDoubleClicked((index, id))) => {
            start_inline_edit(app, index, id)
        }
        Message::Ui(UiEvent::EntryToolbar((index, id, action))) => match action {
            ToolbarAction::Rename => start_inline_edit(app, index, id),
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
        },
        Message::Ui(UiEvent::EntryMoved((index, id, bounds))) => {
            if let Some(image) = app.images.get_mut(index) {
                image.project.set_view_bounds(id, bounds);
            }
            Task::none()
        }
        Message::Ui(UiEvent::EditAction(action)) => {
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
                let project = &mut app.images[index].project;
                if project.profiles.selected_id() == project.profiles.original_id() {
                    let name = project.profiles.next_available_name();
                    let forked = project.profiles.add(name.clone());
                    project.profiles.select(forked);
                    app.status = format!(
                        "Edit forked into '{name}': the OCR text stays untouched."
                    );
                }
            }
            let project = &mut app.images[index].project;
            project
                .profiles
                .selected_mut()
                .set_translation(id, Some(text));
            Task::none()
        }
        Message::Ui(UiEvent::EditRect(rect)) => {
            if app.editing.is_some() {
                app.editing_rect = Some(rect);
            }
            Task::none()
        }
        Message::Ui(UiEvent::EditSubmit) => {
            clear_editing(app);
            Task::none()
        }
        Message::Ui(UiEvent::StyleBold(bold)) => {
            let Some((index, id)) = app.selected else { return Task::none() };
            app.style_working.bold = bold;
            app.images[index].project.set_entry_style(id, app.style_working);
            Task::none()
        }
        Message::Ui(UiEvent::StyleItalic(italic)) => {
            let Some((index, id)) = app.selected else { return Task::none() };
            app.style_working.italic = italic;
            app.images[index].project.set_entry_style(id, app.style_working);
            Task::none()
        }
        Message::Ui(UiEvent::StyleTextHex(text)) => {
            let Some((index, id)) = app.selected else { return Task::none() };
            app.style_text_hex = text;
            if let Some(color) = parse_hex(&app.style_text_hex) {
                app.style_working.text_color = color;
                app.images[index].project.set_entry_style(id, app.style_working);
            }
            Task::none()
        }
        Message::Ui(UiEvent::StyleStrokeHex(text)) => {
            let Some((index, id)) = app.selected else { return Task::none() };
            app.style_stroke_hex = text;
            if let Some(color) = parse_hex(&app.style_stroke_hex) {
                app.style_working.stroke_color = color;
                app.images[index].project.set_entry_style(id, app.style_working);
            }
            Task::none()
        }
        Message::Ui(UiEvent::StyleBgHex(text)) => {
            let Some((index, id)) = app.selected else { return Task::none() };
            app.style_bg_hex = text;
            if let Some(color) = parse_hex(&app.style_bg_hex) {
                app.style_working.bg_color = color;
                app.images[index].project.set_entry_style(id, app.style_working);
            }
            Task::none()
        }
        Message::Ui(UiEvent::StyleStrokeWidth(text)) => {
            let Some((index, id)) = app.selected else { return Task::none() };
            app.style_stroke_width = text;
            if let Ok(width) = app.style_stroke_width.parse::<f32>() {
                app.style_working.stroke_width = width.max(0.0);
                app.images[index].project.set_entry_style(id, app.style_working);
            }
            Task::none()
        }
        Message::Ui(UiEvent::StyleBgRadius(text)) => {
            let Some((index, id)) = app.selected else { return Task::none() };
            app.style_bg_radius = text;
            if let Ok(radius) = app.style_bg_radius.parse::<f32>() {
                app.style_working.bg_radius = radius.max(0.0);
                app.images[index].project.set_entry_style(id, app.style_working);
            }
            Task::none()
        }
        Message::TranslateFinished(jobs, result) => {
            app.translating = false;
            match result {
                Ok(translations) => {
                    if translations.len() != jobs.len() {
                        app.status = "Translation count mismatch; nothing saved.".to_string();
                        return Task::none();
                    }
                    let profile_name = translation::profile_name(&app.translate_lang);
                    let mut current_image: Option<usize> = None;
                    for ((image_index, entry_id, _), translation) in
                        jobs.iter().zip(translations.iter())
                    {
                        if current_image != Some(*image_index) {
                            let image = &mut app.images[*image_index];
                            let id = image.project.profiles.find_by_name(&profile_name).unwrap_or_else(
                                || image.project.profiles.add(profile_name.clone()),
                            );
                            image.project.profiles.select(id);
                            current_image = Some(*image_index);
                        }
                        let image = &mut app.images[*image_index];
                        image
                            .project
                            .profiles
                            .selected_mut()
                            .set_translation(*entry_id, Some(translation.clone()));
                    }
                    app.status = format!(
                        "Translated {} line(s) into '{profile_name}'.",
                        translations.len()
                    );
                }
                Err(e) => {
                    app.status = e;
                }
            }
            Task::none()
        }
    }
}

pub fn view(app: &App) -> Element<'_, Message> {
    let content: Element<'_, UiEvent> = row![main_area::view(app), panel::view(app)]
        .spacing(2)
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
    Element::map(content, Message::from)
}
