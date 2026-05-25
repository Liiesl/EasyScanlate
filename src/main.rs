use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use iced::widget::{button, column, container, row, scrollable, text, text_input, Column};
use iced::widget::canvas::{self, Canvas, Fill, Geometry, Image};
use iced::widget::image::Handle;
use iced::{
    Color, Element, Fill as FillLength, Font, Length, Pixels, Point, Rectangle, Renderer, Size,
    Task, Theme,
};

use rapidocr_core::config::{
    DetConfig, InferenceOptions, LimitType, PipelineConfig, RapidOcrConfig, RecConfig,
};
use rapidocr_core::types::OcrLine;
use rapidocr_core::{is_cancelled_error, OcrCancellationToken, RapidOcr};

const MODEL_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/models");
const KOREAN_FONT_PATH: &str = "C:\\Windows\\Fonts\\malgun.ttf";
const KOREAN_FONT_NAME: &str = "Malgun Gothic";
const OVERLAY_TEXT_SIZE: f32 = 14.0;

#[derive(Clone)]
struct Engine(Arc<Mutex<RapidOcr>>);

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Engine")
    }
}

#[derive(Debug, Clone)]
enum Message {
    PathChanged(String),
    LoadImage,
    ImageLoaded(Result<(Handle, u32, u32), String>),
    StartOcr,
    StopOcr,
    EngineReady(Result<Engine, String>),
    OcrFinished(Result<Vec<OcrLine>, String>),
    FontLoaded,
}

struct LoadedImage {
    handle: Handle,
    width: f32,
    height: f32,
}

struct App {
    path: String,
    image: Option<LoadedImage>,
    engine: Option<Engine>,
    cancel: Option<OcrCancellationToken>,
    running: bool,
    results: Vec<OcrLine>,
    cache: canvas::Cache,
    font: Option<Font>,
    status: String,
}

impl App {
    fn new() -> Self {
        Self {
            path: String::new(),
            image: None,
            engine: None,
            cancel: None,
            running: false,
            results: Vec::new(),
            cache: canvas::Cache::new(),
            font: None,
            status: "Idle - load an image to begin.".to_string(),
        }
    }
}

struct Overlay<'a> {
    handle: &'a Handle,
    boxes: &'a [OcrLine],
    font: Font,
    cache: &'a canvas::Cache,
}

fn bbox_rect(points: &[[f32; 2]; 4]) -> [f32; 4] {
    let min_x = points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
    let min_y = points.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
    let max_x = points.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max);
    let max_y = points.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max);
    [min_x, min_y, max_x, max_y]
}

impl<Message> canvas::Program<Message> for Overlay<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let geometry = self.cache.draw(renderer, bounds.size(), |frame| {
            frame.draw_image(bounds, Image::new(self.handle.clone()));
            for line in self.boxes {
                let [min_x, min_y, max_x, max_y] = bbox_rect(&line.bbox.points);
                let width = (max_x - min_x).max(0.0);
                let height = (max_y - min_y).max(0.0);
                frame.fill_rectangle(
                    Point::new(min_x, min_y),
                    Size::new(width, height),
                    Fill::from(Color::from_rgba(0.08, 0.08, 0.12, 0.55)),
                );
                frame.fill_text(canvas::Text {
                    content: line.text.clone(),
                    position: Point::new(min_x, min_y),
                    max_width: width.max(8.0),
                    size: Pixels(OVERLAY_TEXT_SIZE),
                    color: Color::from_rgb8(255, 230, 90),
                    font: self.font,
                    ..canvas::Text::default()
                });
            }
        });
        vec![geometry]
    }
}

fn ocr_config() -> RapidOcrConfig {
    let model_dir = PathBuf::from(MODEL_DIR);
    RapidOcrConfig {
        pipeline: PipelineConfig::without_cls(),
        inference: InferenceOptions {
            intra_threads: 4,
            ..Default::default()
        },
        text_score: 0.5,
        min_side_len: 30,
        max_side_len: 2000,
        min_height: 30,
        width_height_ratio: 8.0,
        det: Some(DetConfig {
            model_path: model_dir.join("PP-OCRv6_det_tiny.onnx"),
            limit_side_len: 736,
            limit_type: LimitType::Min,
            mean: [0.5, 0.5, 0.5],
            std: [0.5, 0.5, 0.5],
            thresh: 0.3,
            box_thresh: 0.5,
            max_candidates: 1000,
            unclip_ratio: 1.6,
            min_size: 3,
            input_limits: Default::default(),
        }),
        cls: None,
        rec: Some(RecConfig {
            model_path: model_dir.join("korean_PP-OCRv5_rec_mobile.onnx"),
            dict_path: model_dir.join("korean_dict.txt"),
            image_shape: [3, 48, 320],
            batch_size: 6,
        }),
    }
}

fn build_engine() -> Result<Engine, String> {
    RapidOcr::new(ocr_config())
        .map(|ocr| Engine(Arc::new(Mutex::new(ocr))))
        .map_err(|e| format!("Engine init failed: {e}"))
}

fn start_ocr_run(app: &mut App, engine: Engine) -> Task<Message> {
    let path = app.path.clone();
    let token = OcrCancellationToken::new();
    app.cancel = Some(token.clone());
    app.running = true;
    app.status = "Running OCR...".to_string();
    Task::perform(
        async move {
            let mut engine = engine
                .0
                .lock()
                .map_err(|e| format!("Engine lock poisoned: {e}"))?;
            engine
                .run_path_cancellable(&path, &token)
                .map(|output| output.lines)
                .map_err(|e| {
                    if is_cancelled_error(&e) {
                        "cancelled".to_string()
                    } else {
                        format!("OCR failed: {e}")
                    }
                })
        },
        Message::OcrFinished,
    )
}

fn boot() -> (App, Task<Message>) {
    let font_task = match std::fs::read(KOREAN_FONT_PATH) {
        Ok(bytes) => iced::font::load(bytes).map(|_| Message::FontLoaded),
        Err(_) => Task::none(),
    };
    (App::new(), font_task)
}

fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::PathChanged(path) => {
            app.path = path;
            Task::none()
        }
        Message::LoadImage => {
            let path = app.path.clone();
            if path.trim().is_empty() {
                app.status = "Enter an image path first.".to_string();
                return Task::none();
            }
            app.status = "Loading image...".to_string();
            Task::perform(
                async move {
                    let img = image::ImageReader::open(&path)
                        .map_err(|e| format!("Failed to open image: {e}"))?
                        .decode()
                        .map_err(|e| format!("Failed to decode image: {e}"))?;
                    Ok((Handle::from_path(path), img.width(), img.height()))
                },
                Message::ImageLoaded,
            )
        }
        Message::ImageLoaded(result) => match result {
            Ok((handle, width, height)) => {
                app.image = Some(LoadedImage {
                    handle,
                    width: width as f32,
                    height: height as f32,
                });
                app.results.clear();
                app.cache.clear();
                app.status = format!("Loaded image ({width} x {height}).");
                Task::none()
            }
            Err(e) => {
                app.status = e;
                Task::none()
            }
        },
        Message::StartOcr => {
            if app.image.is_none() {
                app.status = "Load an image first.".to_string();
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
                    Task::perform(async move { build_engine() }, Message::EngineReady)
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
        Message::OcrFinished(result) => {
            app.running = false;
            app.cancel = None;
            match result {
                Ok(lines) => {
                    app.results = lines;
                    app.status = format!("OCR done: {} line(s).", app.results.len());
                    app.cache.clear();
                }
                Err(e) => {
                    app.status = format!("Cancelled: {e}").replace("Cancelled: cancelled", "OCR cancelled.");
                }
            }
            Task::none()
        }
        Message::FontLoaded => {
            app.font = Some(Font::with_name(KOREAN_FONT_NAME));
            app.status = format!(
                "{} font ready. {}",
                KOREAN_FONT_NAME,
                if app.image.is_none() {
                    "Load an image to begin."
                } else {
                    ""
                }
            );
            Task::none()
        }
    }
}

fn right_column(app: &App) -> Element<'_, Message> {
    let results_list: Vec<Element<'_, Message>> = if app.results.is_empty() {
        vec![text("No results yet. Run OCR to detect text.")
            .size(12)
            .color(Color::from_rgb(0.6, 0.6, 0.6))
            .into()]
    } else {
        app.results
            .iter()
            .map(|line| {
                let [min_x, min_y, _, _] = bbox_rect(&line.bbox.points);
                text(format!(
                    "{:.2}  {}  ({:.0}, {:.0})",
                    line.score, line.text, min_x, min_y
                ))
                .size(12)
                .into()
            })
            .collect()
    };

    container(
        column![
            text("Scanlateit").size(24),
            text_input("Path to image", &app.path)
                .on_input(Message::PathChanged)
                .on_submit(Message::LoadImage),
            row![
                button("Load").on_press(Message::LoadImage),
                button("Start OCR").on_press_maybe(
                    (app.image.is_some() && !app.running).then_some(Message::StartOcr)
                ),
                button("Stop").on_press_maybe(app.running.then_some(Message::StopOcr)),
            ]
            .spacing(6),
            text(&app.status).size(12),
            text(format!("{} result(s)", app.results.len())).size(13),
            scrollable(Column::with_children(results_list).spacing(2))
                .height(FillLength)
                .width(FillLength),
        ]
        .spacing(8),
    )
    .width(300)
    .height(FillLength)
    .padding(10)
    .style(|_theme| container::Style {
        background: Some(Color::from_rgb8(34, 36, 44).into()),
        border: iced::Border::default().rounded(4),
        ..container::Style::default()
    })
    .into()
}

fn view(app: &App) -> Element<'_, Message> {
    let left: Element<'_, Message> = match &app.image {
        Some(image) => {
            let overlay = Overlay {
                handle: &image.handle,
                boxes: &app.results,
                font: app.font.unwrap_or(Font::DEFAULT),
                cache: &app.cache,
            };
            scrollable(
                Canvas::new(overlay)
                    .width(Length::Fixed(image.width))
                    .height(Length::Fixed(image.height)),
            )
            .width(FillLength)
            .height(FillLength)
            .into()
        }
        None => container(text("No image loaded. Enter a path and press Load."))
            .width(FillLength)
            .height(FillLength)
            .into(),
    };

    row![left, right_column(app)]
        .spacing(2)
        .width(FillLength)
        .height(FillLength)
        .into()
}

fn main() -> iced::Result {
    iced::application(boot, update, view)
        .title("Scanlateit")
        .window_size(Size::new(1400.0, 900.0))
        .theme(Theme::Dark)
        .run()
}