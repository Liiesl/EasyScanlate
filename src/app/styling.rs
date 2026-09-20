use std::collections::HashSet;

use iced::{Task, window};
use easyscanlate_model::{EntryId, EntryStyle, Quad, TextAlign, TextGradientDir};
#[cfg(feature = "styling")]
use easyscanlate_styling::Engine as StylingEngine;
use easyscanlate_ui::event::{StyleField, UiEvent};

use easyscanlate_ui::UiState;

use super::{App, Message};
use super::edit::seed_style_inputs;



// ---- UI handlers ----

pub fn handle_bold(app: &mut App, bold: bool) -> Task<Message> {
    let Some((_index, id)) = app.active_tab_mut().selected else { return Task::none() };
    // Per-font availability: ignore enabling Bold when the selected family
    // has no Bold face (button is disabled in the panel; this is defense for
    // stale events). Clearing is always allowed.
    // Exclusive families (Bold + Italic but no BoldItalic) act as a switcher:
    // enabling Bold clears Italic in the same single undo event.
    if bold {
        let support = easyscanlate_ui::main_area::overlay::font_support(
            app.active_tab().style_working.font_family.as_deref(),
        );
        if !support.has_bold {
            return Task::none();
        }
        app.active_tab_mut().style_working.bold = true;
        if support.is_exclusive() {
            app.active_tab_mut().style_working.italic = false;
        }
    } else {
        app.active_tab_mut().style_working.bold = false;
    }
    let style = app.active_tab().style_working.clone();
    let ev = app.active_tab_mut().project.set_entry_style_with_event(id, style);
    crate::app::handle_model_event(app.active_tab_mut(), ev);
    Task::none()
}

pub fn handle_italic(app: &mut App, italic: bool) -> Task<Message> {
    let Some((_index, id)) = app.active_tab_mut().selected else { return Task::none() };
    // Mirror of `handle_bold`: ignore enabling Italic without an Italic face;
    // exclusive families switch off Bold when Italic turns on.
    if italic {
        let support = easyscanlate_ui::main_area::overlay::font_support(
            app.active_tab().style_working.font_family.as_deref(),
        );
        if !support.has_italic {
            return Task::none();
        }
        app.active_tab_mut().style_working.italic = true;
        if support.is_exclusive() {
            app.active_tab_mut().style_working.bold = false;
        }
    } else {
        app.active_tab_mut().style_working.italic = false;
    }
    let style = app.active_tab().style_working.clone();
    let ev = app.active_tab_mut().project.set_entry_style_with_event(id, style);
    crate::app::handle_model_event(app.active_tab_mut(), ev);
    Task::none()
}

pub fn handle_font(app: &mut App, name: String) -> Task<Message> {
    let Some((_index, id)) = app.active_tab_mut().selected else { return Task::none() };
    // Intentionally preserves `bold`/`italic`: when the new family lacks the
    // face, the panel shows the stored (e.g. auto-detected) value as disabled
    // yet visually selected instead of clearing it. Rendering degrades in
    // `styled_font_for_text`.
    app.active_tab_mut().style_working.font_family = Some(name.clone());
    let style = app.active_tab().style_working.clone();
    let ev = app.active_tab_mut().project.set_entry_style_with_event(id, style);
    crate::app::handle_model_event(app.active_tab_mut(), ev);
    // Bundled fonts are already embedded in the binary via `include_bytes!`
    // in `main.rs` — no `font::load` needed. System-installed duplicates are
    // already in the fontdb scan. Only load non-bundled families on demand.
    if is_bundled(&name) {
        // Ensure it's marked loaded even if picker injected it without a system path.
        app.loaded_fonts.insert(name);
        return Task::none();
    }
    if !app.loaded_fonts.contains(&name) {
        app.loaded_fonts.insert(name.clone());
        let Some(path) = app.system_fonts.get(&name).cloned() else {
            return Task::none();
        };
        match std::fs::read(path) {
            Ok(bytes) => iced::font::load(bytes).map(move |_| Message::StyleFontLoaded(name.clone())),
            Err(_) => Task::none(),
        }
    } else {
        Task::none()
    }
}

/// First-screen batch for the font dropdown's visible-only lazy preload:
/// with `menu_max_height(300)` about 5–6 families are visible (each takes
/// a label + a preview row), so 12 covers the first screen plus over-scroll.
/// Taken in dropdown display order (Featured first, then All fonts);
/// bundled Featured families skip loading in `preview_load_task` anyway.
const FONT_PREVIEW_BATCH: usize = 12;

fn is_bundled(name: &str) -> bool {
    easyscanlate_model::BUNDLED_FONTS
        .iter()
        .any(|f| f.eq_ignore_ascii_case(name))
}

/// Queue `iced::font::load` for `names` that are neither bundled (already
/// embedded) nor loaded before. Marks families loaded optimistically and
/// dedupes shared files within the batch; completion is the silent
/// `StyleFontPreviewLoaded` (never touches the status bar or the style).
fn preview_load_task(app: &mut App, names: &[String]) -> Task<Message> {
    let mut tasks = Vec::new();
    let mut seen_paths = HashSet::new();
    for name in names {
        if is_bundled(name) || app.loaded_fonts.contains(name) {
            continue;
        }
        let Some(path) = app.system_fonts.get(name).cloned() else {
            continue;
        };
        if !seen_paths.insert(path.clone()) {
            app.loaded_fonts.insert(name.clone());
            continue;
        }
        app.loaded_fonts.insert(name.clone());
        if let Ok(bytes) = std::fs::read(&path) {
            let name = name.clone();
            tasks.push(
                iced::font::load(bytes)
                    .map(move |_| Message::StyleFontPreviewLoaded(name.clone())),
            );
        }
    }
    Task::batch(tasks)
}

pub fn handle_font_preview_open(app: &mut App) -> Task<Message> {
    let (featured, rest) = easyscanlate_model::partition_featured_fonts(&app.installed_fonts);
    let names: Vec<String> = featured
        .into_iter()
        .chain(rest)
        .take(FONT_PREVIEW_BATCH)
        .map(|index| app.installed_fonts[index].clone())
        .collect();
    preview_load_task(app, &names)
}

pub fn handle_font_preview_hover(app: &mut App, name: String) -> Task<Message> {
    preview_load_task(app, &[name])
}

pub fn handle_text_align(app: &mut App, align: TextAlign) -> Task<Message> {
    let Some((_index, id)) = app.active_tab_mut().selected else { return Task::none() };
    app.active_tab_mut().style_working.text_align = align;
    let style = app.active_tab().style_working.clone();
    let ev = app.active_tab_mut().project.set_entry_style_with_event(id, style);
    crate::app::handle_model_event(app.active_tab_mut(), ev);
    Task::none()
}

pub fn handle_gradient_toggle(app: &mut App, enabled: bool) -> Task<Message> {
    let Some((_index, id)) = app.active_tab_mut().selected else { return Task::none() };
    app.active_tab_mut().style_working.text_gradient = enabled;
    let style = app.active_tab().style_working.clone();
    let ev = app.active_tab_mut().project.set_entry_style_with_event(id, style);
    crate::app::handle_model_event(app.active_tab_mut(), ev);
    Task::none()
}

pub fn handle_gradient_dir(app: &mut App, dir: TextGradientDir) -> Task<Message> {
    let Some((_index, id)) = app.active_tab_mut().selected else { return Task::none() };
    app.active_tab_mut().style_working.gradient_dir = dir;
    let style = app.active_tab().style_working.clone();
    let ev = app.active_tab_mut().project.set_entry_style_with_event(id, style);
    crate::app::handle_model_event(app.active_tab_mut(), ev);
    Task::none()
}

pub fn handle_color_open(app: &mut App, field: StyleField) -> Task<Message> {
    let current = app.active_tab_mut().style_picker;
    if current == Some(field) {
        return Task::none();
    }
    if current.is_some() {
        // Switch while open: the shared top-level picker owns its color while
        // shown and only re-seeds on close->reopen, so close first and reopen
        // on the next update to pick up the new field's color.
        app.active_tab_mut().style_picker = None;
        return Task::done(Message::Ui(UiEvent::StyleColorOpen(field)));
    }
    app.active_tab_mut().style_picker = Some(field);
    Task::none()
}

pub fn handle_color_cancel(app: &mut App, _field: StyleField) -> Task<Message> {
    app.active_tab_mut().style_picker = None;
    Task::none()
}

/// The eye dropper requested a fresh window snapshot: capture the window
/// and hand the frame to the shared [`DropperBuffer`](neverliie_iced_widgets::color_picker::DropperBuffer).
/// The widget consumes it on its next update pass and enters picking mode.
pub fn handle_dropper_capture(app: &mut App) -> Task<Message> {
    let _ = app;
    window::latest()
        .and_then(window::screenshot)
        .map(|shot| Message::Ui(UiEvent::StyleDropperShot(Box::new(shot))))
}

/// A window screenshot for the eye dropper arrived: deposit it in the shared
/// buffer. Each frame is consumed exactly once when the widget enters
/// picking mode.
pub fn handle_dropper_shot(
    app: &mut App,
    shot: Box<iced::window::screenshot::Screenshot>,
) -> Task<Message> {
    app.dropper_buffer.store(&shot);
    Task::none()
}

pub fn handle_color_submit(app: &mut App, field: StyleField, color: iced::Color) -> Task<Message> {
    app.active_tab_mut().style_picker = None;
    // Clear the hex text buffer for this field so the input shows the canonical
    // hex from the picked color instead of stale typed text.
    app.active_tab_mut().style_hex_overrides.remove(&field);
    let Some((_index, id)) = app.active_tab_mut().selected else { return Task::none() };
    let rgba = color.into_rgba8();
    match field {
        StyleField::Text => app.active_tab_mut().style_working.text_color = rgba,
        StyleField::Stroke => app.active_tab_mut().style_working.stroke_color = rgba,
        StyleField::Background => app.active_tab_mut().style_working.bg_color = rgba,
        StyleField::GradientA => app.active_tab_mut().style_working.gradient_a = rgba,
        StyleField::GradientB => app.active_tab_mut().style_working.gradient_b = rgba,
    }
    let style = app.active_tab().style_working.clone();
    let ev = app.active_tab_mut().project.set_entry_style_with_event(id, style);
    crate::app::handle_model_event(app.active_tab_mut(), ev);
    Task::none()
}

pub fn handle_hex_input(app: &mut App, field: StyleField, text: String) -> Task<Message> {
    let Some((_index, id)) = app.active_tab_mut().selected else { return Task::none() };
    // Keep the raw buffer so intermediate invalid states don't snap back.
    // Empty string clears the buffer to show the canonical value.
    if text.is_empty() {
        app.active_tab_mut().style_hex_overrides.remove(&field);
        return Task::none();
    }
    app.active_tab_mut().style_hex_overrides.insert(field, text.clone());

    // Live-apply only when the text is a valid hex (or "None").
    let Some(color) = easyscanlate_ui::color::parse_hex_color(&text) else {
        // Invalid intermediate – keep buffer, don't update style.
        return Task::none();
    };
    // Keep buffer to avoid snap-back while typing; cleared on selection
    // change / picker / preset. `index`/`id` already validated above.
    let rgba = color.into_rgba8();
    match field {
        StyleField::Text => app.active_tab_mut().style_working.text_color = rgba,
        StyleField::Stroke => app.active_tab_mut().style_working.stroke_color = rgba,
        StyleField::Background => app.active_tab_mut().style_working.bg_color = rgba,
        StyleField::GradientA => app.active_tab_mut().style_working.gradient_a = rgba,
        StyleField::GradientB => app.active_tab_mut().style_working.gradient_b = rgba,
    }
    let style = app.active_tab().style_working.clone();
    let ev = app.active_tab_mut().project.set_entry_style_with_event(id, style);
    crate::app::handle_model_event(app.active_tab_mut(), ev);
    // Update buffer to canonical? Keep original to avoid jump; but if the
    // parsed color's canonical label differs only in case, keep typed text.
    // (No extra work needed.)
    Task::none()
}

pub fn handle_stroke_width(app: &mut App, text: String) -> Task<Message> {
    let Some((_index, id)) = app.active_tab_mut().selected else { return Task::none() };
    app.active_tab_mut().style_stroke_width = text;
    if let Ok(width) = app.active_tab_mut().style_stroke_width.parse::<f32>() {
        app.active_tab_mut().style_working.stroke_width = width.max(0.0);
        let style = app.active_tab().style_working.clone();
    let ev = app.active_tab_mut().project.set_entry_style_with_event(id, style);
        crate::app::handle_model_event(app.active_tab_mut(), ev);
    }
    Task::none()
}

pub fn handle_bg_radius(app: &mut App, text: String) -> Task<Message> {
    let Some((_index, id)) = app.active_tab_mut().selected else { return Task::none() };
    app.active_tab_mut().style_bg_radius = text;
    if let Ok(radius) = app.active_tab_mut().style_bg_radius.parse::<f32>() {
        app.active_tab_mut().style_working.bg_radius = radius.max(0.0);
        let style = app.active_tab().style_working.clone();
    let ev = app.active_tab_mut().project.set_entry_style_with_event(id, style);
        crate::app::handle_model_event(app.active_tab_mut(), ev);
    }
    Task::none()
}

pub fn handle_preset_apply(app: &mut App, preset: usize) -> Task<Message> {
    let Some((_index, id)) = app.active_tab_mut().selected else { return Task::none() };
    let Some(preset_style) = app.presets.get(preset) else {
        return Task::none();
    };
    seed_style_inputs(app, preset_style.clone());
    let ev = app.active_tab_mut().project.set_entry_style_with_event(id, preset_style);
    crate::app::handle_model_event(app.active_tab_mut(), ev);
    Task::none()
}

pub fn handle_preset_add(app: &mut App) -> Task<Message> {
    let style = app.active_tab().style_working.clone();
    app.presets.add(style);
    let _ = easyscanlate_settings::modify(|s| s.style_presets = app.presets.clone());
    Task::none()
}

pub fn handle_preset_replace(app: &mut App, preset: usize) -> Task<Message> {
    let style = app.active_tab().style_working.clone();
    app.presets.replace(preset, style);
    let _ = easyscanlate_settings::modify(|s| s.style_presets = app.presets.clone());
    Task::none()
}

pub fn handle_preset_remove(app: &mut App, preset: usize) -> Task<Message> {
    app.presets.remove(preset);
    let _ = easyscanlate_settings::modify(|s| s.style_presets = app.presets.clone());
    Task::none()
}

pub fn handle_auto_detect(app: &mut App) -> Task<Message> {
    if app.active_state().is_bulk_busy() {
        app.active_tab_mut().status = "Wait for current task to finish.".to_string();
        return Task::none();
    }
    #[cfg(feature = "styling")]
    {
        use easyscanlate_styling::tracker::PendingSingle;
        let Some((index, id)) = app.active_tab().selected else { return Task::none() };
        // Validate entry still exists and get its view quad.
        let (path, quad) = {
            let tab = app.active_tab();
            let Some(entry) = tab.project.entry(id) else { return Task::none() };
            let Some(img) = tab.images.get(index) else { return Task::none() };
            let image_id = img.image_id;
            let path = tab
                .project
                .image(image_id)
                .map(|m| m.path.clone())
                .unwrap_or_default();
            let quad = tab.project.view_quad(entry);
            (path, quad)
        };
        app.active_tab_mut().styling.reopen(index, id);
        // Global engine cache (pool, like OCR/inpaint/segment): classify immediately if loaded.
        if let Some(engine) = app.engines.styling.clone() {
            app.active_tab_mut().styling.mark_done(index, id);
            let engine_clone = engine.clone();
            let tid = app.active_tab().id;
            return Task::perform(
                async move {
                    let classified = tokio::task::spawn_blocking(move || {
                        engine_clone.classify_entry(&path, &quad)
                    })
                    .await
                    .unwrap_or_else(|e| Err(format!("styling task cancelled: {e}")));
                    (index, id, classified)
                },
                move |(idx, eid, result)| Message::Tab(tid, crate::app::TabMessage::StyleDetected(idx, eid, result)),
            );
        }
        // Engine not yet built: remember the *original* request so ready resumes single entry.
        app.active_tab_mut().styling.set_pending_single(PendingSingle {
            index,
            id,
            path,
            quad,
        });
        // Queue gate for the manual single build (weight 2) so parallel tabs
        // backpressure instead of spawning duplicate builds.
        let tid = app.active_tab().id;
        let job_id = {
            use crate::app::queue::{AcquireResult, JobKind, owner_of};
            // If a styling build is already in flight globally, just wait for it.
            if app.engines.is_styling_building()
                || app.engines.queue.running_for(owner_of(tid), JobKind::Styling).is_some()
            {
                return Task::none();
            }
            match app.engines.queue.try_acquire_or_enqueue(owner_of(tid), JobKind::Styling) {
                AcquireResult::Acquired(job) => job.id,
                AcquireResult::Queued(_, pos) => {
                    let used = app.engines.queue.used_weight();
                    app.active_tab_mut().status = format!(
                        "Queued {} (pos {}, pool {}/{}) ...",
                        JobKind::Styling.label(),
                        pos,
                        used,
                        crate::app::queue::POOL_CAPACITY
                    );
                    return Task::none();
                }
            }
        };
        app.engines.mark_styling_building();
        app.active_tab_mut().status = "Loading the styling model...".to_string();
        Task::perform(
            async move {
                use crate::app::queue::engine_ready_msg;
                use easyscanlate_engine_pool::BuiltEngine;
                let kind = crate::app::queue::JobKind::Styling;
                match StylingEngine::build() {
                    Ok(engine) => engine_ready_msg(tid, job_id, kind, Ok(BuiltEngine::Styling(engine))),
                    Err(e) => engine_ready_msg(tid, job_id, kind, Err(e)),
                }
            },
            |msg| msg,
        )
    }
    #[cfg(not(feature = "styling"))]
    {
        let Some((_index, id)) = app.active_tab_mut().selected else { return Task::none() };
        let style = EntryStyle {
            bold: true,
            italic: false,
            ..EntryStyle::default()
        };
        let ev = app.active_tab_mut().project.set_entry_style_with_event(id, style);
        crate::app::handle_model_event(app.active_tab_mut(), ev);
        app.active_tab_mut().status = "Applied a fake auto-detected text style (no styling model in this build)."
            .to_string();
        Task::none()
    }
}

// ---- TabId-aware wrappers for Phase 2 ----
#[cfg(feature = "styling")]
fn collect_jobs(app: &App, tab_id: crate::app::tab::TabId) -> Vec<(usize, EntryId, String, Quad)> {
    let tab = match app.tab_by_id(tab_id) { Some(t) => t, None => return Vec::new() };
    let mut jobs: Vec<(usize, EntryId, String, Quad)> = Vec::new();
    for (index, image) in tab.images.iter().enumerate() {
        let image_id = image.image_id;
        let path = tab.project.image(image_id).map(|m| m.path.clone()).unwrap_or_default();
        for entry in tab.project.visible_for(image_id).collect::<Vec<_>>() {
            if tab.styling.is_done(index, entry.id) { continue; }
            jobs.push((index, entry.id, path.clone(), tab.project.view_quad(entry)));
        }
    }
    jobs
}
#[cfg(feature = "styling")]
pub fn classify(app: &mut App, tab_id: crate::app::tab::TabId) -> Task<Message> {
    // queue gate — weight 2, backfill + priority (cap 5)
    {
        use crate::app::queue::{AcquireResult, JobKind, owner_of};
        let already_reserved = app.engines.queue.running_for(owner_of(tab_id), JobKind::Styling).is_some();
        if !already_reserved {
            let idx_tmp = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i)=>i, None=>return Task::none()};
            match app.engines.queue.try_acquire_or_enqueue(owner_of(tab_id), JobKind::Styling) {
                AcquireResult::Acquired(_) => {},
                AcquireResult::Queued(_, pos) => {
                    app.tabs[idx_tmp].status = format!("Queued {} (pos {}, pool {}/{}) ...", JobKind::Styling.label(), pos, app.engines.queue.used_weight(), crate::app::queue::POOL_CAPACITY);
                    // mark pipeline active if deferred chain expected
                    let deferred = easyscanlate_settings::get(|s| s.auto_inpaint) && cfg!(feature = "inpaint");
                    if deferred {
                        #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
                        { app.tabs[idx_tmp].pipeline_active = true; }
                    }
                    return Task::none();
                }
            }
        }
    }
    let deferred = easyscanlate_settings::get(|s| s.auto_inpaint) && cfg!(feature = "inpaint");
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    let engine_opt = app.engines.styling.clone();
    match engine_opt {
        Some(engine) => start_jobs(app, tab_id, engine, deferred),
        None => {
            app.engines.mark_styling_building();
            if deferred {
                #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
                { app.tabs[idx].pipeline_active = true; }
            }
            app.tabs[idx].status = format!("Loading the styling model... pool {}/{}", app.engines.queue.used_weight(), crate::app::queue::POOL_CAPACITY);
            let job_id = app
                .engines
                .queue
                .running_for(crate::app::queue::owner_of(tab_id), crate::app::queue::JobKind::Styling)
                .map(|j| j.id)
                .unwrap_or(0);
            let tid = tab_id;
            Task::perform(
                async move {
                    use crate::app::queue::engine_ready_msg;
                    use easyscanlate_engine_pool::BuiltEngine;
                    let kind = crate::app::queue::JobKind::Styling;
                    match StylingEngine::build() {
                        Ok(engine) => engine_ready_msg(tid, job_id, kind, Ok(BuiltEngine::Styling(engine))),
                        Err(e) => engine_ready_msg(tid, job_id, kind, Err(e)),
                    }
                },
                |msg| msg,
            )
        }
    }
}
#[cfg(feature = "styling")]
fn start_jobs(app: &mut App, tab_id: crate::app::tab::TabId, engine: StylingEngine, deferred: bool) -> Task<Message> {
    let jobs = collect_jobs(app, tab_id);
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    if jobs.is_empty() {
        if deferred {
            #[cfg(all(feature = "styling", feature = "inpaint"))]
            { return super::pipeline::dispatch_inpaint(app, tab_id, Vec::new()); }
        }
        return Task::none();
    }
    for (index, id, _, _) in &jobs { app.tabs[idx].styling.mark_done(*index, *id); }
    if deferred {
        #[cfg(all(feature = "styling", feature = "inpaint"))]
        {
            app.tabs[idx].pipeline_style_pending = jobs.len();
            app.tabs[idx].pipeline_style_results = Vec::with_capacity(jobs.len());
            #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
            { app.tabs[idx].pipeline_active = true; }
            app.tabs[idx].status = format!("Classifying {} entries (deferred for bg-aware inpaint)...", jobs.len());
            let tasks: Vec<Task<Message>> = jobs.into_iter().map(|(index, id, path, quad)| {
                let engine = engine.clone();
                let qc = quad;
                let tid = tab_id;
                Task::perform(
                    async move {
                        let res = tokio::task::spawn_blocking(move || engine.classify_entry_with_prediction(&path, &qc)).await.unwrap_or_else(|e| Err(format!("styling task cancelled: {e}")));
                        (index, id, res)
                    },
                    move |(index, id, result)| Message::Tab(tid, crate::app::TabMessage::PipelineStyleDetected(index, id, result)),
                )
            }).collect();
            return Task::batch(tasks);
        }
    }
    let tasks: Vec<Task<Message>> = jobs.into_iter().map(|(index, id, path, quad)| {
        let engine = engine.clone();
        let tid = tab_id;
        Task::perform(
            async move {
                let classified = tokio::task::spawn_blocking(move || engine.classify_entry(&path, &quad)).await.unwrap_or_else(|e| Err(format!("styling task cancelled: {e}")));
                (index, id, classified)
            },
            move |(index, id, result)| Message::Tab(tid, crate::app::TabMessage::StyleDetected(index, id, result)),
        )
    }).collect();
    Task::batch(tasks)
}
#[cfg(feature = "styling")]
pub fn handle_styling_ready(app: &mut App, tab_id: crate::app::tab::TabId, result: Result<StylingEngine, String>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    match result {
        Ok(engine) => {
            let is_pipeline = {
                #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
                { app.tabs[idx].pipeline_active }
                #[cfg(not(all(feature = "styling", feature = "inpaint", feature = "segment")))]
                { false }
            };
            let was_building = app.engines.set_styling_engine(engine.clone());
            if !was_building { return Task::none(); }
            if let Some(pending) = app.tabs[idx].styling.take_pending_single() {
                let (pi, pid, ppath, pquad) = (pending.index, pending.id, pending.path, pending.quad);
                app.tabs[idx].styling.mark_done(pi, pid);
                let eng = engine.clone();
                let tid = tab_id;
                return Task::perform(
                    async move {
                        let classified = tokio::task::spawn_blocking(move || eng.classify_entry(&ppath, &pquad)).await.unwrap_or_else(|e| Err(format!("styling task cancelled: {e}")));
                        (pi, pid, classified)
                    },
                    move |(index, id, result)| Message::Tab(tid, crate::app::TabMessage::StyleDetected(index, id, result)),
                );
            }
            if is_pipeline {
                #[cfg(all(feature = "styling", feature = "inpaint"))]
                { start_jobs(app, tab_id, engine, true)}
                #[cfg(not(all(feature = "styling", feature = "inpaint")))]
                { let _ = engine; return Task::none(); }
            } else {
                start_jobs(app, tab_id, engine, false)
            }
        }
        Err(e) => {
            app.engines.fail_styling_build();
            app.tabs[idx].styling.clear_pending_single();
            app.tabs[idx].status = e.clone();
            #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
            { app.tabs[idx].pipeline_active = false; }
            // free queue weight (build failed) and promote
            app.engines.queue.complete(crate::app::queue::owner_of(tab_id), crate::app::queue::JobKind::Styling);
            let promote = crate::app::queue::dispatch_pending(app);
            crate::app::queue::refresh_queued_statuses(app);
            promote
        }
    }
}
#[cfg(feature = "styling")]
pub fn handle_style_detected(app: &mut App, tab_id: crate::app::tab::TabId, index: usize, id: EntryId, result: Result<EntryStyle, String>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    // For single style detect (manual), free queue weight after completion
    let is_queued_style = app.engines.queue.running_for(crate::app::queue::owner_of(tab_id), crate::app::queue::JobKind::Styling).is_some();
    match result {
        Ok(style) => {
            if index < app.tabs[idx].images.len() {
                let ev = app.tabs[idx].project.set_entry_style_with_event(id, style);
                crate::app::handle_model_event(&mut app.tabs[idx], ev);
                app.tabs[idx].styling.mark_done(index, id);
                app.tabs[idx].status = "Applied auto-detected text style.".to_string();
            }
        }
        Err(e) => { app.tabs[idx].status = format!("Style detect failed for {}:{:?}: {}", index, id, e); }
    }
    if is_queued_style {
        // Check if this was the last pending single? For single detect, we free now.
        // For bulk pipeline, free is handled in pipeline handler's pending==0.
        // Detect bulk by pipeline_style_pending >0 — don't free yet
        if app.tabs[idx].pipeline_style_pending == 0 {
            app.engines.queue.complete(crate::app::queue::owner_of(tab_id), crate::app::queue::JobKind::Styling);
            let promote = crate::app::queue::dispatch_pending(app);
            crate::app::queue::refresh_queued_statuses(app);
            return promote;
        }
    }
    Task::none()
}
#[cfg(all(feature = "styling", feature = "inpaint"))]
pub fn handle_pipeline_style_detected(app: &mut App, tab_id: crate::app::tab::TabId, index: usize, id: EntryId, result: Result<(EntryStyle, easyscanlate_styling::StylePrediction), String>) -> Task<Message> {
    let idx = match app.tabs.iter().position(|t| t.id == tab_id) { Some(i) => i, None => return Task::none() };
    let (quad, path) = {
        let tab = &app.tabs[idx];
        let quad = tab.project.entry_including_deleted(id).map(|e| tab.project.view_quad(e)).unwrap_or(Quad { points: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]] });
        let path = tab.images.get(index).and_then(|img| tab.project.image(img.image_id).map(|m| m.path.clone())).unwrap_or_default();
        (quad, path)
    };
    app.tabs[idx].pipeline_style_results.push((index, id, result, quad, path));
    app.tabs[idx].pipeline_style_pending = app.tabs[idx].pipeline_style_pending.saturating_sub(1);
    if app.tabs[idx].pipeline_style_pending == 0 {
        // style bulk done — free queue weight for Style
        app.engines.queue.complete(crate::app::queue::owner_of(tab_id), crate::app::queue::JobKind::Styling);
        crate::app::queue::refresh_queued_statuses(app);
        let buffered = std::mem::take(&mut app.tabs[idx].pipeline_style_results);
        // dispatch_inpaint will enqueue inpaint jobs via queue (backfill + priority)
        let inpaint_task = super::pipeline::dispatch_inpaint(app, tab_id, buffered);
        let promote = crate::app::queue::dispatch_pending(app);
        return Task::batch([inpaint_task, promote]);
    }
    Task::none()
}
