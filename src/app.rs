use std::collections::{HashMap, HashSet};
#[cfg(all(feature = "test-ui", not(feature = "translation")))]
use std::collections::BTreeMap;
use std::sync::Arc;
#[cfg(feature = "ocr")]
use std::time::Duration;

#[cfg(any(feature = "inpaint", feature = "test-ui"))]
use iced::widget::image::Handle;
use iced::widget::{pane_grid, text_editor};
#[cfg(feature = "ocr")]
use iced::futures::{SinkExt, StreamExt};
use iced::{Color, Element, Font, Length, Rectangle, Subscription, Task, Theme};
use neverliie_iced_widgets::title_bar::{FrameAction, NativeFrame};

use fontdb;
#[cfg(feature = "inpaint")]
use scanlateit_inpaint::Engine as InpaintEngine;
use scanlateit_model::{
    EntryId, EntryStyle, NewEntry, Project, Quad, StylePresets, TextAlign,
    TextGradientDir,
};
#[cfg(feature = "inpaint")]
use scanlateit_settings::InpaintBackend;
#[cfg(feature = "inpaint")]
use scanlateit_model::InpaintPatch;
#[cfg(feature = "ocr")]
use scanlateit_ocr::{self as ocr, Engine, OcrCancellationToken, ParallelEngine};
#[cfg(feature = "styling")]
use scanlateit_styling::{Engine as StylingEngine, JobTracker};
#[cfg(feature = "segment")]
use scanlateit_segment::Engine as SegmentEngine;
use scanlateit_ui::scale;
use scanlateit_ui::translation as translation;
use scanlateit_ui::color::rgba_to_color;
#[cfg(feature = "inpaint")]
use scanlateit_ui::loaded::InpaintLayer;
use scanlateit_ui::main_area::decode::{DecodedPage, PageDecode, Scheduler, Tier};
use scanlateit_ui::panel::results::scroll_to_row;
use scanlateit_ui::{
    event::{EditOrigin, MainAreaMode, SettingsTab, StyleField, ToolbarAction, UiEvent},
    main_area, panel, settings as settings_modal, toolbar, ConnectModal, KOREAN_FONT_NAME,
    KOREAN_FONT_PATH, LoadedImage, UiState,
};

/// Widget id of the floating inline editor shown over a double-clicked entry.
const EDIT_INPUT_ID: &'static str = "overlay-editor";

/// Widget id of the multi-line editor shown in a results-list row while the
/// entry is edited from the panel.
const PANEL_EDIT_INPUT_ID: &'static str = "panel-editor";

const IMAGE_FILTERS: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "webp", "tiff", "avif"];

/// The pane the side panel occupies at launch: ~74% of the default window
/// width (about 1036px of the 1400px window), leaving the main area a third
/// of its previous ~1120px default.
const MAIN_AREA_DEFAULT_RATIO: f32 = 0.26;

/// Default share of the styling panel vs the results panel inside the side pane.
const STYLING_DEFAULT_RATIO: f32 = 0.36;

/// Default share of the styling inspector vs the inpaint/layers list inside the styling column.
/// ~70% top (taller), 30% bottom (shorter, not dramatic) – vertically stacked, resizable.
const STYLING_TOP_RATIO: f32 = 0.70;

/// Transparent gap shown between every top-level component (toolbar / main area / action / styling / results).
const GAP: f32 = 12.0;

/// Corner radius of the floating panel cards.
const CARD_RADIUS: f32 = 12.0;

/// Padding around the whole app window — shows the aurora as an outer frame.
const OUTER_PADDING: f32 = 10.0;

/// The two panes of the app window: the page viewer and the side panel.
#[derive(Debug, Clone, Copy)]
pub enum PaneKind {
    MainArea,
    Panel,
}

/// The two panes inside the side panel: styling on the left, results/translation on the right.
#[derive(Debug, Clone, Copy)]
pub enum SidePaneKind {
    Styling,
    Results,
}

/// The two stacked panes inside the styling column: inspector on top (taller), inpaint/layers list at bottom.
#[derive(Debug, Clone, Copy)]
pub enum StylingPaneKind {
    Inspector,
    Layers,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Frame actions from the custom title bar.
    Frame(FrameAction),
    /// A widget-level event from the ui crate.
    Ui(UiEvent),
    ImagesPicked(Result<Vec<(String, u32, u32)>, String>),
    #[cfg(feature = "ocr")]
    EngineReady(Result<Engine, String>),
    #[cfg(feature = "ocr")]
    ParallelEngineReady(Result<ParallelEngine, String>),
    /// One parallel-pipeline OCR run's inference outcome: raw lines for
    /// assembly, or an already-assembled result from the fallback path (see
    /// [`ocr::RunEvent`]).
    #[cfg(feature = "ocr")]
    OcrStreamRun(Result<ocr::RunEvent, String>),
    /// The OCR stream ended without delivering every run (pipeline error or
    /// cancellation); the handler finalizes the run.
    #[cfg(feature = "ocr")]
    OcrStreamFailed(String),
    /// Frame tick while the OCR stream is running: keeps the iced frame loop
    /// alive so queued `OcrStreamRun` messages are drained per run.
    #[cfg(feature = "ocr")]
    OcrTick,
    #[cfg(feature = "inpaint")]
    InpaintEngineReady(Result<InpaintEngine, String>),
    #[cfg(feature = "inpaint")]
    InpaintFinished(usize, Result<Vec<(image::RgbaImage, [f32; 4])>, String>),
    #[cfg(feature = "styling")]
    StylingEngineReady(Result<StylingEngine, String>),
    #[cfg(feature = "styling")]
    StyleDetected(usize, EntryId, Result<EntryStyle, String>),
    #[cfg(feature = "segment")]
    SegmentEngineReady(Result<SegmentEngine, String>),
    #[cfg(feature = "segment")]
    SegmentFiltered(Result<Vec<(usize, EntryId)>, String>),
    FontLoaded,
    /// The boot-time enumeration of installed system fonts as
    /// `(family name, font file path)` pairs.
    SystemFonts(Vec<(String, String)>),
    /// A picked font family was loaded into iced's font system.
    StyleFontLoaded(String),
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
    /// The per-row retranslate finished: the new text for `(image index,
    /// entry id)`, or the error message. The result is stored in the selected
    /// profile (forking a new one when the Default profile is selected).
    RetranslateFinished((usize, EntryId), Result<String, String>),
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
    #[cfg(feature = "ocr")]
    engine: Option<Engine>,
    /// The parallel OCR pipeline (K detection workers + one recognition
    /// worker). Built lazily on first OCR run; dropped (workers exit) when a
    /// run finishes, so the next run rebuilds it fresh.
    #[cfg(feature = "ocr")]
    pipeline: Option<ParallelEngine>,
    #[cfg(feature = "ocr")]
    cancel: Option<OcrCancellationToken>,
    #[cfg(feature = "ocr")]
    ocr_plans: Vec<ocr::RunPlan>,
    /// All images' `(width, height)` of the current run, in image order.
    #[cfg(feature = "ocr")]
    ocr_dims: Vec<(u32, u32)>,
    #[cfg(feature = "inpaint")]
    inpaint_engine: Option<InpaintEngine>,
    /// Buffered inpainting job waiting for the engine to finish loading,
    /// as `(image index, path, rect, mask quads)`.
    #[cfg(feature = "inpaint")]
    pending_inpaint: Option<(usize, String, [f32; 4], Vec<Quad>)>,
    /// True while an inpainting inference is in flight.
    pub(crate) inpainting: bool,
    /// Whether inpainting range drags are enabled; when `true` a drag on
    /// any tile selects the range to clean.
    pub(crate) inpaint_mode: bool,
    /// Whether the overlay text is drawn over the pages in the main area.
    pub(crate) show_overlay_text: bool,
    /// Whether applied inpainting patches are drawn over the pages.
    pub(crate) show_inpaint: bool,
    /// The display mode of the main area: the single overlay column or the
    /// original-vs-current side-by-side comparison.
    pub(crate) view_mode: MainAreaMode,
    /// The latest scroll offset published by a main-area viewer, in content
    /// pixels; in Compare mode the panes mirror each other through it, and
    /// switching modes keeps the scroll position.
    pub(crate) viewer_scroll: f32,
    /// Auto style-detection bookkeeping: the lazily-built classifier engine,
    /// the in-flight build flag and the `(image index, entry id)` pairs
    /// already classified (see [`JobTracker`]).
    #[cfg(feature = "styling")]
    styling: JobTracker,
    /// Segmentation engine for SFX filtering (manga-mimic grid).
    #[cfg(feature = "segment")]
    segment_engine: Option<SegmentEngine>,
    /// True while an SFX-filter run is in flight.
    #[cfg(feature = "segment")]
    segment_filtering: bool,
    pub(crate) running: bool,
    pub(crate) font: Option<Font>,
    pub(crate) status: String,
    #[cfg(feature = "ocr")]
    pending: usize,
    #[cfg(feature = "ocr")]
    ocr_total: usize,
    #[cfg(feature = "ocr")]
    ocr_failed: usize,
    #[cfg(feature = "ocr")]
    ocr_cancelled: bool,
    /// Total OCR runs in the current run's plan; `pending` counts down from
    /// this (one run may cover several images).
    #[cfg(feature = "ocr")]
    ocr_runs: usize,
    /// Boundary candidates of the last completed run, awaiting the next run's
    /// re-detections in its top margin (see [`ocr::resolve_boundary`]). Taken
    /// by the next run's task; flushed if the run fails or is cancelled so no
    /// captured bubble is lost.
    #[cfg(feature = "ocr")]
    held_boundary: Option<ocr::BoundaryState>,
    pub(crate) translating: bool,
    /// The connected-provider session: stored connections, selection, model
    /// picker lists and the free-only filter (see [`translation::Session`]).
    pub(crate) tx: translation::Session,
    pub(crate) translate_lang: String,
    /// The API-key entry modal open over the settings modal, if any.
    pub(crate) connect_modal: Option<ConnectModal>,
    /// True while the settings modal is open.
    pub(crate) settings_open: bool,
    /// The settings tab shown inside the modal.
    pub(crate) settings_tab: SettingsTab,
    /// True while the Manage Models overlay (over the settings modal) is open.
    pub(crate) manage_models_open: bool,
    /// Filter text of the Manage Models search field; not persisted between opens.
    pub(crate) manage_models_search: String,
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
    /// The settled-viewport decode scheduler: debounces visible-range
    /// changes, backs the settled window with full-res decodes and evicts
    /// far-away full caches (see [`Scheduler`]).
    scheduler: Scheduler,
    /// Staged style of the selected entry. Mirrors the entry's stored style
    /// on selection; mutations are written back to that entry only.
    pub(crate) style_working: EntryStyle,
    /// Installed system fonts: family name -> font file path, from the boot
    /// fontdb scan. Used to load a picked font into iced on demand.
    pub(crate) system_fonts: HashMap<String, String>,
    /// Installed font family names, sorted, for the styling panel picker.
    pub(crate) installed_fonts: Vec<String>,
    /// Font families already handed to `iced::font::load`.
    pub(crate) loaded_fonts: HashSet<String>,
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
    pub(crate) presets: StylePresets,
    /// The draggable split between the main area and the side panel.
    pub(crate) panes: pane_grid::State<PaneKind>,
    /// The draggable split between the styling panel and the translation/results panel.
    pub(crate) side_panes: pane_grid::State<SidePaneKind>,
    /// The vertical split inside the styling column: inspector (top, taller) vs layers/inpaint list (bottom).
    pub(crate) styling_panes: pane_grid::State<StylingPaneKind>,
    /// The custom window frame (title bar) – single-window via `install_latest`.
    pub frame: NativeFrame,
}
impl App {
    pub fn theme(&self) -> Theme {
        // Transparent frame: make the palette backgrounds transparent so the
        // aurora canvas shows through the title bar and the window surface.
        // Keep caption hover/pressed colors opaque by restoring them from an
        // opaque palette; only `base` (surface) and `weakest` (title bar) stay
        // transparent. This avoids patching `NeverLiieIcedWidgets`.
        use iced::theme::palette::{Extended, Palette};
        let is_dark = scanlateit_settings::get(|s| s.aurora_is_dark);
        let base_palette = if is_dark { Palette::DARK } else { Palette::LIGHT };
        let opaque_bg = base_palette.background;
        let mut transparent_palette = base_palette;
        transparent_palette.background = Color {
            a: 0.0,
            ..opaque_bg
        };
        Theme::custom_with_fn("TransparentAurora", transparent_palette, move |p| {
            let mut ext = Extended::generate(p);
            let opaque_palette = Palette {
                background: opaque_bg,
                ..p
            };
            let opaque_ext = Extended::generate(opaque_palette);
            // caption hover/pressed use these – keep opaque
            ext.background.weak = opaque_ext.background.weak;
            ext.background.strong = opaque_ext.background.strong;
            ext.background.stronger = opaque_ext.background.stronger;
            ext.background.strongest = opaque_ext.background.strongest;
            ext.background.weaker = opaque_ext.background.weaker;
            ext.background.neutral = opaque_ext.background.neutral;
            // keep text readable on aurora
            ext.background.base.text = opaque_ext.background.base.text;
            ext.background.weakest.text = opaque_ext.background.weakest.text;
            ext
        })
    }

    fn new(frame: NativeFrame) -> Self {
        let style = EntryStyle::default();
        Self {
            frame,
            images: Vec::new(),
            #[cfg(feature = "ocr")]
            engine: None,
            #[cfg(feature = "ocr")]
            pipeline: None,
            #[cfg(feature = "ocr")]
            cancel: None,
            #[cfg(feature = "ocr")]
            ocr_plans: Vec::new(),
            #[cfg(feature = "ocr")]
            ocr_dims: Vec::new(),
            #[cfg(feature = "inpaint")]
            inpaint_engine: None,
            #[cfg(feature = "inpaint")]
            pending_inpaint: None,
            inpainting: false,
            inpaint_mode: false,
            show_overlay_text: true,
            show_inpaint: true,
            view_mode: MainAreaMode::View,
            viewer_scroll: 0.0,
            #[cfg(feature = "styling")]
            styling: JobTracker::new(),
            #[cfg(feature = "segment")]
            segment_engine: None,
            #[cfg(feature = "segment")]
            segment_filtering: false,
            running: false,
            font: None,
            status: "Idle - open images to begin.".to_string(),
            #[cfg(feature = "ocr")]
            pending: 0,
            #[cfg(feature = "ocr")]
            ocr_total: 0,
            #[cfg(feature = "ocr")]
            ocr_failed: 0,
            #[cfg(feature = "ocr")]
            ocr_cancelled: false,
            #[cfg(feature = "ocr")]
            ocr_runs: 0,
            #[cfg(feature = "ocr")]
            held_boundary: None,
            translating: false,
            tx: translation::Session::default(),
            translate_lang: translation::LANGUAGES[0].to_string(),
            connect_modal: None,
            settings_open: false,
            settings_tab: SettingsTab::General,
            manage_models_open: false,
            manage_models_search: String::new(),
            selected: None,
            editing: None,
            editing_origin: EditOrigin::Overlay,
            edit_content: None,
            editing_dirty: false,
            editing_rect: None,
            scheduler: Scheduler::new(),
            style_working: style.clone(),
            system_fonts: HashMap::new(),
            installed_fonts: Vec::new(),
            loaded_fonts: HashSet::new(),
            style_picker: None,
            style_stroke_width: style.stroke_width.to_string(),
            style_bg_radius: style.bg_radius.to_string(),
            presets: StylePresets::default_presets(),
            panes: {
                let (mut panes, main) = pane_grid::State::new(PaneKind::MainArea);
                let (_, split) = panes
                    .split(pane_grid::Axis::Vertical, main, PaneKind::Panel)
                    .expect("initial pane split must succeed");
                panes.resize(split, MAIN_AREA_DEFAULT_RATIO);
                panes
            },
            side_panes: {
                let (mut panes, styling) = pane_grid::State::new(SidePaneKind::Styling);
                let (_, split) = panes
                    .split(pane_grid::Axis::Vertical, styling, SidePaneKind::Results)
                    .expect("side pane split must succeed");
                panes.resize(split, STYLING_DEFAULT_RATIO);
                panes
            },
            styling_panes: {
                let (mut panes, inspector) = pane_grid::State::new(StylingPaneKind::Inspector);
                let (_, split) = panes
                    .split(pane_grid::Axis::Horizontal, inspector, StylingPaneKind::Layers)
                    .expect("styling pane split must succeed");
                panes.resize(split, STYLING_TOP_RATIO);
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
        tasks.push(scroll_to_row::<Message>(index, id));
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
    app.style_stroke_width = style.stroke_width.to_string();
    app.style_bg_radius = style.bg_radius.to_string();
    app.style_working = style;
    app.style_picker = None;
}

/// Selects `(index, id)`: seeds the style inputs and, when the entry's page
/// is outside the currently settled decode window (a panel-driven reveal
/// moved the viewport without a `TilesVisible` event), schedules a full-res
/// settle for that page.
fn select_entry(app: &mut App, index: usize, id: EntryId) -> Task<Message> {
    app.selected = Some((index, id));
    seed_style_inputs(app, app.images[index].project.entry_style(id));
    if app.scheduler.needs_settle(index, app.images.len()) {
        app.scheduler
            .schedule(index..index + 1, Message::SettleElapsed)
    } else {
        Task::none()
    }
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

    fn translate_model_groups(&self) -> &[(String, String, Vec<(String, String)>)] {
        self.tx.model_groups()
    }

    fn translate_model_selection(&self) -> (String, String) {
        (self.tx.selected_id.clone(), self.tx.selected_model.clone())
    }

    fn translate_lang(&self) -> &str {
        &self.translate_lang
    }

    fn connect_modal(&self) -> Option<&ConnectModal> {
        self.connect_modal.as_ref()
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
        self.presets.as_slice()
    }

    fn installed_fonts(&self) -> &[String] {
        &self.installed_fonts
    }

    fn style_font_family(&self) -> Option<&str> {
        self.style_working.font_family.as_deref()
    }

    fn style_text_align(&self) -> TextAlign {
        self.style_working.text_align
    }

    fn style_gradient_a(&self) -> Color {
        rgba_to_color(self.style_working.gradient_a)
    }

    fn style_gradient_b(&self) -> Color {
        rgba_to_color(self.style_working.gradient_b)
    }

    fn style_gradient_dir(&self) -> TextGradientDir {
        self.style_working.gradient_dir
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

    fn inpaint_mode(&self) -> bool {
        self.inpaint_mode
    }

    fn show_overlay_text(&self) -> bool {
        self.show_overlay_text
    }

    fn show_inpaint(&self) -> bool {
        self.show_inpaint
    }

    fn view_mode(&self) -> MainAreaMode {
        self.view_mode
    }

    fn viewer_scroll(&self) -> f32 {
        self.viewer_scroll
    }

    fn settings_open(&self) -> bool {
        self.settings_open
    }

    fn settings_tab(&self) -> SettingsTab {
        self.settings_tab
    }

    fn manage_models_open(&self) -> bool {
        self.manage_models_open
    }

    fn manage_models_search(&self) -> &str {
        &self.manage_models_search
    }

    fn all_model_groups(&self) -> Vec<(String, String, Vec<(String, String)>)> {
        self.tx.all_model_groups()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use scanlateit_model::INITIAL_PRESET_SLOTS;

    fn app_with_entry() -> (App, EntryId) {
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
    fn switching_main_area_mode_updates_state() {
        let (mut app, _id) = app_with_entry();
        assert_eq!(app.view_mode, MainAreaMode::View);

        let _ = update(&mut app, Message::Ui(UiEvent::MainAreaMode(MainAreaMode::Compare)));
        assert_eq!(app.view_mode, MainAreaMode::Compare);
        assert!(app.status.contains("Compare mode"));

        let _ = update(&mut app, Message::Ui(UiEvent::MainAreaMode(MainAreaMode::View)));
        assert_eq!(app.view_mode, MainAreaMode::View);
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
        let _ = update(
            &mut app,
            Message::Ui(UiEvent::EntryToolbar((0, missing, ToolbarAction::RevertTransform))),
        );

        assert_eq!(app.editing, None);
        assert_eq!(app.selected, None);
        assert_eq!(app.images[0].project.ocr.visible_count(), 1);
    }

    #[test]
    fn toolbar_revert_drops_the_view_quad_back_to_the_ocr_quad() {
        let (mut app, id) = app_with_entry();
        let ocr_quad = app.images[0].project.ocr.get(id).unwrap().quad;
        let ocr_tl = ocr_quad.ordered()[0];
        let moved = ocr_quad.translate(15.0, 8.0).rotate([50.0, 50.0], 0.4);
        let _ = update(&mut app, Message::Ui(UiEvent::EntryMoved((0, id, moved))));
        assert_ne!(app.images[0].project.view_quad(app.images[0].project.ocr.get(id).unwrap()), ocr_quad);
        app.selected = Some((0, id));

        let _ = update(
            &mut app,
            Message::Ui(UiEvent::EntryToolbar((0, id, ToolbarAction::RevertTransform))),
        );

        let view = app.images[0].project.view_quad(app.images[0].project.ocr.get(id).unwrap());
        let tl = view.ordered()[0];
        assert!(
            (tl[0] - (ocr_tl[0] + 15.0)).abs() < 1e-3 && (tl[1] - (ocr_tl[1] + 8.0)).abs() < 1e-3,
            "revert must keep the box's position, got {tl:?}"
        );
        assert_eq!(view.translate(-(tl[0] - ocr_tl[0]), -(tl[1] - ocr_tl[1])), ocr_quad,
            "revert must restore the OCR shape/rotation/size");
        assert_eq!(app.selected, Some((0, id)), "revert must keep the selection");
        assert!(
            !app.images[0].project.ocr.get(id).unwrap().deleted,
            "revert must not touch the entry"
        );
    }

    #[test]
    fn applying_a_preset_seeds_working_style_and_entry() {
        let (mut app, id) = app_with_entry();
        app.selected = Some((0, id));
        app.style_working.bg_color = [1, 2, 3, 255];
        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetApply(1)));

        let preset = app.presets.get(1).expect("preset 1 seeded");
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
        app.images[0].project.set_entry_style(id, app.style_working.clone());
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

        assert_eq!(app.presets.len(), INITIAL_PRESET_SLOTS);
        assert_eq!(app.presets.get(5), Some(app.style_working.clone()));
        assert_eq!(app.presets.get(6), Some(app.style_working.clone()));
        assert_eq!(app.presets.get(7), Some(app.style_working.clone()));
    }

    #[test]
    fn add_preset_appends_when_all_slots_are_full() {
        let (mut app, _id) = app_with_entry();
        for i in 0..INITIAL_PRESET_SLOTS {
            let mut style = EntryStyle::default();
            style.text_color = [i as u8, 0, 0, 255];
            app.presets.replace(i, style);
        }
        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetAdd));

        assert_eq!(app.presets.len(), INITIAL_PRESET_SLOTS + 1);
        assert_eq!(app.presets.get(INITIAL_PRESET_SLOTS), Some(app.style_working.clone()));
    }

    #[test]
    fn add_preset_refills_an_emptied_slot_before_appending() {
        let (mut app, _id) = app_with_entry();
        app.style_working.text_color = [7, 7, 7, 255];
        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetRemove(2)));
        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetAdd));

        assert_eq!(app.presets.len(), INITIAL_PRESET_SLOTS);
        assert_eq!(app.presets.get(2), Some(app.style_working.clone()));
    }

    #[test]
    fn replace_preset_overwrites_filled_and_empty_slots() {
        let (mut app, _id) = app_with_entry();
        app.style_working.text_color = [42, 0, 0, 255];

        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetReplace(1)));
        assert_eq!(app.presets.get(1), Some(app.style_working.clone()));

        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetReplace(6)));
        assert_eq!(app.presets.get(6), Some(app.style_working.clone()));

        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetReplace(999)));
        assert_eq!(app.presets.len(), INITIAL_PRESET_SLOTS);
    }

    #[test]
    fn remove_preset_empties_the_slot() {
        let (mut app, _id) = app_with_entry();
        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetRemove(0)));
        let _ = update(&mut app, Message::Ui(UiEvent::StylePresetRemove(999)));

        assert!(app.presets.get(0).is_none());
        assert_eq!(app.presets.len(), INITIAL_PRESET_SLOTS);
    }

    #[test]
    fn style_font_sets_family_and_loads_font() {
        let (mut app, id) = app_with_entry();
        app.selected = Some((0, id));
        app.system_fonts
            .insert("Test".into(), "C:\\Windows\\Fonts\\arial.ttf".into());

        let _ = update(&mut app, Message::Ui(UiEvent::StyleFont("Test".to_string())));

        assert_eq!(app.style_working.font_family.as_deref(), Some("Test"));
        assert_eq!(
            app.images[0].project.entry_style(id).font_family.as_deref(),
            Some("Test")
        );
    }

    #[test]
    fn style_text_align_sets_alignment() {
        let (mut app, id) = app_with_entry();
        app.selected = Some((0, id));

        let _ = update(&mut app, Message::Ui(UiEvent::StyleTextAlign(TextAlign::Right)));

        assert_eq!(app.style_working.text_align, TextAlign::Right);
        assert_eq!(
            app.images[0].project.entry_style(id).text_align,
            TextAlign::Right
        );
    }

    #[test]
    fn style_gradient_dir_and_toggle_set_fields() {
        let (mut app, id) = app_with_entry();
        app.selected = Some((0, id));

        let _ = update(&mut app, Message::Ui(UiEvent::StyleGradientToggle(true)));
        assert!(app.style_working.text_gradient);
        assert!(app.images[0].project.entry_style(id).text_gradient);

        let _ = update(
            &mut app,
            Message::Ui(UiEvent::StyleGradientDir(TextGradientDir::LeftToRight)),
        );
        assert_eq!(app.style_working.gradient_dir, TextGradientDir::LeftToRight);
        assert_eq!(
            app.images[0].project.entry_style(id).gradient_dir,
            TextGradientDir::LeftToRight
        );
    }

    #[cfg(feature = "translation")]
    #[test]
    fn retranslate_without_connection_is_rejected() {
        let (mut app, id) = app_with_entry();
        let _ = update(&mut app, Message::Ui(UiEvent::RetranslateEntry((0, id))));
        assert!(!app.translating);
        assert!(app.status.contains("Connect a translation service"));
    }

    #[cfg(feature = "translation")]
    #[test]
    fn retranslate_missing_entry_is_rejected() {
        let (mut app, _) = app_with_entry();
        let _ = update(
            &mut app,
            Message::Ui(UiEvent::RetranslateEntry((0, EntryId(999)))),
        );
        assert!(!app.translating);
        assert!(app.status.contains("no longer exists"));
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
        assert!(app.translating, "retranslate must set the translating flag");
        assert!(app.status.starts_with("Retranslating"));
    }

    #[cfg(feature = "translation")]
    #[test]
    fn retranslate_finished_forks_a_profile_off_the_default() {
        let (mut app, id) = app_with_entry();
        let _ = update(
            &mut app,
            Message::RetranslateFinished((0, id), Ok("Hello".to_string())),
        );
        let project = &app.images[0].project;
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
        app.images[0]
            .project
            .profiles
            .selected_mut()
            .set_translation(id, Some("Hello".into()));
        let jp = app.images[0].project.profiles.add("JP");
        app.images[0].project.profiles.select(jp);
        let _ = update(
            &mut app,
            Message::RetranslateFinished((0, id), Ok("Hola".to_string())),
        );
        let project = &app.images[0].project;
        assert_eq!(project.profiles.len(), 2, "no fork on non-original profiles");
        assert_eq!(project.profiles.selected_id(), jp);
        assert_eq!(project.profiles.selected().translation_of(id), Some("Hola"));
    }

    #[cfg(feature = "translation")]
    #[test]
    fn retranslate_finished_error_leaves_the_profile_untouched() {
        let (mut app, id) = app_with_entry();
        app.images[0]
            .project
            .profiles
            .selected_mut()
            .set_translation(id, Some("Hello".into()));
        let _ = update(
            &mut app,
            Message::RetranslateFinished((0, id), Err("boom".to_string())),
        );
        assert!(!app.translating);
        assert_eq!(app.status, "boom");
        let project = &app.images[0].project;
        assert_eq!(project.profiles.len(), 1);
        assert_eq!(project.profiles.selected().translation_of(id), Some("Hello"));
    }

    #[cfg(feature = "translation")]
    #[test]
    fn retranslate_finished_strips_quotes_and_clears_when_identical_to_ocr() {
        let (mut app, id) = app_with_entry();
        let _ = update(
            &mut app,
            Message::RetranslateFinished((0, id), Ok("\"안녕\"".to_string())),
        );
        let project = &app.images[0].project;
        assert_eq!(project.profiles.len(), 2, "the fork still happens");
        assert_eq!(
            project.profiles.selected().translation_of(id),
            None,
            "same as the OCR text: delta cleared, original shown"
        );
        assert_eq!(project.display_text(project.ocr.get(id).unwrap()), "안녕");
    }
}

/// Re-syncs the translation session's persisted mirrors from the shared
/// settings store: connections, free-only filter and hidden models. The
/// current selection is kept (`sync` falls back when it dropped out); used
/// at boot and on the single [`UiEvent::SettingsChanged`] announcement.
fn sync_tx_from_store(app: &mut App) {
    scanlateit_settings::get(|s| {
        app.tx.connections = s.connections.clone();
        app.tx.free_only = s.free_models_only;
        app.tx.hidden_models = s.hidden_models.clone();
    });
    app.tx.sync();
}

pub fn boot(frame: NativeFrame) -> (App, Task<Message>) {
    scanlateit_settings::init();
    let font_task = match std::fs::read(KOREAN_FONT_PATH) {
        Ok(bytes) => iced::font::load(bytes).map(|_| Message::FontLoaded),
        Err(_) => Task::none(),
    };
    #[cfg_attr(
        not(any(
            feature = "translation",
            feature = "styling",
            feature = "ocr",
            feature = "test-ui"
        )),
        allow(unused_mut)
    )]
    let mut app = App::new(frame);
    #[cfg(feature = "translation")]
    {
        let (connections, last_provider, free_only, hidden) = scanlateit_settings::get(|s| {
            (
                s.connections.clone(),
                s.last_provider.clone(),
                s.free_models_only,
                s.hidden_models.clone(),
            )
        });
        app.tx = translation::Session::new(connections, last_provider);
        app.tx.free_only = free_only;
        app.tx.hidden_models = hidden;
        app.tx.sync();
    }
    #[cfg(all(not(feature = "translation"), feature = "test-ui"))]
    {
        // test-ui uses fake translation – still restore hidden models so the
        // Manage Models overlay persists in that build as well.
        sync_tx_from_store(&mut app);
    }
    #[cfg(feature = "translation")]
    let models_task = {
        let fetch_ids = app.tx.fetch_ids();
        let cloud_task = if fetch_ids.is_empty() {
            Task::none()
        } else {
            Task::perform(translation::fetch_providers(fetch_ids), Message::ModelsFetched)
        };
        let local_endpoints = app.tx.local_fetch_endpoints();
        let local_task = if local_endpoints.is_empty() {
            Task::none()
        } else {
            Task::perform(
                translation::fetch_local_providers(local_endpoints),
                Message::ModelsFetched,
            )
        };
        Task::batch([cloud_task, local_task])
    };
    #[cfg(not(feature = "translation"))]
    let models_task = Task::none();
    #[cfg(feature = "test-ui")]
    {
        // TEST-UI build: preload a fake white page with fake OCR entries so
        // the overlay, panel, editing and styling UI are usable without any
        // ML backend (no ort, no rig compiled).
        let width = 900u32;
        let height = 1200u32;
        let white = image::RgbaImage::from_pixel(width, height, image::Rgba([245, 245, 245, 255]));
        let pixels = bytes::Bytes::from(white.into_raw());
        let page = std::sync::Arc::new(DecodedPage {
            handle: Handle::from_rgba(width, height, pixels),
            width,
            height,
        });
        let mut project = Project::new();
        project.append_ocr(fake_ocr_entries());
        app.images.push(LoadedImage {
            width: width as f32,
            height: height as f32,
            path: "fake-white-page.png".to_string(),
            project,
            decode: PageDecode {
                thumb: Tier::Ready(page.clone()),
                full: Tier::Ready(page),
            },
            inpaint: Vec::new(),
        });
        // TEST-UI without the translation subsystem: seed a fake connected
        // provider with its fake model list so the translation bar and
        // settings tab are fully exercisable (no rig, no API keys) — the
        // translation analogue of the fake OCR entries above. The connection
        // is mirrored into the settings store too: the ui reads connected
        // state from there.
        #[cfg(all(feature = "test-ui", not(feature = "translation")))]
        {
            let _ = scanlateit_settings::modify(|s| {
                s.connections.insert(
                    translation::FAKE_PROVIDER.to_string(),
                    scanlateit_settings::Connection {
                        api_key: "fake-key-1234".to_string(),
                        base_url: None,
                        model: None,
                    },
                );
            });
            let mut tx = translation::Session::new(
                BTreeMap::from([(
                    translation::FAKE_PROVIDER.to_string(),
                    translation::Connection {
                        api_key: "fake-key-1234".to_string(),
                        base_url: None,
                        model: None,
                    },
                )]),
                Some(translation::FAKE_PROVIDER.to_string()),
            );
            tx.fetched.insert(
                translation::FAKE_PROVIDER.to_string(),
                translation::catalog_provider(translation::FAKE_PROVIDER)
                    .expect("the fake provider must be in the fake catalog")
                    .clone(),
            );
            tx.sync_models();
            app.tx = tx;
        }
        app.status = "TEST-UI build: fake white page with fake OCR entries and fake translation loaded."
            .to_string();
    }
    let fonts_task =
        Task::perform(async move { enumerate_system_fonts() }, Message::SystemFonts);
    (app, Task::batch([font_task, models_task, fonts_task]))
}

/// Fake OCR entries for TEST builds: a small batch of Korean bubbles spread
/// over a page-sized canvas. Used at `test-ui` boot and by the fake OCR run.
#[cfg_attr(all(feature = "ocr", not(feature = "test-ui")), allow(dead_code))]
fn fake_ocr_entries() -> Vec<NewEntry> {
    use scanlateit_model::EntrySource;
    let (w, h) = (900.0f32, 1200.0f32);
    let box_at = |cx: f32, cy: f32, bw: f32, bh: f32| {
        Quad {
            points: [
                [cx - bw / 2.0, cy - bh / 2.0],
                [cx + bw / 2.0, cy - bh / 2.0],
                [cx + bw / 2.0, cy + bh / 2.0],
                [cx - bw / 2.0, cy + bh / 2.0],
            ],
        }
    };
    let specs = [
        ("안녕하세요!", 0.5 * w, 0.10 * h, 0.28 * w, 0.05 * h),
        ("오늘은 좋은 날이네요.", 0.45 * w, 0.22 * h, 0.32 * w, 0.05 * h),
        ("저기 보이는 게 뭐지?", 0.55 * w, 0.34 * h, 0.30 * w, 0.05 * h),
        ("조심해서 가자.", 0.35 * w, 0.50 * h, 0.24 * w, 0.05 * h),
        ("정말 멋진 풍경이야!", 0.60 * w, 0.62 * h, 0.32 * w, 0.05 * h),
        ("다음에 또 만나요.", 0.45 * w, 0.78 * h, 0.26 * w, 0.05 * h),
    ];
    specs
        .into_iter()
        .map(|(text, cx, cy, bw, bh)| NewEntry {
            source: EntrySource::AutoOcr,
            text: text.to_string(),
            score: 0.9,
            quad: box_at(cx, cy, bw, bh),
        })
        .collect()
}

/// Enumerates installed system fonts (family name + file path) with fontdb
/// (the same version iced's text stack uses), off the UI thread, once at
/// boot. Duplicate family names are deduped by the caller.
fn enumerate_system_fonts() -> Vec<(String, String)> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let mut out = Vec::new();
    for face in db.faces() {
        let path = match &face.source {
            fontdb::Source::File(path) => path.to_string_lossy().into_owned(),
            fontdb::Source::SharedFile(path, _) => path.to_string_lossy().into_owned(),
            fontdb::Source::Binary(_) => continue,
        };
        for (name, _language) in &face.families {
            out.push((name.clone(), path.clone()));
        }
    }
    out
}

/// Spawns the parallel OCR stream: the [`ocr::RunSession`] does the
/// windowing/ordering on the pipeline, this task only forwards its events
/// into the iced channel. Only the inference (detect + recognize on a
/// stitched canvas) runs on the pipeline's worker threads; assembly —
/// boundary resolution, dedup, distribution and commit — happens in the
/// `OcrStreamRun` handler, on the UI thread, where the committed state is
/// authoritative. The old [`Engine`] is kept for the undecodable-page
/// fallback path only.
#[cfg(feature = "ocr")]
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
    let workers = scanlateit_settings::get(|s| s.ocr_workers.parse::<usize>().unwrap_or(2)).max(1);
    let mut session = ocr::RunSession::new(runs, dims, paths, above_paths, below_paths, workers);
    Task::stream(
        iced::stream::try_channel(1, move |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
            while let Some(event) = session.step(&pipeline, &fallback, &token)? {
                if sender
                    .send(Message::OcrStreamRun(Ok::<ocr::RunEvent, String>(event)))
                    .await
                    .is_err()
                {
                    return Ok(());
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

/// Spawns one inpainting run: the full-res decode, mask build and LaMa
/// inference all happen on the blocking pool, the UI thread only stores the
/// finished patch.
#[cfg(feature = "inpaint")]
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
#[cfg(feature = "styling")]
fn classify_entries(app: &mut App) -> Task<Message> {
    match app.styling.engine() {
        Some(engine) => start_style_jobs(app, engine.clone()),
        None => {
            app.styling.mark_building();
            app.status = "Loading the styling model...".to_string();
            Task::perform(async move { StylingEngine::build() }, Message::StylingEngineReady)
        }
    }
}

/// Spawns one classification `Task` per unclassified visible entry, each on
/// the blocking pool. Entries are marked in-flight up front so a later
/// auto-run never schedules the same entry twice.
#[cfg(feature = "styling")]
fn start_style_jobs(app: &mut App, engine: StylingEngine) -> Task<Message> {
    // Immutable reference captured by the inner closures so a `move` does not
    // move `app` itself; disjoint-field borrows of the images and the tracker
    // coexist until `collect` finishes.
    let styling = &app.styling;
    let jobs: Vec<(usize, EntryId, String, Quad)> = app
        .images
        .iter()
        .enumerate()
        .flat_map(|(index, image)| {
            image
                .project
                .ocr
                .visible()
                .filter(move |entry| !styling.is_done(index, entry.id))
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
        app.styling.mark_done(*index, *id);
    }
    let tasks: Vec<Task<Message>> = jobs
        .into_iter()
        .map(|(index, id, path, quad)| {
            let engine = engine.clone();
            Task::perform(
                async move {
                    let classified = tokio::task::spawn_blocking(move || {
                        engine.classify_entry(&path, &quad)
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

#[cfg(feature = "segment")]
fn start_segment_filter(app: &mut App) -> Task<Message> {
    if app.images.is_empty() {
        return Task::none();
    }
    if !scanlateit_settings::get(|s| s.auto_sfx_filter) {
        return Task::none();
    }
    match &app.segment_engine {
        Some(engine) => {
            let engine = engine.clone();
            let dims: Vec<(u32, u32)> = app
                .images
                .iter()
                .map(|img| (img.width as u32, img.height as u32))
                .collect();
            let paths: Vec<String> = app.images.iter().map(|img| img.path.clone()).collect();
            // Snapshot OCR boxes per page (visible entries, view_quad bounds)
            let ocr_boxes: Vec<Vec<([f32; 4], EntryId)>> = app
                .images
                .iter()
                .map(|img| {
                    img.project
                        .ocr
                        .visible()
                        .map(|e| (img.project.view_quad(e).bounds(), e.id))
                        .collect()
                })
                .collect();
            app.segment_filtering = true;
            app.status = "Filtering SFX via segmentation...".to_string();
            Task::perform(
                async move {
                    let res = tokio::task::spawn_blocking(move || {
                        run_segment_filter_blocking(&engine, &dims, &paths, &ocr_boxes)
                    })
                    .await
                    .unwrap_or_else(|e| Err(format!("segment task cancelled: {e}")));
                    res
                },
                Message::SegmentFiltered,
            )
        }
        None => {
            app.segment_filtering = true;
            app.status = "Loading segmentation model...".to_string();
            Task::perform(async move { SegmentEngine::build() }, Message::SegmentEngineReady)
        }
    }
}

#[cfg(feature = "segment")]
fn run_segment_filter_blocking(
    engine: &SegmentEngine,
    dims: &[(u32, u32)],
    paths: &[String],
    ocr_boxes: &[Vec<([f32; 4], EntryId)>],
) -> Result<Vec<(usize, EntryId)>, String> {
    use scanlateit_segment::filter::{DetBox, sfx_filter_indexes};
    use scanlateit_segment::grid::{build_grid_canvas, grid_det_to_page, plan_grids};
    use scanlateit_segment::SegClass;
    if dims.is_empty() {
        return Ok(Vec::new());
    }
    let runs = plan_grids(dims);
    // Load all images for grid building (same as OCR load_rgb)
    let images: Vec<image::RgbImage> = paths
        .iter()
        .map(|p| scanlateit_ocr::load_rgb(p).unwrap_or_else(|| image::RgbImage::new(1, 1)))
        .collect();
    let mut to_delete: Vec<(usize, EntryId)> = Vec::new();
    // For each grid run, build canvas and detect
    for run in &runs {
        let canvas = build_grid_canvas(&images, run);
        let dets = engine.detect_canvas(&canvas).map_err(|e| format!("segment detect failed: {e}"))?;
        // Map dets to per-page
        let mut balloons_per_page: Vec<Vec<DetBox>> = vec![Vec::new(); dims.len()];
        let mut sfx_per_page: Vec<Vec<DetBox>> = vec![Vec::new(); dims.len()];
        for det in dets {
            if let Some((page, bbox)) = grid_det_to_page(det.bbox, run, dims) {
                let db = DetBox {
                    bbox,
                    confidence: det.confidence,
                };
                match det.class {
                    SegClass::Balloon => balloons_per_page[page].push(db),
                    SegClass::Onomatopoeia => sfx_per_page[page].push(db),
                    _ => {}
                }
            }
        }
        // For each page touched by this run, run filter
        let mut touched_pages: Vec<usize> = run.cols.iter().flat_map(|c| c.pages.clone()).collect();
        touched_pages.sort_unstable();
        touched_pages.dedup();
        for page in touched_pages {
            if page >= ocr_boxes.len() {
                continue;
            }
            let entries = &ocr_boxes[page];
            let bboxes: Vec<[f32; 4]> = entries.iter().map(|(bb, _)| *bb).collect();
            let idxs = sfx_filter_indexes(&bboxes, &balloons_per_page[page], &sfx_per_page[page]);
            for idx in idxs {
                let (_, id) = entries[idx];
                to_delete.push((page, id));
            }
        }
    }
    Ok(to_delete)
}

/// Appends per-page entries to their projects, updating `ocr_total`. The
/// assembly itself (resolve, dedup, distribute) lives in [`ocr::assemble`].
#[cfg(feature = "ocr")]
fn commit_per_page(app: &mut App, per_page: Vec<(usize, Vec<NewEntry>)>) {
    for (page, entries) in per_page {
        let Some(image) = app.images.get_mut(page) else {
            continue;
        };
        app.ocr_total += image.project.append_ocr(entries);
    }
}

/// Appends any boundary candidates still held — their bubbles were captured
/// whole on the page above the seam and must not be lost when the next run
/// fails, is cancelled, never starts or took the fallback path.
#[cfg(feature = "ocr")]
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
#[cfg(feature = "ocr")]
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

#[cfg(feature = "ocr")]
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

pub fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::Frame(action) => app.frame.update(action, Message::Frame),
        Message::FetchModels => {
            let ids = app.tx.fetch_ids();
            if ids.is_empty() {
                Task::none()
            } else {
                Task::perform(translation::fetch_providers(ids), Message::ModelsFetched)
            }
        }
        Message::ModelsFetched(providers) => {
            app.tx.on_fetched(providers);
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
                app.scheduler
                    .decode_thumbs(&mut app.images, Message::ThumbDecoded)
            }
            Err(e) => {
                app.status = e;
                Task::none()
            }
        },
        #[cfg(feature = "ocr")]
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
                let workers =
                    scanlateit_settings::get(|s| s.ocr_workers.parse::<usize>().unwrap_or(2))
                        .max(1);
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
        // TEST builds without OCR: append fake entries instantly instead of
        // spawning engines.
        #[cfg(not(feature = "ocr"))]
        Message::Ui(UiEvent::StartOcr) => {
            if app.images.is_empty() {
                app.status = "Open images first.".to_string();
                return Task::none();
            }
            if app.running {
                return Task::none();
            }
            app.running = true;
            let mut added = 0;
            for image in &mut app.images {
                added += image.project.append_ocr(fake_ocr_entries());
            }
            app.running = false;
            app.status = format!("Fake OCR done: {added} line(s) (no OCR engine in this build).");
            Task::none()
        }
        #[cfg(feature = "ocr")]
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
        #[cfg(feature = "ocr")]
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
        #[cfg(feature = "inpaint")]
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
        #[cfg(feature = "styling")]
        Message::StylingEngineReady(result) => match result {
            Ok(engine) => {
                if app.styling.set_engine(engine.clone()) {
                    start_style_jobs(app, engine)
                } else {
                    Task::none()
                }
            }
            Err(e) => {
                app.styling.fail_build();
                app.status = e;
                Task::none()
            }
        },
        #[cfg(feature = "styling")]
        Message::StyleDetected(index, id, result) => {
            if let (Some(image), Ok(style)) = (app.images.get_mut(index), result) {
                image.project.set_entry_style(id, style);
                app.styling.mark_done(index, id);
                app.status = "Applied auto-detected text style.".to_string();
            }
            Task::none()
        }
        #[cfg(feature = "segment")]
        Message::SegmentEngineReady(result) => match result {
            Ok(engine) => {
                app.segment_engine = Some(engine.clone());
                app.segment_filtering = false;
                start_segment_filter(app)
            }
            Err(e) => {
                app.segment_filtering = false;
                app.status = e;
                Task::none()
            }
        },
        #[cfg(feature = "segment")]
        Message::SegmentFiltered(result) => {
            app.segment_filtering = false;
            match result {
                Ok(to_delete) => {
                    let n = to_delete.len();
                    for (idx, id) in to_delete {
                        if let Some(img) = app.images.get_mut(idx) {
                            img.project.delete_entry(id);
                            if app.selected == Some((idx, id)) {
                                app.selected = None;
                            }
                        }
                    }
                    if n > 0 {
                        app.status = format!("SFX filter removed {n} entry(s). {}", app.status);
                    } else {
                        app.status = format!("SFX filter: no entries removed. {}", app.status);
                    }
                }
                Err(e) => {
                    app.status = format!("SFX filter failed: {e}");
                }
            }
            Task::none()
        }
        #[cfg(feature = "inpaint")]
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
                    app.inpaint_mode = false;
                    app.show_inpaint = true;
                    app.status = format!("Inpainted {count} region(s).");
                }
                Err(e) => {
                    app.status = e;
                }
            }
            Task::none()
        }
        #[cfg(feature = "ocr")]
        Message::Ui(UiEvent::StopOcr) => {
            if let Some(token) = &app.cancel {
                token.cancel();
            }
            app.running = false;
            app.status = "Cancelling OCR...".to_string();
            Task::none()
        }
        #[cfg(not(feature = "ocr"))]
        Message::Ui(UiEvent::StopOcr) => {
            app.status = "OCR is not available in this build.".to_string();
            Task::none()
        }
        #[cfg(feature = "ocr")]
        Message::OcrStreamRun(result) => {
            app.pending = app.pending.saturating_sub(1);
            match result {
                Ok(ocr::RunEvent::Canvas {
                    index,
                    width,
                    margin_top,
                    lines,
                }) => {
                    let run = app.ocr_plans[index];
                    let prev = run.dedup.map(|(page, offset)| {
                        let quads: Vec<Quad> = app.images[page]
                            .project
                            .ocr
                            .all()
                            .map(|entry| entry.quad)
                            .collect();
                        (quads, app.images[page].width as u32, offset)
                    });
                    let run_result = ocr::assemble(
                        index,
                        width,
                        margin_top,
                        lines,
                        &app.ocr_plans,
                        &app.ocr_dims,
                        app.held_boundary.take(),
                        prev,
                    );
                    app.held_boundary = run_result.held;
                    commit_per_page(app, run_result.per_page);
                }
                Ok(ocr::RunEvent::Fallback { result, .. }) => {
                    // The fallback produced no re-detections; the previous
                    // run's held candidates cannot be resolved against them
                    // and are flushed so no captured bubble is lost.
                    flush_held_boundary(app);
                    commit_per_page(app, result.per_page);
                }
                Err(e) => {
                    app.ocr_failed += 1;
                    if e == "cancelled" {
                        app.ocr_cancelled = true;
                    }
                }
            }
            #[cfg_attr(not(any(feature = "styling", feature = "segment")), allow(unused_mut))]
            let mut tasks: Vec<Task<Message>> = Vec::new();
            if app.pending == 0 || app.ocr_cancelled {
                finalize_run(app);
                // After OCR is done, auto-filter SFX outside balloons via segmentation grid.
                #[cfg(feature = "segment")]
                if !app.ocr_cancelled && scanlateit_settings::get(|s| s.auto_sfx_filter) {
                    tasks.push(start_segment_filter(app));
                }
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
            #[cfg(feature = "styling")]
            if scanlateit_settings::get(|s| s.auto_style_detect) {
                tasks.push(classify_entries(app));
            }
            if tasks.is_empty() {
                Task::none()
            } else {
                Task::batch(tasks)
            }
        }
        #[cfg(feature = "ocr")]
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
        #[cfg(feature = "ocr")]
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
        Message::SystemFonts(fonts) => {
            app.system_fonts = fonts.into_iter().collect();
            let mut names: Vec<String> = app.system_fonts.keys().cloned().collect();
            names.sort();
            names.dedup();
            app.installed_fonts = names;
            Task::none()
        }
        Message::StyleFontLoaded(name) => {
            app.status = format!("Font \"{name}\" loaded.");
            Task::none()
        }
        Message::Ui(UiEvent::ProfileSelect(id)) => {
            if app.images.is_empty() {
                return Task::none();
            }
            for img in &mut app.images {
                img.project.profiles.select(id);
            }
            let name = app.images[0].project.profiles.selected().name.clone();
            app.status = format!("Profile: {name}");
            Task::none()
        }
        Message::Ui(UiEvent::ProfileCreate) => {
            if app.images.is_empty() {
                return Task::none();
            }
            for img in &mut app.images {
                let name = img.project.profiles.next_available_name();
                let id = img.project.profiles.add(name);
                img.project.profiles.select(id);
            }
            let name = app.images[0].project.profiles.selected().name.clone();
            app.status = format!("Profile: {name} (created)");
            Task::none()
        }
        Message::Ui(UiEvent::TilesVisible(range)) => app
            .scheduler
            .schedule(range, Message::SettleElapsed),
        Message::SettleElapsed(seq) => {
            if app.scheduler.accept_elapsed(seq) {
                app.scheduler
                    .settle(&mut app.images, Message::FullDecoded)
            } else {
                Task::none()
            }
        }
        Message::Ui(UiEvent::TileScrollEnded) => app
            .scheduler
            .settle(&mut app.images, Message::FullDecoded),
        Message::FullDecoded(index, result) => {
            if index < app.images.len() {
                let keep = app.scheduler.keep_full(app.images.len(), index);
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
            if !app.tx.is_connected() {
                app.status = "Connect a translation service in Settings first.".to_string();
                return Task::none();
            }
            let jobs: Vec<(usize, EntryId, String, String)> = app
                .images
                .iter()
                .enumerate()
                .flat_map(|(index, image)| {
                    let filename = translation::file_tag(&image.path);
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
            let (provider, api_key) = match app.tx.selected_provider() {
                Some(provider) => (provider, app.tx.selected_api_key()),
                None => {
                    app.translating = false;
                    app.status = "Translation service is not connected.".to_string();
                    return Task::none();
                }
            };
            let model = app.tx.selected_model.clone();
            app.status = format!(
                "Translating {} line(s) to {} via {model} ({})...",
                jobs.len(),
                app.translate_lang,
                provider.name
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
        Message::Ui(UiEvent::TranslateModelSelect { provider, model }) => {
            app.tx.select_model(provider.clone(), model);
            let _ = scanlateit_settings::modify(|s| s.last_provider = Some(provider));
            Task::none()
        }
        Message::Ui(UiEvent::TranslateLang(lang)) => {
            app.translate_lang = lang;
            Task::none()
        }
        Message::Ui(UiEvent::TranslateConnect(provider_id)) => {
            let is_custom = translation::is_custom(&provider_id);
            let existing = app.tx.connections.get(&provider_id);
            app.connect_modal = Some(ConnectModal {
                provider_id,
                is_custom,
                api_key: existing.map(|c| c.api_key.clone()).unwrap_or_default(),
                base_url: existing
                    .and_then(|c| c.base_url.clone())
                    .unwrap_or_default(),
                model: existing.and_then(|c| c.model.clone()).unwrap_or_default(),
                error: None,
            });
            Task::none()
        }
        Message::Ui(UiEvent::TranslateDisconnect(provider_id)) => {
            let _ = scanlateit_settings::modify(|s| {
                s.connections.remove(&provider_id);
                if s.last_provider.as_deref() == Some(provider_id.as_str()) {
                    s.last_provider = None;
                }
            });
            app.tx.disconnect(&provider_id);
            app.status = format!(
                "Disconnected {}. Its API key was removed.",
                translation::provider_name(&provider_id)
            );
            Task::none()
        }
        Message::Ui(UiEvent::ConnectModalKey(key)) => {
            if let Some(modal) = &mut app.connect_modal {
                modal.api_key = key;
                modal.error = None;
            }
            Task::none()
        }
        Message::Ui(UiEvent::ConnectModalBaseUrl(url)) => {
            if let Some(modal) = &mut app.connect_modal {
                modal.base_url = url;
                modal.error = None;
            }
            Task::none()
        }
        Message::Ui(UiEvent::ConnectModalModel(model)) => {
            if let Some(modal) = &mut app.connect_modal {
                modal.model = model;
                modal.error = None;
            }
            Task::none()
        }
        Message::Ui(UiEvent::ConnectModalSubmit) => {
            let Some(modal) = app.connect_modal.take() else {
                return Task::none();
            };
            if let Some(error) = translation::validate_connection_for(
                &modal.provider_id,
                &modal.api_key,
                &modal.base_url,
                &modal.model,
            ) {
                app.connect_modal = Some(ConnectModal {
                    error: Some(error),
                    ..modal
                });
                return Task::none();
            }
            let id = modal.provider_id.clone();
            let is_local = translation::is_local(&id);
            let is_custom = translation::is_custom(&id);
            let base_url = modal.base_url.trim().to_string();
            let connection = translation::Connection {
                api_key: if is_local {
                    id.clone()
                } else {
                    modal.api_key.trim().to_string()
                },
                base_url: if is_local || is_custom {
                    Some(base_url.clone())
                } else {
                    None
                },
                model: if is_custom {
                    Some(modal.model.trim().to_string())
                } else {
                    None
                },
            };
            let _ = scanlateit_settings::modify(|s| {
                s.connections.insert(id.clone(), connection.clone());
                s.last_provider = Some(id.clone());
            });
            app.tx.connect(id.clone(), connection);
            app.status = format!("Connected {}.", translation::provider_name(&id));
            if is_custom {
                Task::none()
            } else if is_local {
                let base = base_url.clone();
                let fetch_id = id.clone();
                Task::perform(
                    async move {
                        let provider =
                            translation::fetch_local_provider(&fetch_id, &base).await;
                        let mut map = HashMap::new();
                        map.insert(fetch_id, provider);
                        map
                    },
                    Message::ModelsFetched,
                )
            } else {
                Task::perform(
                    translation::fetch_providers(vec![id]),
                    Message::ModelsFetched,
                )
            }
        }
        Message::Ui(UiEvent::ConnectModalCancel) => {
            app.connect_modal = None;
            Task::none()
        }
        Message::Ui(UiEvent::ManageModelsOpen) => {
            app.manage_models_open = true;
            app.manage_models_search.clear();
            Task::none()
        }
        Message::Ui(UiEvent::ManageModelsClose) => {
            app.manage_models_open = false;
            app.manage_models_search.clear();
            Task::none()
        }
        Message::Ui(UiEvent::ManageModelsSearch(query)) => {
            app.manage_models_search = query;
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
                    Task::batch([select_entry(app, index, id), scroll_to_row::<Message>(index, id)])
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
        Message::Ui(UiEvent::RetranslateEntry((index, entry_id))) => {
            if app.translating || app.running {
                return Task::none();
            }
            if !app.tx.is_connected() {
                app.status = "Connect a translation service in Settings first.".to_string();
                return Task::none();
            }
            let (text, filename, context_items) = {
                let Some(image) = app.images.get(index) else {
                    app.status = "That result no longer exists.".to_string();
                    return Task::none();
                };
                let Some(entry) = image.project.ocr.get(entry_id) else {
                    app.status = "That result no longer exists.".to_string();
                    return Task::none();
                };
                let filename = translation::file_tag(&image.path);
                let context_items: Vec<translation::TranslateItem> = image
                    .project
                    .ocr
                    .visible()
                    .map(|e| translation::TranslateItem {
                        filename: filename.clone(),
                        id: e.id.0,
                        text: e.text.clone(),
                    })
                    .collect();
                (entry.text.clone(), filename, context_items)
            };
            let target = app.translate_lang.clone();
            let (provider, api_key) = match app.tx.selected_provider() {
                Some(provider) => (provider, app.tx.selected_api_key()),
                None => {
                    app.status = "Translation service is not connected.".to_string();
                    return Task::none();
                }
            };
            let model = app.tx.selected_model.clone();
            app.translating = true;
            app.status = format!(
                "Retranslating 1 line to {} via {model} ({})...",
                app.translate_lang, provider.name
            );
            Task::perform(
                async move {
                    let result = translation::translate_one_with_context(
                        &text,
                        &target,
                        &provider,
                        &model,
                        api_key,
                        &context_items,
                        entry_id.0,
                        &filename,
                    )
                    .await;
                    ((index, entry_id), result)
                },
                |(job, result)| Message::RetranslateFinished(job, result),
            )
        }
        Message::Ui(UiEvent::Inpaint) => {
            if app.inpainting || app.running || app.translating || app.images.is_empty() {
                return Task::none();
            }
            app.inpaint_mode = !app.inpaint_mode;
            app.status = if app.inpaint_mode {
                "Inpaint mode: drag a rectangle over the text to remove; \
                           click Inpaint again to cancel."
                    .to_string()
            } else {
                "Inpaint mode cancelled.".to_string()
            };
            Task::none()
        }
        Message::Ui(UiEvent::ToggleOverlayText) => {
            app.show_overlay_text = !app.show_overlay_text;
            app.status = if app.show_overlay_text {
                "Overlay text shown."
            } else {
                "Overlay text hidden."
            }
            .to_string();
            Task::none()
        }
        Message::Ui(UiEvent::ToggleInpaintLayer) => {
            app.show_inpaint = !app.show_inpaint;
            app.status = if app.show_inpaint {
                "Inpaint layer shown."
            } else {
                "Inpaint layer hidden."
            }
            .to_string();
            Task::none()
        }
        Message::Ui(UiEvent::MainAreaMode(mode)) => {
            app.view_mode = mode;
            app.status = match mode {
                MainAreaMode::View => "View mode: single column with overlay.".to_string(),
                MainAreaMode::Compare => {
                    "Compare mode: original (left) vs current (right), scrolling in sync."
                        .to_string()
                }
            };
            Task::none()
        }
        Message::Ui(UiEvent::ViewerScroll(offset)) => {
            app.viewer_scroll = offset;
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
        },
        Message::Ui(UiEvent::EntryMoved((index, id, quad))) => {
            if let Some(image) = app.images.get_mut(index) {
                image.project.set_view_quad(id, quad);
            }
            Task::none()
        }
        #[cfg(feature = "inpaint")]
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
                .filter(|quad| quad.intersects_rect(rect))
                .collect();
            if quads.is_empty() {
                app.status = "Inpaint: no OCR boxes in the range; the whole selection \
                              will be cleaned."
                    .to_string();
            }
            let path = image.path.clone();
            let (backend, radius) = scanlateit_settings::get(|s| {
                (
                    s.inpaint_backend,
                    s.inpaint_radius.parse::<i32>().unwrap_or(5).max(1),
                )
            });
            let cached = app
                .inpaint_engine
                .clone()
                .filter(|engine| engine.backend() == backend && engine.radius() == radius);
            match cached {
                Some(engine) => start_inpaint(app, engine, index, path, rect, quads),
                None => {
                    app.pending_inpaint = Some((index, path, rect, quads));
                    app.status = match backend {
                        InpaintBackend::Lama => "Loading the inpainting model...".to_string(),
                        InpaintBackend::Telea => "Inpainting...".to_string(),
                    };
                    Task::perform(
                        async move { InpaintEngine::build(backend, radius) },
                        Message::InpaintEngineReady,
                    )
                }
            }
        }
        #[cfg(not(feature = "inpaint"))]
        Message::Ui(UiEvent::InpaintSelection(_)) => {
            app.status = "Inpaint is not available in this build.".to_string();
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
                if let Some(name) = project.profiles.fork_for_edit() {
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
            app.images[index].project.set_entry_style(id, app.style_working.clone());
            Task::none()
        }
        Message::Ui(UiEvent::StyleItalic(italic)) => {
            let Some((index, id)) = app.selected else { return Task::none() };
            app.style_working.italic = italic;
            app.images[index].project.set_entry_style(id, app.style_working.clone());
            Task::none()
        }
        Message::Ui(UiEvent::StyleFont(name)) => {
            let Some((index, id)) = app.selected else { return Task::none() };
            app.style_working.font_family = Some(name.clone());
            app.images[index].project.set_entry_style(id, app.style_working.clone());
            if !app.loaded_fonts.contains(&name) {
                app.loaded_fonts.insert(name.clone());
                let Some(path) = app.system_fonts.get(&name).cloned() else {
                    return Task::none();
                };
                match std::fs::read(path) {
                    Ok(bytes) => iced::font::load(bytes).map(move |_| Message::StyleFontLoaded(name.clone())),
                    Err(_) => Task::none(),
                }
            } else {
                Task::none()
            }
        }
        Message::Ui(UiEvent::StyleTextAlign(align)) => {
            let Some((index, id)) = app.selected else { return Task::none() };
            app.style_working.text_align = align;
            app.images[index].project.set_entry_style(id, app.style_working.clone());
            Task::none()
        }
        Message::Ui(UiEvent::StyleGradientToggle(enabled)) => {
            let Some((index, id)) = app.selected else { return Task::none() };
            app.style_working.text_gradient = enabled;
            app.images[index].project.set_entry_style(id, app.style_working.clone());
            Task::none()
        }
        Message::Ui(UiEvent::StyleGradientDir(dir)) => {
            let Some((index, id)) = app.selected else { return Task::none() };
            app.style_working.gradient_dir = dir;
            app.images[index].project.set_entry_style(id, app.style_working.clone());
            Task::none()
        }
        Message::Ui(UiEvent::StyleColorOpen(field)) => {
            app.style_picker = Some(field);
            Task::none()
        }
        Message::Ui(UiEvent::StyleColorCancel(_field)) => {
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
                StyleField::GradientA => app.style_working.gradient_a = rgba,
                StyleField::GradientB => app.style_working.gradient_b = rgba,
            }
            app.images[index].project.set_entry_style(id, app.style_working.clone());
            Task::none()
        }
        Message::Ui(UiEvent::StyleStrokeWidth(text)) => {
            let Some((index, id)) = app.selected else { return Task::none() };
            app.style_stroke_width = text;
            if let Ok(width) = app.style_stroke_width.parse::<f32>() {
                app.style_working.stroke_width = width.max(0.0);
                app.images[index].project.set_entry_style(id, app.style_working.clone());
            }
            Task::none()
        }
        Message::Ui(UiEvent::StyleBgRadius(text)) => {
            let Some((index, id)) = app.selected else { return Task::none() };
            app.style_bg_radius = text;
            if let Ok(radius) = app.style_bg_radius.parse::<f32>() {
                app.style_working.bg_radius = radius.max(0.0);
                app.images[index].project.set_entry_style(id, app.style_working.clone());
            }
            Task::none()
        }
        Message::Ui(UiEvent::StylePresetApply(preset)) => {
            let Some((index, id)) = app.selected else { return Task::none() };
            let Some(preset_style) = app.presets.get(preset) else {
                return Task::none();
            };
            seed_style_inputs(app, preset_style.clone());
            app.images[index].project.set_entry_style(id, preset_style);
            Task::none()
        }
        Message::Ui(UiEvent::StylePresetAdd) => {
            app.presets.add(app.style_working.clone());
            Task::none()
        }
        Message::Ui(UiEvent::StylePresetReplace(preset)) => {
            app.presets.replace(preset, app.style_working.clone());
            Task::none()
        }
        Message::Ui(UiEvent::StylePresetRemove(preset)) => {
            app.presets.remove(preset);
            Task::none()
        }
        Message::Ui(UiEvent::StylePresetMenuDismiss) => Task::none(),
        #[cfg(feature = "styling")]
        Message::Ui(UiEvent::StyleAutoDetect) => {
            let Some((index, id)) = app.selected else { return Task::none() };
            // The entry must leave the done set so it is eligible again, even
            // if auto-detect already classified it.
            app.styling.reopen(index, id);
            classify_entries(app)
        }
        // TEST builds without the styling model: apply a canned default
        // style instantly so the styling panel still has something to show.
        #[cfg(not(feature = "styling"))]
        Message::Ui(UiEvent::StyleAutoDetect) => {
            let Some((index, id)) = app.selected else { return Task::none() };
            let style = EntryStyle {
                bold: true,
                italic: false,
                ..EntryStyle::default()
            };
            app.images[index].project.set_entry_style(id, style);
            app.status = "Applied a fake auto-detected text style (no styling model in this build)."
                .to_string();
            Task::none()
        }
        Message::Ui(UiEvent::PanelResized(resized)) => {
            app.panes.resize(resized.split, resized.ratio);
            Task::none()
        }
        Message::Ui(UiEvent::SidePanelResized(resized)) => {
            app.side_panes.resize(resized.split, resized.ratio);
            Task::none()
        }
        Message::Ui(UiEvent::StylingPaneResized(resized)) => {
            app.styling_panes.resize(resized.split, resized.ratio);
            Task::none()
        }
        Message::Ui(UiEvent::SettingsOpen) => {
            app.settings_open = true;
            Task::none()
        }
        Message::Ui(UiEvent::SettingsOpenTab(tab)) => {
            app.settings_open = true;
            app.settings_tab = tab;
            Task::none()
        }
        Message::Ui(UiEvent::SettingsClose) => {
            app.settings_open = false;
            app.manage_models_open = false;
            app.connect_modal = None;
            Task::none()
        }
        Message::Ui(UiEvent::SettingsTab(tab)) => {
            app.settings_tab = tab;
            Task::none()
        }
        Message::Ui(UiEvent::SettingsChanged) => {
            // The ui crate already wrote the change into the settings store;
            // re-sync every runtime mirror from there. This is the single
            // message for all settings edits.
            sync_tx_from_store(app);
            app.status = "Settings saved.".to_string();
            Task::none()
        }
        Message::Ui(UiEvent::SettingEdit(edit)) => {
            // Button-driven edits are deferred (button builders evaluate
            // eagerly during view): apply the named change now, then sync.
            let _ = scanlateit_settings::modify(|s| match edit {
                scanlateit_ui::event::SettingEdit::AuroraDarkMode(v) => s.aurora_is_dark = v,
                scanlateit_ui::event::SettingEdit::AuroraBlobCount(v) => {
                    s.aurora_blob_count = v.clamp(1, 5);
                }
                scanlateit_ui::event::SettingEdit::AuroraSchema(v) => s.aurora_schema = v % 4,
                scanlateit_ui::event::SettingEdit::HiddenModelsReset(provider) => {
                    s.hidden_models.remove(&provider);
                }
                scanlateit_ui::event::SettingEdit::HiddenModelsResetAll => {
                    s.hidden_models.clear();
                }
                scanlateit_ui::event::SettingEdit::UiFontSize(v) => {
                    s.ui_font_size = v.clamp(8, 30);
                }
            });
            sync_tx_from_store(app);
            app.status = "Settings saved.".to_string();
            Task::none()
        }
        Message::Ui(UiEvent::OpenUrl(url)) => {
            if let Err(e) = open::that(&url) {
                eprintln!("[app] failed to open {url}: {e}");
                app.status = format!("Failed to open {url}: {e}");
            }
            Task::none()
        }
        Message::TranslateFinished(jobs, result) => {
            app.translating = false;
            match result {
                Ok(translations) => {
                    let profile_name = translation::profile_name(&app.translate_lang);
                    if translations.len() != jobs.len() {
                        // Legacy mismatch: store positionally what we can, but warn.
                        let mut saved = 0usize;
                        for ((image_index, entry_id, _path, _text), translation) in
                            jobs.iter().zip(translations.iter())
                        {
                            if translation.is_empty() {
                                continue;
                            }
                            let image = &mut app.images[*image_index];
                            image
                                .project
                                .store_translation(&profile_name, *entry_id, Some(translation.clone()));
                            saved += 1;
                        }
                        app.status = format!(
                            "Translated {saved} of {} line(s) into '{profile_name}' (count mismatch, partial).",
                            jobs.len()
                        );
                    } else {
                        let mut saved = 0usize;
                        let mut skipped = 0usize;
                        for ((image_index, entry_id, _path, _text), translation) in
                            jobs.iter().zip(translations.iter())
                        {
                            if translation.is_empty() {
                                skipped += 1;
                                continue;
                            }
                            let image = &mut app.images[*image_index];
                            image
                                .project
                                .store_translation(&profile_name, *entry_id, Some(translation.clone()));
                            saved += 1;
                        }
                        if skipped > 0 {
                            app.status = format!(
                                "Translated {saved} of {} line(s) into '{profile_name}' ({skipped} still missing after retry, skipped).",
                                jobs.len()
                            );
                        } else {
                            app.status = format!(
                                "Translated {saved} line(s) into '{profile_name}'."
                            );
                        }
                    }
                }
                Err(e) => {
                    app.status = e;
                }
            }
            Task::none()
        }
        Message::RetranslateFinished((index, entry_id), result) => {
            app.translating = false;
            match result {
                Ok(mut text) => {
                    if text.len() >= 2 {
                        let quoted = (text.starts_with('"') && text.ends_with('"'))
                            || (text.starts_with('\'') && text.ends_with('\''));
                        if quoted {
                            text = text[1..text.len() - 1].to_string();
                        }
                    }
                    let Some(image) = app.images.get_mut(index) else {
                        app.status = "Retranslated, but that image is gone.".to_string();
                        return Task::none();
                    };
                    let equals_original = image
                        .project
                        .ocr
                        .get(entry_id)
                        .is_some_and(|entry| entry.text == text);
                    let stored = if equals_original { None } else { Some(text) };
                    let forked_name = image.project.profiles.fork_for_edit();
                    image
                        .project
                        .profiles
                        .selected_mut()
                        .set_translation(entry_id, stored);
                    let label = forked_name
                        .unwrap_or_else(|| image.project.profiles.selected().name.clone());
                    app.status = format!("Retranslated 1 line into '{label}'.");
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
    let frame_sub = app.frame.subscription().map(Message::Frame);

    #[cfg(feature = "ocr")]
    if app.running {
        return Subscription::batch([
            frame_sub,
            iced::time::every(Duration::from_millis(16)).map(|_| Message::OcrTick),
        ]);
    }
    frame_sub
}

fn title_icon_handle() -> Option<iced::widget::image::Handle> {
    use std::sync::OnceLock;
    static ICON: OnceLock<Option<iced::widget::image::Handle>> = OnceLock::new();
    ICON.get_or_init(|| {
        const BYTES: &[u8] = include_bytes!("../app_icon.ico");
        match image::load_from_memory(BYTES) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let (w, h) = (rgba.width(), rgba.height());
                Some(iced::widget::image::Handle::from_rgba(w, h, rgba.into_raw()))
            }
            Err(_) => None,
        }
    })
    .clone()
}

pub fn view(app: &App) -> Element<'_, Message> {
    let grid: Element<'_, UiEvent> = pane_grid::PaneGrid::new(&app.panes, |_, kind, _| {
        pane_grid::Content::new(match kind {
            PaneKind::MainArea => {
                let el: Element<'_, UiEvent> = iced::widget::container(main_area::view(app))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(|_theme| iced::widget::container::Style {
                        background: Some(panel::PANEL_BG.into()),
                        border: iced::Border::default().rounded(scale::s(CARD_RADIUS)),
                        ..Default::default()
                    })
                    .into();
                el
            }
            PaneKind::Panel => {
                // Inner split: styling ↔ translation/results, drag resizable with same GAP — base at 12pt.
                let side_grid: Element<'_, UiEvent> =
                    pane_grid::PaneGrid::new(&app.side_panes, |_, inner, _| {
                        pane_grid::Content::new(match inner {
                            SidePaneKind::Styling => {
                                // Vertical stack inside the styling column: inspector (top, taller) + layers/inpaint (bottom, shorter).
                                // Each has its own card BG and the gap between them shows the aurora.
                                let el: Element<'_, UiEvent> = pane_grid::PaneGrid::new(
                                    &app.styling_panes,
                                    |_, kind, _| {
                                        let body: Element<'_, UiEvent> = match kind {
                                            StylingPaneKind::Inspector => {
                                                iced::widget::container(panel::styling::view(app))
                                                    .padding(scale::s(10.0))
                                                    .width(Length::Fill)
                                                    .height(Length::Fill)
                                                    .style(|_theme| {
                                                        iced::widget::container::Style {
                                                            background: Some(
                                                                panel::PANEL_BG.into(),
                                                            ),
                                                            border: iced::Border::default()
                                                                .rounded(scale::s(CARD_RADIUS)),
                                                            ..Default::default()
                                                        }
                                                    })
                                                    .into()
                                            }
                                            StylingPaneKind::Layers => {
                                                iced::widget::container(
                                                    panel::inpaint::view(app),
                                                )
                                                .padding(scale::s(10.0))
                                                .width(Length::Fill)
                                                .height(Length::Fill)
                                                .style(|_theme| {
                                                    iced::widget::container::Style {
                                                        background: Some(
                                                            panel::PANEL_BG.into(),
                                                        ),
                                                        border: iced::Border::default()
                                                            .rounded(scale::s(CARD_RADIUS)),
                                                        ..Default::default()
                                                    }
                                                })
                                                .into()
                                            }
                                        };
                                        pane_grid::Content::new(body)
                                    },
                                )
                                .spacing(scale::s(GAP))
                                .min_size(scale::s(90.0))
                                .on_resize(scale::s(GAP), UiEvent::StylingPaneResized)
                                .width(Length::Fill)
                                .height(Length::Fill)
                                .into();
                                el
                            }
                            SidePaneKind::Results => {
                                let el: Element<'_, UiEvent> = iced::widget::container(
                                    panel::results::view(app),
                                )
                                .padding(scale::s(10.0))
                                .width(Length::Fill)
                                .height(Length::Fill)
                                .style(|_theme| iced::widget::container::Style {
                                    background: Some(panel::PANEL_BG.into()),
                                    border: iced::Border::default().rounded(scale::s(CARD_RADIUS)),
                                    ..Default::default()
                                })
                                .into();
                                el
                            }
                        })
                    })
                    .spacing(scale::s(GAP))
                    .min_size(scale::s(120.0))
                    .on_resize(scale::s(GAP), UiEvent::SidePanelResized)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into();

                // Action bar is transparent; styling/results are translucent cards with GAP between them.
                let el: Element<'_, UiEvent> =
                    iced::widget::column![panel::actions::view(app), side_grid]
                        .spacing(scale::s(GAP))
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into();
                el
            }
        })
    })
    .spacing(scale::s(GAP))
    .min_size(scale::s(160.0))
    .on_resize(scale::s(GAP), UiEvent::PanelResized)
    .width(Length::Fill)
    .height(Length::Fill)
    .into();
    let content: Element<'_, UiEvent> = iced::widget::row![toolbar::view(app), grid]
        .spacing(scale::s(GAP))
        .height(Length::Fill)
        .into();
    // OUTER_PADDING is applied to the content only – the title bar stays
    // edge-to-edge (outer_padding = 0 on the frame) per the requirements.
    let padded_content: Element<'_, UiEvent> = iced::widget::container(content)
        .padding(scale::s(OUTER_PADDING))
        .width(Length::Fill)
        .height(Length::Fill)
        .into();

    // Modals are chained at the UiEvent level before framing; they dim the
    // content but the title bar (outside the frame's content) stays visible.
    // Aurora is kept outside the frame so it can show through the
    // transparent title bar.
    let inner_with_modals: Element<'_, UiEvent> = {
        let base: Element<'_, UiEvent> = padded_content;
        let v: Element<'_, UiEvent> = if app.settings_open {
            settings_modal::view(app, base)
        } else {
            base
        };
        let v: Element<'_, UiEvent> = if app.connect_modal.is_some() {
            scanlateit_ui::connect::view(app, v)
        } else {
            v
        };
        if app.manage_models_open {
            scanlateit_ui::manage_models::view(app, v)
        } else {
            v
        }
    };
    let inner_mapped: Element<'_, Message> = inner_with_modals.map(Message::from);

    // Frame it – single-window via `primary_window`. The frame's own title
    // is left empty; we draw a truly-centered title+icon ourselves.
    let framed: Element<'_, Message> = if let Some(window_id) = app.frame.primary_window() {
        app.frame.view(window_id, "", None, None, inner_mapped, Message::Frame)
    } else {
        // First frame before `install_latest` resolves: show content undecorated.
        inner_mapped
    };

    // Aurora behind the transparent frame (title bar + surface).
    let aurora_cfg = scanlateit_ui::background::AuroraConfig::from_store();
    let aurora: Element<'_, Message> =
        scanlateit_ui::background::AuroraBackground::new(aurora_cfg)
            .view()
            .map(Message::from);
    let base_with_aurora: Element<'_, Message> =
        iced::widget::Stack::with_children(vec![aurora, framed]).into();

    // Truly centered title + app icon. The library's `show_title` is false
    // and `window_icon` is None – this overlay is centered in the window,
    // not in the filler between the icon and the caption buttons.
    let title_overlay: Element<'_, Message> = {
        let h = app.frame.config().title_bar_height;
        let is_dark = scanlateit_settings::get(|s| s.aurora_is_dark);
        let title_color = if is_dark {
            Color::from_rgb(0.92, 0.92, 0.92)
        } else {
            Color::from_rgb(0.12, 0.12, 0.12)
        };
        let icon_element: Element<'_, Message> = match title_icon_handle() {
            Some(handle) => iced::widget::image(handle)
                .width(Length::Fixed(16.0))
                .height(Length::Fixed(16.0))
                .into(),
            None => iced::widget::space::horizontal()
                .width(Length::Fixed(16.0))
                .height(Length::Fixed(16.0))
                .into(),
        };
        let row = iced::widget::row![
            icon_element,
            iced::widget::text("Scanlateit").size(13).color(title_color)
        ]
        .spacing(8)
        .align_y(iced::Center);
        iced::widget::container(row)
            .width(Length::Fill)
            .height(Length::Fixed(h))
            .center_x(Length::Fill)
            .center_y(Length::Fixed(h))
            .into()
    };

    // Stack the centered overlay on top of the aurora+frame. The overlay
    // is inert (no mouse_area) so drags/double-clicks pass through to the
    // frame's draggable filler underneath; only the caption buttons capture.
    let title_bar_container: Element<'_, Message> = iced::widget::container(title_overlay)
        .width(Length::Fill)
        .height(Length::Fixed(app.frame.config().title_bar_height))
        .into();

    iced::widget::Stack::with_children(vec![base_with_aurora, title_bar_container]).into()
}
