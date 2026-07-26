use iced::widget::text;
use iced::Font;
use lucide_icons::Icon;

/// Returns a `Text` widget rendering the given Lucide icon with the correct font.
/// Use `.size()` and `.center()` etc. on the returned value.
pub fn lucide(icon: Icon) -> text::Text<'static, iced::Theme> {
    let glyph = char::from(icon).to_string();
    text(glyph).font(Font::with_name("lucide"))
}
