use iced::{Color, Font, Rectangle};
use iced::widget::text_editor;
use easyscanlate_model::{EntryId, EntryStyle, TextAlign, TextGradientDir};
use easyscanlate_ui::color::rgba_to_color;
use easyscanlate_ui::event::{EditOrigin, MainAreaMode, ManualMode, SettingsTab, StyleField, TargetProfileSelection, TranslationPanelMode};
use easyscanlate_ui::layout::{PaneKind, SidePaneKind, StylingPaneKind};
use easyscanlate_ui::{ConnectModal, LoadedImage, UiState};
use easyscanlate_ui::state::TabMeta;

use easyscanlate_model::ModelEvent;

use super::App;
use super::tab::Tab;

/// Stage weights for the unified Start OCR progress: OCR x5, SFX segment x2,
/// style classify x1, auto-inpaint x3. Normalized over the enabled stages.
const OCR_W: f32 = 5.0;
const SEG_W: f32 = 2.0;
const STYLE_W: f32 = 1.0;
const INPAINT_W: f32 = 3.0;

pub(crate) fn pipeline_progress_for_tab(tab: &Tab) -> Option<f32> {
    // Pipeline-originated busy only (exclude manual translate/inpaint and
    // manual single style detect so the Start button stays plain for those).
    let ocr_busy = tab.running;
    #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
    let chain_busy = tab.pipeline_active;
    #[cfg(not(all(feature = "styling", feature = "inpaint", feature = "segment")))]
    let chain_busy = {
        #[cfg(all(feature = "styling", feature = "inpaint"))]
        {
            tab.pipeline_style_pending > 0
        }
        #[cfg(not(all(feature = "styling", feature = "inpaint")))]
        {
            false
        }
    };
    #[cfg(feature = "segment")]
    let seg_busy = tab.segment_filtering;
    #[cfg(not(feature = "segment"))]
    let seg_busy = false;
    #[cfg(all(feature = "styling", feature = "inpaint"))]
    let style_pending_busy =
        tab.pipeline_style_pending > 0 || !tab.pipeline_style_results.is_empty();
    #[cfg(not(all(feature = "styling", feature = "inpaint")))]
    let style_pending_busy = false;
    #[cfg(feature = "styling")]
    let style_building = tab.styling.is_building();
    #[cfg(not(feature = "styling"))]
    let style_building = false;
    // Only count a styling build as pipeline progress when it belongs to the
    // chain (raw OCR running, chain active, or deferred style jobs exist).
    // A manual single AutoDetect otherwise leaves the button plain.
    let style_busy = style_pending_busy || (style_building && (ocr_busy || chain_busy || style_pending_busy));
    #[cfg(feature = "inpaint")]
    let inpaint_busy = tab.auto_inpaint_pending > 0
        || tab.pending_auto_telea_jobs.is_some()
        || tab.pending_auto_lama_jobs.is_some()
        || tab.pending_auto_aot_jobs.is_some();
    #[cfg(not(feature = "inpaint"))]
    let inpaint_busy = false;

    if !(ocr_busy || chain_busy || seg_busy || style_busy || inpaint_busy) {
        return None;
    }

    let (do_sfx, do_style, do_inpaint) = easyscanlate_settings::get(|s| {
        (s.auto_sfx_filter, s.auto_style_detect, s.auto_inpaint)
    });
    #[cfg(not(feature = "segment"))]
    let do_sfx = { let _ = &do_sfx; false };
    #[cfg(not(feature = "styling"))]
    let do_style = { let _ = &do_style; false };
    #[cfg(not(feature = "inpaint"))]
    let do_inpaint = { let _ = &do_inpaint; false };

    #[cfg(feature = "ocr")]
    let ocr_enabled = tab.ocr_runs > 0 || tab.running;
    #[cfg(not(feature = "ocr"))]
    let ocr_enabled = false;

    let mut weighted = 0.0f32;
    let mut divisor = 0.0f32;

    // OCR fraction from run counts.
    let ocr_frac = {
        #[cfg(feature = "ocr")]
        {
            if tab.ocr_runs == 0 {
                0.0
            } else {
                let done = tab.ocr_runs.saturating_sub(tab.pending) as f32;
                (done / tab.ocr_runs as f32).clamp(0.0, 1.0)
            }
        }
        #[cfg(not(feature = "ocr"))]
        {
            0.0
        }
    };
    if ocr_enabled {
        weighted += OCR_W * ocr_frac;
        divisor += OCR_W;
    }

    // SFX segment: granular while streaming (done/total grids), else 0/1 step.
    if do_sfx {
        #[cfg(feature = "segment")]
        {
            let frac = if tab.segment_total > 0 {
                let done = tab.segment_total.saturating_sub(tab.segment_pending) as f32;
                (done / tab.segment_total as f32).clamp(0.0, 1.0)
            } else if tab.segment_filtering || ocr_frac < 1.0 {
                0.0
            } else if tab.pipeline_seg_done {
                1.0
            } else {
                0.0
            };
            weighted += SEG_W * frac;
            divisor += SEG_W;
        }
    }

    // Style classify: incremental while deferred, else 0/1 step.
    if do_style {
        #[cfg(feature = "styling")]
        {
            #[cfg(all(feature = "styling", feature = "inpaint"))]
            let deferred_active = tab.pipeline_style_pending > 0
                || !tab.pipeline_style_results.is_empty();
            #[cfg(not(all(feature = "styling", feature = "inpaint")))]
            let deferred_active = false;
            let frac = if deferred_active {
                #[cfg(all(feature = "styling", feature = "inpaint"))]
                {
                    let total =
                        tab.pipeline_style_pending + tab.pipeline_style_results.len();
                    if total == 0 {
                        1.0
                    } else {
                        tab.pipeline_style_results.len() as f32 / total as f32
                    }
                }
                #[cfg(not(all(feature = "styling", feature = "inpaint")))]
                {
                    0.0
                }
            } else if tab.styling.is_building() || ocr_frac < 1.0 {
                0.0
            } else {
                1.0
            };
            weighted += STYLE_W * frac.clamp(0.0, 1.0);
            divisor += STYLE_W;
        }
    }

    // Auto-inpaint: incremental from total vs remaining.
    if do_inpaint {
        #[cfg(feature = "inpaint")]
        {
            let queued = tab.pending_auto_telea_jobs.is_some()
                || tab.pending_auto_lama_jobs.is_some()
                || tab.pending_auto_aot_jobs.is_some();
            let frac = if tab.auto_inpaint_pending > 0 || tab.auto_inpaint_total > 0 || queued
            {
                if tab.auto_inpaint_total == 0 {
                    0.0
                } else {
                    let done = tab
                        .auto_inpaint_total
                        .saturating_sub(tab.auto_inpaint_pending)
                        as f32;
                    (done / tab.auto_inpaint_total as f32).clamp(0.0, 1.0)
                }
            } else if ocr_frac < 1.0 {
                0.0
            } else {
                1.0
            };
            weighted += INPAINT_W * frac;
            divisor += INPAINT_W;
        }
    }

    if divisor <= 0.0 {
        return None;
    }
    Some((weighted / divisor).clamp(0.0, 1.0))
}

pub(crate) struct ActiveTab<'a> {
    pub app: &'a App,
    pub tab: &'a Tab,
}

impl UiState for ActiveTab<'_> {
    fn images(&self) -> &[LoadedImage] {
        &self.tab.images
    }

    fn project(&self) -> &easyscanlate_model::Project {
        &self.tab.project
    }

    fn running(&self) -> bool {
        self.tab.running
    }

    fn translating(&self) -> bool {
        self.tab.translating
    }

    fn status(&self) -> &str {
        &self.tab.status
    }

    fn translate_model_groups(&self) -> &[(String, String, Vec<(String, String)>)] {
        self.app.tx.model_groups()
    }

    fn translate_model_selection(&self) -> (String, String) {
        (self.app.tx.selected_id.clone(), self.app.tx.selected_model.clone())
    }

    fn translate_lang(&self) -> &str {
        &self.tab.translate_lang
    }

    fn connect_modal(&self) -> Option<&ConnectModal> {
        self.app.connect_modal.as_ref()
    }

    fn selected(&self) -> Option<(usize, EntryId)> {
        self.tab.selected
    }

    fn selected_inpaint(&self) -> Option<(usize, usize)> {
        self.tab.selected_inpaint
    }

    fn style_working(&self) -> &EntryStyle {
        &self.tab.style_working
    }

    fn style_text_color(&self) -> Color {
        rgba_to_color(self.tab.style_working.text_color)
    }

    fn style_stroke_color(&self) -> Color {
        rgba_to_color(self.tab.style_working.stroke_color)
    }

    fn style_bg_color(&self) -> Color {
        rgba_to_color(self.tab.style_working.bg_color)
    }

    fn style_picker_open(&self) -> Option<StyleField> {
        self.tab.style_picker
    }

    fn style_stroke_width(&self) -> &str {
        &self.tab.style_stroke_width
    }

    fn style_bg_radius(&self) -> &str {
        &self.tab.style_bg_radius
    }

    fn style_presets(&self) -> &[Option<EntryStyle>] {
        self.app.presets.as_slice()
    }

    fn installed_fonts(&self) -> &[String] {
        &self.app.installed_fonts
    }

    fn style_font_family(&self) -> Option<&str> {
        self.tab.style_working.font_family.as_deref()
    }

    fn style_text_align(&self) -> TextAlign {
        self.tab.style_working.text_align
    }

    fn style_gradient_a(&self) -> Color {
        rgba_to_color(self.tab.style_working.gradient_a)
    }

    fn style_gradient_b(&self) -> Color {
        rgba_to_color(self.tab.style_working.gradient_b)
    }

    fn style_gradient_dir(&self) -> TextGradientDir {
        self.tab.style_working.gradient_dir
    }

    fn style_hex_override(&self, field: StyleField) -> Option<&str> {
        self.tab.style_hex_overrides.get(&field).map(|s| s.as_str())
    }

    fn editing(&self) -> Option<(usize, EntryId)> {
        self.tab.editing
    }

    fn editing_origin(&self) -> EditOrigin {
        self.tab.editing_origin
    }

    fn editing_rect(&self) -> Option<Rectangle> {
        self.tab.editing_rect
    }

    fn edit_content(&self) -> Option<&text_editor::Content> {
        self.tab.edit_content.as_ref()
    }

    fn font(&self) -> Option<Font> {
        self.app.font
    }

    fn inpaint_mode(&self) -> bool {
        self.tab.manual_mode == ManualMode::Inpaint
    }

    fn ocr_mode(&self) -> bool {
        self.tab.manual_mode == ManualMode::Ocr
    }

    fn manual_mode(&self) -> ManualMode {
        self.tab.manual_mode
    }

    fn manual_selections(&self) -> &[(usize, Rectangle)] {
        &self.tab.manual_selections
    }

    fn is_inpainting(&self) -> bool {
        self.tab.inpainting
    }

    fn is_manual_ocring(&self) -> bool {
        #[cfg(feature = "ocr")]
        { self.tab.manual_ocring }
        #[cfg(not(feature = "ocr"))]
        { false }
    }

    fn is_pipeline_running(&self) -> bool {
        #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
        { self.tab.pipeline_active }
        #[cfg(not(all(feature = "styling", feature = "inpaint", feature = "segment")))]
        {
            #[cfg(all(feature = "styling", feature = "inpaint"))]
            { self.tab.pipeline_style_pending > 0 }
            #[cfg(not(all(feature = "styling", feature = "inpaint")))]
            { false }
        }
    }

    fn is_auto_inpainting(&self) -> bool {
        #[cfg(feature = "inpaint")]
        { self.tab.auto_inpaint_pending > 0 || self.tab.auto_inpaint_loading }
        #[cfg(not(feature = "inpaint"))]
        { false }
    }

    fn is_segment_filtering(&self) -> bool {
        #[cfg(feature = "segment")]
        { self.tab.segment_filtering }
        #[cfg(not(feature = "segment"))]
        { false }
    }

    fn is_styling_busy(&self) -> bool {
        #[cfg(feature = "styling")]
        {
            if self.tab.pipeline_style_pending > 0 { return true; }
            if self.tab.styling.is_building() { return true; }
            false
        }
        #[cfg(not(feature = "styling"))]
        { false }
    }

    fn pipeline_progress(&self) -> Option<f32> {
        pipeline_progress_for_tab(self.tab)
    }

    fn is_bulk_busy(&self) -> bool {
        self.tab.running
            || self.tab.translating
            || self.tab.inpainting
            || self.is_manual_ocring()
            || self.is_pipeline_running()
            || self.is_auto_inpainting()
            || self.is_segment_filtering()
            || self.is_styling_busy()
    }

    fn show_overlay_text(&self) -> bool {
        self.tab.show_overlay_text
    }

    fn show_inpaint(&self) -> bool {
        self.tab.show_inpaint
    }

    fn view_mode(&self) -> MainAreaMode {
        self.tab.view_mode
    }

    fn viewer_scroll(&self) -> f32 {
        self.tab.viewer_scroll
    }

    fn settings_open(&self) -> bool {
        self.app.settings_open
    }

    fn settings_tab(&self) -> SettingsTab {
        self.app.settings_tab
    }

    fn settings_search(&self) -> &str {
        &self.app.settings_search
    }

    fn manage_models_open(&self) -> bool {
        self.app.manage_models_open
    }

    fn manage_models_search(&self) -> &str {
        &self.app.manage_models_search
    }

    fn backdrop_blur(&self) -> Option<iced::widget::image::Handle> {
        self.app.backdrop_blur.clone()
    }

    fn all_model_groups(&self) -> Vec<(String, String, Vec<(String, String)>)> {
        self.app.tx.all_model_groups()
    }

    fn translation_panel_mode(&self) -> TranslationPanelMode {
        self.tab.translation_panel_mode
    }

    fn base_profile(&self) -> Option<easyscanlate_model::ProfileId> {
        if let Some(id) = self.tab.translate_base
            && self.tab.project.profiles.iter().any(|p| p.id == id) {
                return Some(id);
            }
        if self.tab.images.is_empty() {
            return None;
        }
        Some(self.tab.project.profiles.selected_id())
    }

    fn target_profile(&self) -> TargetProfileSelection {
        if let TargetProfileSelection::AutoPlaceholder(name) = &self.tab.translate_target
            && let Some(id) = self.tab.project.profiles.find_by_name(name) {
                let base = self.base_profile();
                if Some(id) != base {
                    return TargetProfileSelection::Existing(id);
                }
            }
        self.tab.translate_target.clone()
    }

    fn target_placeholder_name(&self) -> String {
        format!("{}(auto)", self.tab.translate_lang)
    }

    fn app_view(&self) -> easyscanlate_ui::state::AppView {
        if self.app.onboarding.is_some() {
            return easyscanlate_ui::state::AppView::Onboarding;
        }
        if self.tab.is_home() {
            easyscanlate_ui::state::AppView::Home
        } else {
            easyscanlate_ui::state::AppView::Editor
        }
    }

    fn recent_projects(&self) -> &[easyscanlate_settings::RecentProject] {
        &self.app.recent_projects
    }

    fn new_project_overlay(&self) -> Option<easyscanlate_ui::state::NewProjectOverlay> {
        self.app.new_project.as_ref().map(|np| easyscanlate_ui::state::NewProjectOverlay {
            source_paths: np.source_files.iter().map(|(p, _, _)| p.clone()).collect(),
            original_lang: np.original_lang.clone(),
            project_location: np.project_location.clone(),
        })
    }

    fn translation_anim_phase(&self) -> f32 {
        self.tab.translate_anim_phase
    }

    fn is_loading(&self) -> bool {
        self.tab.loading
    }

    fn loading_phase(&self) -> f32 {
        self.tab.loading_phase
    }

    fn is_exporting(&self) -> bool {
        self.tab.exporting
    }

    fn export_progress(&self) -> Option<(usize, usize, usize)> {
        if !self.tab.exporting {
            return None;
        }
        Some((self.tab.export_done, self.tab.export_total, self.tab.export_failed))
    }

    fn export_folder(&self) -> Option<String> {
        self.tab.export_folder.as_ref().map(|p| p.display().to_string())
    }

    fn loading_title(&self) -> String {
        self.tab.title.clone()
    }

    fn update_current_version(&self) -> String {
        crate::updater::get_current_version()
    }
    #[cfg(feature = "updates")]
    fn update_available_version(&self) -> Option<String> {
        self.app.update_info.as_ref().map(|i| i.TargetFullRelease.Version.to_string())
    }
    #[cfg(not(feature = "updates"))]
    fn update_available_version(&self) -> Option<String> {
        None
    }
    fn update_downloading(&self) -> bool {
        self.app.update_downloading
    }
    fn update_progress(&self) -> i16 {
        self.app.update_progress
    }
    fn update_ready(&self) -> bool {
        self.app.update_ready
    }
    #[cfg(feature = "updates")]
    fn update_notes(&self) -> Option<String> {
        self.app.update_info.as_ref().and_then(|i| {
            let n = i.TargetFullRelease.NotesMarkdown.clone();
            if n.trim().is_empty() { None } else { Some(n) }
        })
    }
    #[cfg(not(feature = "updates"))]
    fn update_notes(&self) -> Option<String> {
        None
    }
    fn update_popup_visible(&self) -> bool { self.app.update_popup_visible }
    fn update_blur(&self) -> Option<iced::widget::image::Handle> { self.app.update_blur.clone() }

    fn onboarding_open(&self) -> bool { self.app.onboarding.is_some() }
    fn onboarding_step(&self) -> u8 { self.app.onboarding.as_ref().map(|o| o.step).unwrap_or(0) }
    fn onboarding_models(&self) -> Vec<(String, String, easyscanlate_ui::state::ModelDownloadStatus)> {
        self.app.onboarding.as_ref().map(|o| o.models.clone()).unwrap_or_default()
    }
    fn onboarding_overall_progress(&self) -> f32 { self.app.onboarding.as_ref().map(|o| o.overall_progress()).unwrap_or(0.0) }
    fn onboarding_downloading(&self) -> bool { self.app.onboarding.as_ref().map(|o| o.downloading).unwrap_or(false) }
    fn onboarding_all_done(&self) -> bool { self.app.onboarding.as_ref().map(|o| o.is_all_done()).unwrap_or(true) }

    fn tab_metas(&self) -> Vec<TabMeta> {
        self.app.tabs.iter().enumerate().map(|(idx, t)| TabMeta {
            id: t.id.0,
            title: t.title.clone(),
            dirty: t.dirty,
            is_home: t.is_home() && idx == 0,
        }).collect()
    }
    fn active_tab_id(&self) -> u64 { self.app.tabs.get(self.app.active).map(|t| t.id.0).unwrap_or(0) }
    fn pending_close(&self) -> Option<TabMeta> {
        let id = self.app.pending_close?;
        let tab = self.app.tab_by_id(id).or_else(|| self.app.tabs.get(self.app.active))?;
        Some(TabMeta { id: id.0, title: tab.title.clone(), dirty: tab.dirty, is_home: false })
    }
    fn titlebar_height(&self) -> f32 { self.app.frame.config().title_bar_height }
    fn editor_panes(&self) -> Option<(&iced::widget::pane_grid::State<PaneKind>, &iced::widget::pane_grid::State<SidePaneKind>, &iced::widget::pane_grid::State<StylingPaneKind>)> {
        if self.tab.is_home() { return None; }
        Some((&self.tab.panes, &self.tab.side_panes, &self.tab.styling_panes))
    }
}

// Shim kept for view lifetime: `view.rs` returns `Element<'a, Message>` tied to
// `&'a App`. `ActiveTab<'a>` is a temporary containing `&'a App` + `&'a Tab`;
// borrowing it would tie the `Element` to the temporary's stack frame (E0515).
// Keeping `UiState for App` forwarding to `tabs[active]` lets `view` pass `app`
// directly and keep the `Element` tied to `app` (the caller). `ActiveTab` remains
// the canonical impl for tests and non-view call sites (`app.active_state()`).
impl UiState for App {
    fn images(&self) -> &[LoadedImage] { &self.tabs[self.active].images }
    fn project(&self) -> &easyscanlate_model::Project { &self.tabs[self.active].project }
    fn running(&self) -> bool { self.tabs[self.active].running }
    fn translating(&self) -> bool { self.tabs[self.active].translating }
    fn status(&self) -> &str { &self.tabs[self.active].status }
    fn translate_model_groups(&self) -> &[(String, String, Vec<(String, String)>)] { self.tx.model_groups() }
    fn translate_model_selection(&self) -> (String, String) { (self.tx.selected_id.clone(), self.tx.selected_model.clone()) }
    fn translate_lang(&self) -> &str { &self.tabs[self.active].translate_lang }
    fn connect_modal(&self) -> Option<&ConnectModal> { self.connect_modal.as_ref() }
    fn selected(&self) -> Option<(usize, EntryId)> { self.tabs[self.active].selected }
    fn selected_inpaint(&self) -> Option<(usize, usize)> { self.tabs[self.active].selected_inpaint }
    fn style_working(&self) -> &EntryStyle { &self.tabs[self.active].style_working }
    fn style_text_color(&self) -> Color { rgba_to_color(self.tabs[self.active].style_working.text_color) }
    fn style_stroke_color(&self) -> Color { rgba_to_color(self.tabs[self.active].style_working.stroke_color) }
    fn style_bg_color(&self) -> Color { rgba_to_color(self.tabs[self.active].style_working.bg_color) }
    fn style_picker_open(&self) -> Option<StyleField> { self.tabs[self.active].style_picker }
    fn style_stroke_width(&self) -> &str { &self.tabs[self.active].style_stroke_width }
    fn style_bg_radius(&self) -> &str { &self.tabs[self.active].style_bg_radius }
    fn style_presets(&self) -> &[Option<EntryStyle>] { self.presets.as_slice() }
    fn installed_fonts(&self) -> &[String] { &self.installed_fonts }
    fn style_font_family(&self) -> Option<&str> { self.tabs[self.active].style_working.font_family.as_deref() }
    fn style_text_align(&self) -> TextAlign { self.tabs[self.active].style_working.text_align }
    fn style_gradient_a(&self) -> Color { rgba_to_color(self.tabs[self.active].style_working.gradient_a) }
    fn style_gradient_b(&self) -> Color { rgba_to_color(self.tabs[self.active].style_working.gradient_b) }
    fn style_gradient_dir(&self) -> TextGradientDir { self.tabs[self.active].style_working.gradient_dir }
    fn style_hex_override(&self, field: StyleField) -> Option<&str> { self.tabs[self.active].style_hex_overrides.get(&field).map(|s| s.as_str()) }
    fn editing(&self) -> Option<(usize, EntryId)> { self.tabs[self.active].editing }
    fn editing_origin(&self) -> EditOrigin { self.tabs[self.active].editing_origin }
    fn editing_rect(&self) -> Option<Rectangle> { self.tabs[self.active].editing_rect }
    fn edit_content(&self) -> Option<&text_editor::Content> { self.tabs[self.active].edit_content.as_ref() }
    fn font(&self) -> Option<Font> { self.font }
    fn inpaint_mode(&self) -> bool { self.tabs[self.active].manual_mode == ManualMode::Inpaint }
    fn ocr_mode(&self) -> bool { self.tabs[self.active].manual_mode == ManualMode::Ocr }
    fn manual_mode(&self) -> ManualMode { self.tabs[self.active].manual_mode }
    fn manual_selections(&self) -> &[(usize, Rectangle)] { &self.tabs[self.active].manual_selections }
    fn is_inpainting(&self) -> bool { self.tabs[self.active].inpainting }
    fn is_manual_ocring(&self) -> bool {
        #[cfg(feature = "ocr")]
        { self.tabs[self.active].manual_ocring }
        #[cfg(not(feature = "ocr"))]
        { false }
    }
    fn is_pipeline_running(&self) -> bool {
        #[cfg(all(feature = "styling", feature = "inpaint", feature = "segment"))]
        { self.tabs[self.active].pipeline_active }
        #[cfg(not(all(feature = "styling", feature = "inpaint", feature = "segment")))]
        {
            #[cfg(all(feature = "styling", feature = "inpaint"))]
            { self.tabs[self.active].pipeline_style_pending > 0 }
            #[cfg(not(all(feature = "styling", feature = "inpaint")))]
            { false }
        }
    }
    fn is_auto_inpainting(&self) -> bool {
        #[cfg(feature = "inpaint")]
        { self.tabs[self.active].auto_inpaint_pending > 0 || self.tabs[self.active].auto_inpaint_loading }
        #[cfg(not(feature = "inpaint"))]
        { false }
    }
    fn is_segment_filtering(&self) -> bool {
        #[cfg(feature = "segment")]
        { self.tabs[self.active].segment_filtering }
        #[cfg(not(feature = "segment"))]
        { false }
    }
    fn is_styling_busy(&self) -> bool {
        #[cfg(feature = "styling")]
        {
            if self.tabs[self.active].pipeline_style_pending > 0 { return true; }
            if self.tabs[self.active].styling.is_building() { return true; }
            false
        }
        #[cfg(not(feature = "styling"))]
        { false }
    }
    fn pipeline_progress(&self) -> Option<f32> {
        self.tabs.get(self.active).and_then(pipeline_progress_for_tab)
    }
    fn is_bulk_busy(&self) -> bool {
        self.tabs[self.active].running
            || self.tabs[self.active].translating
            || self.tabs[self.active].inpainting
            || self.is_manual_ocring()
            || self.is_pipeline_running()
            || self.is_auto_inpainting()
            || self.is_segment_filtering()
            || self.is_styling_busy()
    }
    fn show_overlay_text(&self) -> bool { self.tabs[self.active].show_overlay_text }
    fn show_inpaint(&self) -> bool { self.tabs[self.active].show_inpaint }
    fn view_mode(&self) -> MainAreaMode { self.tabs[self.active].view_mode }
    fn viewer_scroll(&self) -> f32 { self.tabs[self.active].viewer_scroll }
    fn settings_open(&self) -> bool { self.settings_open }
    fn settings_tab(&self) -> SettingsTab { self.settings_tab }
    fn settings_search(&self) -> &str { &self.settings_search }
    fn manage_models_open(&self) -> bool { self.manage_models_open }
    fn manage_models_search(&self) -> &str { &self.manage_models_search }
    fn backdrop_blur(&self) -> Option<iced::widget::image::Handle> { self.backdrop_blur.clone() }
    fn all_model_groups(&self) -> Vec<(String, String, Vec<(String, String)>)> { self.tx.all_model_groups() }
    fn translation_panel_mode(&self) -> TranslationPanelMode { self.tabs[self.active].translation_panel_mode }
    fn base_profile(&self) -> Option<easyscanlate_model::ProfileId> {
        let tab = &self.tabs[self.active];
        if let Some(id) = tab.translate_base
            && tab.project.profiles.iter().any(|p| p.id == id) {
                return Some(id);
            }
        if tab.images.is_empty() {
            return None;
        }
        Some(tab.project.profiles.selected_id())
    }
    fn target_profile(&self) -> TargetProfileSelection {
        let tab = &self.tabs[self.active];
        if let TargetProfileSelection::AutoPlaceholder(name) = &tab.translate_target
            && let Some(id) = tab.project.profiles.find_by_name(name) {
                let base = self.base_profile();
                if Some(id) != base {
                    return TargetProfileSelection::Existing(id);
                }
            }
        tab.translate_target.clone()
    }
    fn target_placeholder_name(&self) -> String { format!("{}(auto)", self.tabs[self.active].translate_lang) }
    fn app_view(&self) -> easyscanlate_ui::state::AppView {
        if self.onboarding.is_some() {
            return easyscanlate_ui::state::AppView::Onboarding;
        }
        if self.tabs[self.active].is_home() {
            easyscanlate_ui::state::AppView::Home
        } else {
            easyscanlate_ui::state::AppView::Editor
        }
    }
    fn recent_projects(&self) -> &[easyscanlate_settings::RecentProject] { &self.recent_projects }
    fn new_project_overlay(&self) -> Option<easyscanlate_ui::state::NewProjectOverlay> {
        self.new_project.as_ref().map(|np| easyscanlate_ui::state::NewProjectOverlay {
            source_paths: np.source_files.iter().map(|(p, _, _)| p.clone()).collect(),
            original_lang: np.original_lang.clone(),
            project_location: np.project_location.clone(),
        })
    }
    fn translation_anim_phase(&self) -> f32 { self.tabs[self.active].translate_anim_phase }
    fn is_loading(&self) -> bool { self.tabs[self.active].loading }
    fn loading_phase(&self) -> f32 { self.tabs[self.active].loading_phase }
    fn is_exporting(&self) -> bool { self.tabs.get(self.active).is_some_and(|t| t.exporting) }
    fn export_progress(&self) -> Option<(usize, usize, usize)> {
        let t = self.tabs.get(self.active)?;
        if !t.exporting { return None; }
        Some((t.export_done, t.export_total, t.export_failed))
    }
    fn export_folder(&self) -> Option<String> {
        self.tabs.get(self.active)?.export_folder.as_ref().map(|p| p.display().to_string())
    }
    fn loading_title(&self) -> String { self.tabs[self.active].title.clone() }
    fn update_current_version(&self) -> String { crate::updater::get_current_version() }
    #[cfg(feature = "updates")]
    fn update_available_version(&self) -> Option<String> {
        self.update_info.as_ref().map(|i| i.TargetFullRelease.Version.to_string())
    }
    #[cfg(not(feature = "updates"))]
    fn update_available_version(&self) -> Option<String> {
        let _ = &self.update_info;
        None
    }
    fn update_downloading(&self) -> bool { self.update_downloading }
    fn update_progress(&self) -> i16 { self.update_progress }
    fn update_ready(&self) -> bool { self.update_ready }
    #[cfg(feature = "updates")]
    fn update_notes(&self) -> Option<String> {
        self.update_info.as_ref().and_then(|i| {
            let n = i.TargetFullRelease.NotesMarkdown.clone();
            if n.trim().is_empty() { None } else { Some(n) }
        })
    }
    #[cfg(not(feature = "updates"))]
    fn update_notes(&self) -> Option<String> {
        let _ = &self.update_info;
        None
    }
    fn update_popup_visible(&self) -> bool { self.update_popup_visible }
    fn update_blur(&self) -> Option<iced::widget::image::Handle> { self.update_blur.clone() }
    fn onboarding_open(&self) -> bool { self.onboarding.is_some() }
    fn onboarding_step(&self) -> u8 { self.onboarding.as_ref().map(|o| o.step).unwrap_or(0) }
    fn onboarding_models(&self) -> Vec<(String, String, easyscanlate_ui::state::ModelDownloadStatus)> { self.onboarding.as_ref().map(|o| o.models.clone()).unwrap_or_default() }
    fn onboarding_overall_progress(&self) -> f32 { self.onboarding.as_ref().map(|o| o.overall_progress()).unwrap_or(0.0) }
    fn onboarding_downloading(&self) -> bool { self.onboarding.as_ref().map(|o| o.downloading).unwrap_or(false) }
    fn onboarding_all_done(&self) -> bool { self.onboarding.as_ref().map(|o| o.is_all_done()).unwrap_or(true) }
    fn tab_metas(&self) -> Vec<TabMeta> {
        self.tabs.iter().enumerate().map(|(idx, t)| TabMeta {
            id: t.id.0,
            title: t.title.clone(),
            dirty: t.dirty,
            is_home: t.is_home() && idx == 0,
        }).collect()
    }
    fn active_tab_id(&self) -> u64 { self.tabs.get(self.active).map(|t| t.id.0).unwrap_or(0) }
    fn pending_close(&self) -> Option<TabMeta> {
        let id = self.pending_close?;
        let tab = self.tab_by_id(id).or_else(|| self.tabs.get(self.active))?;
        Some(TabMeta { id: id.0, title: tab.title.clone(), dirty: tab.dirty, is_home: false })
    }
    fn titlebar_height(&self) -> f32 { self.frame.config().title_bar_height }
    fn editor_panes(&self) -> Option<(&iced::widget::pane_grid::State<PaneKind>, &iced::widget::pane_grid::State<SidePaneKind>, &iced::widget::pane_grid::State<StylingPaneKind>)> {
        let tab = self.tabs.get(self.active)?;
        if tab.is_home() { return None; }
        Some((&tab.panes, &tab.side_panes, &tab.styling_panes))
    }
}

pub(crate) fn handle_model_event(tab: &mut Tab, event: ModelEvent) {
    match &event {
        ModelEvent::EntryDeleted { .. }
        | ModelEvent::EntriesReordered { .. }
        | ModelEvent::EntryMoved { .. }
        | ModelEvent::EntriesAdded { .. }
        | ModelEvent::ImageAdded { .. }
        | ModelEvent::EntryTextUpdated { .. }
        | ModelEvent::EntryStyleUpdated { .. }
        | ModelEvent::ProfileCreated { .. }
        | ModelEvent::ProfileRemoved { .. }
        | ModelEvent::ProfileSelected { .. }
        | ModelEvent::ProfileRenamed { .. }
        | ModelEvent::InpaintAdded { .. }
        | ModelEvent::InpaintRemoved { .. }
        | ModelEvent::NoteUpdated { .. }
        | ModelEvent::EntryRestored { .. } => {
            tab.dirty = true;
        }
    }
    match event {
        ModelEvent::EntryDeleted { id } => {
            if tab.selected.is_some_and(|(_, sel_id)| sel_id == id) {
                tab.selected = None;
                crate::app::edit::clear_editing_tab(tab);
            }
            if tab.editing.is_some_and(|(_, eid)| eid == id) {
                crate::app::edit::clear_editing_tab(tab);
            }
        }
        ModelEvent::EntryRestored { .. } => {}
        ModelEvent::EntriesReordered { .. } => {}
        ModelEvent::EntryMoved { .. } => {}
        ModelEvent::EntriesAdded { .. } => {
            debug_assert!(tab.images.len() == tab.project.image_count());
        }
        ModelEvent::ImageAdded { .. } => {
            debug_assert!(tab.images.len() == tab.project.image_count());
        }
        ModelEvent::EntryTextUpdated { .. } => {}
        ModelEvent::EntryStyleUpdated { .. } => {}
        ModelEvent::ProfileCreated { .. }
        | ModelEvent::ProfileRemoved { .. }
        | ModelEvent::ProfileSelected { .. }
        | ModelEvent::ProfileRenamed { .. } => {}
        ModelEvent::InpaintAdded { .. } | ModelEvent::InpaintRemoved { .. } => {}
        ModelEvent::NoteUpdated { .. } => {}
    }
}


