use iced::Task;
use scanlateit_model::{EntryId, EntryStyle, Quad, TextAlign, TextGradientDir};
#[cfg(feature = "styling")]
use scanlateit_styling::Engine as StylingEngine;
use scanlateit_ui::event::StyleField;

use super::{App, Message};
use super::edit::seed_style_inputs;

#[cfg(feature = "styling")]
pub fn classify_entries(app: &mut App) -> Task<Message> {
    match app.styling.engine() {
        Some(engine) => {
            start_style_jobs(app, engine.clone())
        },
        None => {
            app.styling.mark_building();
            app.status = "Loading the styling model...".to_string();
            Task::perform(async move { StylingEngine::build() }, Message::StylingEngineReady)
        }
    }
}

#[cfg(feature = "styling")]
fn start_style_jobs(app: &mut App, engine: StylingEngine) -> Task<Message> {
    let mut jobs: Vec<(usize, EntryId, String, Quad)> = Vec::new();
    for (index, image) in app.images.iter().enumerate() {
        let image_id = image.image_id;
        let path = app
            .project
            .image(image_id)
            .map(|m| m.path.clone())
            .unwrap_or_default();
        for entry in app.project.visible_for(image_id).collect::<Vec<_>>() {
            if app.styling.is_done(index, entry.id) {
                continue;
            }
            jobs.push((index, entry.id, path.clone(), app.project.view_quad(entry)));
        }
    }
    if jobs.is_empty() {
        return Task::none();
    }
    for (index, id, _, _) in &jobs {
        app.styling.mark_done(*index, *id);
    }
    let tasks: Vec<Task<Message>> = jobs
        .into_iter()
        .map(|(index, id, path, quad)| {
            let engine = engine.clone();
            Task::perform(
                async move {
                    let classified = tokio::task::spawn_blocking(move || {
                        engine.classify_entry(&path, &quad)
                    })
                    .await
                    .unwrap_or_else(|e| Err(format!("styling task cancelled: {e}")));
                    (index, id, classified)
                },
                |(index, id, result)| Message::StyleDetected(index, id, result),
            )
        })
        .collect();
    Task::batch(tasks)
}

#[cfg(all(feature = "styling", feature = "inpaint"))]
pub fn start_pipeline_style_deferred(app: &mut App) -> Task<Message> {
    let engine_opt = app.styling.engine().cloned();
    match engine_opt {
        Some(engine) => {
            start_pipeline_style_jobs(app, engine)
        },
        None => {
            app.styling.mark_building();
            #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
            {
                app.pipeline_active = true;
            }
            app.status = "Loading the styling model...".to_string();
            Task::perform(async move { StylingEngine::build() }, Message::StylingEngineReady)
        }
    }
}

#[cfg(all(feature = "styling", feature = "inpaint"))]
fn start_pipeline_style_jobs(app: &mut App, engine: StylingEngine) -> Task<Message> {
    let mut jobs: Vec<(usize, EntryId, String, Quad)> = Vec::new();
    for (index, image) in app.images.iter().enumerate() {
        let image_id = image.image_id;
        let path = app
            .project
            .image(image_id)
            .map(|m| m.path.clone())
            .unwrap_or_default();
        for entry in app.project.visible_for(image_id).collect::<Vec<_>>() {
            if app.styling.is_done(index, entry.id) {
                continue;
            }
            jobs.push((index, entry.id, path.clone(), app.project.view_quad(entry)));
        }
    }
    if jobs.is_empty() {
        #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
        {
            if app.pipeline_active {
                return super::pipeline::dispatch_pipeline_inpaint_after_style(app, Vec::new());
            }
        }
        return super::pipeline::dispatch_pipeline_inpaint_after_style(app, Vec::new());
    }
    for (index, id, _, _) in &jobs {
        app.styling.mark_done(*index, *id);
    }
    app.pipeline_style_pending = jobs.len();
    app.pipeline_style_results = Vec::with_capacity(jobs.len());
    #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
    {
        app.pipeline_active = true;
    }
    app.status = format!("Classifying {} entries (deferred for bg-aware inpaint)...", jobs.len());
    let tasks: Vec<Task<Message>> = jobs
        .into_iter()
        .map(|(index, id, path, quad)| {
            let engine = engine.clone();
            let quad_clone = quad;
            Task::perform(
                async move {
                    let res = tokio::task::spawn_blocking(move || {
                        engine.classify_entry_with_prediction(&path, &quad_clone)
                    })
                    .await
                    .unwrap_or_else(|e| Err(format!("styling task cancelled: {e}")));
                    (index, id, res)
                },
                |(index, id, result)| Message::PipelineStyleDetected(index, id, result),
            )
        })
        .collect();
    Task::batch(tasks)
}

#[cfg(feature = "styling")]
pub fn handle_styling_ready(app: &mut App, result: Result<StylingEngine, String>) -> Task<Message> {
    match result {
        Ok(engine) => {
            let is_pipeline = {
                #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
                { app.pipeline_active }
                #[cfg(not(all(feature = "styling", feature = "inpaint", feature = "segment")))]
                { false }
            };
            if is_pipeline {
                #[cfg(all(feature = "styling", feature = "inpaint"))]
                {
                    if app.styling.set_engine(engine.clone()) {
                        start_pipeline_style_jobs(app, engine)
                    } else {
                        Task::none()
                    }
                }
                #[cfg(not(all(feature = "styling", feature = "inpaint")))]
                {
                    let _ = engine;
                    Task::none()
                }
            } else if app.styling.set_engine(engine.clone()) {
                start_style_jobs(app, engine)
            } else {
                Task::none()
            }
        }
        Err(e) => {
            app.styling.fail_build();
            app.status = e.clone();
            #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
            {
                app.pipeline_active = false;
            }
            Task::none()
        }
    }
}

#[cfg(feature = "styling")]
pub fn handle_style_detected(app: &mut App, index: usize, id: EntryId, result: Result<EntryStyle, String>) -> Task<Message> {
    match result {
        Ok(style) => {
            if index < app.images.len() {
                let ev = app.project.set_entry_style_with_event(id, style);
                crate::app::handle_model_event(app, ev);
                app.styling.mark_done(index, id);
                app.status = "Applied auto-detected text style.".to_string();
            }
        }
        Err(e) => {
            app.status = format!("Style detect failed for {}:{:?}: {}", index, id, e);
        }
    }
    Task::none()
}

#[cfg(all(feature = "styling", feature = "inpaint"))]
pub fn handle_pipeline_style_detected(app: &mut App, index: usize, id: EntryId, result: Result<(EntryStyle, scanlateit_styling::StylePrediction), String>) -> Task<Message> {
    let quad = app
        .project
        .entry_including_deleted(id)
        .map(|e| app.project.view_quad(e))
        .unwrap_or(Quad { points: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]] });
    let path = app
        .images
        .get(index)
        .and_then(|img| app.project.image(img.image_id).map(|m| m.path.clone()))
        .unwrap_or_default();
    app.pipeline_style_results.push((index, id, result, quad, path));
    app.pipeline_style_pending = app.pipeline_style_pending.saturating_sub(1);
    if app.pipeline_style_pending == 0 {
        let buffered = std::mem::take(&mut app.pipeline_style_results);
        return super::pipeline::dispatch_pipeline_inpaint_after_style(app, buffered);
    }
    Task::none()
}

// ---- UI handlers ----

pub fn handle_bold(app: &mut App, bold: bool) -> Task<Message> {
    let Some((_index, id)) = app.selected else { return Task::none() };
    app.style_working.bold = bold;
    let ev = app.project.set_entry_style_with_event(id, app.style_working.clone());
    crate::app::handle_model_event(app, ev);
    Task::none()
}

pub fn handle_italic(app: &mut App, italic: bool) -> Task<Message> {
    let Some((_index, id)) = app.selected else { return Task::none() };
    app.style_working.italic = italic;
    let ev = app.project.set_entry_style_with_event(id, app.style_working.clone());
    crate::app::handle_model_event(app, ev);
    Task::none()
}

pub fn handle_font(app: &mut App, name: String) -> Task<Message> {
    let Some((_index, id)) = app.selected else { return Task::none() };
    app.style_working.font_family = Some(name.clone());
    let ev = app.project.set_entry_style_with_event(id, app.style_working.clone());
    crate::app::handle_model_event(app, ev);
    // Bundled fonts are already embedded in the binary via `include_bytes!`
    // in `main.rs` — no `font::load` needed. System-installed duplicates are
    // already in the fontdb scan. Only load non-bundled families on demand.
    let is_bundled = scanlateit_model::BUNDLED_FONTS
        .iter()
        .any(|f| f.eq_ignore_ascii_case(&name));
    if is_bundled {
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

pub fn handle_text_align(app: &mut App, align: TextAlign) -> Task<Message> {
    let Some((_index, id)) = app.selected else { return Task::none() };
    app.style_working.text_align = align;
    let ev = app.project.set_entry_style_with_event(id, app.style_working.clone());
    crate::app::handle_model_event(app, ev);
    Task::none()
}

pub fn handle_gradient_toggle(app: &mut App, enabled: bool) -> Task<Message> {
    let Some((_index, id)) = app.selected else { return Task::none() };
    app.style_working.text_gradient = enabled;
    let ev = app.project.set_entry_style_with_event(id, app.style_working.clone());
    crate::app::handle_model_event(app, ev);
    Task::none()
}

pub fn handle_gradient_dir(app: &mut App, dir: TextGradientDir) -> Task<Message> {
    let Some((_index, id)) = app.selected else { return Task::none() };
    app.style_working.gradient_dir = dir;
    let ev = app.project.set_entry_style_with_event(id, app.style_working.clone());
    crate::app::handle_model_event(app, ev);
    Task::none()
}

pub fn handle_color_open(app: &mut App, field: StyleField) -> Task<Message> {
    app.style_picker = Some(field);
    Task::none()
}

pub fn handle_color_cancel(app: &mut App, _field: StyleField) -> Task<Message> {
    app.style_picker = None;
    Task::none()
}

pub fn handle_color_submit(app: &mut App, field: StyleField, color: iced::Color) -> Task<Message> {
    app.style_picker = None;
    // Clear the hex text buffer for this field so the input shows the canonical
    // hex from the picked color instead of stale typed text.
    app.style_hex_overrides.remove(&field);
    let Some((_index, id)) = app.selected else { return Task::none() };
    let rgba = color.into_rgba8();
    match field {
        StyleField::Text => app.style_working.text_color = rgba,
        StyleField::Stroke => app.style_working.stroke_color = rgba,
        StyleField::Background => app.style_working.bg_color = rgba,
        StyleField::GradientA => app.style_working.gradient_a = rgba,
        StyleField::GradientB => app.style_working.gradient_b = rgba,
    }
    let ev = app.project.set_entry_style_with_event(id, app.style_working.clone());
    crate::app::handle_model_event(app, ev);
    Task::none()
}

pub fn handle_hex_input(app: &mut App, field: StyleField, text: String) -> Task<Message> {
    let Some((_index, id)) = app.selected else { return Task::none() };
    // Keep the raw buffer so intermediate invalid states don't snap back.
    // Empty string clears the buffer to show the canonical value.
    if text.is_empty() {
        app.style_hex_overrides.remove(&field);
        return Task::none();
    }
    app.style_hex_overrides.insert(field, text.clone());

    // Live-apply only when the text is a valid hex (or "None").
    let Some(color) = scanlateit_ui::color::parse_hex_color(&text) else {
        // Invalid intermediate – keep buffer, don't update style.
        return Task::none();
    };
    // Keep buffer to avoid snap-back while typing; cleared on selection
    // change / picker / preset. `index`/`id` already validated above.
    let rgba = color.into_rgba8();
    match field {
        StyleField::Text => app.style_working.text_color = rgba,
        StyleField::Stroke => app.style_working.stroke_color = rgba,
        StyleField::Background => app.style_working.bg_color = rgba,
        StyleField::GradientA => app.style_working.gradient_a = rgba,
        StyleField::GradientB => app.style_working.gradient_b = rgba,
    }
    let ev = app.project.set_entry_style_with_event(id, app.style_working.clone());
    crate::app::handle_model_event(app, ev);
    // Update buffer to canonical? Keep original to avoid jump; but if the
    // parsed color's canonical label differs only in case, keep typed text.
    // (No extra work needed.)
    Task::none()
}

pub fn handle_stroke_width(app: &mut App, text: String) -> Task<Message> {
    let Some((_index, id)) = app.selected else { return Task::none() };
    app.style_stroke_width = text;
    if let Ok(width) = app.style_stroke_width.parse::<f32>() {
        app.style_working.stroke_width = width.max(0.0);
        let ev = app.project.set_entry_style_with_event(id, app.style_working.clone());
        crate::app::handle_model_event(app, ev);
    }
    Task::none()
}

pub fn handle_bg_radius(app: &mut App, text: String) -> Task<Message> {
    let Some((_index, id)) = app.selected else { return Task::none() };
    app.style_bg_radius = text;
    if let Ok(radius) = app.style_bg_radius.parse::<f32>() {
        app.style_working.bg_radius = radius.max(0.0);
        let ev = app.project.set_entry_style_with_event(id, app.style_working.clone());
        crate::app::handle_model_event(app, ev);
    }
    Task::none()
}

pub fn handle_preset_apply(app: &mut App, preset: usize) -> Task<Message> {
    let Some((_index, id)) = app.selected else { return Task::none() };
    let Some(preset_style) = app.presets.get(preset) else {
        return Task::none();
    };
    seed_style_inputs(app, preset_style.clone());
    let ev = app.project.set_entry_style_with_event(id, preset_style);
    crate::app::handle_model_event(app, ev);
    Task::none()
}

pub fn handle_preset_add(app: &mut App) -> Task<Message> {
    app.presets.add(app.style_working.clone());
    let _ = scanlateit_settings::modify(|s| s.style_presets = app.presets.clone());
    Task::none()
}

pub fn handle_preset_replace(app: &mut App, preset: usize) -> Task<Message> {
    app.presets.replace(preset, app.style_working.clone());
    let _ = scanlateit_settings::modify(|s| s.style_presets = app.presets.clone());
    Task::none()
}

pub fn handle_preset_remove(app: &mut App, preset: usize) -> Task<Message> {
    app.presets.remove(preset);
    let _ = scanlateit_settings::modify(|s| s.style_presets = app.presets.clone());
    Task::none()
}

pub fn handle_auto_detect(app: &mut App) -> Task<Message> {
    #[cfg(feature = "styling")]
    {
        let Some((index, id)) = app.selected else { return Task::none() };
        app.styling.reopen(index, id);
        classify_entries(app)
    }
    #[cfg(not(feature = "styling"))]
    {
        let Some((_index, id)) = app.selected else { return Task::none() };
        let style = EntryStyle {
            bold: true,
            italic: false,
            ..EntryStyle::default()
        };
        let ev = app.project.set_entry_style_with_event(id, style);
        crate::app::handle_model_event(app, ev);
        app.status = "Applied a fake auto-detected text style (no styling model in this build)."
            .to_string();
        Task::none()
    }
}
