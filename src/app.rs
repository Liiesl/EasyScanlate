use std::collections::VecDeque;
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use iced::advanced::widget::operation::{self as widget_op, Operation, Outcome, Scrollable};
use iced::advanced::widget::{operate, Id as WidgetId};
use iced::widget::image::Handle;
use iced::widget::operation::AbsoluteOffset;
use iced::widget::{pane_grid, text_editor};
use iced::futures::{SinkExt, StreamExt};
use iced::{Color, Element, Font, Length, Rectangle, Subscription, Task, Vector};

use scanlateit_inpaint::Engine as InpaintEngine;
use scanlateit_model::{EntryId, EntryStyle, InpaintPatch, ProfileId, Project, Quad};
use scanlateit_ocr::{self as ocr, Engine, OcrCancellationToken, ParallelEngine};
use scanlateit_styling::Engine as StylingEngine;
use scanlateit_translation as translation;
use scanlateit_ui::loaded::InpaintLayer;
use scanlateit_ui::main_area::decode::{
    decode_page, DecodedPage, PageDecode, Tier, MAX_DECODE_EDGE, THUMB_DECODE_EDGE,
};
use scanlateit_ui::panel::results::{PANEL_LIST_ID, panel_row_id};
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

/// Number of preset slots seeded in the app: the five built-in styles
/// plus three empty slots. The "+" tile fills the first empty slot or
/// appends a new one when all are full, so the list can grow past this.
const INITIAL_PRESET_SLOTS: usize = 8;

/// The pane the side panel occupies at launch: ~74% of the default window
/// width (about 1036px of the 1400px window), leaving the main area a third
/// of its previous ~1120px default.
const MAIN_AREA_DEFAULT_RATIO: f32 = 0.26;

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
    ParallelEngineReady(Result<ParallelEngine, String>),
    /// One parallel-pipeline OCR run's inference output: raw lines assembled
    /// in the handler, or an already-assembled result from the fallback path
    /// (see [`OcrRunOutcome`]).
    OcrStreamRun(usize, Result<OcrRunOutcome, String>),
    /// The OCR stream ended without delivering every run (pipeline error or
    /// cancellation); the handler finalizes the run.
    OcrStreamFailed(String),
    /// Frame tick while the OCR stream is running: keeps the iced frame loop
    /// alive so queued `OcrStreamRun` messages are drained per run.
    OcrTick,
    InpaintEngineReady(Result<InpaintEngine, String>),
    InpaintFinished(usize, Result<Vec<(image::RgbaImage, [f32; 4])>, String>),
    StylingEngineReady(Result<StylingEngine, String>),
    StyleDetected(usize, EntryId, Result<EntryStyle, String>),
    FontLoaded,
    ThumbDecoded(usize, Result<Arc<DecodedPage>, String>),
    FullDecoded(usize, Result<Arc<DecodedPage>, String>),
    /// The settle debounce elapsed for generation `u64`; stale generations
    /// (a newer scroll already happened) are ignored.
    SettleElapsed(u64),
    /// A request to (re)fetch the translation model lists from the models
    /// mirror; handled the same way as the boot fetch.
    FetchModels,
    /// The fetched translation gateway configs, keyed by provider id (each
    /// already filtered and sorted, or the fallback on failure).
    ModelsFetched(std::collections::HashMap<String, translation::Provider>),
    TranslateFinished(
        Vec<(usize, EntryId, String, String)>,
        Result<Vec<String>, String>,
    ),
}

impl From<UiEvent> for Message {
    fn from(event: UiEvent) -> Self {
        Message::Ui(event)
    }
}

/// One OCR run's inference outcome from the parallel pipeline.
#[derive(Debug, Clone)]
pub enum OcrRunOutcome {
    /// Raw lines plus the canvas metrics the handler needs for assembly.
    /// The handler resolves the previous run's boundary candidates, dedups
    /// against committed quads, distributes to pages and commits — strictly
    /// in run order, on the UI thread where the committed state is
    /// authoritative.
    Canvas {
        width: u32,
        margin_top: u32,
        lines: Vec<ocr::OcrLine>,
    },
    /// Fallback of the undecodable-page path: per-page entries ready to
    /// commit directly, with no boundary candidates (the previous run's held
    /// candidates are flushed instead of resolved).
    Fallback(ocr::RunResult),
}

/// Session state: one loaded image plus everything iced/OCR related that the
/// model doesn't know about (engine handle, per-image canvas cache).
pub struct App {
    pub(crate) images: Vec<LoadedImage>,
    engine: Option<Engine>,
    /// The parallel OCR pipeline (K detection workers + one recognition
    /// worker). Built lazily on first OCR run; dropped (workers exit) when a
    /// run finishes, so the next run rebuilds it fresh.
    pipeline: Option<ParallelEngine>,
    cancel: Option<OcrCancellationToken>,
    /// Number of parallel OCR detection workers, as typed in the settings
    /// modal; parsed (fallback 2) when OCR starts.
    ocr_workers: String,
    /// The current run plan, indexed by run number; the stream closure
    /// captures a clone, the handler reads it back for assembly.
    ocr_plans: Vec<ocr::RunPlan>,
    /// All images' `(width, height)` of the current run, in image order.
    ocr_dims: Vec<(u32, u32)>,
    inpaint_engine: Option<InpaintEngine>,
    /// Buffered inpainting job waiting for the engine to finish loading,
    /// as `(image index, path, rect, mask quads)`.
    pending_inpaint: Option<(usize, String, [f32; 4], Vec<Quad>)>,
    /// True while an inpainting inference is in flight.
    pub(crate) inpainting: bool,
    /// The image whose tile is in inpainting selection mode.
    pub(crate) inpaint_mode: Option<usize>,
    /// The shared ONNX text-styling classifier, built lazily on first use.
    styling_engine: Option<StylingEngine>,
    /// True when a styling classification is queued but the engine is still
    /// loading; jobs start once `StylingEngineReady` arrives.
    styling_pending: bool,
    /// When enabled, newly OCR-detected entries are auto-classified and their
    /// style set from the prediction.
    pub(crate) auto_style_detect: bool,
    /// `(image index, entry id)` pairs whose style has already been set by
    /// the classifier, so auto-run never overrides a manual tweak.
    styled: std::collections::HashSet<(usize, EntryId)>,
    pub(crate) running: bool,
    pub(crate) font: Option<Font>,
    pub(crate) status: String,
    pending: usize,
    ocr_total: usize,
    ocr_failed: usize,
    ocr_cancelled: bool,
    /// Total OCR runs in the current run's plan; `pending` counts down from
    /// this (one run may cover several images).
    ocr_runs: usize,
    /// Boundary candidates of the last completed run, awaiting the next run's
    /// re-detections in its top margin (see [`ocr::resolve_boundary`]). Taken
    /// by the next run's task; flushed if the run fails or is cancelled so no
    /// captured bubble is lost.
    held_boundary: Option<ocr::BoundaryState>,
    pub(crate) translating: bool,
    pub(crate) translate_provider: String,
    /// All translation gateways offered in the UI (provider ids).
    pub(crate) translate_providers: Vec<String>,
    /// Fetched gateway configs (api base, key env var, model ids), keyed by
    /// provider id; every [`translation::PROVIDERS`] id is always present.
    pub(crate) translate_providers_map: std::collections::HashMap<String, translation::Provider>,
    pub(crate) translate_model: String,
    pub(crate) translate_lang: String,
    pub(crate) translate_api_key: String,
    /// Selectable translation models of the current provider (fetched from
    /// the models mirror, with [`translation::MODELS`] as the offline
    /// fallback).
    pub(crate) translate_models: Vec<String>,
    /// When enabled, only free models are offered in the picker.
    pub(crate) free_models_only: bool,
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
    /// The style presets offered in the styling panel, in memory only: a
    /// list of slots, `None` for an empty slot. The "+" swatch fills the
    /// first empty slot with a copy of the current working style (or
    /// appends a new one when all are full), clicking a filled swatch
    /// applies its style to the selected entry, and the right-click menu
    /// replaces or empties a slot.
    pub(crate) style_presets: Vec<Option<EntryStyle>>,
    /// The draggable split between the main area and the side panel.
    pub(crate) panes: pane_grid::State<PaneKind>,
}
impl App {
    fn new() -> Self {
        let style = EntryStyle::default();
        Self {
            images: Vec::new(),
            engine: None,
            pipeline: None,
            cancel: None,
            ocr_workers: "2".to_string(),
            ocr_plans: Vec::new(),
            ocr_dims: Vec::new(),
            inpaint_engine: None,
            pending_inpaint: None,
            inpainting: false,
            inpaint_mode: None,
            styling_engine: None,
            styling_pending: false,
            auto_style_detect: false,
            styled: std::collections::HashSet::new(),
            running: false,
            font: None,
            status: "Idle - open images to begin.".to_string(),
            pending: 0,
            ocr_total: 0,
            ocr_failed: 0,
            ocr_cancelled: false,
            ocr_runs: 0,
            held_boundary: None,
            translating: false,
            translate_provider: translation::PROVIDERS[0].to_string(),
            translate_providers: translation::PROVIDERS
                .iter()
                .map(|p| (*p).to_string())
                .collect(),
            translate_providers_map: std::collections::HashMap::new(),
            translate_model: translation::MODELS[0].to_string(),
            translate_lang: translation::LANGUAGES[0].to_string(),
            translate_api_key: String::new(),
            translate_models: translation::MODELS
                .iter()
                .map(|m| (*m).to_string())
                .collect(),
            free_models_only: false,
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
            style_presets: {
                let mut presets = Vec::with_capacity(INITIAL_PRESET_SLOTS);
                let mut preset = EntryStyle::default();
                presets.push(Some(preset));
                preset.bg_color = [0, 0, 0, 255];
                preset.text_color = [255, 255, 255, 255];
                presets.push(Some(preset));
                preset.bg_color = [0, 0, 0, 0];
                preset.text_color = [0, 0, 0, 255];
                presets.push(Some(preset));
                preset.text_color = [255, 255, 255, 255];
                presets.push(Some(preset));
                preset.bg_color = [255, 0, 0, 255];
                presets.push(Some(preset));
                presets.resize(INITIAL_PRESET_SLOTS, None);
                presets
            },
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

/// The file name (last path component) of an image path, stripped of both
/// separator styles; used as the file tag in translation requests.
fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
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
        // Editing started in the main area: make sure the panel shows the row.
        tasks.push(panel_scroll_task(index, id));
    }
    Task::batch(tasks)
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

/// Selects `(index, id)`: seeds the style inputs and, when the entry's page
/// is outside the currently settled decode window (a panel-driven reveal
/// moved the viewport without a `TilesVisible` event), schedules a full-res
/// settle for that page.
fn select_entry(app: &mut App, index: usize, id: EntryId) -> Task<Message> {
    app.selected = Some((index, id));
    seed_style_inputs(app, app.images[index].project.entry_style(id));
    let needs_settle = app.settled.as_ref().is_none_or(|range| !range.contains(&index));
    if needs_settle {
        schedule_settle(app, index..index + 1)
    } else {
        Task::none()
    }
}

/// Points the model picker at the current provider's (already fetched)
/// model list, falling back to the offline list if the provider is missing.
/// The selected model is reset when it is no longer on the list. When
/// `free_models_only` is enabled, only free models are offered.
fn sync_translate_models(app: &mut App) {
    let free_only = app.free_models_only;
    let models: Vec<String> = app
        .translate_providers_map
        .get(&app.translate_provider)
        .map(|provider| {
            provider
                .models
                .iter()
                .filter(|model| !free_only || model.free)
                .map(|model| model.id.clone())
                .collect()
        })
        .unwrap_or_else(|| {
            translation::MODELS
                .iter()
                .filter(|m| !free_only || translation::MODELS_FREE.contains(m))
                .map(|m| (*m).to_string())
                .collect()
        });
    if models.is_empty() {
        return;
    }
    app.translate_models = models;
    if !app.translate_models.contains(&app.translate_model) {
        app.translate_model = app.translate_models[0].clone();
    }
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

    fn translate_provider(&self) -> &String {
        &self.translate_provider
    }

    fn translate_providers(&self) -> &[String] {
        &self.translate_providers
    }

    fn translate_model(&self) -> &String {
        &self.translate_model
    }

    fn translate_models(&self) -> &[String] {
        &self.translate_models
    }

    fn translate_lang(&self) -> &str {
        &self.translate_lang
    }

    fn translate_api_key(&self) -> &str {
        &self.translate_api_key
    }

    fn free_models_only(&self) -> bool {
        self.free_models_only
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

    fn style_presets(&self) -> &[Option<EntryStyle>] {
        &self.style_presets
    }

    fn auto_style_detect(&self) -> bool {
        self.auto_style_detect
    }

    fn ocr_workers(&self) -> &str {
        &self.ocr_workers
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

    #[test]
    fn seeded_presets_cover_the_expected_variants() {
        let app = App::new();
        let presets = &app.style_presets;
        assert_eq!(presets.len(), INITIAL_PRESET_SLOTS);
        let filled: Vec<&EntryStyle> = presets.iter().flatten().collect();
        assert_eq!(filled.len(), 5);
        assert_eq!(filled[0].bg_color, [255, 255, 255, 255], "white bg");
        assert_eq!(filled[0].text_color, [0, 0, 0, 255], "black text");
        assert_eq!(filled[1].bg_color, [0, 0, 0, 255], "inverse: black bg");
        assert_eq!(filled[1].text_color, [255, 255, 255, 255], "inverse: white text");
        assert_eq!(filled[2].bg_color, [0, 0, 0, 0], "transparent bg");
        assert_eq!(filled[2].text_color, [0, 0, 0, 255], "black text");
        assert_eq!(filled[3].bg_color, [0, 0, 0, 0], "transparent bg");
        assert_eq!(filled[3].text_color, [255, 255, 255, 255], "white text");
        assert_eq!(filled[4].bg_color, [255, 0, 0, 255], "red bg");
        assert_eq!(filled[4].text_color, [255, 255, 255, 255], "white text");
        assert!(presets[5..].iter().all(|slot| slot.is_none()), "last slots empty");
    }

    #[test]
    fn applying_a_preset_seeds_working_style_and_entry() {
        let (mut app, id) = app_with_entry();
        app.selected = Some((0, id));
        app.style_working.bg_color = [1, 2, 3, 255];
        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetApply(1)));

        let preset = app.style_presets[1].expect("preset 1 seeded");
        assert_eq!(app.style_working, preset);
        assert_eq!(app.images[0].project.entry_style(id), preset);
        assert_eq!(app.style_bg_radius, preset.bg_radius.to_string());
    }

    #[test]
    fn applying_a_preset_without_selection_or_out_of_range_is_a_noop() {
        let (mut app, _id) = app_with_entry();
        app.style_working.bg_color = [1, 2, 3, 255];
        app.selected = None;
        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetApply(0)));
        assert_eq!(app.style_working.bg_color, [1, 2, 3, 255]);

        app.selected = Some((0, app.images[0].project.ocr.visible().next().unwrap().id));
        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetApply(999)));
        assert_eq!(app.style_working.bg_color, [1, 2, 3, 255]);
    }

    #[test]
    fn applying_an_empty_preset_slot_is_a_noop() {
        let (mut app, id) = app_with_entry();
        app.selected = Some((0, id));
        app.style_working.bg_color = [1, 2, 3, 255];
        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetApply(5)));

        assert_eq!(app.style_working.bg_color, [1, 2, 3, 255]);
        assert_eq!(app.images[0].project.entry_style(id).bg_color, [1, 2, 3, 255]);
    }

    #[test]
    fn add_preset_fills_the_first_empty_slot() {
        let (mut app, _id) = app_with_entry();
        app.style_working.bg_color = [9, 9, 9, 255];
        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetAdd));
        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetAdd));
        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetAdd));

        assert_eq!(app.style_presets.len(), INITIAL_PRESET_SLOTS);
        assert_eq!(app.style_presets[5], Some(app.style_working));
        assert_eq!(app.style_presets[6], Some(app.style_working));
        assert_eq!(app.style_presets[7], Some(app.style_working));
    }

    #[test]
    fn add_preset_appends_when_all_slots_are_full() {
        let (mut app, _id) = app_with_entry();
        app.style_presets = (0..INITIAL_PRESET_SLOTS)
            .map(|i| {
                let mut style = EntryStyle::default();
                style.text_color = [i as u8, 0, 0, 255];
                Some(style)
            })
            .collect();
        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetAdd));

        assert_eq!(app.style_presets.len(), INITIAL_PRESET_SLOTS + 1);
        assert_eq!(app.style_presets.last().unwrap(), &Some(app.style_working));
    }

    #[test]
    fn add_preset_refills_an_emptied_slot_before_appending() {
        let (mut app, _id) = app_with_entry();
        app.style_working.text_color = [7, 7, 7, 255];
        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetRemove(2)));
        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetAdd));

        assert_eq!(app.style_presets.len(), INITIAL_PRESET_SLOTS);
        assert_eq!(app.style_presets[2], Some(app.style_working));
    }

    #[test]
    fn replace_preset_overwrites_filled_and_empty_slots() {
        let (mut app, _id) = app_with_entry();
        app.style_working.text_color = [42, 0, 0, 255];

        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetReplace(1)));
        assert_eq!(app.style_presets[1], Some(app.style_working));

        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetReplace(6)));
        assert_eq!(app.style_presets[6], Some(app.style_working));

        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetReplace(999)));
        assert_eq!(app.style_presets.len(), INITIAL_PRESET_SLOTS);
    }

    #[test]
    fn remove_preset_empties_the_slot() {
        let (mut app, _id) = app_with_entry();
        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetRemove(0)));
        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetRemove(999)));

        assert!(app.style_presets[0].is_none());
        assert_eq!(app.style_presets.len(), INITIAL_PRESET_SLOTS);
    }
}

pub fn boot() -> (App, Task<Message>) {
    let font_task = match std::fs::read(KOREAN_FONT_PATH) {
        Ok(bytes) => iced::font::load(bytes).map(|_| Message::FontLoaded),
        Err(_) => Task::none(),
    };
    let mut app = App::new();
    let settings = crate::settings::Settings::load();
    app.translate_api_key = settings.api_key;
    app.auto_style_detect = settings.auto_style_detect;
    app.ocr_workers = settings.ocr_workers.to_string();
    app.free_models_only = settings.free_models_only;
    let models_task = Task::perform(translation::fetch_all_providers(), Message::ModelsFetched);
    (app, Task::batch([font_task, models_task]))
}

/// Spawns the parallel OCR stream for the whole planned run set. The stream
/// keeps a window of canvases in flight (one ahead per detection worker):
/// canvases are built and submitted as results arrive, and per-run results
/// are yielded in run order to the UI. Only the inference (detect + recognize
/// on a stitched canvas) runs on the pipeline's worker threads; assembly —
/// boundary resolution, dedup, distribution and commit — happens in the
/// `OcrStreamRun` handler, on the UI thread, where the committed state is
/// authoritative. The old [`Engine`] is kept for the undecodable-page
/// fallback path only.
fn start_ocr_stream(app: &mut App) -> Task<Message> {
    let pipeline = app
        .pipeline
        .clone()
        .expect("pipeline must be built before starting the stream");
    let fallback = app.engine.clone().expect("engine must be built");
    let token = app
        .cancel
        .clone()
        .expect("cancellation token set before starting the stream");
    let runs = app.ocr_plans.clone();
    let dims = app.ocr_dims.clone();
    let paths: Vec<Vec<String>> = runs
        .iter()
        .map(|run| (run.page_start..=run.page_end).map(|i| app.images[i].path.clone()).collect())
        .collect();
    let above_paths: Vec<Option<String>> = runs
        .iter()
        .map(|run| run.above.map(|(page, _)| app.images[page].path.clone()))
        .collect();
    let below_paths: Vec<Option<String>> = runs
        .iter()
        .map(|run| run.below.map(|(page, _)| app.images[page].path.clone()))
        .collect();
    let workers = app.ocr_workers.parse::<usize>().unwrap_or(2).max(1);
    let window = workers + 1;
    let total = runs.len();
    Task::stream(
        iced::stream::try_channel(1, move |mut sender| async move {
            let mut dispatched = 0usize;
            let mut pending = total;
            let mut in_flight = 0usize;
            let mut canvas_meta: VecDeque<(u32, u32)> = VecDeque::new();
            // Fill the submission window before the first receive.
            while dispatched < total && in_flight < window {
                let outcome = dispatch_run(
                    &mut sender,
                    &pipeline,
                    &fallback,
                    &token,
                    dispatched,
                    &runs,
                    &paths,
                    &above_paths,
                    &below_paths,
                    &dims,
                    &mut canvas_meta,
                    &mut in_flight,
                )
                .await;
                match outcome {
                    Ok(true) => {}
                    // The UI channel closed; the app is gone or restarting.
                    Ok(false) => return Ok(()),
                    Err(e) => return Err(e),
                }
                dispatched += 1;
            }
            while pending > 0 {
                let (idx, lines) = match pipeline.recv() {
                    Ok(received) => received,
                    Err(e) => return Err(e),
                };
                pending -= 1;
                in_flight -= 1;
                let (width, margin_top) = canvas_meta
                    .pop_front()
                    .expect("canvas metadata arrives in submission order");
                let send_t0 = std::time::Instant::now();
                if sender
                    .send(Message::OcrStreamRun(
                        idx,
                        Ok(OcrRunOutcome::Canvas {
                            width,
                            margin_top,
                            lines,
                        }),
                    ))
                    .await
                    .is_err()
                {
                    return Ok(());
                }
                eprintln!(
                    "[ocr-stream] sent run {idx} to UI channel, send took {:?}",
                    send_t0.elapsed()
                );
                if dispatched < total {
                    let outcome = dispatch_run(
                        &mut sender,
                        &pipeline,
                        &fallback,
                        &token,
                        dispatched,
                        &runs,
                        &paths,
                        &above_paths,
                        &below_paths,
                        &dims,
                        &mut canvas_meta,
                        &mut in_flight,
                    )
                    .await;
                    match outcome {
                        Ok(true) => {}
                        Ok(false) => return Ok(()),
                        Err(e) => return Err(e),
                    }
                    dispatched += 1;
                }
            }
            Ok(())
        })
        .map(|item| match item {
            Ok(message) => message,
            Err(e) => Message::OcrStreamFailed(e),
        }),
    )
}

/// What [`build_canvas`] produced for one run.
enum CanvasBuild {
    /// The stitched canvas plus its `(width, margin_top)` for submission.
    Ready(image::RgbImage, u32, u32),
    /// The undecodable-page fallback result, ready to commit directly.
    Fallback(ocr::RunResult),
}

/// Builds the stitched canvas of one run: its pages stacked at the first
/// page's width with the margin strips of the neighboring content above and
/// below (see [`ocr::stack_run`]). When a page fails to decode, falls back
/// to raw per-page OCR through the old engine and returns the ready-to-commit
/// result instead (no canvas, no boundary candidates).
fn build_canvas(
    fallback: &Engine,
    token: &OcrCancellationToken,
    index: usize,
    run: &ocr::RunPlan,
    paths: &[String],
    above_path: Option<&str>,
    below_path: Option<&str>,
    dims: &[(u32, u32)],
) -> Result<CanvasBuild, String> {
    let mut loaded = Vec::with_capacity(paths.len());
    for path in paths {
        match ocr::load_rgb(path) {
            Some(image) => loaded.push(image),
            None => {
                eprintln!(
                    "[ocr-run {index}] undecodable page {path}: falling back to per-page OCR"
                );
                let mut out = Vec::with_capacity(paths.len());
                for (offset, path) in paths.iter().enumerate() {
                    match fallback.run_path_cancellable(path, token) {
                        Ok(lines) => out.push((run.page_start + offset, ocr::to_entries(lines))),
                        Err(e) => return Err(e),
                    }
                }
                return Ok(CanvasBuild::Fallback(ocr::RunResult {
                    per_page: out,
                    held: None,
                }));
            }
        }
    }
    let width = loaded[0].width();
    let body_h = ocr::body_height(&dims[run.page_start..=run.page_end], width, run.band);
    let margin = (ocr::STITCH_MARGIN_RATIO * body_h as f32).round().max(1.0) as u32;
    let above = match (above_path, run.above) {
        (Some(path), Some((_, band))) => ocr::load_rgb(path)
            .and_then(|image| ocr::top_margin_strip(&image, band, width, margin)),
        _ => None,
    };
    let below = match (below_path, run.below) {
        (Some(path), Some((_, band))) => ocr::load_rgb(path)
            .and_then(|image| ocr::bottom_margin_strip(&image, band, width, margin)),
        _ => None,
    };
    let margin_top = above.as_ref().map_or(0, |strip| strip.height());
    let canvas = ocr::stack_run(above, &loaded, below, width, run.band);
    Ok(CanvasBuild::Ready(canvas, width, margin_top))
}

/// Builds and dispatches one run: submits its canvas to the pipeline, or —
/// when the run took the fallback path — emits its ready result to the UI
/// directly. Returns `false` when the UI channel closed (the stream should
/// end quietly), `true` otherwise.
async fn dispatch_run(
    sender: &mut iced::futures::channel::mpsc::Sender<Message>,
    pipeline: &ParallelEngine,
    fallback: &Engine,
    token: &OcrCancellationToken,
    index: usize,
    runs: &[ocr::RunPlan],
    paths: &[Vec<String>],
    above_paths: &[Option<String>],
    below_paths: &[Option<String>],
    dims: &[(u32, u32)],
    canvas_meta: &mut VecDeque<(u32, u32)>,
    in_flight: &mut usize,
) -> Result<bool, String> {
    let run = &runs[index];
    match build_canvas(
        fallback,
        token,
        index,
        run,
        &paths[index],
        above_paths[index].as_deref(),
        below_paths[index].as_deref(),
        dims,
    ) {
        Ok(CanvasBuild::Ready(canvas, width, margin_top)) => {
            canvas_meta.push_back((width, margin_top));
            pipeline
                .submit(index, canvas)
                .map_err(|e| format!("OCR pipeline submit failed: {e}"))?;
            *in_flight += 1;
            Ok(true)
        }
        Ok(CanvasBuild::Fallback(result)) => {
            let sent = sender
                .send(Message::OcrStreamRun(
                    index,
                    Ok(OcrRunOutcome::Fallback(result)),
                ))
                .await;
            Ok(sent.is_ok())
        }
        Err(e) => Err(e),
    }
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

/// Spawns per-entry style-classification jobs for every visible entry whose
/// style has not already been set by the classifier. Builds the engine lazily
/// on first use; queued jobs start when the engine finishes loading.
fn classify_entries(app: &mut App) -> Task<Message> {
    match app.styling_engine.clone() {
        Some(engine) => start_style_jobs(app, engine),
        None => {
            app.styling_pending = true;
            app.status = "Loading the styling model...".to_string();
            Task::perform(async move { StylingEngine::build() }, Message::StylingEngineReady)
        }
    }
}

/// Spawns one classification `Task` per unclassified visible entry, each on
/// the blocking pool. Entries are marked in-flight up front so a later
/// auto-run never schedules the same entry twice.
fn start_style_jobs(app: &mut App, engine: StylingEngine) -> Task<Message> {
    // Immutable reference captured by the inner closures so a `move` does not
    // move `app` itself; disjoint-field borrows of the images and the set
    // coexist until `collect` finishes.
    let styled = &app.styled;
    let jobs: Vec<(usize, EntryId, String, Quad)> = app
        .images
        .iter()
        .enumerate()
        .flat_map(|(index, image)| {
            image
                .project
                .ocr
                .visible()
                .filter(move |entry| !styled.contains(&(index, entry.id)))
                .map(move |entry| {
                    (
                        index,
                        entry.id,
                        image.path.clone(),
                        image.project.view_quad(entry),
                    )
                })
        })
        .collect();
    if jobs.is_empty() {
        return Task::none();
    }
    for (index, id, _, _) in &jobs {
        app.styled.insert((*index, *id));
    }
    let tasks: Vec<Task<Message>> = jobs
        .into_iter()
        .map(|(index, id, path, quad)| {
            let engine = engine.clone();
            Task::perform(
                async move {
                    let classified = tokio::task::spawn_blocking(move || {
                        engine.predict_entry(&path, &quad).map(|pred| {
                            pred.to_entry_style(scanlateit_model::EntryStyle::default())
                        })
                    })
                    .await
                    .unwrap_or_else(|e| Err(format!("styling task cancelled: {e}")));
                    (index, id, classified)
                },
                |(index, id, result)| Message::StyleDetected(index, id, result),
            )
        })
        .collect();
    Task::batch(tasks)
}

/// Assembles one run's raw lines into a commit-ready [`ocr::RunResult`],
/// strictly in run order on the UI thread: merges nearby boxes, resolves the
/// previous run's held boundary candidates against this run's re-detections
/// in its top margin, dedups against the committed quads of the page above,
/// distributes the survivors to their pages and holds this run's own
/// boundary candidates for the next run. Reads the authoritative committed
/// state (`app.held_boundary`, `app.images[..].project.ocr`).
fn assemble_run(
    app: &mut App,
    index: usize,
    width: u32,
    margin_top: u32,
    lines: Vec<ocr::OcrLine>,
) -> ocr::RunResult {
    let run = app.ocr_plans[index];
    let run_dims: Vec<(usize, u32, u32)> = (run.page_start..=run.page_end)
        .map(|i| (i, app.ocr_dims[i].0, app.ocr_dims[i].1))
        .collect();
    let prev_held = app.held_boundary.take();
    let prev_data = run.dedup.map(|(page, offset)| {
        let quads: Vec<Quad> = app.images[page]
            .project
            .ocr
            .all()
            .map(|entry| entry.quad)
            .collect();
        (quads, app.images[page].width as u32, offset)
    });
    let merged = ocr::merge(lines, ocr::MergeConfig::default());
    let (resolved, kept) = match &prev_held {
        Some(state) => {
            let transformed = ocr::transform_candidates(
                &state.candidates,
                state.width,
                state.boundary,
                width,
                margin_top,
            );
            let resolution = ocr::resolve_boundary(&state.candidates, &transformed, merged);
            (resolution.append, resolution.kept)
        }
        None => (Vec::new(), merged),
    };
    let deduped = match &prev_data {
        Some((quads, prev_width, offset)) => {
            ocr::dedup_with_previous(kept, quads, *prev_width, *offset, width)
        }
        None => kept,
    };
    let out = ocr::distribute(deduped, &run_dims, run.band, margin_top);
    let mut per_page = out.per_page;
    for candidate in resolved {
        match per_page
            .iter_mut()
            .find(|(page, _)| *page == candidate.page)
        {
            Some((_, entries)) => entries.push(candidate.entry),
            None => per_page.push((candidate.page, vec![candidate.entry])),
        }
    }
    per_page.sort_by_key(|(page, _)| *page);
    eprintln!(
        "[ocr-run {index}] final per-page: {:?}",
        per_page
            .iter()
            .map(|(p, e)| format!("p{p}:{}", e.len()))
            .collect::<Vec<_>>()
    );
    let held = (!out.held.is_empty()).then(|| ocr::BoundaryState {
        candidates: out.held,
        width,
        boundary: out.boundary,
    });
    ocr::RunResult { per_page, held }
}

/// Commits one run's result: stores the boundary candidates for the next run
/// and appends the per-page entries to their projects, updating `ocr_total`.
fn commit_run_result(app: &mut App, run_result: ocr::RunResult) {
    app.held_boundary = run_result.held;
    for (page, entries) in run_result.per_page {
        let Some(image) = app.images.get_mut(page) else {
            continue;
        };
        app.ocr_total += image.project.append_ocr(entries);
    }
}

/// Appends any boundary candidates still held — their bubbles were captured
/// whole on the page above the seam and must not be lost when the next run
/// fails, is cancelled, never starts or took the fallback path.
fn flush_held_boundary(app: &mut App) {
    if let Some(state) = app.held_boundary.take() {
        for candidate in state.candidates {
            if let Some(image) = app.images.get_mut(candidate.page) {
                app.ocr_total += image.project.append_ocr(vec![candidate.entry]);
            }
        }
    }
}

/// Starts the OCR stream once both engines (the parallel pipeline and the
/// fallback engine) are ready; called from `StartOcr`, `EngineReady` and
/// `ParallelEngineReady`.
fn maybe_start_ocr(app: &mut App) -> Task<Message> {
    if app.running && app.pipeline.is_some() && app.engine.is_some() {
        app.cancel = app
            .pipeline
            .as_ref()
            .map(|pipeline| pipeline.cancellation_token().clone());
        start_ocr_stream(app)
    } else if !app.running {
        // OCR was cancelled while the engines were still loading; drop any
        // freshly built pipeline so its workers exit instead of idling.
        if let Some(pipeline) = app.pipeline.take() {
            pipeline.cancel();
        }
        Task::none()
    } else {
        // Still waiting on the other engine; keep the pipeline.
        Task::none()
    }
}

fn finalize_run(app: &mut App) {
    // Flush any boundary candidates still held — the next run failed, was
    // cancelled or never started: their bubbles were captured whole on the
    // page above the seam and must not be lost.
    flush_held_boundary(app);
    app.running = false;
    app.cancel = None;
    // Drop the pipeline so its worker threads (and ONNX sessions) exit.
    // The next OCR run builds a fresh one.
    app.pipeline = None;
    app.status = if app.ocr_cancelled {
        "OCR cancelled.".to_string()
    } else if app.ocr_failed > 0 {
        format!(
            "OCR done: {} line(s), {} run(s) failed.",
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

/// Schedules the settle debounce for `range` (bumping the generation so
/// stale debounces no-op). Shared by visible-range changes and by the
/// selection-driven reveal (whose viewport change never produces a
/// `TilesVisible` event).
fn schedule_settle(app: &mut App, range: Range<usize>) -> Task<Message> {
    app.settle_seq += 1;
    let seq = app.settle_seq;
    app.pending_settle = Some((seq, range));
    Task::perform(
        async move { tokio::time::sleep(SETTLE_DEBOUNCE).await },
        move |_| Message::SettleElapsed(seq),
    )
}

/// Widget operation: finds the results panel's scrollable and the row
/// container of `(index, id)` during one traversal, then — if that row is
/// not fully visible — chains a `scroll_to` on the panel list (second
/// traversal pass, see the runtime's `Outcome::Chain` handling) that centers
/// the row. All bounds are absolute (iced layouts use absolute coordinates).
struct MeasurePanelRow {
    panel: WidgetId,
    row: WidgetId,
    panel_bounds: Option<Rectangle>,
    panel_offset: f32,
    row_y: Option<f32>,
    row_h: Option<f32>,
}

impl Operation<Message> for MeasurePanelRow {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Message>)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&WidgetId>,
        bounds: Rectangle,
        _content_bounds: Rectangle,
        translation: Vector,
        _state: &mut dyn Scrollable,
    ) {
        if id == Some(&self.panel) {
            self.panel_bounds = Some(bounds);
            self.panel_offset = translation.y;
        }
    }

    fn container(&mut self, id: Option<&WidgetId>, bounds: Rectangle) {
        if id == Some(&self.row) {
            self.row_y = Some(bounds.y);
            self.row_h = Some(bounds.height);
        }
    }

    fn finish(&self) -> Outcome<Message> {
        let (Some(panel), Some(row_y), Some(row_h)) =
            (self.panel_bounds, self.row_y, self.row_h)
        else {
            return Outcome::None;
        };
        let top = row_y - self.panel_offset; // row's window-space top
        let bottom = top + row_h; // row's window-space bottom
        let visible = top >= panel.y && bottom <= panel.y + panel.height;
        if visible {
            return Outcome::None; // already visible: no jump
        }
        let target = (row_y - panel.y - (panel.height - row_h) / 2.0).max(0.0);
        Outcome::Chain(Box::new(widget_op::scrollable::scroll_to(
            self.panel.clone(),
            AbsoluteOffset { x: Some(0.0), y: Some(target) },
        )))
    }
}

/// Scrolls the results list to the row of `(index, id)` if it is out of
/// view; no-op otherwise.
fn panel_scroll_task(index: usize, id: EntryId) -> Task<Message> {
    operate(MeasurePanelRow {
        panel: WidgetId::new(PANEL_LIST_ID),
        row: panel_row_id(index, id),
        panel_bounds: None,
        panel_offset: 0.0,
        row_y: None,
        row_h: None,
    })
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
        Message::FetchModels => {
            Task::perform(translation::fetch_all_providers(), Message::ModelsFetched)
        }
        Message::ModelsFetched(providers) => {
            app.translate_providers_map = providers;
            sync_translate_models(app);
            Task::none()
        }
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
            app.running = true;
            let dims: Vec<(u32, u32)> = app
                .images
                .iter()
                .map(|image| (image.width as u32, image.height as u32))
                .collect();
            let runs = ocr::plan_runs(&dims);
            let run_count = runs.len();
            app.ocr_plans = runs;
            app.ocr_dims = dims;
            app.ocr_runs = run_count;
            app.pending = run_count;
            app.ocr_total = 0;
            app.ocr_failed = 0;
            app.ocr_cancelled = false;
            app.held_boundary = None;
            app.status = format!(
                "Running OCR on {} run(s) covering {} image(s)...",
                run_count,
                app.images.len()
            );
            let mut tasks: Vec<Task<Message>> = Vec::new();
            if app.pipeline.is_none() {
                let workers = app.ocr_workers.parse::<usize>().unwrap_or(2).max(1);
                app.status = format!(
                    "Loading the OCR engine ({workers} detection worker(s))..."
                );
                tasks.push(Task::perform(
                    async move { ParallelEngine::build(workers) },
                    Message::ParallelEngineReady,
                ));
            }
            if app.engine.is_none() {
                tasks.push(Task::perform(
                    async move { Engine::build() },
                    Message::EngineReady,
                ));
            }
            if app.pipeline.is_some() && app.engine.is_some() {
                maybe_start_ocr(app)
            } else if tasks.is_empty() {
                Task::none()
            } else {
                Task::batch(tasks)
            }
        }
        Message::EngineReady(result) => match result {
            Ok(engine) => {
                app.engine = Some(engine.clone());
                maybe_start_ocr(app)
            }
            Err(e) => {
                app.running = false;
                app.status = e;
                Task::none()
            }
        },
        Message::ParallelEngineReady(result) => match result {
            Ok(pipeline) => {
                app.pipeline = Some(pipeline.clone());
                maybe_start_ocr(app)
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
        Message::StylingEngineReady(result) => match result {
            Ok(engine) => {
                app.styling_engine = Some(engine.clone());
                if app.styling_pending {
                    app.styling_pending = false;
                    start_style_jobs(app, engine)
                } else {
                    Task::none()
                }
            }
            Err(e) => {
                app.styling_pending = false;
                app.status = e;
                Task::none()
            }
        },
        Message::StyleDetected(index, id, result) => {
            if let (Some(image), Ok(style)) = (app.images.get_mut(index), result) {
                image.project.set_entry_style(id, style);
                app.styled.insert((index, id));
                app.status = "Applied auto-detected text style.".to_string();
            }
            Task::none()
        }
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
        Message::OcrStreamRun(index, result) => {
            app.pending = app.pending.saturating_sub(1);
            match result {
                Ok(OcrRunOutcome::Canvas {
                    width,
                    margin_top,
                    lines,
                }) => {
                    let run_result = assemble_run(app, index, width, margin_top, lines);
                    commit_run_result(app, run_result);
                }
                Ok(OcrRunOutcome::Fallback(run_result)) => {
                    // The fallback produced no re-detections; the previous
                    // run's held candidates cannot be resolved against them
                    // and are flushed so no captured bubble is lost.
                    flush_held_boundary(app);
                    commit_run_result(app, run_result);
                }
                Err(e) => {
                    app.ocr_failed += 1;
                    if e == "cancelled" {
                        app.ocr_cancelled = true;
                    }
                }
            }
            let mut tasks: Vec<Task<Message>> = Vec::new();
            if app.pending == 0 || app.ocr_cancelled {
                finalize_run(app);
            } else {
                app.status = format!(
                    "OCR in progress: {} of {} run(s) done ({} line(s)).",
                    app.ocr_runs - app.pending,
                    app.ocr_runs,
                    app.ocr_total
                );
            }
            // Classify newly appended entries (including those resolved across
            // a run boundary) when auto-detect is enabled.
            if app.auto_style_detect {
                tasks.push(classify_entries(app));
            }
            if tasks.is_empty() {
                Task::none()
            } else {
                Task::batch(tasks)
            }
        }
        Message::OcrStreamFailed(e) => {
            app.ocr_failed += 1;
            if e == "cancelled" {
                app.ocr_cancelled = true;
            }
            // The stream aborted before every run was delivered; finalize
            // unless the runs already completed.
            if app.pending > 0 {
                app.pending = 0;
                finalize_run(app);
            }
            Task::none()
        }
        Message::OcrTick => Task::none(),
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
        Message::Ui(UiEvent::TilesVisible(range)) => schedule_settle(app, range),
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
            let jobs: Vec<(usize, EntryId, String, String)> = app
                .images
                .iter()
                .enumerate()
                .flat_map(|(index, image)| {
                    let filename = file_name(&image.path);
                    image
                        .project
                        .ocr
                        .visible()
                        .map(move |entry| {
                            (
                                index,
                                entry.id,
                                filename.clone(),
                                entry.text.clone(),
                            )
                        })
                })
                .collect();
            if jobs.is_empty() {
                app.status = "Run OCR first.".to_string();
                return Task::none();
            }
            app.translating = true;
            let items: Vec<translation::TranslateItem> = jobs
                .iter()
                .map(|(_, id, filename, text)| translation::TranslateItem {
                    filename: filename.clone(),
                    id: id.0,
                    text: text.clone(),
                })
                .collect();
            let target = app.translate_lang.clone();
            let provider = app
                .translate_providers_map
                .get(&app.translate_provider)
                .cloned()
                .unwrap_or_else(|| {
                    // Cannot happen after boot, but keeps the task well-typed.
                    translation::Provider {
                        id: app.translate_provider.clone(),
                        api: "https://opencode.ai/zen/v1".to_string(),
                        api_key_env: "OPENCODE_API_KEY".to_string(),
                        models: Vec::new(),
                    }
                });
            let model = app.translate_model.clone();
            let api_key = (!app.translate_api_key.is_empty())
                .then(|| app.translate_api_key.clone());
            app.status = format!(
                "Translating {} line(s) to {} via {model} ({})...",
                jobs.len(),
                app.translate_lang,
                provider.id
            );
            Task::perform(
                async move {
                    let result =
                        translation::translate_all(&items, &target, &provider, &model, api_key)
                            .await;
                    (jobs, result)
                },
                |(jobs, result)| Message::TranslateFinished(jobs, result),
            )
        }
        Message::Ui(UiEvent::TranslateProvider(provider)) => {
            if app.translate_provider != provider {
                app.translate_provider = provider;
                sync_translate_models(app);
            }
            Task::none()
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
        Message::Ui(UiEvent::FreeModelsOnlyToggle(enabled)) => {
            if app.free_models_only != enabled {
                app.free_models_only = enabled;
                sync_translate_models(app);
            }
            Task::none()
        }
        Message::Ui(UiEvent::EntryClicked(selection)) => {
            clear_editing(app);
            match selection {
                Some((index, id))
                    if app
                        .images
                        .get(index)
                        .is_some_and(|image| image.project.ocr.get(id).is_some()) =>
                {
                    Task::batch([select_entry(app, index, id), panel_scroll_task(index, id)])
                }
                _ => {
                    app.selected = None;
                    Task::none()
                }
            }
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
        Message::Ui(UiEvent::StylePresetApply(preset)) => {
            let Some((index, id)) = app.selected else { return Task::none() };
            let Some(Some(preset_style)) = app.style_presets.get(preset).copied() else {
                return Task::none();
            };
            seed_style_inputs(app, preset_style);
            app.images[index].project.set_entry_style(id, preset_style);
            Task::none()
        }
        Message::Ui(UiEvent::StylePresetAdd) => {
            if let Some(slot) = app.style_presets.iter_mut().find(|slot| slot.is_none()) {
                *slot = Some(app.style_working);
            } else {
                app.style_presets.push(Some(app.style_working));
            }
            Task::none()
        }
        Message::Ui(UiEvent::StylePresetReplace(preset)) => {
            if let Some(slot) = app.style_presets.get_mut(preset) {
                *slot = Some(app.style_working);
            }
            Task::none()
        }
        Message::Ui(UiEvent::StylePresetRemove(preset)) => {
            if let Some(slot) = app.style_presets.get_mut(preset) {
                *slot = None;
            }
            Task::none()
        }
        Message::Ui(UiEvent::StylePresetMenuDismiss) => Task::none(),
        Message::Ui(UiEvent::StyleAutoDetect) => {
            let Some((index, id)) = app.selected else { return Task::none() };
            // The entry must leave the done set so it is eligible again, even
            // if auto-detect already classified it.
            app.styled.remove(&(index, id));
            classify_entries(app)
        }
        Message::Ui(UiEvent::StyleAutoDetectToggle(enabled)) => {
            app.auto_style_detect = enabled;
            app.status = if enabled {
                "Auto style detection enabled.".to_string()
            } else {
                "Auto style detection disabled.".to_string()
            };
            Task::none()
        }
        Message::Ui(UiEvent::OcrWorkers(text)) => {
            app.ocr_workers = text;
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
                auto_style_detect: app.auto_style_detect,
                ocr_workers: app.ocr_workers.parse().unwrap_or(2),
                free_models_only: app.free_models_only,
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
                    for ((image_index, entry_id, _path, _text), translation) in
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

/// Keeps the frame loop alive while the OCR stream is running so queued
/// `OcrStreamRun` messages are drained and assembled per run instead of all
/// at once when the stream finishes.
pub fn subscription(app: &App) -> Subscription<Message> {
    if app.running {
        iced::time::every(Duration::from_millis(16)).map(|_| Message::OcrTick)
    } else {
        Subscription::none()
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
