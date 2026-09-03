use iced::Task;
use iced::Rectangle;
use easyscanlate_ui::UiState;
use easyscanlate_ui::event::ManualMode;

use super::{App, Message};

pub fn handle_enter(app: &mut App, mode: ManualMode) -> Task<Message> {
    {
        let tab = app.active_tab();
        eprintln!("[manual] handle_enter mode={:?} current={:?} images={} inpainting={} running={} translating={}", mode, tab.manual_mode, tab.images.len(), tab.inpainting, tab.running, tab.translating);
    }
    if app.active_tab().images.is_empty() {
        app.active_tab_mut().status = "Open images first.".to_string();
        return Task::none();
    }
    if app.active_state().is_bulk_busy() {
        // allow switching manual modes? still block entering new one while bulk busy
        if app.active_tab().manual_mode == ManualMode::None || app.active_tab().manual_mode != mode {
            app.active_tab_mut().status = "Wait for current task to finish.".to_string();
            return Task::none();
        }
    }
    if app.active_tab().inpainting {
        app.active_tab_mut().status = "Wait for inpaint to finish.".to_string();
        return Task::none();
    }
    #[cfg(feature = "ocr")]
    if app.active_tab().manual_ocring {
        app.active_tab_mut().status = "Wait for manual OCR to finish.".to_string();
        return Task::none();
    }
    if app.active_tab().running || app.active_tab().translating {
        app.active_tab_mut().status = "Wait for current task to finish.".to_string();
        return Task::none();
    }
    if app.active_tab().manual_mode == mode {
        eprintln!("[manual] already in mode {:?}", mode);
        return Task::none();
    }
    // switching or entering
    if app.active_tab().manual_mode != ManualMode::None {
        {
            let tab = app.active_tab();
            eprintln!("[manual] switching mode {:?} -> {:?}, clearing {} selections", tab.manual_mode, mode, tab.manual_selections.len());
        }
        app.active_tab_mut().manual_selections.clear();
    } else {
        // first entry, stash view mode
        let prev_view = app.active_tab().view_mode;
        app.active_tab_mut().manual_prev_view_mode = Some(prev_view);
        app.active_tab_mut().view_mode = easyscanlate_ui::event::MainAreaMode::View;
        {
            let tab = app.active_tab();
            eprintln!("[manual] entering mode {:?}, stashed view_mode={:?}", mode, tab.manual_prev_view_mode);
        }
    }
    app.active_tab_mut().manual_mode = mode;
    crate::app::edit::clear_editing(app);
    app.active_tab_mut().selected = None;
    app.active_tab_mut().selected_inpaint = None;
    app.active_tab_mut().status = match mode {
        ManualMode::Inpaint => "Manual Inpaint: drag to select areas, then Start.".to_string(),
        ManualMode::Ocr => "Manual OCR: drag to select text areas, then Start.".to_string(),
        ManualMode::None => "Idle".to_string(),
    };
    {
        let status = app.active_tab().status.clone();
        eprintln!("[manual] entered mode={:?} status={}", mode, status);
    }
    Task::none()
}

pub fn handle_cancel(app: &mut App) -> Task<Message> {
    {
        let tab = app.active_tab();
        eprintln!("[manual] handle_cancel current={:?} selections={}", tab.manual_mode, tab.manual_selections.len());
    }
    if app.active_tab().manual_mode == ManualMode::None {
        return Task::none();
    }
    let prev = app.active_tab().manual_prev_view_mode;
    app.active_tab_mut().manual_mode = ManualMode::None;
    app.active_tab_mut().manual_selections.clear();
    if let Some(prev_val) = app.active_tab_mut().manual_prev_view_mode.take() {
        app.active_tab_mut().view_mode = prev_val;
        eprintln!("[manual] cancelled, restored view_mode={:?}", prev_val);
    } else {
        eprintln!("[manual] cancelled, no prev view_mode");
    }
    eprintln!("[manual] mode reset from {:?} selections cleared", prev);
    app.active_tab_mut().status = "Manual mode cancelled.".to_string();
    Task::none()
}

pub fn handle_reset(app: &mut App) -> Task<Message> {
    {
        let tab = app.active_tab();
        eprintln!("[manual] handle_reset mode={:?} selections={}", tab.manual_mode, tab.manual_selections.len());
    }
    if app.active_tab().manual_mode == ManualMode::None {
        return Task::none();
    }
    if app.active_tab().manual_selections.is_empty() {
        eprintln!("[manual] reset no selections -> none");
        return Task::none();
    }
    {
        let len = app.active_tab().manual_selections.len();
        eprintln!("[manual] clearing {} selections", len);
    }
    app.active_tab_mut().manual_selections.clear();
    app.active_tab_mut().status = "Selections cleared.".to_string();
    Task::none()
}

pub fn handle_selection(app: &mut App, sels: Vec<(usize, Rectangle)>) -> Task<Message> {
    {
        let tab = app.active_tab();
        eprintln!("[manual] handle_selection mode={:?} sels={} before={}", tab.manual_mode, sels.len(), tab.manual_selections.len());
    }
    for (idx, r) in &sels { eprintln!("[manual]   sel idx={} rect=[{:.1},{:.1},{:.1},{:.1}]", idx, r.x, r.y, r.width, r.height); }
    if app.active_tab().manual_mode == ManualMode::None {
        eprintln!("[manual] not in manual mode -> none");
        return Task::none();
    }
    if sels.is_empty() { return Task::none(); }
    if sels.len() == 1 {
        let (idx, rect) = sels[0];
        if idx >= app.active_tab().images.len() {
            eprintln!("[manual] idx {} out of range len={}", idx, app.active_tab().images.len());
            return Task::none();
        }
        if rect.width < 4.0 || rect.height < 4.0 {
            eprintln!("[manual] selection too small {:.1}x{:.1}", rect.width, rect.height);
            app.active_tab_mut().status = "Selection too small.".to_string();
            return Task::none();
        }
    }
    for (idx, rect) in sels {
        if idx >= app.active_tab().images.len() {
            eprintln!("[manual] span idx {} out of range", idx);
            continue;
        }
        if rect.width < 4.0 || rect.height < 4.0 {
            eprintln!("[manual] span too small {:.1}x{:.1} idx={}", rect.width, rect.height, idx);
            continue;
        }
        eprintln!("[manual]   pushing idx={} rect=[{:.1},{:.1},{:.1},{:.1}]", idx, rect.x, rect.y, rect.width, rect.height);
        app.active_tab_mut().manual_selections.push((idx, rect));
    }
    app.active_tab_mut().manual_selections.sort_by(|a, b| {
        a.0.cmp(&b.0).then(a.1.y.total_cmp(&b.1.y)).then(a.1.x.total_cmp(&b.1.x))
    });
    let before = app.active_tab().manual_selections.len();
    app.active_tab_mut().manual_selections.dedup_by(|a, b| a.0 == b.0 && a.1.x == b.1.x && a.1.y == b.1.y && a.1.width == b.1.width && a.1.height == b.1.height);
    let n = app.active_tab().manual_selections.len();
    eprintln!("[manual] added, before_dedup={} after={} selections={:?}", before, n, app.active_tab().manual_selections.iter().map(|(i,r)| (*i, [r.x, r.y, r.width, r.height])).collect::<Vec<_>>());
    app.active_tab_mut().status = format!("{n} selection(s). Press Start to run.");
    Task::none()
}



pub fn handle_start(app: &mut App) -> Task<Message> {
    {
        let tab = app.active_tab();
        eprintln!("[manual] handle_start mode={:?} selections={} inpainting={} running={} translating={}", tab.manual_mode, tab.manual_selections.len(), tab.inpainting, tab.running, tab.translating);
        for (i, (idx, r)) in tab.manual_selections.iter().enumerate() { eprintln!("[manual]   sel {}: idx={} rect=[{:.1},{:.1},{:.1},{:.1}]", i, idx, r.x, r.y, r.width, r.height); }
    }
    if app.active_tab().manual_mode == ManualMode::None {
        eprintln!("[manual] not in manual mode -> none");
        return Task::none();
    }
    if app.active_tab().manual_selections.is_empty() {
        eprintln!("[manual] no selections -> none");
        app.active_tab_mut().status = "No selections to run.".to_string();
        return Task::none();
    }
    if app.active_state().is_bulk_busy() {
        eprintln!("[manual] bulk busy {:?}", app.active_state().is_bulk_busy());
        app.active_tab_mut().status = "Wait for current task to finish.".to_string();
        return Task::none();
    }
    if app.active_tab().inpainting {
        eprintln!("[manual] inpainting busy");
        app.active_tab_mut().status = "Wait for inpaint to finish.".to_string();
        return Task::none();
    }
    #[cfg(feature = "ocr")]
    if app.active_tab().manual_ocring {
        eprintln!("[manual] manual_ocring busy");
        app.active_tab_mut().status = "Wait for manual OCR to finish.".to_string();
        return Task::none();
    }
    if app.active_tab().running || app.active_tab().translating {
        eprintln!("[manual] running/translating busy");
        app.active_tab_mut().status = "Wait for current task to finish.".to_string();
        return Task::none();
    }
    let sels = std::mem::take(&mut app.active_tab_mut().manual_selections);
    eprintln!("[manual] taking {} sels, mode={:?} selections cleared", sels.len(), app.active_tab().manual_mode);
    // auto clear as per spec, mode stays active
    let mode = app.active_tab().manual_mode;
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
                app.active_tab_mut().status = "Inpaint not available in this build.".to_string();
                app.active_tab_mut().manual_selections = sels;
                return Task::none();
            }
        }
        ManualMode::Ocr => {
            #[cfg(feature = "ocr")]
            {
                eprintln!("[manual] -> handle_manual_ocr_selection with {} sels", sels.len());
                let tid = app.active_tab().id;
                return super::ocr::handle_manual_ocr_selection(app, tid, sels);
            }
            #[cfg(not(feature = "ocr"))]
            {
                eprintln!("[manual] ocr not available");
                app.active_tab_mut().status = "OCR not available in this build.".to_string();
                app.active_tab_mut().manual_selections = sels;
                return Task::none();
            }
        }
        ManualMode::None => Task::none(),
    }
}
