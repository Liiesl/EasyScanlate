use std::collections::{HashMap, HashSet};
#[cfg(all(feature = "test-ui", not(feature = "translation")))]
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, mpsc};

use iced::{Color, Element, Font, Subscription, Task, Theme};
use neverliie_iced_widgets::title_bar::{FrameAction, NativeFrame};

#[cfg(feature = "inpaint")]
use easyscanlate_inpaint::Engine as InpaintEngine;
use easyscanlate_model::{EntryId, EntryStyle, ModelEvent, NewEntry};
use easyscanlate_settings::StylePresets;
#[cfg(feature = "inpaint")]
use easyscanlate_settings::InpaintBackend;
#[cfg(feature = "ocr")]
use easyscanlate_ocr::{self as ocr_engine, ParallelEngine};
#[cfg(feature = "styling")]
use easyscanlate_styling::Engine as StylingEngine;
#[cfg(feature = "segment")]
use easyscanlate_segment::Engine as SegmentEngine;
use easyscanlate_ui::translation as ui_translation;
use easyscanlate_ui::main_area::decode::{DecodedPage, Tier};
use easyscanlate_ui::{
    event::{SettingsTab, UiEvent},
    ConnectModal,
};

// ---------------------------------------------------------------------------
// Sub-modules
// ---------------------------------------------------------------------------
pub mod layout;
pub mod chrome;
pub mod boot;
pub mod backdrop;
pub mod state;
pub mod edit;
pub mod ocr;
pub mod inpaint;
pub mod manual;
pub mod styling;
pub mod segment;
pub mod pipeline;
pub mod translation;
pub mod settings;
pub mod mmtl;
pub mod new_project;
pub mod profile;
pub mod entries;
pub mod main_area;
pub mod subscription;
pub mod update;
pub mod update_popup;
pub mod export;
pub mod view;
pub mod tab;
pub mod tabs;
pub mod confirm_close;
pub mod queue;
pub mod onboarding;

use tab::{AutoInpaintJob, EnginePool, Tab, TabId};

// ---------------------------------------------------------------------------
// Shared complex payload aliases (silences `clippy::type_complexity`).
// ---------------------------------------------------------------------------

/// One inpaint patch: RGBA crop, bounds, source quad.
#[cfg(feature = "inpaint")]
type InpaintPatch = (
    image::RgbaImage,
    [f32; 4],
    Option<easyscanlate_model::Quad>,
);
/// Grouped manual inpaint result: per-image patches.
#[cfg(feature = "inpaint")]
type GroupedInpaintPatches = Vec<(usize, Vec<InpaintPatch>)>;
/// Manual multi-inpaint async result.
#[cfg(feature = "inpaint")]
type ManualInpaintResult = Result<GroupedInpaintPatches, String>;
/// One auto-inpaint patch with its target image index.
#[cfg(feature = "inpaint")]
type AutoInpaintPatch = (
    usize,
    image::RgbaImage,
    [f32; 4],
    Option<easyscanlate_model::Quad>,
);
/// Auto single-job async result.
#[cfg(feature = "inpaint")]
type AutoInpaintResult = Result<Vec<AutoInpaintPatch>, String>;
/// One granular auto-inpaint stream item: job index, entry, per-job result.
/// Emitted per finished job (OCR-style), so one failure never drops the rest.
#[cfg(feature = "inpaint")]
type AutoInpaintStreamItem = (usize, EntryId, AutoInpaintResult);
/// One granular manual-inpaint stream item: per-unit partial patches + failed
/// group count in that unit. A unit is one 512-group when truly streaming, or
/// the whole multi-selection batch on the tolerant single-shot path. Either
/// way one group's failure never drops the other groups' patches.
#[cfg(feature = "inpaint")]
type ManualInpaintStreamItem = (GroupedInpaintPatches, usize);
/// One granular segment stream item: grid index + this grid's deletions.
#[cfg(feature = "segment")]
type SegmentStreamItem = (usize, Vec<(usize, EntryId)>);
/// One granular export stream chunk: per finished file `(global_idx, result)`.
/// `Ok(path_display)` counts one saved file, `Err(msg)` counts one failure
/// without dropping the rest (mirrors OCR/segment streaming).
pub type ExportStreamItem = Vec<(usize, Result<String, String>)>;

/// Loaded `.mmtl` project payload (reuses `mmtl` alias to stay in sync).
type MmtlLoadedPayload = mmtl::LoadedProjectResult;

/// Onboarding download progress channel.
type OnboardingProgress = (f32, u64, u64);
pub(crate) type OnboardingRx = Option<Arc<Mutex<mpsc::Receiver<OnboardingProgress>>>>;

#[derive(Debug, Clone)]
pub enum TabMessage {
    /// Granular model change event for this tab.
    Model(ModelEvent),
    ThumbDecoded(usize, Result<Arc<DecodedPage>, String>),
    FullDecoded(usize, Result<Arc<DecodedPage>, String>),
    SettleElapsed(u64),
    #[cfg(feature = "ocr")]
    ParallelEngineReady(Result<ParallelEngine, String>),
    #[cfg(feature = "ocr")]
    ManualOcrEngineReady(Result<easyscanlate_ocr::Engine, String>),
    #[cfg(feature = "ocr")]
    ManualOcrMultiFinished(Result<Vec<(usize, Vec<NewEntry>)>, String>),
    #[cfg(feature = "ocr")]
    OcrStreamRun(Result<ocr_engine::RunEvent, String>),
    #[cfg(feature = "ocr")]
    OcrStreamFailed(String),
    #[cfg(feature = "ocr")]
    OcrTick,
    TranslateTick,
    LoadingTick,
    #[cfg(feature = "inpaint")]
    InpaintEngineReady(Result<InpaintEngine, String>),
    #[cfg(feature = "inpaint")]
    ManualMultiInpaintFinished(ManualInpaintResult),
    #[cfg(feature = "inpaint")]
    AutoInpaintEngineReady(InpaintBackend, Result<InpaintEngine, String>),
    #[cfg(feature = "inpaint")]
    AutoInpaintFinished(usize, EntryId, AutoInpaintResult),
    #[cfg(feature = "inpaint")]
    AutoInpaintStreamRun(Result<AutoInpaintStreamItem, String>),
    #[cfg(feature = "inpaint")]
    AutoInpaintStreamFailed(String),
    #[cfg(feature = "inpaint")]
    ManualInpaintStreamRun(Result<ManualInpaintStreamItem, String>),
    #[cfg(all(feature = "styling", feature = "inpaint"))]
    PipelineStyleDetected(usize, EntryId, Result<(EntryStyle, easyscanlate_styling::StylePrediction), String>),
    #[cfg(feature = "styling")]
    StylingEngineReady(Result<StylingEngine, String>),
    #[cfg(feature = "styling")]
    StyleDetected(usize, EntryId, Result<EntryStyle, String>),
    #[cfg(feature = "segment")]
    SegmentEngineReady(Result<SegmentEngine, String>),
    #[cfg(feature = "segment")]
    SegmentStreamRun(Result<SegmentStreamItem, String>),
    #[cfg(feature = "segment")]
    SegmentStreamFailed(String),
    TranslateFinished(
        Vec<(usize, EntryId, String, String)>,
        Result<Vec<String>, String>,
    ),
    RetranslateFinished((usize, EntryId), Result<String, String>),
    MmtlSavePicked(Option<String>),
    MmtlOpenPicked(Option<String>),
    MmtlSaved(Result<String, String>),
    MmtlLoaded(MmtlLoadedPayload),
    NewProjectSourcePicked(Result<Vec<(String, u32, u32)>, String>),
    NewProjectFolderPicked(Result<Vec<(String, u32, u32)>, String>),
    NewProjectLocationPicked(Option<String>),
    CreateProjectPicked(Result<String, String>),
    RecentPickedToLoad(MmtlLoadedPayload),
    ExportFolderPicked(Option<String>),
    ExportFinished(Result<String, String>),
    ExportStreamRun(Result<ExportStreamItem, String>),
    ExportStreamFailed(String),
    TilesVisible(std::ops::Range<usize>),
    TileScrollEnded,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
// `Tab(TabId, TabMessage)` is large (376 bytes) by design: boxing it would
// add indirection to every UI message for negligible gain. Iced `Message`
// enums commonly allow this lint.
#[allow(clippy::large_enum_variant)]
pub enum Message {
    /// Frame actions from the custom title bar.
    Frame(FrameAction),
    /// A widget-level event from the ui crate.
    Ui(UiEvent),
    /// Per-tab async completion tagged with `TabId`.
    Tab(TabId, TabMessage),
    /// Global model event — forwards to TabId-tagged path (sync ModelEvents).
    /// Wired via `handle_tab_message(..., TabMessage::Model)` to keep dirty flag
    /// correctly attributed when rapid tab switches interleave with sync edits.
    Model(ModelEvent),
    // Global-only async completions (not per-tab)
    FontLoaded,
    SystemFonts(Vec<(String, String)>),
    StyleFontLoaded(String),
    CjkFallbackLoaded(usize),
    FetchModels,
    ModelsFetched(std::collections::HashMap<String, ui_translation::Provider>),
    /// Polled from `subscription`: drain the single-instance TCP listener.
    IpcPoll,
    /// External open requests (CLI forward, drag-drop, IPC). Each string is
    /// a raw path that may contain quotes/spaces.
    ExternalOpen(Vec<String>),
    // ——— Updates (Velopack; stubbed when the `updates` feature is off) ———
    UpdateCheckResult(Box<Option<crate::updater::UpdateInfo>>),
    UpdateDownloadStart,
    UpdateApply,
    UpdateDismiss,
    UpdateCheckAgain,
    UpdatePoll,
    // ——— Onboarding (first-run, blocking) ———
    OnboardingModelDone { id: String, result: Result<(), String> },
    OnboardingModelPoll,
    // ——— Blurred backdrop (Settings / Manage Models) ———
    BackdropCaptured(Box<iced::window::Screenshot>, backdrop::BackdropKind),
    BackdropReady(Option<backdrop::CapturedBackdrop>, backdrop::BackdropKind),
}

impl From<UiEvent> for Message {
    fn from(event: UiEvent) -> Self {
        Message::Ui(event)
    }
}

/// Session state: multi-tab — `tabs[0]` is permanent Home, `active` indexes current tab.
/// Per-tab state (project, images, panes, status, …) lives in `Tab`; globals (fonts, session, recent) stay on `App`.
pub struct App {
    pub(crate) tabs: Vec<Tab>,
    pub(crate) active: usize,
    pub(crate) next_tab_id: u64,
    pub(crate) engines: EnginePool,
    pub(crate) pending_close: Option<TabId>,
    pub(crate) font: Option<Font>,
    pub(crate) system_fonts: HashMap<String, String>,
    pub(crate) installed_fonts: Vec<String>,
    pub(crate) loaded_fonts: HashSet<String>,
    pub(crate) presets: StylePresets,
    pub(crate) tx: ui_translation::Session,
    pub(crate) connect_modal: Option<ConnectModal>,
    pub(crate) settings_open: bool,
    pub(crate) settings_tab: SettingsTab,
    pub(crate) settings_search: String,
    pub(crate) manage_models_open: bool,
    pub(crate) manage_models_search: String,
    pub(crate) backdrop_blur: Option<iced::widget::image::Handle>,
    pub(crate) backdrop_frame: Option<backdrop::CapturedBackdrop>,
    pub(crate) backdrop_pending: Option<backdrop::BackdropKind>,
    pub(crate) loading_blur: Option<iced::widget::image::Handle>,
    pub(crate) pending_load: Option<backdrop::PendingLoad>,
    /// Blurred card cover for the raster-export progress overlay (like
    /// `loading_blur`, cropped to the export card rect).
    pub(crate) export_blur: Option<iced::widget::image::Handle>,
    /// Stashed export job while the clean-base screenshot is captured.
    pub(crate) pending_export: Option<export::PendingExport>,
    pub(crate) recent_projects: Vec<easyscanlate_settings::RecentProject>,
    pub(crate) new_project: Option<new_project::NewProjectState>,
    pub frame: NativeFrame,
    pub(crate) ipc_listener: Option<crate::single_instance::Listener>,
    // ——— Updates (Velopack, per-user, GithubSource Liiesl/EasyScanlate) ———
    // Type is `velopack::UpdateInfo` with `updates`, empty stub without.
    pub update_info: Option<crate::updater::UpdateInfo>,
    pub update_downloading: bool,
    pub update_progress: i16,
    pub update_ready: bool,
    pub update_rx: Option<Arc<Mutex<mpsc::Receiver<i16>>>>,
    pub update_error: Option<String>,
    /// Blocking update-available popup (auto-check at startup). Shown with a
    /// blurred backdrop once the clean-base screenshot is ready; `Later`
    /// hides it but keeps `update_info` so Settings → Updates still offers
    /// Download.
    pub update_popup_visible: bool,
    /// Blurred snapshot cropped to the update popup card rect.
    pub update_blur: Option<iced::widget::image::Handle>,
    /// Update arrived while onboarding was blocking: show after finish.
    pub update_pending_popup: bool,
    // ——— Onboarding (first-run, blocking) ———
    pub onboarding: Option<onboarding::OnboardingState>,
    pub(crate) onboarding_rx: OnboardingRx,
    pub(crate) onboarding_active_id: Option<String>,
}

impl App {
    pub fn theme(&self) -> Theme {
        use iced::theme::palette::{Extended, Palette};
        let is_dark = easyscanlate_settings::get(|s| s.aurora_is_dark);
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
        Self {
            tabs: vec![Tab::home(TabId(0))],
            active: 0,
            next_tab_id: 1,
            engines: EnginePool::default(),
            font: None,
            system_fonts: HashMap::new(),
            installed_fonts: Vec::new(),
            loaded_fonts: HashSet::from([
                easyscanlate_model::ANIME_ACE_FAMILY.to_string(),
                easyscanlate_model::AUGIE_FAMILY.to_string(),
            ]),
            presets: easyscanlate_settings::get(|s| s.style_presets.clone()),
            tx: ui_translation::Session::default(),
            connect_modal: None,
            settings_open: false,
            settings_tab: SettingsTab::General,
            settings_search: String::new(),
            manage_models_open: false,
            manage_models_search: String::new(),
            backdrop_blur: None,
            backdrop_frame: None,
            backdrop_pending: None,
            loading_blur: None,
            pending_load: None,
            export_blur: None,
            pending_export: None,
            recent_projects: easyscanlate_settings::get(|s| s.recent_projects.clone()),
            new_project: None,
            frame,
            pending_close: None,
            ipc_listener: None,
            update_info: None,
            update_downloading: false,
            update_progress: 0,
            update_ready: false,
            update_rx: None,
            update_error: None,
            update_popup_visible: false,
            update_blur: None,
            update_pending_popup: false,
            onboarding: {
                // No `models` feature (e.g. test-ui): bypass the blocking
                // download wizard entirely — there is nothing to download.
                #[cfg(not(feature = "models"))]
                {
                    None
                }
                #[cfg(feature = "models")]
                {
                    let (completed, ver) = easyscanlate_settings::get(|s| (s.onboarding_completed, s.onboarding_version));
                    let is_completed = completed && ver >= easyscanlate_settings::CURRENT_ONBOARDING_VERSION;
                    if is_completed { None } else { Some(onboarding::OnboardingState::new()) }
                }
            },
            onboarding_rx: None,
            onboarding_active_id: None,
        }
    }

    pub(crate) fn active_tab(&self) -> &Tab {
        &self.tabs[self.active]
    }
    pub(crate) fn active_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }
    pub(crate) fn tab_by_id(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }
    pub(crate) fn tab_by_id_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }
    pub(crate) fn active_is_home(&self) -> bool {
        self.tabs[self.active].is_home()
    }
    pub(crate) fn active_state(&self) -> crate::app::state::ActiveTab<'_> {
        crate::app::state::ActiveTab { app: self, tab: &self.tabs[self.active] }
    }
}

pub fn boot(
    frame: NativeFrame,
    initial_mmtl: Option<std::path::PathBuf>,
    ipc_listener: Option<crate::single_instance::Listener>,
) -> (App, Task<Message>) {
    boot::boot(frame, initial_mmtl, ipc_listener)
}

pub(crate) use state::handle_model_event;

fn handle_tab_message(app: &mut App, tab_id: TabId, msg: TabMessage) -> Task<Message> {
    // New-tab creations carry a fresh TabId not yet in `tabs` (allocated at spawn
    // time). Handle them before the `idx` guard so the push isn't dropped.
    match &msg {
        TabMessage::MmtlLoaded(_) | TabMessage::RecentPickedToLoad(_) | TabMessage::CreateProjectPicked(_) => {
            return match msg {
                TabMessage::MmtlLoaded(res) => mmtl::handle_loaded(app, tab_id, res),
                TabMessage::RecentPickedToLoad(res) => {
                    match res {
                        Ok((project, images, display, temp_dir)) => {
                            debug_assert_eq!(project.image_count(), images.len());
                            return mmtl::push_project_tab(app, tab_id, project, images, display, temp_dir);
                        }
                        Err(e) => {
                            // Load resolved (failed): the reusable capture is stale.
                            app.backdrop_frame = None;
                            if let Some(idx) = app.tabs.iter().position(|t| t.id == tab_id) {
                                let tab = &mut app.tabs[idx];
                                if tab.loading {
                                    tab.loading = false;
                                    tab.loading_path = None;
                                    tab.loading_phase = 0.0;
                                }
                                tab.status = format!("Load failed: {e}");
                            } else {
                                app.active_tab_mut().status = format!("Load failed: {e}");
                            }
                            return Task::none();
                        }
                    }
                }
                TabMessage::CreateProjectPicked(res) => {
                    match res {
                        Ok(path_str) => match mmtl::load_created_project(path_str.clone()) {
                            Ok((project, images, display, temp_dir)) => {
                                debug_assert_eq!(project.image_count(), images.len());
                                return mmtl::push_project_tab(app, tab_id, project, images, display, temp_dir);
                            }
                            Err(e) => {
                                app.backdrop_frame = None;
                                if let Some(idx) = app.tabs.iter().position(|t| t.id == tab_id) {
                                    let tab = &mut app.tabs[idx];
                                    if tab.loading {
                                        tab.loading = false;
                                        tab.loading_path = None;
                                        tab.loading_phase = 0.0;
                                    }
                                    tab.status = format!("Created {path_str} but load failed: {e}");
                                } else {
                                    app.active_tab_mut().status = format!("Created {path_str} but load failed: {e}");
                                }
                                easyscanlate_settings::touch_recent(path_str.clone());
                                app.recent_projects = easyscanlate_settings::get(|s| s.recent_projects.clone());
                                return Task::none();
                            }
                        },
                        Err(e) => {
                            app.backdrop_frame = None;
                            if let Some(idx) = app.tabs.iter().position(|t| t.id == tab_id) {
                                let tab = &mut app.tabs[idx];
                                if tab.loading {
                                    tab.loading = false;
                                    tab.loading_path = None;
                                    tab.loading_phase = 0.0;
                                }
                                tab.status = format!("Create failed: {e}");
                            } else {
                                app.active_tab_mut().status = format!("Create failed: {e}");
                            }
                            return Task::none();
                        }
                    }
                }
                _ => unreachable!(),
            };
        }
        _ => {}
    }
    let Some(idx) = app.tabs.iter().position(|t| t.id == tab_id) else {
        return Task::none();
    };
    match msg {
        TabMessage::Model(ev) => {
            handle_model_event(&mut app.tabs[idx], ev);
            Task::none()
        }
        TabMessage::ThumbDecoded(index, result) => {
            if index < app.tabs[idx].images.len() {
                app.tabs[idx].images[index].decode.thumb = match result {
                    Ok(decoded) => Tier::Ready(decoded),
                    Err(_) => Tier::Failed,
                };
            }
            Task::none()
        }
        TabMessage::FullDecoded(index, result) => {
            let len = app.tabs[idx].images.len();
            if index < len {
                let keep = app.tabs[idx].scheduler.keep_full(len, index);
                app.tabs[idx].images[index].decode.full = if keep {
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
        TabMessage::SettleElapsed(seq) => {
            let accept = app.tabs[idx].scheduler.accept_elapsed(seq);
            if accept {
                let project_clone = app.tabs[idx].project.clone();
                let tab = &mut app.tabs[idx];
                tab.scheduler.settle_with_project(&mut tab.images, &project_clone, {
                    let tid = tab_id;
                    move |i, r| Message::Tab(tid, TabMessage::FullDecoded(i, r))
                })
            } else {
                Task::none()
            }
        }
        TabMessage::TilesVisible(range) => {
            let tid = tab_id;
            app.tabs[idx].scheduler.schedule(range, move |seq| Message::Tab(tid, TabMessage::SettleElapsed(seq)))
        }
        TabMessage::TileScrollEnded => {
            let project_clone = app.tabs[idx].project.clone();
            let tab = &mut app.tabs[idx];
            let tid = tab_id;
            tab.scheduler.settle_with_project(&mut tab.images, &project_clone, move |i, r| Message::Tab(tid, TabMessage::FullDecoded(i, r)))
        }
        #[cfg(feature = "ocr")]
        TabMessage::ParallelEngineReady(result) => ocr::handle_parallel_ready(app, tab_id, result),
        #[cfg(feature = "ocr")]
        TabMessage::ManualOcrEngineReady(result) => ocr::handle_manual_ocr_engine_ready(app, tab_id, result),
        #[cfg(feature = "ocr")]
        TabMessage::ManualOcrMultiFinished(result) => ocr::handle_manual_ocr_finished(app, tab_id, result),
        #[cfg(feature = "ocr")]
        TabMessage::OcrStreamRun(result) => ocr::handle_ocr_stream_run(app, tab_id, result),
        #[cfg(feature = "ocr")]
        TabMessage::OcrStreamFailed(e) => ocr::handle_ocr_stream_failed(app, tab_id, e),
        #[cfg(feature = "ocr")]
        TabMessage::OcrTick => {
            // OcrTick is per-tab; nothing to do besides ensure still running
            Task::none()
        }
        TabMessage::TranslateTick => {
            if app.tabs[idx].translating {
                app.tabs[idx].translate_anim_phase = (app.tabs[idx].translate_anim_phase + 0.016) % 6.0;
            } else {
                app.tabs[idx].translate_anim_phase = 0.0;
            }
            Task::none()
        }
        TabMessage::LoadingTick => {
            if app.tabs[idx].loading {
                app.tabs[idx].loading_phase = (app.tabs[idx].loading_phase + 0.016) % 6.0;
            } else {
                app.tabs[idx].loading_phase = 0.0;
            }
            Task::none()
        }
        #[cfg(feature = "inpaint")]
        TabMessage::InpaintEngineReady(result) => inpaint::handle_inpaint_engine_ready(app, tab_id, result),
        #[cfg(feature = "styling")]
        TabMessage::StylingEngineReady(result) => styling::handle_styling_ready(app, tab_id, result),
        #[cfg(feature = "styling")]
        TabMessage::StyleDetected(index, id, result) => styling::handle_style_detected(app, tab_id, index, id, result),
        #[cfg(all(feature = "styling", feature = "inpaint"))]
        TabMessage::PipelineStyleDetected(index, id, result) => styling::handle_pipeline_style_detected(app, tab_id, index, id, result),
        #[cfg(feature = "inpaint")]
        TabMessage::AutoInpaintEngineReady(backend, result) => inpaint::handle_auto_engine_ready(app, tab_id, backend, result),
        #[cfg(feature = "inpaint")]
        TabMessage::AutoInpaintFinished(index, id, result) => inpaint::handle_auto_finished(app, tab_id, index, id, result),
        #[cfg(feature = "inpaint")]
        TabMessage::AutoInpaintStreamRun(result) => inpaint::handle_auto_stream_run(app, tab_id, result),
        #[cfg(feature = "inpaint")]
        TabMessage::AutoInpaintStreamFailed(e) => inpaint::handle_auto_stream_failed(app, tab_id, e),
        #[cfg(feature = "inpaint")]
        TabMessage::ManualInpaintStreamRun(result) => inpaint::handle_manual_stream_run(app, tab_id, result),
        #[cfg(feature = "segment")]
        TabMessage::SegmentEngineReady(result) => segment::handle_engine_ready(app, tab_id, result),
        #[cfg(feature = "segment")]
        TabMessage::SegmentStreamRun(result) => segment::handle_stream_run(app, tab_id, result),
        #[cfg(feature = "segment")]
        TabMessage::SegmentStreamFailed(e) => segment::handle_stream_failed(app, tab_id, e),
        #[cfg(feature = "inpaint")]
        TabMessage::ManualMultiInpaintFinished(result) => inpaint::handle_inpaint_finished(app, tab_id, result),
        TabMessage::TranslateFinished(jobs, result) => translation::handle_translate_finished(app, tab_id, jobs, result),
        TabMessage::RetranslateFinished((index, entry_id), result) => translation::handle_retranslate_finished(app, tab_id, index, entry_id, result),
        TabMessage::MmtlSavePicked(picked) => mmtl::handle_save_picked(app, tab_id, picked),
        TabMessage::MmtlOpenPicked(picked) => match picked {
            // Cancelled picker: status only, never a load — no capture.
            None => mmtl::handle_open_picked(app, tab_id, None),
            Some(path) => backdrop::begin_load(app, backdrop::PendingLoad::OpenPicked { tab_id, path }),
        },
        TabMessage::MmtlSaved(result) => mmtl::handle_saved(app, tab_id, result),
        TabMessage::NewProjectSourcePicked(result) => new_project::handle_source_picked(app, tab_id, result),
        TabMessage::NewProjectFolderPicked(result) => new_project::handle_folder_picked(app, tab_id, result),
        TabMessage::NewProjectLocationPicked(picked) => new_project::handle_location_picked(app, tab_id, picked),
        TabMessage::ExportFolderPicked(picked) => export::handle_export_picked(app, tab_id, picked),
        TabMessage::ExportFinished(result) => export::handle_export_finished(app, tab_id, result),
        TabMessage::ExportStreamRun(result) => export::handle_export_stream_run(app, tab_id, result),
        TabMessage::ExportStreamFailed(e) => export::handle_export_stream_failed(app, tab_id, e),
        TabMessage::MmtlLoaded(_) | TabMessage::CreateProjectPicked(_) | TabMessage::RecentPickedToLoad(_) => {
            unreachable!("MmtlLoaded/Create/Recent are handled before the idx guard")
        }
    }
}

pub fn update(app: &mut App, message: Message) -> Task<Message> {
    // Blocking onboarding: while wizard is open, only onboarding and system messages are allowed.
    if app.onboarding.is_some() {
        let allowed = match &message {
            Message::Ui(ev) => matches!(
                ev,
                UiEvent::OnboardingNext
                    | UiEvent::OnboardingBack
                    | UiEvent::OnboardingDownloadAll
                    | UiEvent::OnboardingRetry(_)
                    | UiEvent::OnboardingToggleTheme
                    | UiEvent::OnboardingFontSize(_)
                    | UiEvent::OnboardingToggleAutoStyle
                    | UiEvent::OnboardingToggleAutoSfx
                    | UiEvent::OnboardingToggleAutoInpaint
                    | UiEvent::OnboardingOpenTranslationSettings
                    | UiEvent::OnboardingSkipTranslation
                    | UiEvent::OnboardingFinish
                    | UiEvent::OnboardingReplay
                    | UiEvent::TranslateConnect(_)
                    | UiEvent::TranslateDisconnect(_)
                    | UiEvent::ConnectModalKey(_)
                    | UiEvent::ConnectModalBaseUrl(_)
                    | UiEvent::ConnectModalModel(_)
                    | UiEvent::ConnectModalSubmit
                    | UiEvent::ConnectModalCancel
                    | UiEvent::OpenUrl(_)
                    | UiEvent::SettingsOpen
                    | UiEvent::SettingsOpenTab(_)
            ),
            Message::OnboardingModelDone { .. }
            | Message::OnboardingModelPoll
            | Message::Frame(_)
            | Message::FontLoaded
            | Message::SystemFonts(_)
            | Message::StyleFontLoaded(_)
            | Message::CjkFallbackLoaded(_)
            | Message::UpdateCheckResult(_)
            | Message::UpdatePoll
            | Message::IpcPoll
            | Message::FetchModels
            | Message::ModelsFetched(_)
            | Message::Tab(_, _)
            | Message::ExternalOpen(_)
            | Message::BackdropCaptured(_, _)
            | Message::BackdropReady(_, _) => true,
            _ => false,
        };
        if !allowed {
            return Task::none();
        }
    }
    
    match message {
        Message::IpcPoll => {
            let pending = app.ipc_listener.as_mut().map(|l| l.poll()).unwrap_or_default();
            let paths: Vec<String> = pending.into_iter().filter(|s| !s.is_empty()).collect();
            if paths.is_empty() { Task::none() } else { backdrop::begin_load(app, backdrop::PendingLoad::External(paths)) }
        }
        Message::ExternalOpen(paths) => backdrop::begin_load(app, backdrop::PendingLoad::External(paths)),
        Message::Frame(action) => app.frame.update(action, Message::Frame),
        Message::Tab(tab_id, tab_msg) => handle_tab_message(app, tab_id, tab_msg),
        Message::Model(ev) => {
            // Legacy flat ModelEvent — forward to TabId-tagged path so
            // rapid tab switches cannot misattribute dirty flag (Q2).
            let tid = app.active_tab().id;
            handle_tab_message(app, tid, TabMessage::Model(ev))
        }
        Message::FetchModels => translation::handle_fetch_models(app),
        Message::ModelsFetched(providers) => translation::handle_models_fetched(app, providers),
        Message::Ui(UiEvent::TabSelected(raw)) => { app.backdrop_frame = None; tabs::handle_selected(app, raw) }
        Message::Ui(UiEvent::TabClose(raw)) => { app.backdrop_frame = None; tabs::handle_close(app, raw) }
        Message::Ui(UiEvent::TabCloseConfirmed(raw, save)) => { app.backdrop_frame = None; tabs::handle_close_confirmed(app, raw, save) }
        Message::Ui(UiEvent::TabCloseCancel) => tabs::handle_close_cancel(app),
        Message::Ui(UiEvent::TabCloseOthers(raw)) => { app.backdrop_frame = None; tabs::handle_close_others(app, raw) }
        Message::Ui(UiEvent::TabCloseAll) => { app.backdrop_frame = None; tabs::handle_close_all(app) }
        Message::Ui(UiEvent::TabNew) => new_project::handle_new(app),
        Message::Ui(UiEvent::HomeNewProject) => new_project::handle_new(app),
        Message::Ui(UiEvent::HomeOpenProject) => mmtl::handle_open(app),
        Message::Ui(UiEvent::HomeRecentClicked(path)) => {
            backdrop::begin_load(app, backdrop::PendingLoad::Recent(path))
        }
        Message::Ui(UiEvent::HomeSettings) => settings::handle_settings_open(app),
        Message::Ui(UiEvent::NewProjectClose) => new_project::handle_close(app),
        Message::Ui(UiEvent::NewProjectSourceImage) => new_project::handle_source_image(app),
        Message::Ui(UiEvent::NewProjectSourceFolder) => new_project::handle_source_folder(app),
        Message::Ui(UiEvent::NewProjectLocationBrowse) => new_project::handle_location_browse(app),
        Message::Ui(UiEvent::NewProjectOriginalLang(lang)) => new_project::handle_original_lang(app, lang),
        Message::Ui(UiEvent::NewProjectCreate) => backdrop::begin_load(app, backdrop::PendingLoad::Create),
        Message::Ui(UiEvent::StartOcr) => ocr::handle_start_ocr(app),
        Message::Ui(UiEvent::StopOcr) => ocr::handle_stop_ocr(app),
        Message::FontLoaded => boot::handle_font_loaded(app),
        Message::SystemFonts(fonts) => boot::handle_system_fonts(app, fonts),
        Message::StyleFontLoaded(name) => boot::handle_style_font_loaded(app, name),
        Message::CjkFallbackLoaded(count) => boot::handle_cjk_fallback_loaded(app, count),
        Message::Ui(UiEvent::ProfileSelect(id)) => profile::handle_select(app, id),
        Message::Ui(UiEvent::ProfileCreate) => profile::handle_create(app),
        Message::Ui(UiEvent::TranslationPanelMode(mode)) => translation::handle_panel_mode(app, mode),
        Message::Ui(UiEvent::BaseProfileSelect(id)) => translation::handle_base_select(app, id),
        Message::Ui(UiEvent::TargetProfileSelect(sel)) => translation::handle_target_select(app, sel),
        Message::Ui(UiEvent::TilesVisible(range)) => {
            let tid = app.active_tab().id;
            handle_tab_message(app, tid, TabMessage::TilesVisible(range))
        },
        Message::Ui(UiEvent::TileScrollEnded) => {
            let tid = app.active_tab().id;
            handle_tab_message(app, tid, TabMessage::TileScrollEnded)
        }

        Message::Ui(UiEvent::Translate) => translation::handle_translate(app),
        Message::Ui(UiEvent::TranslateModelSelect { provider, model }) => translation::handle_model_select(app, provider, model),
        Message::Ui(UiEvent::TranslateLang(lang)) => translation::handle_lang(app, lang),
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
        Message::Ui(UiEvent::ReorderEntries) => entries::handle_reorder(app),
        Message::Ui(UiEvent::ManualModeEnter(mode)) => manual::handle_enter(app, mode),
        Message::Ui(UiEvent::ManualModeCancel) => manual::handle_cancel(app),
        Message::Ui(UiEvent::ManualModeReset) => manual::handle_reset(app),
        Message::Ui(UiEvent::ManualModeStart) => manual::handle_start(app),
        Message::Ui(UiEvent::ManualSelectionAdded(pair)) => manual::handle_selection(app, vec![pair]),
        Message::Ui(UiEvent::ManualSelectionSpan(spans)) => manual::handle_selection(app, spans),
        Message::Ui(UiEvent::ToggleOverlayText) => main_area::handle_toggle_overlay(app),
        Message::Ui(UiEvent::ToggleInpaintLayer) => main_area::handle_toggle_inpaint(app),
        Message::Ui(UiEvent::MainAreaMode(mode)) => main_area::handle_mode(app, mode),
        Message::Ui(UiEvent::ViewerScroll(anchor)) => main_area::handle_viewer_scroll(app, anchor),
        Message::Ui(UiEvent::EntryToolbar((index, id, action))) => edit::handle_entry_toolbar(app, index, id, action),
        Message::Ui(UiEvent::EntryMoved((index, id, quad))) => edit::handle_entry_moved(app, index, id, quad),
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
        Message::Ui(UiEvent::PanelResized(resized)) => main_area::handle_panel_resized(app, resized),
        Message::Ui(UiEvent::SidePanelResized(resized)) => main_area::handle_side_panel_resized(app, resized),
        Message::Ui(UiEvent::StylingPaneResized(resized)) => main_area::handle_styling_pane_resized(app, resized),
        Message::Ui(UiEvent::SettingsOpen) => settings::handle_settings_open(app),
        Message::Ui(UiEvent::SettingsOpenTab(tab)) => settings::handle_settings_open_tab(app, tab),
        Message::Ui(UiEvent::SettingsClose) => settings::handle_settings_close(app),
        Message::Ui(UiEvent::SettingsTab(tab)) => settings::handle_settings_tab(app, tab),
        Message::Ui(UiEvent::SettingsSearch(query)) => settings::handle_settings_search(app, query),
        Message::Ui(UiEvent::SettingsChanged) => settings::handle_settings_changed(app),
        Message::Ui(UiEvent::SettingEdit(edit)) => settings::handle_setting_edit(app, edit),
        Message::Ui(UiEvent::OpenUrl(url)) => settings::handle_open_url(app, url),
        Message::Ui(UiEvent::SaveProject) => mmtl::handle_save(app),
        Message::Ui(UiEvent::ExportAll) => export::handle_export_all(app),
        Message::Ui(UiEvent::ExportCancel) => export::handle_export_cancel(app),
        // ——— Onboarding (first-run, blocking) ———
        Message::Ui(UiEvent::OnboardingNext) => onboarding::handle_next(app),
        Message::Ui(UiEvent::OnboardingBack) => onboarding::handle_back(app),
        Message::Ui(UiEvent::OnboardingDownloadAll) => onboarding::handle_download_all(app),
        Message::Ui(UiEvent::OnboardingRetry(id)) => onboarding::handle_retry(app, id),
        Message::Ui(UiEvent::OnboardingToggleTheme) => onboarding::handle_toggle_theme(app),
        Message::Ui(UiEvent::OnboardingFontSize(inc)) => onboarding::handle_font_size(app, inc),
        Message::Ui(UiEvent::OnboardingToggleAutoStyle) => onboarding::handle_toggle_auto_style(app),
        Message::Ui(UiEvent::OnboardingToggleAutoSfx) => onboarding::handle_toggle_auto_sfx(app),
        Message::Ui(UiEvent::OnboardingToggleAutoInpaint) => onboarding::handle_toggle_auto_inpaint(app),
        Message::Ui(UiEvent::OnboardingOpenTranslationSettings) => {
            // Deprecated: inline is baked into onboarding; no Settings popup (covered)
            Task::none()
        }
        Message::Ui(UiEvent::OnboardingSkipTranslation) => onboarding::handle_skip_translation(app),
        Message::Ui(UiEvent::OnboardingFinish) => onboarding::handle_finish(app),
        Message::Ui(UiEvent::OnboardingReplay) => onboarding::handle_replay(app),
        // ——— Updates (Velopack) ———
        Message::Ui(UiEvent::UpdateCheck) => update::handle_check(app),
        Message::Ui(UiEvent::UpdateDownload) => update::handle_download(app),
        Message::Ui(UiEvent::UpdateApply) => update::handle_apply(app),
        Message::Ui(UiEvent::UpdateDismiss) => update::handle_dismiss(app),
        Message::UpdateCheckResult(info) => update::handle_check_result(app, *info),
        Message::UpdateCheckAgain => update::handle_check_again(app),
        Message::UpdateDownloadStart => update::handle_download(app),
        Message::UpdateApply => update::handle_apply(app),
        Message::UpdateDismiss => update::handle_dismiss(app),
        Message::UpdatePoll => update::handle_poll(app),
        Message::OnboardingModelDone { id, result } => onboarding::handle_model_done(app, id, result),
        Message::OnboardingModelPoll => onboarding::handle_poll(app),
        Message::BackdropCaptured(shot, kind) => backdrop::handle_captured(app, *shot, kind),
        Message::BackdropReady(handle, kind) => backdrop::handle_ready(app, handle, kind),
    }
}

pub fn subscription(app: &App) -> Subscription<Message> {
    subscription::subscription(app)
}

pub fn view(app: &App) -> Element<'_, Message> {
    view::view(app)
}

#[cfg(test)]
mod tests;
