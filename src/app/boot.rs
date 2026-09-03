#[cfg(feature = "test-ui")]
use std::sync::Arc;
use iced::Task;
use neverliie_iced_widgets::title_bar::NativeFrame;
use easyscanlate_model::{EntrySource, NewEntry, Quad};
#[cfg(feature = "test-ui")]
use easyscanlate_ui::main_area::decode::{DecodedPage, PageDecode, Tier};
#[allow(unused_imports)]
use easyscanlate_ui::{KOREAN_FONT_PATH, LoadedImage};
#[cfg(feature = "test-ui")]
use iced::widget::image::Handle;

use super::{App, Message};
use super::translation;

/// CJK families that provide Hangul/Han/Kana coverage on at least one OS.
/// Mirrors `easyscanlate_ui::main_area::overlay::fallback::CJK_FALLBACK_FAMILIES`
/// to avoid a UI→app cycle at boot; keep both lists in sync.
const CJK_FALLBACK_FAMILIES: &[&str] = &[
    "Apple SD Gothic Neo",
    "AppleGothic",
    "Batang",
    "Dotum",
    "Gulim",
    "Hiragino Kaku Gothic Pro",
    "Hiragino Kaku Gothic ProN",
    "Hiragino Mincho ProN",
    "Hiragino Sans",
    "Hiragino Sans GB",
    "Malgun Gothic",
    "Meiryo",
    "MS Gothic",
    "MS Mincho",
    "MS PGothic",
    "Nanum Gothic",
    "Noto Sans CJK",
    "Noto Sans CJK JP",
    "Noto Sans CJK KR",
    "Noto Sans CJK SC",
    "Noto Sans JP",
    "Noto Sans KR",
    "Noto Sans SC",
    "Yu Gothic",
    "Yu Mincho",
];

fn is_cjk_fallback_family(name: &str) -> bool {
    CJK_FALLBACK_FAMILIES
        .iter()
        .any(|f| f.eq_ignore_ascii_case(name))
}

pub fn boot(
    frame: NativeFrame,
    initial_mmtl: Option<std::path::PathBuf>,
    ipc_listener: Option<crate::single_instance::Listener>,
) -> (App, Task<Message>) {
    easyscanlate_settings::init();
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
        let (connections, last_provider, free_only, hidden) = easyscanlate_settings::get(|s| {
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
        translation::sync_tx_from_store(&mut app);
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
        let width = 900u32;
        let height = 1200u32;
        let white = image::RgbaImage::from_pixel(width, height, image::Rgba([245, 245, 245, 255]));
        let pixels = bytes::Bytes::from(white.into_raw());
        let page = Arc::new(DecodedPage {
            handle: Handle::from_rgba(width, height, pixels),
            width,
            height,
        });
        // Live DB: go through granular ModelEvents even for test boot,
        // keeping the single Message::Model hub the reactivity source.
        // P4: keep Home pristine and push a real Project tab for test-ui (was mutating Home).
        {
            let nid = crate::app::tab::TabId(app.next_tab_id);
            app.next_tab_id += 1;
            let mut project = easyscanlate_model::Project::new();
            let (image_id, ev) = project.add_image_with_event("fake-white-page.png", width as f32, height as f32);
            let images = vec![LoadedImage {
                image_id,
                decode: PageDecode {
                    thumb: Tier::Ready(page.clone()),
                    full: Tier::Ready(page),
                },
                inpaint: Vec::new(),
            }];
            let mut tab = crate::app::tab::Tab::project_from_loaded(
                nid,
                "test-ui".to_string(),
                project,
                images,
                std::path::PathBuf::from("test-ui.mmtl"),
                None,
            );
            // project_from_loaded already set dirty=false; replay the add_image event for dirty flag
            crate::app::handle_model_event(&mut tab, ev);
            let image_id = tab.images[0].image_id;
            if let Some(ev2) = tab.project.append_ocr_for_image_with_event(image_id, fake_ocr_entries()) {
                crate::app::handle_model_event(&mut tab, ev2);
            }
            // Ensure the fake tab is dirty=false after boot (like a loaded project)
            tab.dirty = false;
            app.tabs.push(tab);
            app.active = app.tabs.len() - 1;
        }
        #[cfg(all(feature = "test-ui", not(feature = "translation")))]
        {
            use std::collections::BTreeMap;
            let _ = easyscanlate_settings::modify(|s| {
                s.connections.insert(
                    translation::FAKE_PROVIDER.to_string(),
                    easyscanlate_settings::Connection {
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
        app.active_tab_mut().status = "TEST-UI build: fake white page with fake OCR entries and fake translation loaded."
            .to_string();
    }
    let fonts_task =
        Task::perform(async move { enumerate_system_fonts() }, Message::SystemFonts);
    let cjk_task = Task::perform(async move { load_cjk_fallbacks() }, Message::CjkFallbackLoaded);

    // Store single-instance listener (so subscription can poll for forwarded .mmtl).
    app.ipc_listener = ipc_listener;

    // Velopack update check (GithubSource Liiesl/EasyScanlate, per-user). Mirrors
    // ManhwaOCR update.py 2s startup delay + settings toggle; here always on
    // at boot, results surface in Settings → Updates and Home badge.
    let update_task = Task::perform(
        async { tokio::task::spawn_blocking(crate::updater::check_for_updates).await.unwrap_or(None) },
        |info| Message::UpdateCheckResult(Box::new(info)),
    );

    // CLI open: if an .mmtl was passed on the command line, load it into a
    // new project tab immediately (mirrors ManhwaOCR main.py:216-226 which
    // skips Home when sys.argv[1] is .mmtl). Missing file → status error.
    let cli_task = if let Some(path) = initial_mmtl {
        if !path.exists() {
            app.active_tab_mut().status =
                format!("Missing: {}", path.display());
            Task::none()
        } else {
            let display = path.to_string_lossy().to_string();
            let new_id = crate::app::tab::TabId(app.next_tab_id);
            app.next_tab_id += 1;
            // Show loading feedback on the Home tab until the new tab appears.
            app.active_tab_mut().status = format!("Loading {}...", path.display());
            let path_clone = path.clone();
            Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || {
                        crate::app::mmtl::load_created_project(
                            path_clone.to_string_lossy().to_string(),
                        )
                    })
                    .await
                    .unwrap_or_else(|e| Err(format!("load task failed: {e}")))
                },
                move |res| Message::Tab(new_id, crate::app::TabMessage::RecentPickedToLoad(res)),
            )
        }
    } else {
        Task::none()
    };

    (
        app,
        Task::batch([font_task, models_task, fonts_task, cjk_task, cli_task, update_task]),
    )
}

/// Fake OCR entries for TEST builds: a small batch of Korean bubbles spread
/// over a page-sized canvas. Used at `test-ui` boot and by the fake OCR run.
#[cfg_attr(all(feature = "ocr", not(feature = "test-ui")), allow(dead_code))]
pub fn fake_ocr_entries() -> Vec<NewEntry> {
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
pub fn enumerate_system_fonts() -> Vec<(String, String)> {
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

/// Loads only CJK-covering system fonts into iced's `cosmic_text` DB,
/// off the UI thread. Runs concurrently with `enumerate_system_fonts()`.
/// Returns the number of font files loaded (for status reporting).
/// Filtering by `CJK_FALLBACK_FAMILIES` keeps boot fast — not a full
/// `load_system_fonts` mirror into the renderer (issue #24).
pub fn load_cjk_fallbacks() -> usize {
    use std::borrow::Cow;
    use std::collections::HashSet;

    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    // Distinct file paths whose family matches the CJK allow-list.
    let mut paths: HashSet<String> = HashSet::new();
    for face in db.faces() {
        let mut matched = false;
        for (name, _lang) in &face.families {
            if is_cjk_fallback_family(name) {
                matched = true;
                break;
            }
        }
        if !matched {
            continue;
        }
        let path = match &face.source {
            fontdb::Source::File(p) => p.to_string_lossy().into_owned(),
            fontdb::Source::SharedFile(p, _) => p.to_string_lossy().into_owned(),
            fontdb::Source::Binary(_) => continue,
        };
        paths.insert(path);
    }
    let mut loaded = 0usize;
    for path in paths {
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        // `load_font` bumps the global version so `Paragraph`s relayout.
        let cow: Cow<'static, [u8]> = Cow::Owned(bytes);
        if let Ok(mut fs) = iced::advanced::graphics::text::font_system().write() {
            fs.load_font(cow);
            loaded += 1;
        }
    }
    loaded
}
