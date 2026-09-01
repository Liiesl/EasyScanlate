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

use iced::{Color, Element, Font, Subscription, Task, Theme};
use neverliie_iced_widgets::title_bar::{FrameAction, NativeFrame};

#[cfg(feature = "inpaint")]
use scanlateit_inpaint::Engine as InpaintEngine;
use scanlateit_model::{EntryId, EntryStyle, ModelEvent, NewEntry, Project};
use scanlateit_settings::StylePresets;
#[cfg(feature = "inpaint")]
use scanlateit_settings::InpaintBackend;
#[cfg(feature = "ocr")]
use scanlateit_ocr::{self as ocr_engine, ParallelEngine};
#[cfg(feature = "styling")]
use scanlateit_styling::Engine as StylingEngine;
#[cfg(feature = "segment")]
use scanlateit_segment::Engine as SegmentEngine;
use scanlateit_ui::translation as ui_translation;
use scanlateit_ui::main_area::decode::{DecodedPage, PageDecode, Tier};
use scanlateit_ui::{
    event::{EditOrigin, MainAreaMode, ManualMode, SettingsTab, TargetProfileSelection, TranslationPanelMode, UiEvent},
    ConnectModal, LoadedImage, UiState,
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
pub mod manual;
pub mod styling;
pub mod segment;
pub mod pipeline;
pub mod translation;
pub mod settings;
pub mod mmtl;
pub mod export;
pub mod view;
pub mod tab;
pub mod tabs;
pub mod confirm_close;
pub mod queue;

use layout::IMAGE_FILTERS;
use tab::{AutoInpaintJob, EnginePool, Tab, TabId};

#[derive(Debug, Clone)]
pub struct NewProjectState {
    pub source_files: Vec<(String, u32, u32)>,
    pub original_lang: String,
    pub project_location: Option<String>,
}

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
    ManualOcrEngineReady(Result<scanlateit_ocr::Engine, String>),
    #[cfg(feature = "ocr")]
    ManualOcrMultiFinished(Result<Vec<(usize, Vec<NewEntry>)>, String>),
    #[cfg(feature = "ocr")]
    OcrStreamRun(Result<ocr_engine::RunEvent, String>),
    #[cfg(feature = "ocr")]
    OcrStreamFailed(String),
    #[cfg(feature = "ocr")]
    OcrTick,
    TranslateTick,
    #[cfg(feature = "inpaint")]
    InpaintEngineReady(Result<InpaintEngine, String>),
    #[cfg(feature = "inpaint")]
    ManualMultiInpaintFinished(Result<Vec<(usize, Vec<(image::RgbaImage, [f32; 4], Option<scanlateit_model::Quad>)>)>, String>),
    #[cfg(feature = "inpaint")]
    AutoInpaintEngineReady(InpaintBackend, Result<InpaintEngine, String>),
    #[cfg(feature = "inpaint")]
    AutoInpaintFinished(usize, EntryId, Result<Vec<(usize, image::RgbaImage, [f32; 4], Option<scanlateit_model::Quad>)>, String>),
    #[cfg(feature = "inpaint")]
    AutoInpaintLamaBatchFinished(Vec<(usize, EntryId, Result<Vec<(usize, image::RgbaImage, [f32; 4], Option<scanlateit_model::Quad>)>, String>)>),
    #[cfg(feature = "inpaint")]
    AutoInpaintAotBatchFinished(Vec<(usize, EntryId, Result<Vec<(usize, image::RgbaImage, [f32; 4], Option<scanlateit_model::Quad>)>, String>)>),
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
    TilesVisible(std::ops::Range<usize>),
    TileScrollEnded,
}

#[derive(Debug, Clone)]
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
    pub(crate) recent_projects: Vec<scanlateit_settings::RecentProject>,
    pub(crate) new_project: Option<NewProjectState>,
    pub frame: NativeFrame,
    pub(crate) ipc_listener: Option<crate::single_instance::Listener>,
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
        Self {
            tabs: vec![Tab::home(TabId(0))],
            active: 0,
            next_tab_id: 1,
            engines: EnginePool::default(),
            font: None,
            system_fonts: HashMap::new(),
            installed_fonts: Vec::new(),
            loaded_fonts: HashSet::from([
                scanlateit_model::ANIME_ACE_FAMILY.to_string(),
                scanlateit_model::AUGIE_FAMILY.to_string(),
            ]),
            presets: scanlateit_settings::get(|s| s.style_presets.clone()),
            tx: ui_translation::Session::default(),
            connect_modal: None,
            settings_open: false,
            settings_tab: SettingsTab::General,
            settings_search: String::new(),
            manage_models_open: false,
            manage_models_search: String::new(),
            recent_projects: scanlateit_settings::get(|s| s.recent_projects.clone()),
            new_project: None,
            frame,
            pending_close: None,
            ipc_listener: None,
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

pub(crate) fn close_tab_immediate(app: &mut App, id: TabId) -> Task<Message> {
    if let Some(idx) = app.tabs.iter().position(|t| t.id == id) {
        if app.tabs[idx].is_home() {
            return Task::none();
        }
        // cancel any queued/running jobs for this tab
        app.engines.queue.cancel_pending_for_tab(id);
        let freed = !app.engines.queue.cancel_running_for_tab(id).is_empty();
        let promote = if freed {
            crate::app::queue::dispatch_pending(app)
        } else {
            Task::none()
        };
        if freed {
            crate::app::queue::refresh_queued_statuses(app);
        }
        app.tabs.remove(idx);
        if app.active >= app.tabs.len() {
            app.active = app.tabs.len().saturating_sub(1);
        } else if idx < app.active {
            app.active -= 1;
        }
        if app.active >= app.tabs.len() && !app.tabs.is_empty() {
            app.active = app.tabs.len() - 1;
        }
        if app.pending_close == Some(id) {
            app.pending_close = None;
        }
        return promote;
    }
    Task::none()
}

pub(crate) fn handle_model_event(tab: &mut Tab, event: ModelEvent) {
    // Granular live-DB reactivity: every Project::*_with_event flows here via
    // Message::Model so UI state (selection, editing, inpaint cache) stays in
    // sync without coarse "DataChanged" broadcasts.
    // Mark dirty for mutating events (P5 will add modal; for now just flag).
    match &event {
        ModelEvent::EntryDeleted { .. }
        | ModelEvent::EntriesReordered { .. }
        | ModelEvent::EntryMoved { .. }
        | ModelEvent::EntriesAdded { .. }
        | ModelEvent::ImageAdded { .. }
        | ModelEvent::EntryTextUpdated { .. }
        | ModelEvent::EntryStyleUpdated { .. }
        | ModelEvent::ProfileCreated { .. }
        | ModelEvent::ProfileRemoved { .. }
        | ModelEvent::ProfileSelected { .. }
        | ModelEvent::ProfileRenamed { .. }
        | ModelEvent::InpaintAdded { .. }
        | ModelEvent::InpaintRemoved { .. }
        | ModelEvent::NoteUpdated { .. }
        | ModelEvent::EntryRestored { .. } => {
            tab.dirty = true;
        }
    }
    match event {
        ModelEvent::EntryDeleted { id } => {
            if tab.selected.is_some_and(|(_, sel_id)| sel_id == id) {
                tab.selected = None;
                crate::app::edit::clear_editing_tab(tab);
            }
            // editing may be on same id even if not selected (panel origin)
            if tab.editing.is_some_and(|(_, eid)| eid == id) {
                crate::app::edit::clear_editing_tab(tab);
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
            debug_assert!(tab.images.len() == tab.project.image_count());
        }
        ModelEvent::ImageAdded { .. } => {
            debug_assert!(tab.images.len() == tab.project.image_count());
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

fn handle_tab_message(app: &mut App, tab_id: TabId, msg: TabMessage) -> Task<Message> {
    // New-tab creations carry a fresh TabId not yet in `tabs` (allocated at spawn
    // time). Handle them before the `idx` guard so the push isn't dropped.
    match &msg {
        TabMessage::MmtlLoaded(_) | TabMessage::RecentPickedToLoad(_) | TabMessage::CreateProjectPicked(_) => {
            return match msg {
                TabMessage::MmtlLoaded(res) => mmtl::handle_loaded_for(app, tab_id, res),
                TabMessage::RecentPickedToLoad(res) => {
                    match res {
                        Ok((project, images, display, temp_dir)) => {
                            debug_assert_eq!(project.image_count(), images.len());
                            return mmtl::push_project_tab(app, tab_id, project, images, display, temp_dir);
                        }
                        Err(e) => {
                            if let Some(idx) = app.tabs.iter().position(|t| t.id == tab_id) {
                                app.tabs[idx].status = format!("Load failed: {e}");
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
                                if let Some(idx) = app.tabs.iter().position(|t| t.id == tab_id) {
                                    app.tabs[idx].status = format!("Created {path_str} but load failed: {e}");
                                } else {
                                    app.active_tab_mut().status = format!("Created {path_str} but load failed: {e}");
                                }
                                scanlateit_settings::touch_recent(path_str.clone());
                                app.recent_projects = scanlateit_settings::get(|s| s.recent_projects.clone());
                                return Task::none();
                            }
                        },
                        Err(e) => {
                            if let Some(idx) = app.tabs.iter().position(|t| t.id == tab_id) {
                                app.tabs[idx].status = format!("Create failed: {e}");
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
        TabMessage::ParallelEngineReady(result) => ocr::handle_parallel_ready_for(app, tab_id, result),
        #[cfg(feature = "ocr")]
        TabMessage::ManualOcrEngineReady(result) => ocr::handle_manual_ocr_engine_ready_for(app, tab_id, result),
        #[cfg(feature = "ocr")]
        TabMessage::ManualOcrMultiFinished(result) => ocr::handle_manual_ocr_finished_for(app, tab_id, result),
        #[cfg(feature = "ocr")]
        TabMessage::OcrStreamRun(result) => ocr::handle_ocr_stream_run_for(app, tab_id, result),
        #[cfg(feature = "ocr")]
        TabMessage::OcrStreamFailed(e) => ocr::handle_ocr_stream_failed_for(app, tab_id, e),
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
        #[cfg(feature = "inpaint")]
        TabMessage::InpaintEngineReady(result) => inpaint::handle_inpaint_engine_ready_for(app, tab_id, result),
        #[cfg(feature = "styling")]
        TabMessage::StylingEngineReady(result) => styling::handle_styling_ready_for(app, tab_id, result),
        #[cfg(feature = "styling")]
        TabMessage::StyleDetected(index, id, result) => styling::handle_style_detected_for(app, tab_id, index, id, result),
        #[cfg(all(feature = "styling", feature = "inpaint"))]
        TabMessage::PipelineStyleDetected(index, id, result) => styling::handle_pipeline_style_detected_for(app, tab_id, index, id, result),
        #[cfg(feature = "inpaint")]
        TabMessage::AutoInpaintEngineReady(backend, result) => inpaint::handle_auto_engine_ready_for(app, tab_id, backend, result),
        #[cfg(feature = "inpaint")]
        TabMessage::AutoInpaintFinished(index, id, result) => inpaint::handle_auto_finished_for(app, tab_id, index, id, result),
        #[cfg(feature = "inpaint")]
        TabMessage::AutoInpaintLamaBatchFinished(batch) => inpaint::handle_auto_batch_for(app, tab_id, batch),
        #[cfg(feature = "inpaint")]
        TabMessage::AutoInpaintAotBatchFinished(batch) => inpaint::handle_auto_batch_for(app, tab_id, batch),
        #[cfg(feature = "segment")]
        TabMessage::SegmentEngineReady(result) => segment::handle_engine_ready_for(app, tab_id, result),
        #[cfg(feature = "segment")]
        TabMessage::SegmentFiltered(result) => segment::handle_filtered_for(app, tab_id, result),
        #[cfg(feature = "inpaint")]
        TabMessage::ManualMultiInpaintFinished(result) => inpaint::handle_inpaint_finished_for(app, tab_id, result),
        TabMessage::TranslateFinished(jobs, result) => translation::handle_translate_finished_for(app, tab_id, jobs, result),
        TabMessage::RetranslateFinished((index, entry_id), result) => translation::handle_retranslate_finished_for(app, tab_id, index, entry_id, result),
        TabMessage::MmtlSavePicked(picked) => mmtl::handle_save_picked_for(app, tab_id, picked),
        TabMessage::MmtlOpenPicked(picked) => mmtl::handle_open_picked_for(app, tab_id, picked),
        TabMessage::MmtlSaved(result) => mmtl::handle_saved_for(app, tab_id, result),
        TabMessage::NewProjectSourcePicked(result) => {
            match result {
                Ok(files) if !files.is_empty() => {
                    if let Some(np) = app.new_project.as_mut() { np.source_files = files; }
                }
                Ok(_) => {}
                Err(e) => { app.tabs[idx].status = e; }
            }
            Task::none()
        }
        TabMessage::NewProjectFolderPicked(result) => {
            match result {
                Ok(files) if !files.is_empty() => {
                    if let Some(np) = app.new_project.as_mut() { np.source_files = files; }
                }
                Ok(_) => { app.tabs[idx].status = "No images found in folder.".to_string(); }
                Err(e) => { app.tabs[idx].status = e; }
            }
            Task::none()
        }
        TabMessage::NewProjectLocationPicked(picked) => {
            if let Some(p) = picked { if let Some(np) = app.new_project.as_mut() { np.project_location = Some(p); } }
            Task::none()
        }
        TabMessage::ExportFolderPicked(picked) => export::handle_export_picked_for(app, tab_id, picked),
        TabMessage::ExportFinished(result) => export::handle_export_finished_for(app, tab_id, result),
        TabMessage::MmtlLoaded(_) | TabMessage::CreateProjectPicked(_) | TabMessage::RecentPickedToLoad(_) => {
            unreachable!("MmtlLoaded/Create/Recent are handled before the idx guard")
        }
    }
}

fn handle_external_opens(app: &mut App, paths: Vec<String>) -> Task<Message> {
    if paths.is_empty() {
        return Task::none();
    }
    let mut tasks = Vec::new();
    for raw in paths {
        let trimmed = raw.trim().trim_matches('"').trim().to_string();
        if trimmed.is_empty() {
            continue;
        }
        // Only .mmtl is handled; other drops are ignored with a status hint.
        if !trimmed.to_ascii_lowercase().ends_with(".mmtl") {
            app.active_tab_mut().status = format!("Not a .mmtl: {trimmed}");
            continue;
        }
        let path = std::path::PathBuf::from(&trimmed);
        if !path.exists() {
            app.active_tab_mut().status = format!("Missing: {}", path.display());
            continue;
        }
        // Allocate a fresh TabId for the incoming project (same as HomeRecentClicked).
        let new_id = TabId(app.next_tab_id);
        app.next_tab_id += 1;
        app.active_tab_mut().status = format!("Loading {}...", path.display());
        let path_clone = path.clone();
        tasks.push(Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    mmtl::load_created_project(path_clone.to_string_lossy().to_string())
                })
                .await
                .unwrap_or_else(|e| Err(format!("load task failed: {e}")))
            },
            move |res| Message::Tab(new_id, TabMessage::RecentPickedToLoad(res)),
        ));
    }
    if tasks.is_empty() {
        Task::none()
    } else {
        Task::batch(tasks)
    }
}

pub fn update(app: &mut App, message: Message) -> Task<Message> {
    let task = match message {
        Message::IpcPoll => {
            // Drain single-instance listener (secondary forwards via TCP).
            let pending = app
                .ipc_listener
                .as_mut()
                .map(|l| l.poll())
                .unwrap_or_default();
            // Filter out empty "focus-only" pings (secondary launched without path).
            let paths: Vec<String> = pending.into_iter().filter(|s| !s.is_empty()).collect();
            if paths.is_empty() {
                Task::none()
            } else {
                // Bring-forward is already done inside `poll()`, just open files.
                handle_external_opens(app, paths)
            }
        }
        Message::ExternalOpen(paths) => handle_external_opens(app, paths),
        Message::Frame(action) => app.frame.update(action, Message::Frame),
        Message::Tab(tab_id, tab_msg) => handle_tab_message(app, tab_id, tab_msg),
        Message::Model(ev) => {
            // Legacy flat ModelEvent — forward to TabId-tagged path so
            // rapid tab switches cannot misattribute dirty flag (Q2).
            let tid = app.active_tab().id;
            return handle_tab_message(app, tid, TabMessage::Model(ev));
        }
        Message::FetchModels => translation::handle_fetch_models(app),
        Message::ModelsFetched(providers) => translation::handle_models_fetched(app, providers),
        Message::Ui(UiEvent::TabSelected(raw)) => {
            if let Some(idx) = app.tabs.iter().position(|t| t.id.0 == raw) {
                app.active = idx;
            }
            Task::none()
        }
        Message::Ui(UiEvent::TabClose(raw)) => {
            let id = TabId(raw);
            if let Some(idx) = app.tabs.iter().position(|t| t.id == id) {
                if app.tabs[idx].is_home() {
                    return Task::none();
                }
                if app.tabs[idx].dirty {
                    app.pending_close = Some(id);
                } else {
                    return close_tab_immediate(app, id);
                }
            }
            Task::none()
        }
        Message::Ui(UiEvent::TabCloseConfirmed(raw, save)) => {
            let id = TabId(raw);
            let Some(idx) = app.tabs.iter().position(|t| t.id == id) else {
                app.pending_close = None;
                return Task::none();
            };
            if app.tabs[idx].is_home() {
                app.pending_close = None;
                return Task::none();
            }
            if save {
                app.pending_close = Some(id);
                let path_opt = app.tabs[idx].mmtl_path.clone();
                if let Some(path) = path_opt {
                    let project = app.tabs[idx].project.clone();
                    let tid = id;
                    let inpaint = {
                        let tab = &app.tabs[idx];
                        let mut out = Vec::new();
                        for loaded in &tab.images {
                            let image_id = loaded.image_id;
                            for layer in &loaded.inpaint {
                                let (width, height, pixels) = match &layer.handle {
                                    iced::widget::image::Handle::Rgba { width, height, pixels, .. } => (*width, *height, pixels.to_vec()),
                                    iced::widget::image::Handle::Bytes(_id, bytes) => {
                                        if let Ok(img) = image::load_from_memory(bytes) {
                                            let rgba = img.to_rgba8();
                                            let (w, h) = (rgba.width(), rgba.height());
                                            (w, h, rgba.into_raw())
                                        } else { continue; }
                                    }
                                    _ => continue,
                                };
                                out.push(scanlateit_mmtl::InpaintImageData { image_id, bounds: layer.bounds, width, height, rgba: pixels });
                            }
                        }
                        out
                    };
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || {
                                scanlateit_mmtl::save_mmtl(&project, &inpaint, &path).map(|_| path.to_string_lossy().to_string()).map_err(|e| e.to_string())
                            }).await.unwrap_or_else(|e| Err(format!("save task failed: {e}")))
                        },
                        move |res| Message::Tab(tid, TabMessage::MmtlSaved(res)),
                    );
                } else {
                    // Unsaved dirty tab without path -> open Save As dialog; keep pending_close
                    let tid = id;
                    return Task::perform(
                        async move {
                            let file = rfd::AsyncFileDialog::new()
                                .add_filter("Manga Translation (.mmtl)", &["mmtl"])
                                .set_file_name("project.mmtl")
                                .save_file()
                                .await;
                            file.map(|f| f.path().to_string_lossy().to_string())
                        },
                        move |picked| Message::Tab(tid, TabMessage::MmtlSavePicked(picked)),
                    );
                }
            } else {
                // Don't Save
                return close_tab_immediate(app, id);
            }
        }
        Message::Ui(UiEvent::TabCloseCancel) => {
            app.pending_close = None;
            Task::none()
        }
        Message::Ui(UiEvent::TabCloseOthers(raw)) => {
            let keep = TabId(raw);
            // If any dirty tab besides keep, prompt for first dirty one
            if let Some(dirty) = app.tabs.iter().find(|t| t.is_project() && t.id != keep && t.dirty).map(|t| t.id) {
                app.pending_close = Some(dirty);
                return Task::none();
            }
            // queue cleanup for removed tabs
            let remove_ids: Vec<TabId> = app.tabs.iter().filter(|t| t.id != keep && t.is_project()).map(|t| t.id).collect();
            for rid in &remove_ids {
                app.engines.queue.cancel_pending_for_tab(*rid);
                app.engines.queue.cancel_running_for_tab(*rid);
            }
            let keep_idx = app.tabs.iter().position(|t| t.id == keep);
            if let Some(kidx) = keep_idx {
                let mut i = app.tabs.len();
                while i > 0 {
                    i -= 1;
                    if i == 0 { continue; } // Home
                    if app.tabs[i].id == keep { continue; }
                    app.tabs.remove(i);
                    if app.active > i { app.active -= 1; }
                    else if app.active == i { app.active = kidx.min(app.tabs.len().saturating_sub(1)); }
                }
                if let Some(new_k) = app.tabs.iter().position(|t| t.id == keep) {
                    app.active = new_k;
                }
            }
            let promote = crate::app::queue::dispatch_pending(app);
            crate::app::queue::refresh_queued_statuses(app);
            return promote;
        }
        Message::Ui(UiEvent::TabCloseAll) => {
            if let Some(dirty) = app.tabs.iter().find(|t| t.is_project() && t.dirty).map(|t| t.id) {
                app.pending_close = Some(dirty);
                return Task::none();
            }
            // queue cleanup for all project tabs
            let remove_ids: Vec<TabId> = app.tabs.iter().filter(|t| t.is_project()).map(|t| t.id).collect();
            for rid in &remove_ids {
                app.engines.queue.cancel_pending_for_tab(*rid);
                app.engines.queue.cancel_running_for_tab(*rid);
            }
            app.tabs.retain(|t| t.is_home());
            app.active = 0;
            app.pending_close = None;
            let promote = crate::app::queue::dispatch_pending(app);
            crate::app::queue::refresh_queued_statuses(app);
            return promote;
        }
        Message::Ui(UiEvent::TabNew) => {
            app.new_project = Some(NewProjectState {
                source_files: Vec::new(),
                original_lang: "Korean".to_string(),
                project_location: None,
            });
            app.active_tab_mut().status = "New Project...".to_string();
            Task::none()
        }
        Message::Ui(UiEvent::HomeNewProject) => {
            app.new_project = Some(NewProjectState {
                source_files: Vec::new(),
                original_lang: "Korean".to_string(),
                project_location: None,
            });
            app.active_tab_mut().status = "New Project...".to_string();
            Task::none()
        }
        Message::Ui(UiEvent::HomeOpenProject) => mmtl::handle_open(app),
        Message::Ui(UiEvent::HomeRecentClicked(path)) => {
            let p = std::path::PathBuf::from(path.clone());
            if !p.exists() {
                app.active_tab_mut().status = format!("Missing: {path}");
                return Task::none();
            }
            app.active_tab_mut().status = format!("Loading {}...", p.display());
            let path_clone = p.clone();
            let new_id = TabId(app.next_tab_id);
            app.next_tab_id += 1;
            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || mmtl::load_created_project(path_clone.to_string_lossy().to_string()))
                        .await
                        .unwrap_or_else(|e| Err(format!("load task failed: {e}")))
                },
                move |res| Message::Tab(new_id, TabMessage::RecentPickedToLoad(res)),
            )
        }
        Message::Ui(UiEvent::HomeSettings) => settings::handle_settings_open(app),
        Message::Ui(UiEvent::NewProjectClose) => {
            app.new_project = None;
            Task::none()
        }
        Message::Ui(UiEvent::NewProjectSourceImage) => {
            let tid = app.active_tab().id;
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
                move |res| Message::Tab(tid, TabMessage::NewProjectSourcePicked(res)),
            )
        }
        Message::Ui(UiEvent::NewProjectSourceFolder) => {
            let tid = app.active_tab().id;
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
                move |res| Message::Tab(tid, TabMessage::NewProjectFolderPicked(res)),
            )
        }
        Message::Ui(UiEvent::NewProjectLocationBrowse) => {
            let default_dir = app
                .new_project
                .as_ref()
                .and_then(|np| np.source_files.first().map(|(p, _, _)| std::path::Path::new(p).parent().map(|par| par.to_path_buf()).unwrap_or_default()))
                .unwrap_or_default();
            let tid = app.active_tab().id;
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
                move |picked| Message::Tab(tid, TabMessage::NewProjectLocationPicked(picked)),
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
                app.active_tab_mut().status = "Select source and project location.".to_string();
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
            app.active_tab_mut().status = format!("Creating {}...", unique_dest.display());
            let new_id = TabId(app.next_tab_id);
            app.next_tab_id += 1;
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
                move |res| Message::Tab(new_id, TabMessage::CreateProjectPicked(res)),
            )
        }
        Message::Ui(UiEvent::StartOcr) => ocr::handle_start_ocr(app),
        Message::Ui(UiEvent::StopOcr) => ocr::handle_stop_ocr(app),
        Message::FontLoaded => {
            app.font = Some(Font::with_name(scanlateit_ui::KOREAN_FONT_NAME));
            app.active_tab_mut().status = format!(
                "{} font ready. {}",
                scanlateit_ui::KOREAN_FONT_NAME,
                if app.active_tab_mut().images.is_empty() {
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
            app.active_tab_mut().status = format!("Font \"{name}\" loaded.");
            Task::none()
        }
        Message::CjkFallbackLoaded(count) => {
            if count > 0 {
                app.active_tab_mut().status = format!("Loaded {count} CJK fallback font(s).");
            }
            Task::none()
        }
        Message::Ui(UiEvent::ProfileSelect(id)) => {
            if app.active_tab_mut().images.is_empty() {
                return Task::none();
            }
            if let Some(ev) = app.active_tab_mut().project.select_profile_with_event(id) {
                handle_model_event(app.active_tab_mut(), ev);
            } else {
                return Task::none();
            }
            let name = app.active_tab_mut().project.profiles.selected().name.clone();
            app.active_tab_mut().status = format!("Profile: {name}");
            Task::none()
        }
        Message::Ui(UiEvent::ProfileCreate) => {
            if app.active_tab_mut().images.is_empty() {
                return Task::none();
            }
            let name = app.active_tab_mut().project.profiles.next_available_name();
            let (id, ev) = app.active_tab_mut().project.create_profile_with_event(name);
            handle_model_event(app.active_tab_mut(), ev);
            if let Some(sel_ev) = app.active_tab_mut().project.select_profile_with_event(id) {
                handle_model_event(app.active_tab_mut(), sel_ev);
            }
            let name = app.active_tab_mut().project.profiles.selected().name.clone();
            app.active_tab_mut().status = format!("Profile: {name} (created)");
            Task::none()
        }
        Message::Ui(UiEvent::TranslationPanelMode(mode)) => {
            // Initialize base/target when first entering Translate
            if mode == TranslationPanelMode::Translate && app.active_tab_mut().translation_panel_mode != TranslationPanelMode::Translate {
                if app.active_tab_mut().translate_base.is_none() && !app.active_tab_mut().images.is_empty() {
                    app.active_tab_mut().translate_base = Some(app.active_tab_mut().project.profiles.selected_id());
                }
                // Ensure target placeholder reflects current lang if it is AutoPlaceholder
                if let TargetProfileSelection::AutoPlaceholder(_) = app.active_tab_mut().translate_target.clone() {
                    app.active_tab_mut().translate_target = TargetProfileSelection::AutoPlaceholder(format!("{}(auto)", app.active_tab_mut().translate_lang));
                }
                // If placeholder already exists as a profile, convert to Existing so equality checks work
                if let TargetProfileSelection::AutoPlaceholder(name) = app.active_tab_mut().translate_target.clone() {
                    if let Some(id) = app.active_tab_mut().project.profiles.find_by_name(&name) {
                        // Don't select base itself; keep placeholder blank if it would equal base
                        let base = app.active_tab_mut().translate_base.or_else(|| Some(app.active_tab_mut().project.profiles.selected_id()));
                        if Some(id) != base {
                            app.active_tab_mut().translate_target = TargetProfileSelection::Existing(id);
                        }
                    }
                }
                // Prevent base == target (target placeholder may equal base name)
                if let (Some(base), TargetProfileSelection::Existing(tid)) = (app.active_tab_mut().translate_base, app.active_tab_mut().translate_target.clone()) {
                    if base == tid {
                        // keep target as placeholder instead
                        app.active_tab_mut().translate_target = TargetProfileSelection::AutoPlaceholder(format!("{}(auto)", app.active_tab_mut().translate_lang));
                    }
                }
            }
            app.active_tab_mut().translation_panel_mode = mode;
            app.active_tab_mut().status = match mode {
                TranslationPanelMode::Edit => "Edit mode: single profile.".to_string(),
                TranslationPanelMode::Translate => "Translate mode: base → target.".to_string(),
            };
            // Clear panel editing when switching modes
            if app.active_tab_mut().editing.is_some() && app.active_tab_mut().editing_origin == EditOrigin::Panel {
                crate::app::edit::clear_editing(app);
            }
            Task::none()
        }
        Message::Ui(UiEvent::BaseProfileSelect(id)) => {
            if app.active_tab_mut().images.is_empty() {
                return Task::none();
            }
            // validate id exists
            let exists = app.active_tab_mut().project.profiles.iter().any(|p| p.id == id);
            if !exists {
                return Task::none();
            }
            // prevent base == target (when target is Existing)
            if let TargetProfileSelection::Existing(tid) = app.active_tab_mut().translate_target.clone() {
                if tid == id {
                    app.active_tab_mut().status = "Base and target must differ.".to_string();
                    return Task::none();
                }
            }
            // also prevent base name == placeholder name when target is AutoPlaceholder
            if let TargetProfileSelection::AutoPlaceholder(name) = app.active_tab().translate_target.clone() {
                let bprof_name = app.active_tab().project.profiles.iter().find(|p| p.id == id).map(|p| p.name.clone());
                if let Some(bname) = bprof_name {
                    if bname == name {
                        app.active_tab_mut().status = "Base and target must differ.".to_string();
                        return Task::none();
                    }
                }
            }
            app.active_tab_mut().translate_base = Some(id);
            let name = app.active_tab_mut().project.profiles.iter().find(|p| p.id == id).map(|p| p.name.clone()).unwrap_or_default();
            app.active_tab_mut().status = format!("Base: {name}");
            Task::none()
        }
        Message::Ui(UiEvent::TargetProfileSelect(sel)) => {
            if app.active_tab().images.is_empty() {
                return Task::none();
            }
            // validate and prevent == base
            let base = {
                let tab = app.active_tab();
                tab.translate_base.or_else(|| Some(tab.project.profiles.selected_id()))
            };
            match &sel {
                TargetProfileSelection::Existing(id) => {
                    if Some(*id) == base {
                        app.active_tab_mut().status = "Base and target must differ.".to_string();
                        return Task::none();
                    }
                    let exists = app.active_tab().project.profiles.iter().any(|p| p.id == *id);
                    if !exists {
                        return Task::none();
                    }
                }
                TargetProfileSelection::AutoPlaceholder(name) => {
                    if let Some(b) = base {
                        let bprof_name = app.active_tab().project.profiles.iter().find(|p| p.id == b).map(|p| p.name.clone());
                        if let Some(bname) = bprof_name {
                            if &bname == name {
                                app.active_tab_mut().status = "Base and target must differ.".to_string();
                                return Task::none();
                            }
                        }
                    }
                }
            }
            // If AutoPlaceholder actually already exists, convert to Existing
            let resolved = match sel.clone() {
                TargetProfileSelection::AutoPlaceholder(name) => {
                    let found = app.active_tab().project.profiles.find_by_name(&name);
                    if let Some(id) = found {
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
            app.active_tab_mut().translate_target = resolved.clone();
            let label = match resolved {
                TargetProfileSelection::Existing(id) => app.active_tab().project.profiles.iter().find(|p| p.id == id).map(|p| p.name.clone()).unwrap_or_default(),
                TargetProfileSelection::AutoPlaceholder(n) => n,
            };
            app.active_tab_mut().status = format!("Target: {label}");
            Task::none()
        }
        Message::Ui(UiEvent::TilesVisible(range)) => {
            let tid = app.active_tab().id;
            return handle_tab_message(app, tid, TabMessage::TilesVisible(range));
        },
        Message::Ui(UiEvent::TileScrollEnded) => {
            let tid = app.active_tab().id;
            return handle_tab_message(app, tid, TabMessage::TileScrollEnded);
        }

        Message::Ui(UiEvent::Translate) => translation::handle_translate(app),
        Message::Ui(UiEvent::TranslateModelSelect { provider, model }) => translation::handle_model_select(app, provider, model),
        Message::Ui(UiEvent::TranslateLang(lang)) => {
            app.active_tab_mut().translate_lang = lang.clone();
            // Keep placeholder in sync when target is AutoPlaceholder
            if let TargetProfileSelection::AutoPlaceholder(_) = app.active_tab_mut().translate_target.clone() {
                let new_name = format!("{lang}(auto)");
                // If that name already exists as a profile (and !== base), convert to Existing
                if !app.active_tab_mut().images.is_empty() {
                    if let Some(id) = app.active_tab_mut().project.profiles.find_by_name(&new_name) {
                        let base = app.active_tab_mut().translate_base.or_else(|| Some(app.active_tab_mut().project.profiles.selected_id()));
                        if Some(id) != base {
                            app.active_tab_mut().translate_target = TargetProfileSelection::Existing(id);
                        } else {
                            app.active_tab_mut().translate_target = TargetProfileSelection::AutoPlaceholder(new_name);
                        }
                    } else {
                        app.active_tab_mut().translate_target = TargetProfileSelection::AutoPlaceholder(new_name);
                    }
                } else {
                    app.active_tab_mut().translate_target = TargetProfileSelection::AutoPlaceholder(new_name);
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
            if app.active_state().is_bulk_busy() {
                app.active_tab_mut().status = "Wait for current task to finish.".to_string();
                return Task::none();
            }
            if app.active_tab_mut().images.is_empty() {
                app.active_tab_mut().status = "No images to reorder.".to_string();
                return Task::none();
            }
            // Per-image emission per decision 4: one EntriesReordered per image
            let ids: Vec<_> = app.active_tab().project.images().iter().map(|m| m.id).collect();
            if ids.is_empty() {
                let ev = app.active_tab_mut().project.reorder_entries_for_image_with_event(scanlateit_model::ImageId(0));
                handle_model_event(app.active_tab_mut(), ev);
            } else {
                for image_id in ids {
                    let ev = app.active_tab_mut().project.reorder_entries_for_image_with_event(image_id);
                    handle_model_event(app.active_tab_mut(), ev);
                }
            }
            // Per-image file order is the `images` vec order; within each file
            // entries are now Y→X (top first, left→right) by view-quad bounds.
            // Translation iterates `visible_for()` per image, so it immediately
            // benefits — no translation cache to invalidate.
            app.active_tab_mut().status = format!(
                "Reordered {} image(s) by position (higher first, left to right).",
                app.active_tab_mut().images.len()
            );
            Task::none()
        }
        Message::Ui(UiEvent::ManualModeEnter(mode)) => manual::handle_enter(app, mode),
        Message::Ui(UiEvent::ManualModeCancel) => manual::handle_cancel(app),
        Message::Ui(UiEvent::ManualModeReset) => manual::handle_reset(app),
        Message::Ui(UiEvent::ManualModeStart) => manual::handle_start(app),
        Message::Ui(UiEvent::ManualSelectionAdded(pair)) => manual::handle_selection(app, vec![pair]),
        Message::Ui(UiEvent::ManualSelectionSpan(spans)) => manual::handle_selection(app, spans),
        Message::Ui(UiEvent::ToggleOverlayText) => {
            app.active_tab_mut().show_overlay_text = !app.active_tab_mut().show_overlay_text;
            app.active_tab_mut().status = if app.active_tab_mut().show_overlay_text {
                "Overlay text shown."
            } else {
                "Overlay text hidden."
            }
            .to_string();
            Task::none()
        }
        Message::Ui(UiEvent::ToggleInpaintLayer) => {
            app.active_tab_mut().show_inpaint = !app.active_tab_mut().show_inpaint;
            app.active_tab_mut().status = if app.active_tab_mut().show_inpaint {
                "Inpaint layer shown."
            } else {
                "Inpaint layer hidden."
            }
            .to_string();
            Task::none()
        }
        Message::Ui(UiEvent::MainAreaMode(mode)) => {
            if app.active_tab_mut().manual_mode != ManualMode::None {
                app.active_tab_mut().status = "Exit manual mode to switch View/Compare.".to_string();
                return Task::none();
            }
            app.active_tab_mut().view_mode = mode;
            app.active_tab_mut().status = match mode {
                MainAreaMode::View => "View mode: single column with overlay.".to_string(),
                MainAreaMode::Compare => {
                    "Compare mode: original (left) vs current (right), scrolling in sync."
                        .to_string()
                }
            };
            Task::none()
        }
        Message::Ui(UiEvent::ViewerScroll(anchor)) => {
            app.active_tab_mut().viewer_scroll = anchor.clamp(0.0, 1.0);
            Task::none()
        }
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
        Message::Ui(UiEvent::PanelResized(resized)) => {
            // Distinct mins: main 160 vs panel 592 (STYLING+RESULTS+GAP), pane_grid has single min_size (=160)
            // so clamp ratio to prevent panel collapsing to ~100px as in screenshot.
            // At default 1400px window panel 592 needs ratio <= 0.58, main 160 needs ratio >= 0.12
            let ratio = resized.ratio.clamp(0.15, 0.58);
            app.active_tab_mut().panes.resize(resized.split, ratio);
            Task::none()
        }
        Message::Ui(UiEvent::SidePanelResized(resized)) => {
            // Distinct mins: styling 260 vs results 320, pane_grid single min (=260)
            // clamp to keep both above their mins when panel at its min 592 (ratio 0.439-0.459)
            // but allow wider range at larger panel widths; narrow fixed clamp prevents extreme collapse
            let ratio = resized.ratio.clamp(0.38, 0.55);
            app.active_tab_mut().side_panes.resize(resized.split, ratio);
            Task::none()
        }
        Message::Ui(UiEvent::StylingPaneResized(resized)) => {
            app.active_tab_mut().styling_panes.resize(resized.split, resized.ratio);
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
        Message::Ui(UiEvent::ExportAll) => export::handle_export_all(app),
    };
    task
}

pub fn subscription(app: &App) -> Subscription<Message> {
    let frame_sub = app.frame.subscription().map(Message::Frame);
    // Snapshot tab ids for key handling (subscription is rebuilt on every app change)
    let tab_ids: Vec<u64> = app.tabs.iter().map(|t| t.id.0).collect();
    let active_id = app.tabs.get(app.active).map(|t| t.id.0).unwrap_or(0);
    let active_len = app.tabs.len();
    #[derive(Clone, Hash)]
    struct KeysState {
        ids: Vec<u64>,
        active: u64,
        len: usize,
    }
    let keys_state = KeysState {
        ids: tab_ids,
        active: active_id,
        len: active_len,
    };
    let keys = iced::event::listen().with(keys_state).filter_map(|(state, event)| {
        if let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) = event {
            if modifiers.control() && !modifiers.shift() {
                match key.as_ref() {
                    iced::keyboard::Key::Character(c) if c == "s" || c == "S" => {
                        return Some(Message::Ui(UiEvent::SaveProject));
                    }
                    iced::keyboard::Key::Character(c) if c == "o" || c == "O" => {
                        return Some(Message::Ui(UiEvent::HomeOpenProject));
                    }
                    iced::keyboard::Key::Character(c) if c == "t" || c == "T" => {
                        return Some(Message::Ui(UiEvent::TabNew));
                    }
                    iced::keyboard::Key::Character(c) if c == "w" || c == "W" => {
                        return Some(Message::Ui(UiEvent::TabClose(state.active)));
                    }
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab) => {
                        // Ctrl+Tab -> next tab
                        if state.len > 1 {
                            let next_idx = (state.ids.iter().position(|&id| id == state.active).unwrap_or(0) + 1) % state.len;
                            return Some(Message::Ui(UiEvent::TabSelected(state.ids[next_idx])));
                        }
                        return None;
                    }
                    iced::keyboard::Key::Character(c) => {
                        if let Ok(n) = c.parse::<usize>() {
                            if (1..=9).contains(&n) && n <= state.len {
                                return Some(Message::Ui(UiEvent::TabSelected(state.ids[n - 1])));
                            }
                        }
                        return None;
                    }
                    _ => return None,
                }
            } else if modifiers.control() && modifiers.shift() {
                match key.as_ref() {
                    iced::keyboard::Key::Character(c) if c == "w" || c == "W" => {
                        return Some(Message::Ui(UiEvent::TabCloseAll));
                    }
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab) => {
                        // Ctrl+Shift+Tab -> prev tab
                        if state.len > 1 {
                            let pos = state.ids.iter().position(|&id| id == state.active).unwrap_or(0);
                            let prev_idx = if pos == 0 { state.len - 1 } else { pos - 1 };
                            return Some(Message::Ui(UiEvent::TabSelected(state.ids[prev_idx])));
                        }
                        return None;
                    }
                    _ => return None,
                }
            }
            None
        } else {
            None
        }
    });
    let mut subs = vec![frame_sub, keys];

    // Drag-drop: dropping a .mmtl onto the window opens it (Explorer → window).
    let drops = iced::event::listen().filter_map(|event| match event {
        iced::Event::Window(iced::window::Event::FileDropped(path)) => {
            let s = path.to_string_lossy().to_string();
            if s.to_ascii_lowercase().ends_with(".mmtl") {
                Some(Message::ExternalOpen(vec![s]))
            } else {
                None
            }
        }
        _ => None,
    });
    subs.push(drops);

    // Single-instance IPC poll: secondary instances forward their CLI .mmtl via
    // localhost TCP; primary drains it every 250 ms and opens the projects.
    // Only subscribe when we hold the listener (primary); secondary never
    // reaches App.
    if app.ipc_listener.is_some() {
        subs.push(iced::time::every(Duration::from_millis(250)).map(|_| Message::IpcPoll));
    }

    // Per-tab tick subscriptions (Phase 2: each tab ticks independently)
    for tab in &app.tabs {
        let tid = tab.id;
        #[cfg(feature = "ocr")]
        if tab.running {
            subs.push(iced::time::every(Duration::from_millis(16)).with(tid).map(|(tid, _)| Message::Tab(tid, TabMessage::OcrTick)));
        }
        if tab.translating {
            subs.push(iced::time::every(Duration::from_millis(16)).with(tid).map(|(tid, _)| Message::Tab(tid, TabMessage::TranslateTick)));
        }
    }
    Subscription::batch(subs)
}

pub fn view(app: &App) -> Element<'_, Message> {
    view::view(app)
}

#[cfg(test)]
mod tests;
