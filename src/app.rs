use std::collections::{HashMap, HashSet};
#[cfg(all(feature = "test-ui", not(feature = "translation")))]
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

#[cfg(any(feature = "inpaint", feature = "test-ui"))]
use iced::widget::image::Handle;
use iced::widget::{pane_grid, text_editor};
#[cfg(feature = "ocr")]
use iced::futures::{SinkExt, StreamExt};
use iced::{Color, Element, Font, Length, Rectangle, Subscription, Task, Theme};
use neverliie_iced_widgets::title_bar::{FrameAction, NativeFrame};

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
#[cfg(feature = "inpaint")]
use scanlateit_settings::AutoInpaintModel;
#[cfg(feature = "ocr")]
use scanlateit_ocr::{self as ocr_engine, OcrCancellationToken, ParallelEngine};
#[cfg(feature = "styling")]
use scanlateit_styling::{Engine as StylingEngine, JobTracker};
#[cfg(feature = "segment")]
use scanlateit_segment::Engine as SegmentEngine;
use scanlateit_ui::translation as ui_translation;
use scanlateit_ui::main_area::decode::{DecodedPage, PageDecode, Scheduler, Tier};
use scanlateit_ui::{
    event::{EditOrigin, MainAreaMode, SettingsTab, StyleField, ToolbarAction, UiEvent},
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
pub mod view;

use layout::{PaneKind, SidePaneKind, StylingPaneKind};
use layout::{IMAGE_FILTERS, MAIN_AREA_DEFAULT_RATIO, STYLING_DEFAULT_RATIO, STYLING_TOP_RATIO};

#[derive(Debug, Clone)]
pub(crate) struct AutoInpaintJob {
    pub index: usize,
    pub id: EntryId,
    pub path: String,
    pub quad: Quad,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Frame actions from the custom title bar.
    Frame(FrameAction),
    /// A widget-level event from the ui crate.
    Ui(UiEvent),
    ImagesPicked(Result<Vec<(String, u32, u32)>, String>),
    #[cfg(feature = "ocr")]
    ParallelEngineReady(Result<ParallelEngine, String>),
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
    pub(crate) inpainting: bool,
    pub(crate) inpaint_mode: bool,
    pub(crate) show_overlay_text: bool,
    pub(crate) show_inpaint: bool,
    pub(crate) view_mode: MainAreaMode,
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
    pub(crate) connect_modal: Option<ConnectModal>,
    pub(crate) settings_open: bool,
    pub(crate) settings_tab: SettingsTab,
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
    pub frame: NativeFrame,
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
            connect_modal: None,
            settings_open: false,
            settings_tab: SettingsTab::General,
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
            loaded_fonts: HashSet::new(),
            style_picker: None,
            style_stroke_width: style.stroke_width.to_string(),
            style_bg_radius: style.bg_radius.to_string(),
            style_hex_overrides: HashMap::new(),
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

pub fn boot(frame: NativeFrame) -> (App, Task<Message>) {
    boot::boot(frame)
}

pub fn update(app: &mut App, message: Message) -> Task<Message> {
    let task = match message {
        Message::Frame(action) => app.frame.update(action, Message::Frame),
        Message::FetchModels => translation::handle_fetch_models(app),
        Message::ModelsFetched(providers) => translation::handle_models_fetched(app, providers),
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
        Message::Ui(UiEvent::StartOcr) => ocr::handle_start_ocr(app),
        #[cfg(feature = "ocr")]
        Message::ParallelEngineReady(result) => ocr::handle_parallel_ready(app, result),
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
        Message::Ui(UiEvent::Translate) => translation::handle_translate(app),
        Message::Ui(UiEvent::TranslateModelSelect { provider, model }) => translation::handle_model_select(app, provider, model),
        Message::Ui(UiEvent::TranslateLang(lang)) => {
            app.translate_lang = lang;
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
        Message::Ui(UiEvent::Inpaint) => inpaint::handle_inpaint_toggle(app),
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
        Message::Ui(UiEvent::EntryToolbar((index, id, action))) => edit::handle_entry_toolbar(app, index, id, action),
        Message::Ui(UiEvent::EntryMoved((index, id, quad))) => edit::handle_entry_moved(app, index, id, quad),
        Message::Ui(UiEvent::InpaintSelection((index, rect))) => inpaint::handle_inpaint_selection(app, index, rect),
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
        Message::Ui(UiEvent::SettingsOpen) => settings::handle_settings_open(app),
        Message::Ui(UiEvent::SettingsOpenTab(tab)) => settings::handle_settings_open_tab(app, tab),
        Message::Ui(UiEvent::SettingsClose) => settings::handle_settings_close(app),
        Message::Ui(UiEvent::SettingsTab(tab)) => settings::handle_settings_tab(app, tab),
        Message::Ui(UiEvent::SettingsChanged) => settings::handle_settings_changed(app),
        Message::Ui(UiEvent::SettingEdit(edit)) => settings::handle_setting_edit(app, edit),
        Message::Ui(UiEvent::OpenUrl(url)) => settings::handle_open_url(app, url),
        Message::TranslateFinished(jobs, result) => translation::handle_translate_finished(app, jobs, result),
        Message::RetranslateFinished((index, entry_id), result) => translation::handle_retranslate_finished(app, index, entry_id, result),
    };
    task
}

pub fn subscription(app: &App) -> Subscription<Message> {
    let frame_sub = app.frame.subscription().map(Message::Frame);
    #[cfg(feature = "ocr")]
    if app.running {
        return Subscription::batch([
            frame_sub,
            iced::time::every(Duration::from_millis(16)).map(|_| Message::OcrTick),
        ]);
    }
    Subscription::batch([frame_sub])
}

pub fn view(app: &App) -> Element<'_, Message> {
    view::view(app)
}

#[cfg(test)]
mod tests;
