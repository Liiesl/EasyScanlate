use iced::keyboard::{key, Key};
use iced::widget::{space, text_editor};
use iced::{Background, Border, Color, Element, Font, Length, Size};

use scanlateit_model::EntryStyle;

use crate::color::rgba_to_color;
use crate::event::{EditOrigin, UiEvent};
use crate::main_area::overlay::{fit::fit_font_metrics, style::styled_font};
use crate::state::UiState;

/// Widget id of the floating inline editor; must match the app's focus id.
pub const EDIT_INPUT_ID: &str = "overlay-editor";

/// The floating multi-line `TextEditor` used to edit a double-clicked overlay entry.
pub fn edit_overlay<S: UiState + ?Sized>(state: &S) -> Element<'_, UiEvent> {
    let (Some((index, id)), Some(rect)) = (state.editing(), state.editing_rect()) else {
        return space().into();
    };
    if state.editing_origin() != EditOrigin::Overlay {
        return space().into();
    }
    let Some(content) = state.edit_content() else {
        return space().into();
    };
    let project = state.project();
    let (text, style) = match project.entry(id) {
        Some(entry) if state.images().get(index).is_some_and(|img| img.image_id == entry.image_id) => {
            (project.display_text(entry).to_string(), project.entry_style(entry.id))
        }
        _ => (String::new(), EntryStyle::default()),
    };
    let font = styled_font(state.font().unwrap_or(Font::DEFAULT), &style);
    let wrap_width = rect.width.max(8.0);
    let (size, fitted_height) = fit_font_metrics(&text, font, Size::new(wrap_width, rect.height));
    let size = size.max(8.0);
    let text_color = rgba_to_color(style.text_color);
    let editor = text_editor::TextEditor::new(content)
        .id(EDIT_INPUT_ID)
        .font(font)
        .size(size)
        .line_height(1.2)
        .width(rect.width)
        .height(Length::Fixed(fitted_height))
        .padding(0)
        .on_action(UiEvent::EditAction)
        .key_binding(|press| match press.modified_key.as_ref() {
            Key::Named(key::Named::Escape) => Some(text_editor::Binding::Custom(UiEvent::EditSubmit)),
            Key::Named(key::Named::Enter) if press.modifiers.command() => {
                Some(text_editor::Binding::Custom(UiEvent::EditSubmit))
            }
            _ => text_editor::Binding::from_key_press(press),
        })
        .style(move |_theme, _status| text_editor::Style {
            background: Background::Color(Color::TRANSPARENT),
            border: Border::default().rounded(0.0),
            placeholder: text_color,
            value: text_color,
            selection: Color::from_rgba8(92, 190, 255, 0.35),
        });
    let block_top = rect.y + (rect.height - fitted_height).max(0.0) / 2.0;
    iced::widget::Pin::new(editor).x(rect.x).y(block_top).into()
}
