use std::ops::Range;
use std::sync::Arc;

use iced::widget::row;
use iced::{Element, Font, Length, Task};

use rapidocr_core::OcrCancellationToken;

use crate::model::{EntryId, EntryStyle, NewEntry, ProfileId, Project};
use crate::ocr::{self, Engine};
use crate::translation;
use crate::ui::main_area::decode::{decode_page, DecodedPage, PageDecode, MAX_DECODE_EDGE};
use crate::ui::{main_area, panel, KOREAN_FONT_NAME, KOREAN_FONT_PATH};

const DECODE_PRELOAD: usize = 2;

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
    TilesVisible(Range<usize>),
    TileDecoded(usize, Result<Arc<DecodedPage>, String>),
    Translate,
    TranslateModel(String),
    TranslateLang(String),
    TranslateApiKey(String),
    TranslateFinished(Vec<(usize, EntryId, String)>, Result<Vec<String>, String>),
    StyleBold(bool),
    StyleItalic(bool),
    StyleTextHex(String),
    StyleStrokeHex(String),
    StyleStrokeWidth(String),
    StyleBgHex(String),
    StyleBgRadius(String),
}

pub(crate) struct LoadedImage {
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) path: String,
    pub(crate) project: Project,
    pub(crate) decode: PageDecode,
}

/// Session state: one loaded image plus everything iced/OCR related that the
/// model doesn't know about (engine handle, per-image canvas cache).
pub struct App {
    pub(crate) images: Vec<LoadedImage>,
    engine: Option<Engine>,
    cancel: Option<OcrCancellationToken>,
    pub(crate) running: bool,
    pub(crate) font: Option<Font>,
    pub(crate) status: String,
    pending: usize,
    ocr_total: usize,
    ocr_failed: usize,
    ocr_cancelled: bool,
    ocr_index: usize,
    pub(crate) translating: bool,
    pub(crate) translate_model: String,
    pub(crate) translate_lang: String,
    pub(crate) translate_api_key: String,
    /// Global text style applied to every overlay entry.
    pub(crate) style: EntryStyle,
    /// Raw hex text of the styling inputs; kept as typed, only applied to
    /// `style` while it parses.
    pub(crate) style_text_hex: String,
    pub(crate) style_stroke_hex: String,
    pub(crate) style_bg_hex: String,
    pub(crate) style_stroke_width: String,
    pub(crate) style_bg_radius: String,
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
            translating: false,
            translate_model: translation::MODELS[0].to_string(),
            translate_lang: translation::LANGUAGES[0].to_string(),
            translate_api_key: String::new(),
            style: EntryStyle::default(),
            style_text_hex: hex_to_string(EntryStyle::default().text_color),
            style_stroke_hex: hex_to_string(EntryStyle::default().stroke_color),
            style_bg_hex: hex_to_string(EntryStyle::default().bg_color),
            style_stroke_width: EntryStyle::default().stroke_width.to_string(),
            style_bg_radius: EntryStyle::default().bg_radius.to_string(),
        }
    }
}

/// Formats an RGBA color as `#RRGGBBAA`.
fn hex_to_string(rgba: [u8; 4]) -> String {
    format!("#{:02X}{:02X}{:02X}{:02X}", rgba[0], rgba[1], rgba[2], rgba[3])
}

/// Parses `#RGB`, `#RGBA`, `#RRGGBB` or `#RRGGBBAA` into an RGBA color.
/// Shorthand forms expand the alpha to `255`.
pub(crate) fn parse_hex(text: &str) -> Option<[u8; 4]> {
    let digits: Vec<u8> = text
        .strip_prefix('#')
        .and_then(|rest| {
            (rest.len() == 3 || rest.len() == 4 || rest.len() == 6 || rest.len() == 8)
                .then_some(rest)
        })?
        .bytes()
        .map(|b| match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        })
        .collect::<Option<Vec<u8>>>()?;
    let (short, chars) = match digits.len() {
        3 | 4 => (true, digits.len()),
        6 | 8 => (false, digits.len() / 2),
        _ => return None,
    };
    let mut out = [0u8; 4];
    for i in 0..4 {
        let value = if i < chars {
            if short {
                digits[i] * 17
            } else {
                digits[i * 2] * 16 + digits[i * 2 + 1]
            }
        } else {
            255
        };
        out[i] = value;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        for color in [
            [0, 0, 0, 255],
            [255, 230, 90, 255],
            [20, 20, 31, 140],
        ] {
            assert_eq!(parse_hex(&hex_to_string(color)), Some(color));
        }
    }

    #[test]
    fn hex_parses_shorthand_with_alpha_default() {
        assert_eq!(parse_hex("#FFF"), Some([255, 255, 255, 255]));
        assert_eq!(parse_hex("#fff0"), Some([255, 255, 255, 0]));
        assert_eq!(parse_hex("#FFE65A"), Some([255, 230, 90, 255]));
    }

    #[test]
    fn hex_rejects_malformed_input() {
        for bad in ["", "#", "#12", "#GGG", "#12345", "FFE65A", "red", "#123456789"] {
            assert_eq!(parse_hex(bad), None, "expected {bad:?} to be rejected");
        }
    }

    #[test]
    fn default_style_round_trips_all_fields() {
        let style = EntryStyle::default();
        assert_eq!(style.bold, false);
        assert_eq!(style.italic, false);
        assert_eq!(style.stroke_color, [0, 0, 0, 255]);
        assert_eq!(style.stroke_width, 0.0);
        assert_eq!(style.bg_radius, 0.0);
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
                        width: width as f32,
                        height: height as f32,
                        path,
                        project: Project::new(),
                        decode: PageDecode::Pending,
                    });
                }
                app.status = format!("Decoding {} image(s)...", app.images.len());
                let tasks: Vec<Task<Message>> = app
                    .images
                    .iter()
                    .enumerate()
                    .map(|(index, image)| {
                        let path = image.path.clone();
                        Task::perform(
                            async move { decode_page(&path, MAX_DECODE_EDGE).map(Arc::new) },
                            move |result| Message::TileDecoded(index, result),
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
                }
                let name = app.images[0].project.profiles.selected().name.clone();
                app.status = format!("Profile: {name}");
            }
            Task::none()
        }
        Message::TilesVisible(range) => {
            let start = range.start.saturating_sub(DECODE_PRELOAD);
            let end = range.end.saturating_add(DECODE_PRELOAD).min(app.images.len());
            let mut tasks = Vec::new();
            for index in start..end {
                let image = &mut app.images[index];
                if matches!(&image.decode, PageDecode::Pending) {
                    image.decode = PageDecode::Decoding;
                    let path = image.path.clone();
                    tasks.push(Task::perform(
                        async move { decode_page(&path, MAX_DECODE_EDGE).map(Arc::new) },
                        move |result| Message::TileDecoded(index, result),
                    ));
                }
            }
            if tasks.is_empty() {
                Task::none()
            } else {
                Task::batch(tasks)
            }
        }
        Message::TileDecoded(index, result) => {
            if index < app.images.len() {
                app.images[index].decode = match result {
                    Ok(decoded) => PageDecode::Ready(decoded),
                    Err(_) => PageDecode::Failed,
                };
            }
            Task::none()
        }
        Message::Translate => {
            if app.translating || app.running {
                return Task::none();
            }
            let jobs: Vec<(usize, EntryId, String)> = app
                .images
                .iter()
                .enumerate()
                .flat_map(|(index, image)| {
                    image
                        .project
                        .ocr
                        .visible()
                        .map(move |entry| (index, entry.id, entry.text.clone()))
                })
                .collect();
            if jobs.is_empty() {
                app.status = "Run OCR first.".to_string();
                return Task::none();
            }
            app.translating = true;
            let texts: Vec<String> = jobs.iter().map(|(_, _, text)| text.clone()).collect();
            let target = app.translate_lang.clone();
            let model = app.translate_model.clone();
            let api_key = (!app.translate_api_key.is_empty())
                .then(|| app.translate_api_key.clone());
            app.status = format!(
                "Translating {} line(s) to {} via {model}...",
                jobs.len(),
                app.translate_lang
            );
            Task::perform(
                async move {
                    let result = translation::translate_all(&texts, &target, &model, api_key).await;
                    (jobs, result)
                },
                |(jobs, result)| Message::TranslateFinished(jobs, result),
            )
        }
        Message::TranslateModel(model) => {
            app.translate_model = model;
            Task::none()
        }
        Message::TranslateLang(lang) => {
            app.translate_lang = lang;
            Task::none()
        }
        Message::TranslateApiKey(key) => {
            app.translate_api_key = key;
            Task::none()
        }
        Message::StyleBold(bold) => {
            app.style.bold = bold;
            Task::none()
        }
        Message::StyleItalic(italic) => {
            app.style.italic = italic;
            Task::none()
        }
        Message::StyleTextHex(text) => {
            app.style_text_hex = text;
            if let Some(color) = parse_hex(&app.style_text_hex) {
                app.style.text_color = color;
            }
            Task::none()
        }
        Message::StyleStrokeHex(text) => {
            app.style_stroke_hex = text;
            if let Some(color) = parse_hex(&app.style_stroke_hex) {
                app.style.stroke_color = color;
            }
            Task::none()
        }
        Message::StyleBgHex(text) => {
            app.style_bg_hex = text;
            if let Some(color) = parse_hex(&app.style_bg_hex) {
                app.style.bg_color = color;
            }
            Task::none()
        }
        Message::StyleStrokeWidth(text) => {
            app.style_stroke_width = text;
            if let Ok(width) = app.style_stroke_width.parse::<f32>() {
                app.style.stroke_width = width.max(0.0);
            }
            Task::none()
        }
        Message::StyleBgRadius(text) => {
            app.style_bg_radius = text;
            if let Ok(radius) = app.style_bg_radius.parse::<f32>() {
                app.style.bg_radius = radius.max(0.0);
            }
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
                    for ((image_index, entry_id, _), translation) in
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

pub fn view(app: &App) -> Element<'_, Message> {
    row![main_area::view(app), panel::view(app)]
        .spacing(2)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
