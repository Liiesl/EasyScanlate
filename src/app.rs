use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
#[cfg(all(feature = "test-ui", not(feature = "translation")))]
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

/// Natural ordering for file paths: numeric chunks compared as numbers
/// so `2.jpg < 10.jpg < 11.jpg` instead of lexical `10 < 11 < 2`.
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut a_chars = a.chars().peekable();
    let mut b_chars = b.chars().peekable();
    loop {
        match (a_chars.peek(), b_chars.peek()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(&ac), Some(&bc)) => {
                let a_digit = ac.is_ascii_digit();
                let b_digit = bc.is_ascii_digit();
                if a_digit && b_digit {
                    let mut a_num = String::new();
                    while let Some(&c) = a_chars.peek() {
                        if c.is_ascii_digit() { a_num.push(c); a_chars.next(); } else { break; }
                    }
                    let mut b_num = String::new();
                    while let Some(&c) = b_chars.peek() {
                        if c.is_ascii_digit() { b_num.push(c); b_chars.next(); } else { break; }
                    }
                    let a_trim = a_num.trim_start_matches('0');
                    let b_trim = b_num.trim_start_matches('0');
                    // empty means value 0
                    let a_trim = if a_trim.is_empty() { "0" } else { a_trim };
                    let b_trim = if b_trim.is_empty() { "0" } else { b_trim };
                    match a_trim.len().cmp(&b_trim.len()) {
                        Ordering::Equal => match a_trim.cmp(b_trim) {
                            Ordering::Equal => {
                                // same numeric value → fewer leading zeros first for stability
                                match a_num.len().cmp(&b_num.len()) {
                                    Ordering::Equal => continue,
                                    ord => return ord,
                                }
                            }
                            ord => return ord,
                        },
                        ord => return ord,
                    }
                } else {
                    let mut a_chunk = String::new();
                    while let Some(&c) = a_chars.peek() {
                        if !c.is_ascii_digit() { a_chunk.push(c); a_chars.next(); } else { break; }
                    }
                    let mut b_chunk = String::new();
                    while let Some(&c) = b_chars.peek() {
                        if !c.is_ascii_digit() { b_chunk.push(c); b_chars.next(); } else { break; }
                    }
                    let ord = a_chunk.to_ascii_lowercase().cmp(&b_chunk.to_ascii_lowercase());
                    if ord != Ordering::Equal { return ord; }
                    let ord2 = a_chunk.cmp(&b_chunk);
                    if ord2 != Ordering::Equal { return ord2; }
                }
            }
        }
    }
}

use iced::widget::{pane_grid, text_editor};
use iced::{Color, Element, Font, Rectangle, Subscription, Task, Theme};
use neverliie_iced_widgets::title_bar::{FrameAction, NativeFrame};

#[cfg(feature = "inpaint")]
use scanlateit_inpaint::Engine as InpaintEngine;
use scanlateit_model::{EntryId, EntryStyle, ModelEvent, NewEntry, Project, Quad};
use scanlateit_settings::StylePresets;
#[cfg(feature = "inpaint")]
use scanlateit_settings::InpaintBackend;
#[cfg(feature = "ocr")]
use scanlateit_ocr::{self as ocr_engine, OcrCancellationToken, ParallelEngine};
#[cfg(feature = "styling")]
use scanlateit_styling::{Engine as StylingEngine, JobTracker};
#[cfg(feature = "segment")]
use scanlateit_segment::Engine as SegmentEngine;
use scanlateit_ui::translation as ui_translation;
use scanlateit_ui::main_area::decode::{DecodedPage, PageDecode, Scheduler, Tier};
use scanlateit_ui::{
    event::{EditOrigin, MainAreaMode, SettingsTab, StyleField, TargetProfileSelection, TranslationPanelMode, UiEvent},
    ConnectModal, LoadedImage,
};

// ---------------------------------------------------------------------------
// Sub-modules
// ---------------------------------------------------------------------------
pub mod layout;
pub mod chrome;
pub mod boot;
pub mod state;
pub mod edit;
pub mod ocr;
pub mod inpaint;
pub mod styling;
pub mod segment;
pub mod pipeline;
pub mod translation;
pub mod settings;
pub mod mmtl;
pub mod export;
pub mod view;

use layout::{PaneKind, SidePaneKind, StylingPaneKind};
use layout::{IMAGE_FILTERS, MAIN_AREA_DEFAULT_RATIO, STYLING_DEFAULT_RATIO, STYLING_TOP_RATIO};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppView {
    Home,
    Editor,
}

#[derive(Debug, Clone)]
pub struct NewProjectState {
    pub source_files: Vec<(String, u32, u32)>,
    pub original_lang: String,
    pub project_location: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct AutoInpaintJob {
    pub index: usize,
    pub id: EntryId,
    pub path: String,
    pub quad: Quad,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Message {
    /// Frame actions from the custom title bar.
    Frame(FrameAction),
    /// A widget-level event from the ui crate.
    Ui(UiEvent),
    /// Granular model change event (synchronously emitted by `Project` mutators).
    Model(ModelEvent),
    ImagesPicked(Result<Vec<(String, u32, u32)>, String>),
    #[cfg(feature = "ocr")]
    ParallelEngineReady(Result<ParallelEngine, String>),
    #[cfg(feature = "ocr")]
    ManualOcrEngineReady(Result<scanlateit_ocr::Engine, String>),
    #[cfg(feature = "ocr")]
    ManualOcrFinished(usize, Result<Vec<NewEntry>, String>),
    #[cfg(feature = "ocr")]
    ManualOcrSpanFinished(Result<Vec<(usize, Vec<NewEntry>)>, String>),
    #[cfg(feature = "ocr")]
    OcrStreamRun(Result<ocr_engine::RunEvent, String>),
    #[cfg(feature = "ocr")]
    OcrStreamFailed(String),
    #[cfg(feature = "ocr")]
    OcrTick,
    #[cfg(feature = "inpaint")]
    InpaintEngineReady(Result<InpaintEngine, String>),
    #[cfg(feature = "inpaint")]
    InpaintFinished(usize, Result<Vec<(image::RgbaImage, [f32; 4])>, String>),
    #[cfg(feature = "inpaint")]
    InpaintSpanFinished(Result<Vec<(usize, Vec<(image::RgbaImage, [f32; 4])>)>, String>),
    #[cfg(feature = "inpaint")]
    AutoInpaintEngineReady(InpaintBackend, Result<InpaintEngine, String>),
    #[cfg(feature = "inpaint")]
    AutoInpaintFinished(usize, EntryId, Result<Vec<(image::RgbaImage, [f32; 4])>, String>),
    #[cfg(feature = "inpaint")]
    AutoInpaintLamaBatchFinished(Vec<(usize, EntryId, Result<Vec<(image::RgbaImage, [f32; 4])>, String>)>),
    #[cfg(feature = "inpaint")]
    AutoInpaintAotBatchFinished(Vec<(usize, EntryId, Result<Vec<(image::RgbaImage, [f32; 4])>, String>)>),
    #[cfg(all(feature = "styling", feature = "inpaint"))]
    PipelineStyleDetected(usize, EntryId, Result<(EntryStyle, scanlateit_styling::StylePrediction), String>),
    #[cfg(feature = "styling")]
    StylingEngineReady(Result<StylingEngine, String>),
    #[cfg(feature = "styling")]
    StyleDetected(usize, EntryId, Result<EntryStyle, String>),
    #[cfg(feature = "segment")]
    SegmentEngineReady(Result<SegmentEngine, String>),
    #[cfg(feature = "segment")]
    SegmentFiltered(Result<Vec<(usize, EntryId)>, String>),
    FontLoaded,
    SystemFonts(Vec<(String, String)>),
    StyleFontLoaded(String),
    ThumbDecoded(usize, Result<Arc<DecodedPage>, String>),
    FullDecoded(usize, Result<Arc<DecodedPage>, String>),
    SettleElapsed(u64),
    FetchModels,
    ModelsFetched(std::collections::HashMap<String, ui_translation::Provider>),
    TranslateFinished(
        Vec<(usize, EntryId, String, String)>,
        Result<Vec<String>, String>,
    ),
    RetranslateFinished((usize, EntryId), Result<String, String>),
    MmtlSavePicked(Option<String>),
    MmtlOpenPicked(Option<String>),
    MmtlSaved(Result<String, String>),
    MmtlLoaded(Result<(Project, Vec<LoadedImage>, String, Option<std::sync::Arc<tempfile::TempDir>>), String>),
    NewProjectSourcePicked(Result<Vec<(String, u32, u32)>, String>),
    NewProjectFolderPicked(Result<Vec<(String, u32, u32)>, String>),
    NewProjectLocationPicked(Option<String>),
    CreateProjectPicked(Result<String, String>),
    RecentPickedToLoad(Result<(Project, Vec<LoadedImage>, String, Option<std::sync::Arc<tempfile::TempDir>>), String>),
    ExportFolderPicked(Option<String>),
    ExportFinished(Result<String, String>),
}

impl From<UiEvent> for Message {
    fn from(event: UiEvent) -> Self {
        Message::Ui(event)
    }
}

/// Session state: chapter-wide model (`project`) plus per-image view caches.
pub struct App {
    pub(crate) project: Project,
    pub(crate) images: Vec<LoadedImage>,
    #[cfg(feature = "ocr")]
    pipeline: Option<ParallelEngine>,
    #[cfg(feature = "ocr")]
    cancel: Option<OcrCancellationToken>,
    #[cfg(feature = "ocr")]
    ocr_plans: Vec<ocr_engine::RunPlan>,
    #[cfg(feature = "ocr")]
    ocr_dims: Vec<(u32, u32)>,
    #[cfg(feature = "inpaint")]
    inpaint_engine: Option<InpaintEngine>,
    #[cfg(feature = "inpaint")]
    pending_inpaint: Option<(usize, String, [f32; 4], Vec<Quad>)>,
    #[cfg(feature = "inpaint")]
    pending_inpaint_span: Option<Vec<(usize, String, [f32; 4], Vec<Quad>)>>,
    pub(crate) inpainting: bool,
    pub(crate) inpaint_mode: bool,
    pub(crate) ocr_mode: bool,
    #[cfg(feature = "ocr")]
    pub(crate) manual_ocring: bool,
    #[cfg(feature = "ocr")]
    manual_ocr_engine: Option<scanlateit_ocr::Engine>,
    #[cfg(feature = "ocr")]
    pending_manual_ocr: Option<(usize, Rectangle)>,
    #[cfg(feature = "ocr")]
    pending_manual_ocr_span: Option<Vec<(usize, Rectangle)>>,
    pub(crate) show_overlay_text: bool,
    pub(crate) show_inpaint: bool,
    pub(crate) view_mode: MainAreaMode,
    /// Normalized center anchor `0..1` (`(offset+viewport/2)/content_height`);
    /// mirrored across Compare panes and stable across resize / `View↔Compare`.
    pub(crate) viewer_scroll: f32,
    #[cfg(feature = "styling")]
    styling: JobTracker,
    #[cfg(feature = "segment")]
    segment_engine: Option<SegmentEngine>,
    #[cfg(feature = "segment")]
    segment_filtering: bool,
    #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
    pipeline_active: bool,
    #[cfg(all(feature = "styling", feature = "inpaint"))]
    pipeline_style_pending: usize,
    #[cfg(all(feature = "styling", feature = "inpaint"))]
    pipeline_style_results: Vec<(usize, EntryId, Result<(EntryStyle, scanlateit_styling::StylePrediction), String>, Quad, String)>,
    #[cfg(feature = "inpaint")]
    auto_inpaint_pending: usize,
    #[cfg(feature = "inpaint")]
    auto_telea_engine: Option<InpaintEngine>,
    #[cfg(feature = "inpaint")]
    auto_lama_engine: Option<InpaintEngine>,
    #[cfg(feature = "inpaint")]
    auto_aot_engine: Option<InpaintEngine>,
    #[cfg(feature = "inpaint")]
    pending_auto_telea_jobs: Option<Vec<AutoInpaintJob>>,
    #[cfg(feature = "inpaint")]
    pending_auto_lama_jobs: Option<Vec<AutoInpaintJob>>,
    #[cfg(feature = "inpaint")]
    pending_auto_aot_jobs: Option<Vec<AutoInpaintJob>>,
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
    #[cfg(feature = "ocr")]
    ocr_runs: usize,
    #[cfg(feature = "ocr")]
    held_boundary: Option<ocr_engine::BoundaryState>,
    pub(crate) translating: bool,
    pub(crate) tx: ui_translation::Session,
    pub(crate) translate_lang: String,
    pub(crate) translation_panel_mode: TranslationPanelMode,
    pub(crate) translate_base: Option<scanlateit_model::ProfileId>,
    pub(crate) translate_target: TargetProfileSelection,
    pub(crate) connect_modal: Option<ConnectModal>,
    pub(crate) settings_open: bool,
    pub(crate) settings_tab: SettingsTab,
    pub(crate) settings_search: String,
    pub(crate) manage_models_open: bool,
    pub(crate) manage_models_search: String,
    pub(crate) selected: Option<(usize, EntryId)>,
    pub(crate) selected_inpaint: Option<(usize, usize)>,
    pub(crate) editing: Option<(usize, EntryId)>,
    pub(crate) editing_origin: EditOrigin,
    pub(crate) edit_content: Option<text_editor::Content>,
    pub(crate) editing_dirty: bool,
    pub(crate) editing_rect: Option<Rectangle>,
    scheduler: Scheduler,
    pub(crate) style_working: EntryStyle,
    pub(crate) system_fonts: HashMap<String, String>,
    pub(crate) installed_fonts: Vec<String>,
    pub(crate) loaded_fonts: HashSet<String>,
    pub(crate) style_picker: Option<StyleField>,
    pub(crate) style_stroke_width: String,
    pub(crate) style_bg_radius: String,
    pub(crate) style_hex_overrides: HashMap<StyleField, String>,
    pub(crate) presets: StylePresets,
    pub(crate) panes: pane_grid::State<PaneKind>,
    pub(crate) side_panes: pane_grid::State<SidePaneKind>,
    pub(crate) styling_panes: pane_grid::State<StylingPaneKind>,
    pub(crate) mmtl_path: Option<std::path::PathBuf>,
    pub(crate) mmtl_temp_dir: Option<std::sync::Arc<tempfile::TempDir>>,
    pub frame: NativeFrame,
    pub(crate) app_view: AppView,
    pub(crate) new_project: Option<NewProjectState>,
    pub(crate) recent_projects: Vec<scanlateit_settings::RecentProject>,
}

impl App {
    pub fn theme(&self) -> Theme {
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
            ext.background.weak = opaque_ext.background.weak;
            ext.background.strong = opaque_ext.background.strong;
            ext.background.stronger = opaque_ext.background.stronger;
            ext.background.strongest = opaque_ext.background.strongest;
            ext.background.weaker = opaque_ext.background.weaker;
            ext.background.neutral = opaque_ext.background.neutral;
            ext.background.base.text = opaque_ext.background.base.text;
            ext.background.weakest.text = opaque_ext.background.weakest.text;
            ext
        })
    }

    pub(crate) fn new(frame: NativeFrame) -> Self {
        let style = EntryStyle::default();
        Self {
            frame,
            project: Project::new(),
            images: Vec::new(),
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
            #[cfg(feature = "inpaint")]
            pending_inpaint_span: None,
            inpainting: false,
            inpaint_mode: false,
            ocr_mode: false,
            #[cfg(feature = "ocr")]
            manual_ocring: false,
            #[cfg(feature = "ocr")]
            manual_ocr_engine: None,
            #[cfg(feature = "ocr")]
            pending_manual_ocr: None,
            #[cfg(feature = "ocr")]
            pending_manual_ocr_span: None,
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
            #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
            pipeline_active: false,
            #[cfg(all(feature = "styling", feature = "inpaint"))]
            pipeline_style_pending: 0,
            #[cfg(all(feature = "styling", feature = "inpaint"))]
            pipeline_style_results: Vec::new(),
            #[cfg(feature = "inpaint")]
            auto_inpaint_pending: 0,
            #[cfg(feature = "inpaint")]
            auto_telea_engine: None,
            #[cfg(feature = "inpaint")]
            auto_lama_engine: None,
            #[cfg(feature = "inpaint")]
            auto_aot_engine: None,
            #[cfg(feature = "inpaint")]
            pending_auto_telea_jobs: None,
            #[cfg(feature = "inpaint")]
            pending_auto_lama_jobs: None,
            #[cfg(feature = "inpaint")]
            pending_auto_aot_jobs: None,
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
            tx: ui_translation::Session::default(),
            translate_lang: ui_translation::LANGUAGES[0].to_string(),
            translation_panel_mode: TranslationPanelMode::Edit,
            translate_base: None,
            translate_target: TargetProfileSelection::AutoPlaceholder(format!(
                "{}(auto)",
                ui_translation::LANGUAGES[0]
            )),
            connect_modal: None,
            settings_open: false,
            settings_tab: SettingsTab::General,
            settings_search: String::new(),
            manage_models_open: false,
            manage_models_search: String::new(),
            selected: None,
            selected_inpaint: None,
            editing: None,
            editing_origin: EditOrigin::Overlay,
            edit_content: None,
            editing_dirty: false,
            editing_rect: None,
            scheduler: Scheduler::new(),
            style_working: style.clone(),
            system_fonts: HashMap::new(),
            installed_fonts: Vec::new(),
            // Bundled fonts are embedded via `include_bytes!` in `main.rs`, so
            // mark them as already loaded to avoid a redundant `font::load` in `handle_font`.
            loaded_fonts: HashSet::from([
                scanlateit_model::ANIME_ACE_FAMILY.to_string(),
                scanlateit_model::AUGIE_FAMILY.to_string(),
            ]),
            style_picker: None,
            style_stroke_width: style.stroke_width.to_string(),
            style_bg_radius: style.bg_radius.to_string(),
            style_hex_overrides: HashMap::new(),
            presets: scanlateit_settings::get(|s| s.style_presets.clone()),
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
            mmtl_path: None,
            mmtl_temp_dir: None,
            app_view: AppView::Home,
            new_project: None,
            recent_projects: scanlateit_settings::get(|s| s.recent_projects.clone()),
        }
    }
}

pub fn boot(frame: NativeFrame) -> (App, Task<Message>) {
    boot::boot(frame)
}

pub(crate) fn handle_model_event(app: &mut App, event: ModelEvent) {
    // Granular live-DB reactivity: every Project::*_with_event flows here via
    // Message::Model so UI state (selection, editing, inpaint cache) stays in
    // sync without coarse "DataChanged" broadcasts.
    match event {
        ModelEvent::EntryDeleted { id } => {
            if app.selected.is_some_and(|(_, sel_id)| sel_id == id) {
                app.selected = None;
                crate::app::edit::clear_editing(app);
            }
            // editing may be on same id even if not selected (panel origin)
            if app.editing.is_some_and(|(_, eid)| eid == id) {
                crate::app::edit::clear_editing(app);
            }
        }
        ModelEvent::EntryRestored { .. } => {
            // dormant: kept for future "undo delete" — no selection fixup needed,
            // the restored entry becomes visible via visible_for() again.
        }
        ModelEvent::EntriesReordered { .. } => {
            // no per-entry selection fixup needed; ordering is global per-image
        }
        ModelEvent::EntryMoved { .. } => {
            // view-quad move — ordering may be stale until next ReorderEntries;
            // no selection fixup.
        }
        ModelEvent::EntriesAdded { .. } => {
            debug_assert!(app.images.len() == app.project.image_count());
        }
        ModelEvent::ImageAdded { .. } => {
            debug_assert!(app.images.len() == app.project.image_count());
        }
        ModelEvent::EntryTextUpdated { .. } => {
            // panel/results reads via Project::resolved_text_for / display_text;
            // no extra app state to sync — text is live.
        }
        ModelEvent::EntryStyleUpdated { .. } => {
            // styling panel reads via Project::entry_style; working inputs are
            // seeded on selection, so no global refresh needed.
        }
        ModelEvent::ProfileCreated { .. }
        | ModelEvent::ProfileRemoved { .. }
        | ModelEvent::ProfileSelected { .. }
        | ModelEvent::ProfileRenamed { .. } => {
            // profile dropdown / translation panel read directly from Project;
            // translation's base/target validation happens in the UiEvent handlers.
            // Keep dormant events exhaustive for future UI listeners.
        }
        ModelEvent::InpaintAdded { .. } | ModelEvent::InpaintRemoved { .. } => {
            // Live DB owns bounds+InpaintId; ui::LoadedImage::inpaint is a
            // derived GPU-cache keyed by InpaintId. Handlers in inpaint.rs keep
            // both in sync and already call handle_model_event for each.
            // No extra app state — parity is ensured by emit-per-op.
        }
        ModelEvent::NoteUpdated { .. } => {
            // Extras::notes live in Project; no UI listener yet.
        }
    }
}

pub fn update(app: &mut App, message: Message) -> Task<Message> {
    let task = match message {
        Message::Frame(action) => app.frame.update(action, Message::Frame),
        Message::Model(ev) => {
            handle_model_event(app, ev);
            Task::none()
        }
        Message::FetchModels => translation::handle_fetch_models(app),
        Message::ModelsFetched(providers) => translation::handle_models_fetched(app, providers),
        Message::Ui(UiEvent::HomeNewProject) => {
            app.new_project = Some(NewProjectState {
                source_files: Vec::new(),
                original_lang: "Korean".to_string(),
                project_location: None,
            });
            app.status = "New Project...".to_string();
            Task::none()
        }
        Message::Ui(UiEvent::HomeOpenProject) => mmtl::handle_open(app),
        Message::Ui(UiEvent::HomeRecentClicked(path)) => {
            let p = std::path::PathBuf::from(path.clone());
            if !p.exists() {
                app.status = format!("Missing: {path}");
                return Task::none();
            }
            app.status = format!("Loading {}...", p.display());
            let path_clone = p.clone();
            Task::perform(
                async move {
                    let path_for_task = path_clone.clone();
                    tokio::task::spawn_blocking(move || -> Result<(Project, Vec<LoadedImage>, String, Option<Arc<tempfile::TempDir>>), String> {
                        let res = scanlateit_mmtl::load_mmtl(&path_for_task)?;
                        let mut inpaint_map: std::collections::HashMap<scanlateit_model::ImageId, Vec<scanlateit_ui::loaded::InpaintLayer>> = std::collections::HashMap::new();
                        for (img_id, bounds, png_path) in &res.inpaint_files {
                            let data = std::fs::read(png_path).map_err(|e| e.to_string())?;
                            let img = image::load_from_memory(&data).map_err(|e| e.to_string())?.to_rgba8();
                            let (w, h) = (img.width(), img.height());
                            let handle = iced::widget::image::Handle::from_rgba(w, h, bytes::Bytes::from(img.into_raw()));
                            inpaint_map.entry(*img_id).or_default().push(scanlateit_ui::loaded::InpaintLayer { bounds: *bounds, handle, width: w, height: h });
                        }
                        let mut out_images = Vec::new();
                        for meta in res.project.images() {
                            let layers = inpaint_map.remove(&meta.id).unwrap_or_default();
                            out_images.push(LoadedImage {
                                image_id: meta.id,
                                decode: PageDecode::default(),
                                inpaint: layers,
                            });
                        }
                        debug_assert_eq!(res.project.image_count(), out_images.len());
                        let display = path_for_task.to_string_lossy().to_string();
                        Ok((res.project, out_images, display, Some(Arc::new(res.temp_dir))))
                    })
                    .await
                    .unwrap_or_else(|e| Err(format!("load task failed: {e}")))
                },
                Message::RecentPickedToLoad,
            )
        }
        Message::Ui(UiEvent::HomeSettings) => settings::handle_settings_open(app),
        Message::Ui(UiEvent::NewProjectClose) => {
            app.new_project = None;
            Task::none()
        }
        Message::Ui(UiEvent::NewProjectSourceImage) => {
            Task::perform(
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
                                    Ok((w, h)) => out.push((path, w, h)),
                                    Err(e) => return Err(e),
                                }
                            }
                            Ok(out)
                        }
                        None => Ok(Vec::new()),
                    }
                },
                Message::NewProjectSourcePicked,
            )
        }
        Message::Ui(UiEvent::NewProjectSourceFolder) => {
            Task::perform(
                async {
                    let folder = rfd::AsyncFileDialog::new().pick_folder().await;
                    let Some(folder) = folder else { return Ok(Vec::new()) };
                    let dir = folder.path().to_path_buf();
                    let entries = std::fs::read_dir(&dir).map_err(|e| e.to_string())?;
                    let mut out = Vec::new();
                    for entry in entries {
                        let entry = entry.map_err(|e| e.to_string())?;
                        let path = entry.path();
                        if !path.is_file() { continue; }
                        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
                        if !IMAGE_FILTERS.contains(&ext.as_str()) { continue; }
                        let pstr = path.to_string_lossy().into_owned();
                        let dims = image::ImageReader::open(&path)
                            .map_err(|e| format!("Failed to open {pstr}: {e}"))?
                            .into_dimensions()
                            .map_err(|e| format!("Failed to decode {pstr}: {e}"));
                        match dims {
                            Ok((w, h)) => out.push((pstr, w, h)),
                            Err(e) => return Err(e),
                        }
                    }
                    out.sort_by(|a, b| natural_cmp(&a.0, &b.0));
                    Ok(out)
                },
                Message::NewProjectFolderPicked,
            )
        }
        Message::Ui(UiEvent::NewProjectLocationBrowse) => {
            let default_dir = app
                .new_project
                .as_ref()
                .and_then(|np| np.source_files.first().map(|(p, _, _)| std::path::Path::new(p).parent().map(|par| par.to_path_buf()).unwrap_or_default()))
                .unwrap_or_default();
            Task::perform(
                async move {
                    let mut dlg = rfd::AsyncFileDialog::new()
                        .add_filter("Manga Translation (.mmtl)", &["mmtl"])
                        .set_file_name("project.mmtl");
                    if default_dir.exists() {
                        dlg = dlg.set_directory(&default_dir);
                    }
                    let file = dlg.save_file().await;
                    file.map(|f| f.path().to_string_lossy().to_string())
                },
                Message::NewProjectLocationPicked,
            )
        }
        Message::Ui(UiEvent::NewProjectOriginalLang(lang)) => {
            if let Some(np) = app.new_project.as_mut() {
                np.original_lang = lang;
            }
            Task::none()
        }
        Message::Ui(UiEvent::NewProjectCreate) => {
            let Some(np) = app.new_project.clone() else { return Task::none() };
            if np.source_files.is_empty() || np.project_location.is_none() {
                app.status = "Select source and project location.".to_string();
                return Task::none();
            }
            // Build project and save directly
            let dest_str = np.project_location.clone().unwrap();
            let mut dest = std::path::PathBuf::from(&dest_str);
            // Ensure .mmtl extension
            if dest.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()) != Some("mmtl".to_string()) {
                let mut os = dest.as_os_str().to_owned();
                os.push(".mmtl");
                dest = std::path::PathBuf::from(os);
            }
            // Explorer-style dedup:  "{name} ({num}).mmtl"
            let unique_dest = {
                if !dest.exists() {
                    dest.clone()
                } else {
                    let parent = dest.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                    let stem = dest.file_stem().and_then(|s| s.to_str()).unwrap_or("project").to_string();
                    let ext = dest.extension().and_then(|e| e.to_str()).unwrap_or("mmtl").to_string();
                    let mut n = 1;
                    let mut cand;
                    loop {
                        cand = parent.join(format!("{stem} ({n}).{ext}"));
                        if !cand.exists() { break; }
                        n += 1;
                        if n > 999 { break; }
                    }
                    cand
                }
            };
            if let Some(parent) = unique_dest.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let files = np.source_files.clone();
            let dest_for_task = unique_dest.clone();
            // Reset UI immediately to home editor: we'll transition on success
            app.new_project = None;
            app.status = format!("Creating {}...", unique_dest.display());
            Task::perform(
                async move {
                    let res: Result<String, String> = tokio::task::spawn_blocking(move || -> Result<String, String> {
                        let mut project = Project::new();
                        let mut metas: Vec<(String, u32, u32)> = files;
                        metas.sort_by(|a, b| natural_cmp(&a.0, &b.0));
                        let mut loaded: Vec<LoadedImage> = Vec::new();
                        for (path, w, h) in metas {
                            let image_id = project.add_image(path.clone(), w as f32, h as f32);
                            loaded.push(LoadedImage { image_id, decode: PageDecode::default(), inpaint: Vec::new() });
                        }
                        debug_assert_eq!(project.image_count(), loaded.len());
                        scanlateit_mmtl::save_mmtl(&project, &[], &dest_for_task)
                            .map_err(|e| e.to_string())?;
                        Ok(dest_for_task.to_string_lossy().to_string())
                    })
                    .await
                    .unwrap_or_else(|e| Err(format!("create task failed: {e}")));
                    res
                },
                Message::CreateProjectPicked,
            )
        }
        Message::NewProjectSourcePicked(result) => {
            match result {
                Ok(files) if !files.is_empty() => {
                    if let Some(np) = app.new_project.as_mut() {
                        np.source_files = files;
                    }
                }
                Ok(_) => {}
                Err(e) => { app.status = e; }
            }
            Task::none()
        }
        Message::NewProjectFolderPicked(result) => {
            match result {
                Ok(files) if !files.is_empty() => {
                    if let Some(np) = app.new_project.as_mut() {
                        np.source_files = files;
                    }
                }
                Ok(_) => { app.status = "No images found in folder.".to_string(); }
                Err(e) => { app.status = e; }
            }
            Task::none()
        }
        Message::NewProjectLocationPicked(picked) => {
            if let Some(p) = picked {
                if let Some(np) = app.new_project.as_mut() {
                    np.project_location = Some(p);
                }
            }
            Task::none()
        }
        Message::CreateProjectPicked(result) => {
            match result {
                Ok(path_str) => match mmtl::load_created_project(path_str.clone()) {
                    Ok((project, images, display, temp_dir)) => {
                        debug_assert_eq!(project.image_count(), images.len());
                        app.project = project;
                        app.images = images;
                        app.mmtl_path = Some(std::path::PathBuf::from(display.clone()));
                        app.mmtl_temp_dir = temp_dir;
                        app.selected = None;
                        app.selected_inpaint = None;
                        app.editing = None;
                        app.edit_content = None;
                        app.app_view = AppView::Editor;
                        app.recent_projects = scanlateit_settings::get(|s| s.recent_projects.clone());
                        scanlateit_settings::touch_recent(display.clone());
                        app.recent_projects = scanlateit_settings::get(|s| s.recent_projects.clone());
                        app.status = format!("Created {} ({} image(s))", display, app.images.len());
                        let project = &app.project;
                        return app.scheduler.decode_thumbs_with_project(&mut app.images, project, Message::ThumbDecoded);
                    }
                    Err(e) => {
                        app.status = format!("Created {path_str} but load failed: {e}");
                        scanlateit_settings::touch_recent(path_str.clone());
                        app.recent_projects = scanlateit_settings::get(|s| s.recent_projects.clone());
                    }
                },
                Err(e) => { app.status = format!("Create failed: {e}"); }
            }
            Task::none()
        }
        Message::RecentPickedToLoad(result) => {
            match result {
                Ok((project, images, display, temp_dir)) => {
                    debug_assert_eq!(project.image_count(), images.len());
                    app.project = project;
                    app.images = images;
                    app.mmtl_path = Some(std::path::PathBuf::from(display.clone()));
                    app.mmtl_temp_dir = temp_dir;
                    app.selected = None;
                    app.selected_inpaint = None;
                    app.editing = None;
                    app.edit_content = None;
                    app.app_view = AppView::Editor;
                    scanlateit_settings::touch_recent(display.clone());
                    app.recent_projects = scanlateit_settings::get(|s| s.recent_projects.clone());
                    app.status = format!("Loaded {} ({} image(s))", display, app.images.len());
                    let project = &app.project;
                    return app.scheduler.decode_thumbs_with_project(&mut app.images, project, Message::ThumbDecoded);
                }
                Err(e) => { app.status = format!("Load failed: {e}"); }
            }
            Task::none()
        }
        Message::ImagesPicked(_) => Task::none(),
        Message::Ui(UiEvent::StartOcr) => ocr::handle_start_ocr(app),
        #[cfg(feature = "ocr")]
        Message::ParallelEngineReady(result) => ocr::handle_parallel_ready(app, result),
        #[cfg(feature = "ocr")]
        Message::ManualOcrEngineReady(result) => ocr::handle_manual_ocr_engine_ready(app, result),
        #[cfg(feature = "ocr")]
        Message::ManualOcrFinished(index, result) => ocr::handle_manual_ocr_finished(app, index, result),
        #[cfg(feature = "ocr")]
        Message::ManualOcrSpanFinished(result) => ocr::handle_manual_ocr_span_finished(app, result),
        #[cfg(feature = "inpaint")]
        Message::InpaintEngineReady(result) => inpaint::handle_inpaint_engine_ready(app, result),
        #[cfg(feature = "styling")]
        Message::StylingEngineReady(result) => styling::handle_styling_ready(app, result),
        #[cfg(feature = "styling")]
        Message::StyleDetected(index, id, result) => styling::handle_style_detected(app, index, id, result),
        #[cfg(all(feature = "styling", feature = "inpaint"))]
        Message::PipelineStyleDetected(index, id, result) => styling::handle_pipeline_style_detected(app, index, id, result),
        #[cfg(feature = "inpaint")]
        Message::AutoInpaintEngineReady(backend, result) => inpaint::handle_auto_engine_ready(app, backend, result),
        #[cfg(feature = "inpaint")]
        Message::AutoInpaintFinished(index, id, result) => inpaint::handle_auto_finished(app, index, id, result),
        #[cfg(feature = "inpaint")]
        Message::AutoInpaintLamaBatchFinished(batch) => inpaint::handle_auto_lama_batch(app, batch),
        #[cfg(feature = "inpaint")]
        Message::AutoInpaintAotBatchFinished(batch) => inpaint::handle_auto_aot_batch(app, batch),
        #[cfg(feature = "segment")]
        Message::SegmentEngineReady(result) => segment::handle_engine_ready(app, result),
        #[cfg(feature = "segment")]
        Message::SegmentFiltered(result) => segment::handle_filtered(app, result),
        #[cfg(feature = "inpaint")]
        Message::InpaintFinished(index, result) => inpaint::handle_inpaint_finished(app, index, result),
        #[cfg(feature = "inpaint")]
        Message::InpaintSpanFinished(result) => inpaint::handle_inpaint_span_finished(app, result),
        Message::Ui(UiEvent::StopOcr) => ocr::handle_stop_ocr(app),
        #[cfg(feature = "ocr")]
        Message::OcrStreamRun(result) => ocr::handle_ocr_stream_run(app, result),
        #[cfg(feature = "ocr")]
        Message::OcrStreamFailed(e) => ocr::handle_ocr_stream_failed(app, e),
        #[cfg(feature = "ocr")]
        Message::OcrTick => Task::none(),
        Message::FontLoaded => {
            app.font = Some(Font::with_name(scanlateit_ui::KOREAN_FONT_NAME));
            app.status = format!(
                "{} font ready. {}",
                scanlateit_ui::KOREAN_FONT_NAME,
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
            // Always offer bundled families in the picker, even when not
            // installed system-wide (they are embedded in the binary). When
            // installed, `fontdb` already provided the name — dedup case-insensitively
            // to handle `augie` vs `Augie` variations.
            for bundled in scanlateit_model::BUNDLED_FONTS {
                if !names.iter().any(|n| n.eq_ignore_ascii_case(bundled)) {
                    names.push(bundled.to_string());
                }
            }
            names.sort();
            names.dedup();
            // `dedup` is case-sensitive, so a second pass for `augie` casing
            // duplicates (keeps first occurrence, which is the system spelling).
            {
                let mut seen_lower = std::collections::HashSet::new();
                names.retain(|n| seen_lower.insert(n.to_ascii_lowercase()));
            }
            names.sort();
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
            if let Some(ev) = app.project.select_profile_with_event(id) {
                handle_model_event(app, ev);
            } else {
                return Task::none();
            }
            let name = app.project.profiles.selected().name.clone();
            app.status = format!("Profile: {name}");
            Task::none()
        }
        Message::Ui(UiEvent::ProfileCreate) => {
            if app.images.is_empty() {
                return Task::none();
            }
            let name = app.project.profiles.next_available_name();
            let (id, ev) = app.project.create_profile_with_event(name);
            handle_model_event(app, ev);
            if let Some(sel_ev) = app.project.select_profile_with_event(id) {
                handle_model_event(app, sel_ev);
            }
            let name = app.project.profiles.selected().name.clone();
            app.status = format!("Profile: {name} (created)");
            Task::none()
        }
        Message::Ui(UiEvent::TranslationPanelMode(mode)) => {
            // Initialize base/target when first entering Translate
            if mode == TranslationPanelMode::Translate && app.translation_panel_mode != TranslationPanelMode::Translate {
                if app.translate_base.is_none() && !app.images.is_empty() {
                    app.translate_base = Some(app.project.profiles.selected_id());
                }
                // Ensure target placeholder reflects current lang if it is AutoPlaceholder
                if let TargetProfileSelection::AutoPlaceholder(_) = app.translate_target.clone() {
                    app.translate_target = TargetProfileSelection::AutoPlaceholder(format!("{}(auto)", app.translate_lang));
                }
                // If placeholder already exists as a profile, convert to Existing so equality checks work
                if let TargetProfileSelection::AutoPlaceholder(name) = app.translate_target.clone() {
                    if let Some(id) = app.project.profiles.find_by_name(&name) {
                        // Don't select base itself; keep placeholder blank if it would equal base
                        let base = app.translate_base.or_else(|| Some(app.project.profiles.selected_id()));
                        if Some(id) != base {
                            app.translate_target = TargetProfileSelection::Existing(id);
                        }
                    }
                }
                // Prevent base == target (target placeholder may equal base name)
                if let (Some(base), TargetProfileSelection::Existing(tid)) = (app.translate_base, app.translate_target.clone()) {
                    if base == tid {
                        // keep target as placeholder instead
                        app.translate_target = TargetProfileSelection::AutoPlaceholder(format!("{}(auto)", app.translate_lang));
                    }
                }
            }
            app.translation_panel_mode = mode;
            app.status = match mode {
                TranslationPanelMode::Edit => "Edit mode: single profile.".to_string(),
                TranslationPanelMode::Translate => "Translate mode: base → target.".to_string(),
            };
            // Clear panel editing when switching modes
            if app.editing.is_some() && app.editing_origin == EditOrigin::Panel {
                crate::app::edit::clear_editing(app);
            }
            Task::none()
        }
        Message::Ui(UiEvent::BaseProfileSelect(id)) => {
            if app.images.is_empty() {
                return Task::none();
            }
            // validate id exists
            let exists = app.project.profiles.iter().any(|p| p.id == id);
            if !exists {
                return Task::none();
            }
            // prevent base == target (when target is Existing)
            if let TargetProfileSelection::Existing(tid) = app.translate_target.clone() {
                if tid == id {
                    app.status = "Base and target must differ.".to_string();
                    return Task::none();
                }
            }
            // also prevent base name == placeholder name when target is AutoPlaceholder
            if let TargetProfileSelection::AutoPlaceholder(name) = app.translate_target.clone() {
                if let Some(bprof) = app.project.profiles.iter().find(|p| p.id == id) {
                    if bprof.name == name {
                        app.status = "Base and target must differ.".to_string();
                        return Task::none();
                    }
                }
            }
            app.translate_base = Some(id);
            let name = app.project.profiles.iter().find(|p| p.id == id).map(|p| p.name.clone()).unwrap_or_default();
            app.status = format!("Base: {name}");
            Task::none()
        }
        Message::Ui(UiEvent::TargetProfileSelect(sel)) => {
            if app.images.is_empty() {
                return Task::none();
            }
            // validate and prevent == base
            let base = app.translate_base.or_else(|| Some(app.project.profiles.selected_id()));
            match &sel {
                TargetProfileSelection::Existing(id) => {
                    if Some(*id) == base {
                        app.status = "Base and target must differ.".to_string();
                        return Task::none();
                    }
                    let exists = app.project.profiles.iter().any(|p| p.id == *id);
                    if !exists {
                        return Task::none();
                    }
                }
                TargetProfileSelection::AutoPlaceholder(name) => {
                    if let Some(b) = base {
                        if let Some(bprof) = app.project.profiles.iter().find(|p| p.id == b) {
                            if &bprof.name == name {
                                app.status = "Base and target must differ.".to_string();
                                return Task::none();
                            }
                        }
                    }
                }
            }
            // If AutoPlaceholder actually already exists, convert to Existing
            let resolved = match sel.clone() {
                TargetProfileSelection::AutoPlaceholder(name) => {
                    if let Some(id) = app.project.profiles.find_by_name(&name) {
                        if Some(id) != base {
                            TargetProfileSelection::Existing(id)
                        } else {
                            TargetProfileSelection::AutoPlaceholder(name)
                        }
                    } else {
                        TargetProfileSelection::AutoPlaceholder(name)
                    }
                }
                other => other,
            };
            app.translate_target = resolved.clone();
            let label = match resolved {
                TargetProfileSelection::Existing(id) => app.project.profiles.iter().find(|p| p.id == id).map(|p| p.name.clone()).unwrap_or_default(),
                TargetProfileSelection::AutoPlaceholder(n) => n,
            };
            app.status = format!("Target: {label}");
            Task::none()
        }
        Message::Ui(UiEvent::TilesVisible(range)) => app
            .scheduler
            .schedule(range, Message::SettleElapsed),
        Message::SettleElapsed(seq) => {
            if app.scheduler.accept_elapsed(seq) {
                let project = &app.project;
                app.scheduler
                    .settle_with_project(&mut app.images, project, Message::FullDecoded)
            } else {
                Task::none()
            }
        }
        Message::Ui(UiEvent::TileScrollEnded) => {
            let project = &app.project;
            app.scheduler
                .settle_with_project(&mut app.images, project, Message::FullDecoded)
        }
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
        Message::Ui(UiEvent::Translate) => translation::handle_translate(app),
        Message::Ui(UiEvent::TranslateModelSelect { provider, model }) => translation::handle_model_select(app, provider, model),
        Message::Ui(UiEvent::TranslateLang(lang)) => {
            app.translate_lang = lang.clone();
            // Keep placeholder in sync when target is AutoPlaceholder
            if let TargetProfileSelection::AutoPlaceholder(_) = app.translate_target.clone() {
                let new_name = format!("{lang}(auto)");
                // If that name already exists as a profile (and !== base), convert to Existing
                if !app.images.is_empty() {
                    if let Some(id) = app.project.profiles.find_by_name(&new_name) {
                        let base = app.translate_base.or_else(|| Some(app.project.profiles.selected_id()));
                        if Some(id) != base {
                            app.translate_target = TargetProfileSelection::Existing(id);
                        } else {
                            app.translate_target = TargetProfileSelection::AutoPlaceholder(new_name);
                        }
                    } else {
                        app.translate_target = TargetProfileSelection::AutoPlaceholder(new_name);
                    }
                } else {
                    app.translate_target = TargetProfileSelection::AutoPlaceholder(new_name);
                }
            }
            Task::none()
        }
        Message::Ui(UiEvent::TranslateConnect(provider_id)) => translation::handle_connect(app, provider_id),
        Message::Ui(UiEvent::TranslateDisconnect(provider_id)) => translation::handle_disconnect(app, provider_id),
        Message::Ui(UiEvent::ConnectModalKey(key)) => translation::handle_connect_modal_key(app, key),
        Message::Ui(UiEvent::ConnectModalBaseUrl(url)) => translation::handle_connect_modal_base_url(app, url),
        Message::Ui(UiEvent::ConnectModalModel(model)) => translation::handle_connect_modal_model(app, model),
        Message::Ui(UiEvent::ConnectModalSubmit) => translation::handle_connect_modal_submit(app),
        Message::Ui(UiEvent::ConnectModalCancel) => translation::handle_connect_modal_cancel(app),
        Message::Ui(UiEvent::ManageModelsOpen) => settings::handle_manage_models_open(app),
        Message::Ui(UiEvent::ManageModelsClose) => settings::handle_manage_models_close(app),
        Message::Ui(UiEvent::ManageModelsSearch(query)) => settings::handle_manage_models_search(app, query),
        Message::Ui(UiEvent::EntryClicked(selection)) => edit::handle_entry_clicked(app, selection),
        Message::Ui(UiEvent::EntryDoubleClicked(pair)) => edit::handle_entry_double_clicked(app, pair),
        Message::Ui(UiEvent::PanelEntryEdit(pair)) => edit::handle_panel_entry_edit(app, pair),
        Message::Ui(UiEvent::InpaintClicked(selection)) => inpaint::handle_inpaint_clicked(app, selection),
        Message::Ui(UiEvent::InpaintDelete(pair)) => inpaint::handle_inpaint_delete(app, pair.0, pair.1),
        Message::Ui(UiEvent::InpaintRepaint(pair)) => inpaint::handle_inpaint_repaint(app, pair.0, pair.1),
        Message::Ui(UiEvent::InpaintToolbar((image_index, patch_idx, action))) => inpaint::handle_inpaint_toolbar(app, image_index, patch_idx, action),
        Message::Ui(UiEvent::RetranslateEntry(pair)) => translation::handle_retranslate_entry(app, pair.0, pair.1),
        Message::Ui(UiEvent::ReorderEntries) => {
            if app.images.is_empty() {
                app.status = "No images to reorder.".to_string();
                return Task::none();
            }
            // Per-image emission per decision 4: one EntriesReordered per image
            let ids: Vec<_> = app.project.images().iter().map(|m| m.id).collect();
            if ids.is_empty() {
                let ev = app.project.reorder_entries_for_image_with_event(scanlateit_model::ImageId(0));
                handle_model_event(app, ev);
            } else {
                for image_id in ids {
                    let ev = app.project.reorder_entries_for_image_with_event(image_id);
                    handle_model_event(app, ev);
                }
            }
            // Per-image file order is the `images` vec order; within each file
            // entries are now Y→X (top first, left→right) by view-quad bounds.
            // Translation iterates `visible_for()` per image, so it immediately
            // benefits — no translation cache to invalidate.
            app.status = format!(
                "Reordered {} image(s) by position (higher first, left to right).",
                app.images.len()
            );
            Task::none()
        }
        Message::Ui(UiEvent::Inpaint) => inpaint::handle_inpaint_toggle(app),
        Message::Ui(UiEvent::ManualOcr) => ocr::handle_manual_ocr_toggle(app),
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
        Message::Ui(UiEvent::ViewerScroll(anchor)) => {
            app.viewer_scroll = anchor.clamp(0.0, 1.0);
            Task::none()
        }
        Message::Ui(UiEvent::EntryToolbar((index, id, action))) => edit::handle_entry_toolbar(app, index, id, action),
        Message::Ui(UiEvent::EntryMoved((index, id, quad))) => edit::handle_entry_moved(app, index, id, quad),
        Message::Ui(UiEvent::InpaintSelection((index, rect))) => inpaint::handle_inpaint_selection(app, index, rect),
        Message::Ui(UiEvent::InpaintSelectionSpan(spans)) => inpaint::handle_inpaint_span(app, spans),
        Message::Ui(UiEvent::ManualOcrSelection((index, rect))) => ocr::handle_manual_ocr_selection(app, index, rect),
        Message::Ui(UiEvent::ManualOcrSelectionSpan(spans)) => ocr::handle_manual_ocr_span(app, spans),
        Message::Ui(UiEvent::EditAction(action)) => edit::handle_edit_action(app, action),
        Message::Ui(UiEvent::EditRect(rect)) => edit::handle_edit_rect(app, rect),
        Message::Ui(UiEvent::EditSubmit) => edit::handle_edit_submit(app),
        Message::Ui(UiEvent::StyleBold(bold)) => styling::handle_bold(app, bold),
        Message::Ui(UiEvent::StyleItalic(italic)) => styling::handle_italic(app, italic),
        Message::Ui(UiEvent::StyleFont(name)) => styling::handle_font(app, name),
        Message::Ui(UiEvent::StyleTextAlign(align)) => styling::handle_text_align(app, align),
        Message::Ui(UiEvent::StyleGradientToggle(enabled)) => styling::handle_gradient_toggle(app, enabled),
        Message::Ui(UiEvent::StyleGradientDir(dir)) => styling::handle_gradient_dir(app, dir),
        Message::Ui(UiEvent::StyleColorOpen(field)) => styling::handle_color_open(app, field),
        Message::Ui(UiEvent::StyleColorCancel(field)) => styling::handle_color_cancel(app, field),
        Message::Ui(UiEvent::StyleColorSubmit(field, color)) => styling::handle_color_submit(app, field, color),
        Message::Ui(UiEvent::StyleHexInput(field, text)) => styling::handle_hex_input(app, field, text),
        Message::Ui(UiEvent::StyleStrokeWidth(text)) => styling::handle_stroke_width(app, text),
        Message::Ui(UiEvent::StyleBgRadius(text)) => styling::handle_bg_radius(app, text),
        Message::Ui(UiEvent::StyleInpaintBackground) => inpaint::handle_style_inpaint_background(app),
        Message::Ui(UiEvent::StylePresetApply(preset)) => styling::handle_preset_apply(app, preset),
        Message::Ui(UiEvent::StylePresetAdd) => styling::handle_preset_add(app),
        Message::Ui(UiEvent::StylePresetReplace(preset)) => styling::handle_preset_replace(app, preset),
        Message::Ui(UiEvent::StylePresetRemove(preset)) => styling::handle_preset_remove(app, preset),
        Message::Ui(UiEvent::StylePresetMenuDismiss) => Task::none(),
        Message::Ui(UiEvent::StyleAutoDetect) => styling::handle_auto_detect(app),
        Message::Ui(UiEvent::PanelResized(resized)) => {
            // Distinct mins: main 160 vs panel 592 (STYLING+RESULTS+GAP), pane_grid has single min_size (=160)
            // so clamp ratio to prevent panel collapsing to ~100px as in screenshot.
            // At default 1400px window panel 592 needs ratio <= 0.58, main 160 needs ratio >= 0.12
            let ratio = resized.ratio.clamp(0.15, 0.58);
            app.panes.resize(resized.split, ratio);
            Task::none()
        }
        Message::Ui(UiEvent::SidePanelResized(resized)) => {
            // Distinct mins: styling 260 vs results 320, pane_grid single min (=260)
            // clamp to keep both above their mins when panel at its min 592 (ratio 0.439-0.459)
            // but allow wider range at larger panel widths; narrow fixed clamp prevents extreme collapse
            let ratio = resized.ratio.clamp(0.38, 0.55);
            app.side_panes.resize(resized.split, ratio);
            Task::none()
        }
        Message::Ui(UiEvent::StylingPaneResized(resized)) => {
            app.styling_panes.resize(resized.split, resized.ratio);
            Task::none()
        }
        Message::Ui(UiEvent::SettingsOpen) => settings::handle_settings_open(app),
        Message::Ui(UiEvent::SettingsOpenTab(tab)) => settings::handle_settings_open_tab(app, tab),
        Message::Ui(UiEvent::SettingsClose) => settings::handle_settings_close(app),
        Message::Ui(UiEvent::SettingsTab(tab)) => settings::handle_settings_tab(app, tab),
        Message::Ui(UiEvent::SettingsSearch(query)) => settings::handle_settings_search(app, query),
        Message::Ui(UiEvent::SettingsChanged) => settings::handle_settings_changed(app),
        Message::Ui(UiEvent::SettingEdit(edit)) => settings::handle_setting_edit(app, edit),
        Message::Ui(UiEvent::OpenUrl(url)) => settings::handle_open_url(app, url),
        Message::Ui(UiEvent::SaveProject) => mmtl::handle_save(app),
        Message::Ui(UiEvent::SaveProjectAs) => mmtl::handle_save_as(app),
        Message::Ui(UiEvent::OpenProject) => mmtl::handle_open(app),
        Message::Ui(UiEvent::ExportAll) => export::handle_export_all(app),
        Message::MmtlSavePicked(picked) => mmtl::handle_save_picked(app, picked),
        Message::MmtlOpenPicked(picked) => mmtl::handle_open_picked(app, picked),
        Message::MmtlSaved(result) => mmtl::handle_saved(app, result),
        Message::MmtlLoaded(result) => mmtl::handle_loaded(app, result),
        Message::ExportFolderPicked(picked) => export::handle_export_picked(app, picked),
        Message::ExportFinished(result) => export::handle_export_finished(app, result),
        Message::TranslateFinished(jobs, result) => translation::handle_translate_finished(app, jobs, result),
        Message::RetranslateFinished((index, entry_id), result) => translation::handle_retranslate_finished(app, index, entry_id, result),
    };
    task
}

pub fn subscription(app: &App) -> Subscription<Message> {
    let frame_sub = app.frame.subscription().map(Message::Frame);
    let keys = iced::event::listen().filter_map(|event| {
        if let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) = event {
            if modifiers.control() {
                match key.as_ref() {
                    iced::keyboard::Key::Character(c) if c == "s" || c == "S" => {
                        if modifiers.shift() {
                            Some(Message::Ui(UiEvent::SaveProjectAs))
                        } else {
                            Some(Message::Ui(UiEvent::SaveProject))
                        }
                    }
                    iced::keyboard::Key::Character(c) if c == "o" || c == "O" => {
                        Some(Message::Ui(UiEvent::OpenProject))
                    }
                    _ => None,
                }
            } else {
                None
            }
        } else {
            None
        }
    });
    #[cfg(feature = "ocr")]
    if app.running {
        return Subscription::batch([
            frame_sub,
            keys,
            iced::time::every(Duration::from_millis(16)).map(|_| Message::OcrTick),
        ]);
    }
    Subscription::batch([frame_sub, keys])
}

pub fn view(app: &App) -> Element<'_, Message> {
    view::view(app)
}

#[cfg(test)]
mod tests;
