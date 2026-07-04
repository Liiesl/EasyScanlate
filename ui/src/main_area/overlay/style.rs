use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use iced::font::{Style as FontStyle, Weight as FontWeight};
use iced::Font;

use scanlateit_model::EntryStyle;

/// A `Font` for the installed family `name`, memoized: iced's `Font::with_name` requires `&'static str`.
fn family_font(name: &str) -> Font {
    static NAMES: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let names = NAMES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = names.lock().expect("font name cache poisoned");
    let leaked = guard
        .entry(name.to_owned())
        .or_insert_with(|| Box::leak(name.to_owned().into_boxed_str()));
    Font::with_name(leaked)
}

/// Entry's font family + weight/style applied on top of base font.
pub(crate) fn styled_font(font: Font, style: &EntryStyle) -> Font {
    let mut font = style
        .font_family
        .as_deref()
        .map(family_font)
        .unwrap_or(font);
    font.weight = if style.bold {
        FontWeight::Bold
    } else {
        FontWeight::Normal
    };
    font.style = if style.italic {
        FontStyle::Italic
    } else {
        FontStyle::Normal
    };
    font
}
