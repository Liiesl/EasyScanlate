use iced::keyboard::{key, Key};
use iced::widget::{space, text_editor};
use iced::widget::text::Wrapping;
use iced::{Background, Border, Element, Font, Length, Padding, Size};

use easyscanlate_model::EntryStyle;

use crate::color::rgba_to_color;
use crate::event::{EditOrigin, UiEvent};
use crate::main_area::overlay::{fit::fit_font_metrics, style::styled_font_for_text};
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
    let font = styled_font_for_text(state.font().unwrap_or(Font::DEFAULT), &style, &text);
    let wrap_width = rect.width.max(8.0);
    let (size, fitted_height) = fit_font_metrics(&text, font, Size::new(wrap_width, rect.height));
    let size = size.max(8.0);
    // Breathing room so the caret/selection is never clipped at the text
    // bounds. Sizing is unchanged: the text still wraps at `rect.width` and
    // the font still fits the original `rect`.
    // The fitted font makes the longest line exactly `rect.width` wide, so its
    // ink sits flush at the text area's right edge (left has glyph side
    // bearings, top/bottom have line-height slack). The right side gets extra
    // reserve so the visible gap looks equal.
    let pad = (size * 0.2).clamp(4.0, 8.0);
    let right_extra = (size * 0.12).clamp(2.0, 5.0);
    let right_pad = pad + right_extra;
    let box_width = rect.width + pad + right_pad;
    let box_height = fitted_height + pad * 2.0;
    let text_color = rgba_to_color(style.text_color);
    let bg_color = rgba_to_color(style.bg_color);
    let select_border = crate::accent::accent();
    let editor = text_editor::TextEditor::new(content)
        .id(EDIT_INPUT_ID)
        .font(font)
        .size(size)
        .line_height(1.2)
        .wrapping(Wrapping::WordOrGlyph)
        .width(box_width)
        .height(Length::Fixed(box_height))
        .padding(Padding {
            top: pad,
            right: right_pad,
            bottom: pad,
            left: pad,
        })
        .on_action(UiEvent::EditAction)
        .key_binding(|press| match press.modified_key.as_ref() {
            Key::Named(key::Named::Escape) => Some(text_editor::Binding::Custom(UiEvent::EditSubmit)),
            Key::Named(key::Named::Enter) if press.modifiers.command() => {
                Some(text_editor::Binding::Custom(UiEvent::EditSubmit))
            }
            _ => text_editor::Binding::from_key_press(press),
        })
        .style(move |_theme, _status| text_editor::Style {
            background: Background::Color(bg_color),
            border: Border::default()
                .color(select_border)
                .width(2.0)
                .rounded(0.0),
            placeholder: text_color,
            value: text_color,
            selection: crate::accent::accent_translucent(0.35),
        });
    let block_top = rect.y + (rect.height - fitted_height).max(0.0) / 2.0 - pad;
    iced::widget::Pin::new(editor).x(rect.x - pad).y(block_top).into()
}
