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
    OcrAllFinished(Vec<(usize, Result<Vec<NewEntry>, String>)>),
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

fn start_ocr_run(app: &mut App, engine: Engine) -> Task<Message> {
    let paths: Vec<String> = app.images.iter().map(|img| img.path.clone()).collect();
    let token = OcrCancellationToken::new();
    app.cancel = Some(token.clone());
    app.running = true;
    app.status = format!("Running OCR on {} image(s)...", paths.len());
    Task::perform(
        async move {
            let mut results = Vec::with_capacity(paths.len());
            for (index, path) in paths.iter().enumerate() {
                let entry = engine
                    .run_path_cancellable(path, &token)
                    .map(ocr::to_entries);
                results.push((index, entry));
            }
            results
        },
        Message::OcrAllFinished,
    )
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
            match app.engine.clone() {
                Some(engine) => start_ocr_run(app, engine),
                None => {
                    app.running = true;
                    app.status = "Initializing OCR engine...".to_string();
                    Task::perform(async move { Engine::build() }, Message::EngineReady)
                }
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
        Message::OcrAllFinished(results) => {
            app.running = false;
            app.cancel = None;
            let mut total = 0;
            let mut failed = 0;
            let mut cancelled = false;
            for (index, result) in results.into_iter() {
                match result {
                    Ok(entries) => {
                        let count = app.images[index].project.append_ocr(entries);
                        app.images[index].cache.clear();
                        total += count;
                    }
                    Err(e) => {
                        failed += 1;
                        if e == "cancelled" {
                            cancelled = true;
                        }
                    }
                }
            }
            app.status = if cancelled {
                "OCR cancelled.".to_string()
            } else if failed > 0 {
                format!("OCR done: {total} line(s), {failed} image(s) failed.")
            } else {
                format!("OCR done: {total} line(s).")
            };
            Task::none()
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
