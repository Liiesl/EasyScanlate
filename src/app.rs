use iced::widget::canvas::{self, Canvas};
use iced::widget::image::Handle;
use iced::widget::{column, container, responsive, row, scrollable, text};
use iced::{Element, Font, Length, Task};

use rapidocr_core::OcrCancellationToken;

use crate::model::{NewEntry, ProfileId, Project};
use crate::ocr::{self, Engine};
use crate::ui::overlay::{Overlay, OverlayEntry};
use crate::ui::{side_panel, KOREAN_FONT_NAME, KOREAN_FONT_PATH};

const IMAGE_FILTERS: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "webp", "tiff", "avif"];

#[derive(Debug, Clone)]
pub enum Message {
    OpenImages,
    ImagesPicked(Result<Vec<(String, u32, u32)>, String>),
    StartOcr,
    StopOcr,
    EngineReady(Result<Engine, String>),
    OcrFinished(usize, Result<Vec<NewEntry>, String>),
    FontLoaded,
    CycleProfile,
}

pub(crate) struct LoadedImage {
    handle: Handle,
    width: f32,
    height: f32,
    pub(crate) path: String,
    pub(crate) project: Project,
    cache: canvas::Cache,
}

/// Session state: one loaded image plus everything iced/OCR related that the
/// model doesn't know about (engine handle, per-image canvas cache).
pub struct App {
    pub(crate) images: Vec<LoadedImage>,
    engine: Option<Engine>,
    cancel: Option<OcrCancellationToken>,
    pub(crate) running: bool,
    font: Option<Font>,
    pub(crate) status: String,
    pending: usize,
    ocr_total: usize,
    ocr_failed: usize,
    ocr_cancelled: bool,
    ocr_index: usize,
}

impl App {
    fn new() -> Self {
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
        }
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
/// starts. The shared token is created once per run in [`Message::StartOcr`].
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
        Message::OpenImages => Task::perform(
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
                        handle: Handle::from_path(&path),
                        width: width as f32,
                        height: height as f32,
                        path,
                        project: Project::new(),
                        cache: canvas::Cache::new(),
                    });
                }
                app.status = format!("Loaded {} image(s).", app.images.len());
                Task::none()
            }
            Err(e) => {
                app.status = e;
                Task::none()
            }
        },
        Message::StartOcr => {
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
        Message::StopOcr => {
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
                    app.images[index].cache.clear();
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
        Message::CycleProfile => {
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
                    img.cache.clear();
                }
                let name = app.images[0].project.profiles.selected().name.clone();
                app.status = format!("Profile: {name}");
            }
            Task::none()
        }
    }
}

pub fn view(app: &App) -> Element<'_, Message> {
    let left: Element<'_, Message> = if app.images.is_empty() {
        container(text("No images loaded. Click \"Open Images\" to pick some."))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        let mut pages = column![].width(Length::Fill).spacing(0);
        for image in &app.images {
            let entries: Vec<OverlayEntry<'_>> = image
                .project
                .ocr
                .visible()
                .map(|entry| OverlayEntry {
                    text: image.project.display_text(entry),
                    bounds: entry.quad.bounds(),
                    style: image.project.entry_style(entry.id),
                })
                .collect();
            let aspect = image.height / image.width;
            let overlay = Overlay::new(
                &image.handle,
                entries,
                app.font.unwrap_or(Font::DEFAULT),
                &image.cache,
                image.width,
            );
            pages = pages.push(
                responsive(move |size| {
                    let page: Element<'_, Message> = Canvas::new(overlay.clone())
                        .width(Length::Fill)
                        .height(Length::Fixed(size.width * aspect))
                        .into();
                    page
                })
                .height(Length::Shrink),
            );
        }
        scrollable(pages)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    };

    row![left, side_panel::view(app)]
        .spacing(2)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
