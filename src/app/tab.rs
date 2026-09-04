//! Tab data model for multi-project session (Phase 0 scaffold).
//!
//! Permanent `Home` is `tabs[0]` (non-closable). Each `Project` tab owns
//! its chapter-wide `Project` + per-image view caches + local UI state.
//! Heavy engines (`ocr::ParallelEngine`, `inpaint::Engine`, `styling::Engine`,
//! `segment::Engine`) live **once** in `EnginePool` on `App` (Q3), not here.

use std::collections::HashMap;
use std::sync::Arc;

use iced::widget::{pane_grid, text_editor};
use iced::Rectangle;

#[cfg(feature = "inpaint")]
use easyscanlate_inpaint::Engine as InpaintEngine;
use easyscanlate_model::{EntryId, EntryStyle, ProfileId, Project, Quad};
#[cfg(feature = "ocr")]
use easyscanlate_ocr::{self as ocr_engine, OcrCancellationToken, ParallelEngine};
#[cfg(feature = "segment")]
use easyscanlate_segment::Engine as SegmentEngine;
#[cfg(feature = "styling")]
use easyscanlate_styling::{JobTracker, StylePrediction};
use easyscanlate_ui::event::{EditOrigin, MainAreaMode, ManualMode, StyleField, TargetProfileSelection, TranslationPanelMode};
use easyscanlate_ui::main_area::decode::Scheduler;
use easyscanlate_ui::LoadedImage;

use easyscanlate_ui::layout::{PaneKind, SidePaneKind, StylingPaneKind, MAIN_AREA_DEFAULT_RATIO, STYLING_DEFAULT_RATIO, STYLING_TOP_RATIO};

// ---------------------------------------------------------------------------
// Tab identity
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(pub u64);

impl std::fmt::Display for TabId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TabId({})", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabKind {
    Home,
    Project,
}

impl TabKind {
    pub fn is_home(self) -> bool {
        matches!(self, Self::Home)
    }
    pub fn is_project(self) -> bool {
        matches!(self, Self::Project)
    }
}

// ---------------------------------------------------------------------------
// Per-tab pending inpaint job (mirrors `crate::app::AutoInpaintJob`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct AutoInpaintJob {
    pub index: usize,
    pub id: EntryId,
    pub path: String,
    pub quad: Quad,
}

// ---------------------------------------------------------------------------
// Engine pool — shared heavy engines (Q3). One per App.
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct EnginePool {
    #[cfg(feature = "ocr")]
    pub pipeline: Option<ParallelEngine>,
    #[cfg(feature = "ocr")]
    pub manual_ocr: Option<easyscanlate_ocr::Engine>,
    #[cfg(feature = "inpaint")]
    pub inpaint: Option<InpaintEngine>,
    #[cfg(feature = "inpaint")]
    pub auto_telea: Option<InpaintEngine>,
    #[cfg(feature = "inpaint")]
    pub auto_lama: Option<InpaintEngine>,
    #[cfg(feature = "inpaint")]
    pub auto_aot: Option<InpaintEngine>,
    #[cfg(feature = "segment")]
    pub segment: Option<SegmentEngine>,
    pub queue: crate::app::queue::EngineQueue,
}

// ---------------------------------------------------------------------------
// Shared complex payload aliases (silences `clippy::type_complexity`).
// ---------------------------------------------------------------------------

/// One manual inpaint selection: image index, path, rect, quads.
#[cfg(feature = "inpaint")]
pub type ManualInpaintSelection = (usize, String, [f32; 4], Vec<Quad>);

/// One deferred pipeline style result with its inpaint job payload.
#[cfg(all(feature = "styling", feature = "inpaint"))]
pub type PipelineStyleItem = (
    usize,
    EntryId,
    Result<(EntryStyle, StylePrediction), String>,
    Quad,
    String,
);

// ---------------------------------------------------------------------------
// Tab — full per-tab state (Phase 0: scaffold, not yet wired)
// ---------------------------------------------------------------------------

pub struct Tab {
    pub id: TabId,
    pub kind: TabKind,
    pub title: String,
    pub dirty: bool,

    // chapter-wide model + derived GPU caches
    pub project: Project,
    pub images: Vec<LoadedImage>,
    pub mmtl_path: Option<std::path::PathBuf>,
    pub mmtl_temp_dir: Option<Arc<tempfile::TempDir>>,
    pub status: String,

    // ocr run state (per-tab)
    #[cfg(feature = "ocr")]
    pub cancel: Option<OcrCancellationToken>,
    #[cfg(feature = "ocr")]
    pub ocr_plans: Vec<ocr_engine::RunPlan>,
    #[cfg(feature = "ocr")]
    pub ocr_dims: Vec<(u32, u32)>,
    #[cfg(feature = "ocr")]
    pub pending: usize,
    #[cfg(feature = "ocr")]
    pub ocr_total: usize,
    #[cfg(feature = "ocr")]
    pub ocr_failed: usize,
    #[cfg(feature = "ocr")]
    pub ocr_cancelled: bool,
    #[cfg(feature = "ocr")]
    pub ocr_runs: usize,
    #[cfg(feature = "ocr")]
    pub held_boundary: Option<ocr_engine::BoundaryState>,
    pub running: bool,

    // inpaint (per-tab pending queues)
    #[cfg(feature = "inpaint")]
    pub pending_manual_multi: Option<Vec<ManualInpaintSelection>>,
    #[cfg(feature = "inpaint")]
    pub pending_background_stitch: Option<(AutoInpaintJob, f32, Option<String>, Option<String>)>,
    pub inpainting: bool,
    pub manual_mode: ManualMode,
    pub manual_selections: Vec<(usize, Rectangle)>,
    pub manual_prev_view_mode: Option<MainAreaMode>,
    #[cfg(feature = "ocr")]
    pub manual_ocring: bool,
    #[cfg(feature = "ocr")]
    pub pending_manual_multi_ocr: Option<Vec<(usize, Rectangle)>>,
    pub show_overlay_text: bool,
    pub show_inpaint: bool,
    pub view_mode: MainAreaMode,
    pub viewer_scroll: f32,

    // styling / segment / pipeline counters (per-tab)
    #[cfg(feature = "styling")]
    pub styling: JobTracker,
    #[cfg(feature = "segment")]
    pub segment_filtering: bool,
    #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
    pub pipeline_active: bool,
    #[cfg(all(feature = "styling", feature = "inpaint"))]
    pub pipeline_style_pending: usize,
    #[cfg(all(feature = "styling", feature = "inpaint"))]
    pub pipeline_style_results: Vec<PipelineStyleItem>,
    #[cfg(feature = "inpaint")]
    pub auto_inpaint_pending: usize,
    #[cfg(feature = "inpaint")]
    pub auto_inpaint_total: usize,
    #[cfg(feature = "segment")]
    pub pipeline_seg_done: bool,
    #[cfg(feature = "inpaint")]
    pub pending_auto_telea_jobs: Option<Vec<AutoInpaintJob>>,
    #[cfg(feature = "inpaint")]
    pub pending_auto_lama_jobs: Option<Vec<AutoInpaintJob>>,
    #[cfg(feature = "inpaint")]
    pub pending_auto_aot_jobs: Option<Vec<AutoInpaintJob>>,

    // selection / editing
    pub selected: Option<(usize, EntryId)>,
    pub selected_inpaint: Option<(usize, usize)>,
    pub editing: Option<(usize, EntryId)>,
    pub editing_origin: EditOrigin,
    pub edit_content: Option<text_editor::Content>,
    pub editing_dirty: bool,
    pub editing_rect: Option<Rectangle>,
    pub scheduler: Scheduler,
    pub style_working: EntryStyle,
    pub style_picker: Option<StyleField>,
    pub style_stroke_width: String,
    pub style_bg_radius: String,
    pub style_hex_overrides: HashMap<StyleField, String>,

    pub panes: pane_grid::State<PaneKind>,
    pub side_panes: pane_grid::State<SidePaneKind>,
    pub styling_panes: pane_grid::State<StylingPaneKind>,

    // translation per-tab slice (Q5)
    pub translating: bool,
    pub translate_anim_phase: f32,
    pub translate_lang: String,
    pub translation_panel_mode: TranslationPanelMode,
    pub translate_base: Option<ProfileId>,
    pub translate_target: TargetProfileSelection,

    // project open loading placeholder (instant tab + overlay)
    pub loading: bool,
    pub loading_path: Option<std::path::PathBuf>,
    pub loading_phase: f32,
}

impl Tab {
    /// Permanent Home tab (pinned, non-closable). `id` is typically `TabId(0)`.
    pub fn home(id: TabId) -> Self {
        let style = EntryStyle::default();
        // Replicate `App::new` pane defaults exactly so per-tab layout matches today's single doc.
        let panes = {
            let (mut panes, main) = pane_grid::State::new(PaneKind::MainArea);
            let (_, split) = panes
                .split(pane_grid::Axis::Vertical, main, PaneKind::Panel)
                .expect("initial pane split must succeed");
            panes.resize(split, MAIN_AREA_DEFAULT_RATIO);
            panes
        };
        let side_panes = {
            let (mut panes, styling) = pane_grid::State::new(SidePaneKind::Styling);
            let (_, split) = panes
                .split(pane_grid::Axis::Vertical, styling, SidePaneKind::Results)
                .expect("side pane split must succeed");
            panes.resize(split, STYLING_DEFAULT_RATIO);
            panes
        };
        let styling_panes = {
            let (mut panes, inspector) = pane_grid::State::new(StylingPaneKind::Inspector);
            let (_, split) = panes
                .split(pane_grid::Axis::Horizontal, inspector, StylingPaneKind::Layers)
                .expect("styling pane split must succeed");
            panes.resize(split, STYLING_TOP_RATIO);
            panes
        };
        Self {
            id,
            kind: TabKind::Home,
            title: "Home".to_string(),
            dirty: false,
            project: Project::new(),
            images: Vec::new(),
            mmtl_path: None,
            mmtl_temp_dir: None,
            status: "Idle — open images to begin.".to_string(),
            #[cfg(feature = "ocr")]
            cancel: None,
            #[cfg(feature = "ocr")]
            ocr_plans: Vec::new(),
            #[cfg(feature = "ocr")]
            ocr_dims: Vec::new(),
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
            running: false,
            #[cfg(feature = "inpaint")]
            pending_manual_multi: None,
            #[cfg(feature = "inpaint")]
            pending_background_stitch: None,
            inpainting: false,
            manual_mode: ManualMode::None,
            manual_selections: Vec::new(),
            manual_prev_view_mode: None,
            #[cfg(feature = "ocr")]
            manual_ocring: false,
            #[cfg(feature = "ocr")]
            pending_manual_multi_ocr: None,
            show_overlay_text: true,
            show_inpaint: true,
            view_mode: MainAreaMode::View,
            viewer_scroll: 0.0,
            #[cfg(feature = "styling")]
            styling: JobTracker::new(),
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
            auto_inpaint_total: 0,
            #[cfg(feature = "segment")]
            pipeline_seg_done: false,
            #[cfg(feature = "inpaint")]
            pending_auto_telea_jobs: None,
            #[cfg(feature = "inpaint")]
            pending_auto_lama_jobs: None,
            #[cfg(feature = "inpaint")]
            pending_auto_aot_jobs: None,
            selected: None,
            selected_inpaint: None,
            editing: None,
            editing_origin: EditOrigin::Overlay,
            edit_content: None,
            editing_dirty: false,
            editing_rect: None,
            scheduler: Scheduler::new(),
            style_working: style.clone(),
            style_picker: None,
            style_stroke_width: style.stroke_width.to_string(),
            style_bg_radius: style.bg_radius.to_string(),
            style_hex_overrides: HashMap::new(),
            panes,
            side_panes,
            styling_panes,
            translating: false,
            translate_anim_phase: 0.0,
            translate_lang: easyscanlate_ui::translation::LANGUAGES[0].to_string(),
            translation_panel_mode: TranslationPanelMode::Edit,
            translate_base: None,
            translate_target: TargetProfileSelection::AutoPlaceholder(format!(
                "{}(auto)",
                easyscanlate_ui::translation::LANGUAGES[0]
            )),
            loading: false,
            loading_path: None,
            loading_phase: 0.0,
        }
    }

    /// Project tab created from a loaded `.mmtl` (Phase 3+). Thin wrapper over
    /// filled fields; `title` is expected to be `file_stem`.
    pub fn project_from_loaded(
        id: TabId,
        title: String,
        project: Project,
        images: Vec<LoadedImage>,
        mmtl_path: std::path::PathBuf,
        mmtl_temp_dir: Option<Arc<tempfile::TempDir>>,
    ) -> Self {
        let mut tab = Self::home(id);
        tab.kind = TabKind::Project;
        tab.title = title;
        tab.project = project;
        tab.images = images;
        tab.mmtl_path = Some(mmtl_path);
        tab.mmtl_temp_dir = mmtl_temp_dir;
        tab.dirty = false;
        tab.status = format!("Loaded {} ({} image(s))", tab.title, tab.images.len());
        tab
    }

    pub fn is_home(&self) -> bool {
        self.kind.is_home()
    }
    pub fn is_project(&self) -> bool {
        self.kind.is_project()
    }

    /// Instant placeholder shown while the `.mmtl` is being extracted off the UI thread.
    /// Title is file_stem, `loading_path` is canonical for dedup/spam guard.
    pub fn loading_placeholder(
        id: TabId,
        title: String,
        path: std::path::PathBuf,
    ) -> Self {
        let mut tab = Self::home(id);
        tab.kind = TabKind::Project;
        tab.title = title;
        tab.mmtl_path = Some(path.clone());
        tab.loading = true;
        tab.loading_path = Some(path.clone());
        tab.loading_phase = 0.0;
        tab.status = format!("Loading {}...", path.display());
        tab.dirty = false;
        tab
    }

    /// Hydrate a previously created `loading_placeholder` with the real project/images.
    pub fn hydrate_from_loaded(
        &mut self,
        project: Project,
        images: Vec<LoadedImage>,
        mmtl_path: std::path::PathBuf,
        mmtl_temp_dir: Option<Arc<tempfile::TempDir>>,
    ) {
        self.project = project;
        self.images = images;
        self.mmtl_path = Some(mmtl_path);
        self.mmtl_temp_dir = mmtl_temp_dir;
        self.loading = false;
        self.loading_path = None;
        self.loading_phase = 0.0;
        self.dirty = false;
        let n = self.images.len();
        // Keep title already set (file_stem); update status like project_from_loaded.
        self.status = format!("Loaded {} ({} image(s))", self.title, n);
    }
}
