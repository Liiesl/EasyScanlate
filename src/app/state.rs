use iced::{Color, Font, Rectangle};
use iced::widget::text_editor;
use scanlateit_model::{EntryId, EntryStyle, TextAlign, TextGradientDir};
use scanlateit_ui::color::rgba_to_color;
use scanlateit_ui::event::{EditOrigin, MainAreaMode, SettingsTab, StyleField};
use scanlateit_ui::{ConnectModal, LoadedImage, UiState};

use super::App;

impl UiState for App {
    fn images(&self) -> &[LoadedImage] {
        &self.images
    }

    fn running(&self) -> bool {
        self.running
    }

    fn translating(&self) -> bool {
        self.translating
    }

    fn status(&self) -> &str {
        &self.status
    }

    fn translate_model_groups(&self) -> &[(String, String, Vec<(String, String)>)] {
        self.tx.model_groups()
    }

    fn translate_model_selection(&self) -> (String, String) {
        (self.tx.selected_id.clone(), self.tx.selected_model.clone())
    }

    fn translate_lang(&self) -> &str {
        &self.translate_lang
    }

    fn connect_modal(&self) -> Option<&ConnectModal> {
        self.connect_modal.as_ref()
    }

    fn selected(&self) -> Option<(usize, EntryId)> {
        self.selected
    }

    fn selected_inpaint(&self) -> Option<(usize, usize)> {
        self.selected_inpaint
    }

    fn style_working(&self) -> &EntryStyle {
        &self.style_working
    }

    fn style_text_color(&self) -> Color {
        rgba_to_color(self.style_working.text_color)
    }

    fn style_stroke_color(&self) -> Color {
        rgba_to_color(self.style_working.stroke_color)
    }

    fn style_bg_color(&self) -> Color {
        rgba_to_color(self.style_working.bg_color)
    }

    fn style_picker_open(&self) -> Option<StyleField> {
        self.style_picker
    }

    fn style_stroke_width(&self) -> &str {
        &self.style_stroke_width
    }

    fn style_bg_radius(&self) -> &str {
        &self.style_bg_radius
    }

    fn style_presets(&self) -> &[Option<EntryStyle>] {
        self.presets.as_slice()
    }

    fn installed_fonts(&self) -> &[String] {
        &self.installed_fonts
    }

    fn style_font_family(&self) -> Option<&str> {
        self.style_working.font_family.as_deref()
    }

    fn style_text_align(&self) -> TextAlign {
        self.style_working.text_align
    }

    fn style_gradient_a(&self) -> Color {
        rgba_to_color(self.style_working.gradient_a)
    }

    fn style_gradient_b(&self) -> Color {
        rgba_to_color(self.style_working.gradient_b)
    }

    fn style_gradient_dir(&self) -> TextGradientDir {
        self.style_working.gradient_dir
    }

    fn style_hex_override(&self, field: StyleField) -> Option<&str> {
        self.style_hex_overrides.get(&field).map(|s| s.as_str())
    }

    fn editing(&self) -> Option<(usize, EntryId)> {
        self.editing
    }

    fn editing_origin(&self) -> EditOrigin {
        self.editing_origin
    }

    fn editing_rect(&self) -> Option<Rectangle> {
        self.editing_rect
    }

    fn edit_content(&self) -> Option<&text_editor::Content> {
        self.edit_content.as_ref()
    }

    fn font(&self) -> Option<Font> {
        self.font
    }

    fn inpaint_mode(&self) -> bool {
        self.inpaint_mode
    }

    fn show_overlay_text(&self) -> bool {
        self.show_overlay_text
    }

    fn show_inpaint(&self) -> bool {
        self.show_inpaint
    }

    fn view_mode(&self) -> MainAreaMode {
        self.view_mode
    }

    fn viewer_scroll(&self) -> f32 {
        self.viewer_scroll
    }

    fn settings_open(&self) -> bool {
        self.settings_open
    }

    fn settings_tab(&self) -> SettingsTab {
        self.settings_tab
    }

    fn manage_models_open(&self) -> bool {
        self.manage_models_open
    }

    fn manage_models_search(&self) -> &str {
        &self.manage_models_search
    }

    fn all_model_groups(&self) -> Vec<(String, String, Vec<(String, String)>)> {
        self.tx.all_model_groups()
    }
}
