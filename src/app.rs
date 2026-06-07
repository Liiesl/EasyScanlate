use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use iced::widget::image::Handle;
use iced::widget::{pane_grid, text_editor};
use iced::{Color, Element, Font, Length, Rectangle, Task};

use scanlateit_inpaint::Engine as InpaintEngine;
use scanlateit_model::{EntryId, EntryStyle, InpaintPatch, NewEntry, ProfileId, Project, Quad};
use scanlateit_ocr::{self as ocr, Engine, OcrCancellationToken};
use scanlateit_translation as translation;
use scanlateit_ui::loaded::InpaintLayer;
use scanlateit_ui::main_area::decode::{
    decode_page, DecodedPage, PageDecode, Tier, MAX_DECODE_EDGE, THUMB_DECODE_EDGE,
};
use scanlateit_ui::{
    event::{EditOrigin, SettingsTab, StyleField, ToolbarAction, UiEvent},
    main_area, panel, settings as settings_modal, toolbar, KOREAN_FONT_NAME, KOREAN_FONT_PATH,
    LoadedImage, UiState,
};

const DECODE_PRELOAD: usize = 2;

/// How long the viewport must stop scrolling before the full-resolution
/// decode of its neighborhood kicks in.
const SETTLE_DEBOUNCE: Duration = Duration::from_millis(150);

/// How many pages beyond the full-backed window a full decode survives a
/// settle before it is evicted.
const FULL_KEEP_MARGIN: usize = 4;

/// Widget id of the floating inline editor shown over a double-clicked entry.
const EDIT_INPUT_ID: &'static str = "overlay-editor";

/// Widget id of the multi-line editor shown in a results-list row while the
/// entry is edited from the panel.
const PANEL_EDIT_INPUT_ID: &'static str = "panel-editor";

const IMAGE_FILTERS: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "webp", "tiff", "avif"];

/// The pane the side panel occupies at launch: ~300px out of the 1400px
/// default window, as a fraction of the main area's width.
const MAIN_AREA_DEFAULT_RATIO: f32 = 0.78;

/// The two panes of the app window: the page viewer and the side panel.
#[derive(Debug, Clone, Copy)]
pub enum PaneKind {
    MainArea,
    Panel,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// A widget-level event from the ui crate.
    Ui(UiEvent),
    ImagesPicked(Result<Vec<(String, u32, u32)>, String>),
    EngineReady(Result<Engine, String>),
    OcrFinished(usize, Result<Vec<NewEntry>, String>),
    InpaintEngineReady(Result<InpaintEngine, String>),
    InpaintFinished(usize, Result<Vec<(image::RgbaImage, [f32; 4])>, String>),
    FontLoaded,
    ThumbDecoded(usize, Result<Arc<DecodedPage>, String>),
    FullDecoded(usize, Result<Arc<DecodedPage>, String>),
    /// The settle debounce elapsed for generation `u64`; stale generations
    /// (a newer scroll already happened) are ignored.
    SettleElapsed(u64),
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
    inpaint_engine: Option<InpaintEngine>,
    /// Buffered inpainting job waiting for the engine to finish loading,
    /// as `(image index, path, rect, mask quads)`.
    pending_inpaint: Option<(usize, String, [f32; 4], Vec<Quad>)>,
    /// True while an inpainting inference is in flight.
    pub(crate) inpainting: bool,
    /// The image whose tile is in inpainting selection mode.
    pub(crate) inpaint_mode: Option<usize>,
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
    /// True while the settings modal is open.
    pub(crate) settings_open: bool,
    /// The settings tab shown inside the modal.
    pub(crate) settings_tab: SettingsTab,
    /// The currently selected overlay entry as `(image index, entry id)`;
    /// the style panel edits exactly this entry and nothing else.
    pub(crate) selected: Option<(usize, EntryId)>,
    /// The entry being edited inline as `(image index, entry id)`; `None`
    /// when no inline edit is active.
    pub(crate) editing: Option<(usize, EntryId)>,
    /// Where the active inline edit was started: the overlay floating
    /// editor or the panel's results list row. `clear_editing` resets it.
    pub(crate) editing_origin: EditOrigin,
    /// The multi-line editor buffer backing the inline edit; always `Some`
    /// while `editing` is. Owned here so the widget can mutate it in place.
    pub(crate) edit_content: Option<text_editor::Content>,
    /// True once a keystroke actually changed the edited text. The fork off
    /// the original profile happens exactly once, on this first change: the
    /// double-click itself never forks anything.
    pub(crate) editing_dirty: bool,
    /// Latest viewport rect of the edited entry, in tile viewer coordinates.
    pub(crate) editing_rect: Option<Rectangle>,
    /// Monotonic generation of the settle debounce: bumped on every visible
    /// range change, so stale debounce timers no-op.
    settle_seq: u64,
    /// The visible range whose settle debounce is still pending.
    pending_settle: Option<(u64, Range<usize>)>,
    /// The visible range the last settle backed with full decodes; results
    /// for pages outside its preload window are dropped on arrival.
    settled: Option<Range<usize>>,
    /// Staged style of the selected entry. Mirrors the entry's stored style
    /// on selection; mutations are written back to that entry only.
    pub(crate) style_working: EntryStyle,
    /// The styling color picker currently open (which color field it edits);
    /// `None` means no picker is shown.
    pub(crate) style_picker: Option<StyleField>,
    pub(crate) style_stroke_width: String,
    pub(crate) style_bg_radius: String,
    /// The draggable split between the main area and the side panel.
    pub(crate) panes: pane_grid::State<PaneKind>,
}
impl App {
    fn new() -> Self {
        let style = EntryStyle::default();
        Self {
            images: Vec::new(),
            engine: None,
            cancel: None,
            inpaint_engine: None,
            pending_inpaint: None,
            inpainting: false,
            inpaint_mode: None,
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
            settings_open: false,
            settings_tab: SettingsTab::General,
            selected: None,
            editing: None,
            editing_origin: EditOrigin::Overlay,
            edit_content: None,
            editing_dirty: false,
            editing_rect: None,
            settle_seq: 0,
            pending_settle: None,
            settled: None,
            style_working: style,
            style_picker: None,
            style_stroke_width: style.stroke_width.to_string(),
            style_bg_radius: style.bg_radius.to_string(),
            panes: {
                let (mut panes, main) = pane_grid::State::new(PaneKind::MainArea);
                let (_, split) = panes
                    .split(pane_grid::Axis::Vertical, main, PaneKind::Panel)
                    .expect("initial pane split must succeed");
                panes.resize(split, MAIN_AREA_DEFAULT_RATIO);
                panes
            },
        }
    }
}

/// Starts an inline edit of `(index, id)`: selects the entry, seeds the
/// editor buffer with its displayed text and selects it all so the first
/// keystroke replaces it, then focuses the editor of `origin` (the floating
/// overlay editor or the panel's results-row editor). Shared by the
/// double-click action, the toolbar's "Rename" button and the panel rows.
fn start_inline_edit(app: &mut App, index: usize, id: EntryId, origin: EditOrigin) -> Task<Message> {
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
    Task::batch([iced::widget::operation::focus(focus_id)])
}

/// Clears every piece of inline-editing state in one place.
fn clear_editing(app: &mut App) {
    app.editing = None;
    app.editing_origin = EditOrigin::Overlay;
    app.edit_content = None;
    app.editing_dirty = false;
    app.editing_rect = None;
}

/// Reseeds the style panel inputs from `style`, closing any open picker and
/// keeping the raw number strings in sync with the resolved values.
fn seed_style_inputs(app: &mut App, style: EntryStyle) {
    app.style_working = style;
    app.style_picker = None;
    app.style_stroke_width = style.stroke_width.to_string();
    app.style_bg_radius = style.bg_radius.to_string();
}

/// Converts an RGBA color value to an iced [`Color`].
fn rgba_to_color(rgba: [u8; 4]) -> Color {
    Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3] as f32 / 255.0)
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

    fn style_text_color(&self) -> Color {
        rgba_to_color(self.style_working.text_color)
    }

    fn style_stroke_color(&self) -> Color {
        rgba_to_color(self.style_working.stroke_color)
    }

    fn style_bg_color(&self) -> Color {
        rgba_to_color(self.style_working.bg_color)
    }

    fn style_picker_open(&self) -> Option<StyleField> {
        self.style_picker
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

    fn editing_origin(&self) -> EditOrigin {
        self.editing_origin
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

    fn inpaint_mode(&self) -> Option<usize> {
        self.inpaint_mode
    }

    fn settings_open(&self) -> bool {
        self.settings_open
    }

    fn settings_tab(&self) -> SettingsTab {
        self.settings_tab
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
            decode: PageDecode::default(),
            inpaint: Vec::new(),
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
        assert_eq!(app.editing_origin, EditOrigin::Overlay);
        assert!(app.edit_content.is_some(), "double-click must seed the editor");
    }

    #[test]
    fn panel_edit_forks_on_first_keystroke() {
        let (mut app, id) = app_with_entry();
        let _ = update(&mut app, Message::Ui(UiEvent::PanelEntryEdit((0, id))));

        assert_eq!(app.editing, Some((0, id)));
        assert_eq!(app.editing_origin, EditOrigin::Panel);
        assert_eq!(app.selected, Some((0, id)), "panel edit must select the row");
        type_text(&mut app, "안녕하세요");

        let project = &app.images[0].project;
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

        assert_eq!(app.editing, None);
        assert_eq!(app.editing_origin, EditOrigin::Overlay);
        assert!(app.edit_content.is_none());
        let entry = app.images[0].project.ocr.get(id).unwrap();
        assert_eq!(app.images[0].project.display_text(entry), "hi");
    }

    #[test]
    fn moving_an_entry_updates_view_quad_but_not_the_ocr_quad() {
        use scanlateit_model::Quad;
        let (mut app, id) = app_with_entry();
        let moved = Quad {
            points: [[20.0, 25.0], [40.0, 25.0], [40.0, 35.0], [20.0, 35.0]],
        };
        let _ = update(&mut app, Message::Ui(UiEvent::EntryMoved((0, id, moved))));

        let image = &app.images[0];
        let entry = image.project.ocr.get(id).unwrap();
        assert_eq!(image.project.view_quad(entry), moved);
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
        app.selected = None;

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
    let mut app = App::new();
    app.translate_api_key = crate::settings::Settings::load().api_key;
    (app, font_task)
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

/// True when the quad's bounding box overlaps `rect` (image pixels). Used
/// to pick the text boxes an inpainting range should mask out.
fn quad_intersects_rect(quad: &Quad, rect: [f32; 4]) -> bool {
    let [x0, y0, x1, y1] = quad.bounds();
    !(x1 <= rect[0] || x0 >= rect[0] + rect[2] || y1 <= rect[1] || y0 >= rect[1] + rect[3])
}

/// Spawns one inpainting run: the full-res decode, mask build and LaMa
/// inference all happen on the blocking pool, the UI thread only stores the
/// finished patch.
fn start_inpaint(
    app: &mut App,
    engine: InpaintEngine,
    index: usize,
    path: String,
    rect: [f32; 4],
    quads: Vec<Quad>,
) -> Task<Message> {
    app.inpainting = true;
    app.status = "inpainting...".to_string();
    Task::perform(
        async move {
            let result = tokio::task::spawn_blocking(move || {
                engine.run_blocking(&path, rect, &quads)
            })
            .await
            .unwrap_or_else(|e| Err(format!("inpaint task cancelled: {e}")));
            (index, result)
        },
        |(index, result)| Message::InpaintFinished(index, result),
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

/// The pages a settled visible range gets backed with full decodes: the
/// range itself plus [`DECODE_PRELOAD`] pages on each side.
fn full_window(len: usize, range: &Range<usize>) -> Range<usize> {
    range.start.saturating_sub(DECODE_PRELOAD)
        ..range.end.saturating_add(DECODE_PRELOAD).min(len)
}

/// Decodes `path` through the tokio blocking pool; the CPU-bound decode
/// never starves the runtime's worker threads (timers, message dispatch).
async fn decode_async(path: String, max_edge: u32) -> Result<Arc<DecodedPage>, String> {
    tokio::task::spawn_blocking(move || decode_page(&path, max_edge).map(Arc::new))
        .await
        .map_err(|e| format!("decode task cancelled: {e}"))?
}

/// Spawns full-res decodes for the pending settle window (visible pages
/// first, then preload pages outward) and evicts far-away full caches.
fn settle_full(app: &mut App) -> Task<Message> {
    let Some((_, range)) = app.pending_settle.take() else {
        return Task::none();
    };
    app.settled = Some(range.clone());
    let window = full_window(app.images.len(), &range);
    let mut indices: Vec<usize> = window.clone().collect();
    // Spawn closest-to-visible pages first so the pages under the viewport
    // swap to full-res before the preload padding.
    let center = (range.start + range.end) as f64 / 2.0;
    indices.sort_by_key(|index| {
        let distance = (*index as f64 - center).abs();
        (distance * 1000.0) as u64
    });
    let mut tasks = Vec::new();
    for index in indices {
        let image = &mut app.images[index];
        if matches!(image.decode.thumb, Tier::Failed)
            || !matches!(image.decode.full, Tier::Absent)
        {
            continue;
        }
        image.decode.full = Tier::Decoding;
        let path = image.path.clone();
        tasks.push(Task::perform(
            decode_async(path, MAX_DECODE_EDGE),
            move |result| Message::FullDecoded(index, result),
        ));
    }
    let keep = range.start.saturating_sub(DECODE_PRELOAD + FULL_KEEP_MARGIN)
        ..range
            .end
            .saturating_add(DECODE_PRELOAD + FULL_KEEP_MARGIN)
            .min(app.images.len());
    for (index, image) in app.images.iter_mut().enumerate() {
        if index < keep.start || index >= keep.end {
            image.decode.full = Tier::Absent;
        }
    }
    if tasks.is_empty() {
        Task::none()
    } else {
        Task::batch(tasks)
    }
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
                        decode: PageDecode::default(),
                        inpaint: Vec::new(),
                    });
                }
                app.status = format!("Decoding {} image(s)...", app.images.len());
                let tasks: Vec<Task<Message>> = app
                    .images
                    .iter_mut()
                    .enumerate()
                    .map(|(index, image)| {
                        image.decode.thumb = Tier::Decoding;
                        let path = image.path.clone();
                        Task::perform(
                            decode_async(path, THUMB_DECODE_EDGE),
                            move |result| Message::ThumbDecoded(index, result),
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
        Message::InpaintEngineReady(result) => match result {
            Ok(engine) => {
                app.inpaint_engine = Some(engine.clone());
                match app.pending_inpaint.take() {
                    Some((index, path, rect, quads)) => {
                        start_inpaint(app, engine, index, path, rect, quads)
                    }
                    None => Task::none(),
                }
            }
            Err(e) => {
                app.pending_inpaint = None;
                app.status = e;
                Task::none()
            }
        },
        Message::InpaintFinished(index, result) => {
            app.inpainting = false;
            match result {
                Ok(patches) => {
                    let Some(image) = app.images.get_mut(index) else {
                        return Task::none();
                    };
                    let count = patches.len();
                    for (patch, bounds) in patches {
                        let (width, height) = (patch.width(), patch.height());
                        let layer = InpaintLayer {
                            bounds,
                            handle: Handle::from_rgba(
                                width,
                                height,
                                bytes::Bytes::from(patch.into_raw()),
                            ),
                            width,
                            height,
                        };
                        image.inpaint.push(layer);
                        image
                            .project
                            .extras
                            .inpaint_patches
                            .push(InpaintPatch { bounds });
                    }
                    app.inpaint_mode = None;
                    app.status = format!("Inpainted {count} region(s).");
                }
                Err(e) => {
                    app.status = e;
                }
            }
            Task::none()
        }
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
            app.settle_seq += 1;
            let seq = app.settle_seq;
            app.pending_settle = Some((seq, range));
            Task::perform(
                async move { tokio::time::sleep(SETTLE_DEBOUNCE).await },
                move |_| Message::SettleElapsed(seq),
            )
        }
        Message::SettleElapsed(seq) => {
            let Some((pending_seq, _)) = app.pending_settle.as_ref() else {
                return Task::none();
            };
            if *pending_seq != seq {
                return Task::none();
            }
            settle_full(app)
        }
        Message::Ui(UiEvent::TileScrollEnded) => settle_full(app),
        Message::FullDecoded(index, result) => {
            if index < app.images.len() {
                let keep = app.settled.as_ref().is_some_and(|range| {
                    full_window(app.images.len(), range).contains(&index)
                });
                app.images[index].decode.full = if keep {
                    match result {
                        Ok(decoded) => Tier::Ready(decoded),
                        Err(_) => Tier::Failed,
                    }
                } else {
                    Tier::Absent
                };
            }
            Task::none()
        }
        Message::ThumbDecoded(index, result) => {
            if index < app.images.len() {
                app.images[index].decode.thumb = match result {
                    Ok(decoded) => Tier::Ready(decoded),
                    Err(_) => Tier::Failed,
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
            start_inline_edit(app, index, id, EditOrigin::Overlay)
        }
        Message::Ui(UiEvent::PanelEntryEdit((index, id))) => {
            start_inline_edit(app, index, id, EditOrigin::Panel)
        }
        Message::Ui(UiEvent::Inpaint) => {
            if app.inpainting || app.running || app.translating || app.images.is_empty() {
                return Task::none();
            }
            let index = app.selected.map(|(i, _)| i).unwrap_or(0);
            app.inpaint_mode = if app.inpaint_mode == Some(index) {
                None
            } else {
                Some(index)
            };
            app.status = match app.inpaint_mode {
                Some(_) => "Inpaint mode: drag a rectangle over the text to remove; \
                           click Inpaint again to cancel."
                    .to_string(),
                None => "Inpaint mode cancelled.".to_string(),
            };
            Task::none()
        }
        Message::Ui(UiEvent::EntryToolbar((index, id, action))) => match action {
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
        },
        Message::Ui(UiEvent::EntryMoved((index, id, quad))) => {
            if let Some(image) = app.images.get_mut(index) {
                image.project.set_view_quad(id, quad);
            }
            Task::none()
        }
        Message::Ui(UiEvent::InpaintSelection((index, rect))) => {
            if app.inpainting || app.running || app.translating {
                return Task::none();
            }
            let rect = [rect.x, rect.y, rect.width, rect.height];
            let Some(image) = app.images.get(index) else {
                return Task::none();
            };
            let quads: Vec<Quad> = image
                .project
                .ocr
                .all()
                .map(|entry| image.project.view_quad(entry))
                .filter(|quad| quad_intersects_rect(quad, rect))
                .collect();
            if quads.is_empty() {
                app.status = "Inpaint: no OCR boxes in the range; the whole selection \
                              will be cleaned."
                    .to_string();
            }
            let path = image.path.clone();
            match app.inpaint_engine.clone() {
                Some(engine) => start_inpaint(app, engine, index, path, rect, quads),
                None => {
                    app.pending_inpaint = Some((index, path, rect, quads));
                    app.status = "Loading the inpainting model...".to_string();
                    Task::perform(
                        async move { InpaintEngine::build() },
                        Message::InpaintEngineReady,
                    )
                }
            }
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
        Message::Ui(UiEvent::StyleColorOpen(field)) => {
            app.style_picker = Some(field);
            Task::none()
        }
        Message::Ui(UiEvent::StyleColorCancel(field)) => {
            app.style_picker = None;
            Task::none()
        }
        Message::Ui(UiEvent::StyleColorSubmit(field, color)) => {
            app.style_picker = None;
            let Some((index, id)) = app.selected else { return Task::none() };
            let rgba = color.into_rgba8();
            match field {
                StyleField::Text => app.style_working.text_color = rgba,
                StyleField::Stroke => app.style_working.stroke_color = rgba,
                StyleField::Background => app.style_working.bg_color = rgba,
            }
            app.images[index].project.set_entry_style(id, app.style_working);
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
        Message::Ui(UiEvent::PanelResized(resized)) => {
            app.panes.resize(resized.split, resized.ratio);
            Task::none()
        }
        Message::Ui(UiEvent::SettingsOpen) => {
            app.settings_open = true;
            Task::none()
        }
        Message::Ui(UiEvent::SettingsClose) => {
            app.settings_open = false;
            let settings = crate::settings::Settings {
                api_key: app.translate_api_key.clone(),
            };
            if let Err(e) = settings.save() {
                app.status = e;
            }
            Task::none()
        }
        Message::Ui(UiEvent::SettingsTab(tab)) => {
            app.settings_tab = tab;
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
    let grid: Element<'_, UiEvent> = pane_grid::PaneGrid::new(&app.panes, |_, kind, _| {
        pane_grid::Content::new(match kind {
            PaneKind::MainArea => main_area::view(app),
            PaneKind::Panel => panel::view(app),
        })
    })
    .spacing(2)
    .min_size(160)
    .on_resize(8, UiEvent::PanelResized)
    .width(Length::Fill)
    .height(Length::Fill)
    .into();
    let content: Element<'_, UiEvent> = iced::widget::row![toolbar::view(app), grid]
        .spacing(2)
        .height(Length::Fill)
        .into();
    let view: Element<'_, UiEvent> = if app.settings_open {
        settings_modal::view(app, content)
    } else {
        content
    };
    Element::map(view, Message::from)
}
