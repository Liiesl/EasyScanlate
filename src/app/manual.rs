use iced::Task;
use iced::Rectangle;
use scanlateit_ui::UiState;
use scanlateit_ui::event::ManualMode;

use super::{App, Message};

pub fn handle_enter(app: &mut App, mode: ManualMode) -> Task<Message> {
    eprintln!("[manual] handle_enter mode={:?} current={:?} images={} inpainting={} running={} translating={}", mode, app.manual_mode, app.images.len(), app.inpainting, app.running, app.translating);
    if app.images.is_empty() {
        app.status = "Open images first.".to_string();
        return Task::none();
    }
    if app.is_bulk_busy() {
        // allow switching manual modes? still block entering new one while bulk busy
        if app.manual_mode == ManualMode::None || app.manual_mode != mode {
            app.status = "Wait for current task to finish.".to_string();
            return Task::none();
        }
    }
    if app.inpainting {
        app.status = "Wait for inpaint to finish.".to_string();
        return Task::none();
    }
    #[cfg(feature = "ocr")]
    if app.manual_ocring {
        app.status = "Wait for manual OCR to finish.".to_string();
        return Task::none();
    }
    if app.running || app.translating {
        app.status = "Wait for current task to finish.".to_string();
        return Task::none();
    }
    if app.manual_mode == mode {
        eprintln!("[manual] already in mode {:?}", mode);
        return Task::none();
    }
    // switching or entering
    if app.manual_mode != ManualMode::None {
        eprintln!("[manual] switching mode {:?} -> {:?}, clearing {} selections", app.manual_mode, mode, app.manual_selections.len());
        app.manual_selections.clear();
    } else {
        // first entry, stash view mode
        app.manual_prev_view_mode = Some(app.view_mode);
        app.view_mode = scanlateit_ui::event::MainAreaMode::View;
        eprintln!("[manual] entering mode {:?}, stashed view_mode={:?}", mode, app.manual_prev_view_mode);
    }
    app.manual_mode = mode;
    crate::app::edit::clear_editing(app);
    app.selected = None;
    app.selected_inpaint = None;
    app.status = match mode {
        ManualMode::Inpaint => "Manual Inpaint: drag to select areas, then Start.".to_string(),
        ManualMode::Ocr => "Manual OCR: drag to select text areas, then Start.".to_string(),
        ManualMode::None => "Idle".to_string(),
    };
    eprintln!("[manual] entered mode={:?} status={}", mode, app.status);
    Task::none()
}

pub fn handle_cancel(app: &mut App) -> Task<Message> {
    eprintln!("[manual] handle_cancel current={:?} selections={}", app.manual_mode, app.manual_selections.len());
    if app.manual_mode == ManualMode::None {
        return Task::none();
    }
    let prev = app.manual_prev_view_mode;
    app.manual_mode = ManualMode::None;
    app.manual_selections.clear();
    if let Some(prev) = app.manual_prev_view_mode.take() {
        app.view_mode = prev;
        eprintln!("[manual] cancelled, restored view_mode={:?}", prev);
    } else {
        eprintln!("[manual] cancelled, no prev view_mode");
    }
    eprintln!("[manual] mode reset from {:?} selections cleared", prev);
    app.status = "Manual mode cancelled.".to_string();
    Task::none()
}

pub fn handle_reset(app: &mut App) -> Task<Message> {
    eprintln!("[manual] handle_reset mode={:?} selections={}", app.manual_mode, app.manual_selections.len());
    if app.manual_mode == ManualMode::None {
        return Task::none();
    }
    if app.manual_selections.is_empty() {
        eprintln!("[manual] reset no selections -> none");
        return Task::none();
    }
    eprintln!("[manual] clearing {} selections", app.manual_selections.len());
    app.manual_selections.clear();
    app.status = "Selections cleared.".to_string();
    Task::none()
}

pub fn handle_selection(app: &mut App, sels: Vec<(usize, Rectangle)>) -> Task<Message> {
    eprintln!("[manual] handle_selection mode={:?} sels={} before={}", app.manual_mode, sels.len(), app.manual_selections.len());
    for (idx, r) in &sels { eprintln!("[manual]   sel idx={} rect=[{:.1},{:.1},{:.1},{:.1}]", idx, r.x, r.y, r.width, r.height); }
    if app.manual_mode == ManualMode::None {
        eprintln!("[manual] not in manual mode -> none");
        return Task::none();
    }
    if sels.is_empty() { return Task::none(); }
    if sels.len() == 1 {
        let (idx, rect) = sels[0];
        if idx >= app.images.len() {
            eprintln!("[manual] idx {} out of range len={}", idx, app.images.len());
            return Task::none();
        }
        if rect.width < 4.0 || rect.height < 4.0 {
            eprintln!("[manual] selection too small {:.1}x{:.1}", rect.width, rect.height);
            app.status = "Selection too small.".to_string();
            return Task::none();
        }
    }
    for (idx, rect) in sels {
        if idx >= app.images.len() {
            eprintln!("[manual] span idx {} out of range", idx);
            continue;
        }
        if rect.width < 4.0 || rect.height < 4.0 {
            eprintln!("[manual] span too small {:.1}x{:.1} idx={}", rect.width, rect.height, idx);
            continue;
        }
        eprintln!("[manual]   pushing idx={} rect=[{:.1},{:.1},{:.1},{:.1}]", idx, rect.x, rect.y, rect.width, rect.height);
        app.manual_selections.push((idx, rect));
    }
    app.manual_selections.sort_by(|a, b| {
        a.0.cmp(&b.0).then(a.1.y.total_cmp(&b.1.y)).then(a.1.x.total_cmp(&b.1.x))
    });
    let before = app.manual_selections.len();
    app.manual_selections.dedup_by(|a, b| a.0 == b.0 && a.1.x == b.1.x && a.1.y == b.1.y && a.1.width == b.1.width && a.1.height == b.1.height);
    let n = app.manual_selections.len();
    eprintln!("[manual] added, before_dedup={} after={} selections={:?}", before, n, app.manual_selections.iter().map(|(i,r)| (*i, [r.x, r.y, r.width, r.height])).collect::<Vec<_>>());
    app.status = format!("{n} selection(s). Press Start to run.");
    Task::none()
}



pub fn handle_start(app: &mut App) -> Task<Message> {
    eprintln!("[manual] handle_start mode={:?} selections={} inpainting={} running={} translating={}", app.manual_mode, app.manual_selections.len(), app.inpainting, app.running, app.translating);
    for (i, (idx, r)) in app.manual_selections.iter().enumerate() { eprintln!("[manual]   sel {}: idx={} rect=[{:.1},{:.1},{:.1},{:.1}]", i, idx, r.x, r.y, r.width, r.height); }
    if app.manual_mode == ManualMode::None {
        eprintln!("[manual] not in manual mode -> none");
        return Task::none();
    }
    if app.manual_selections.is_empty() {
        eprintln!("[manual] no selections -> none");
        app.status = "No selections to run.".to_string();
        return Task::none();
    }
    if app.is_bulk_busy() {
        eprintln!("[manual] bulk busy {:?}", app.is_bulk_busy());
        app.status = "Wait for current task to finish.".to_string();
        return Task::none();
    }
    if app.inpainting {
        eprintln!("[manual] inpainting busy");
        app.status = "Wait for inpaint to finish.".to_string();
        return Task::none();
    }
    #[cfg(feature = "ocr")]
    if app.manual_ocring {
        eprintln!("[manual] manual_ocring busy");
        app.status = "Wait for manual OCR to finish.".to_string();
        return Task::none();
    }
    if app.running || app.translating {
        eprintln!("[manual] running/translating busy");
        app.status = "Wait for current task to finish.".to_string();
        return Task::none();
    }
    let sels = std::mem::take(&mut app.manual_selections);
    eprintln!("[manual] taking {} sels, mode={:?} selections cleared", sels.len(), app.manual_mode);
    // auto clear as per spec, mode stays active
    let mode = app.manual_mode;
    match mode {
        ManualMode::Inpaint => {
            #[cfg(feature = "inpaint")]
            {
                eprintln!("[manual] -> handle_inpaint_selection with {} sels", sels.len());
                return super::inpaint::handle_inpaint_selection(app, sels);
            }
            #[cfg(not(feature = "inpaint"))]
            {
                eprintln!("[manual] inpaint not available");
                app.status = "Inpaint not available in this build.".to_string();
                app.manual_selections = sels;
                return Task::none();
            }
        }
        ManualMode::Ocr => {
            #[cfg(feature = "ocr")]
            {
                eprintln!("[manual] -> handle_manual_ocr_selection with {} sels", sels.len());
                return super::ocr::handle_manual_ocr_selection(app, sels);
            }
            #[cfg(not(feature = "ocr"))]
            {
                eprintln!("[manual] ocr not available");
                app.status = "OCR not available in this build.".to_string();
                app.manual_selections = sels;
                return Task::none();
            }
        }
        ManualMode::None => Task::none(),
    }
}
