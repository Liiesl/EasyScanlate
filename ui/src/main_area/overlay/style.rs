use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use iced::font::{Style as FontStyle, Weight as FontWeight};
use iced::Font;

use easyscanlate_model::EntryStyle;

use super::fallback::contains_cjk;

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

/// Entry's font family + weight/style applied on top of base font, with
/// CJK-aware degradation: when `text` contains Hangul/Han/Kana and bold or
/// italic is requested, the weight/style are forced to `Normal` so that
/// cosmic-text's fallback (Malgun Gothic / Noto) finds the glyph. Otherwise
/// the requested weight/style are preserved. This prevents `□` tofu and
/// avoids expensive fallback scans for missing `Bold Italic` CJK faces
/// (issue #24).
pub(crate) fn styled_font_for_text(font: Font, style: &EntryStyle, text: &str) -> Font {
    let mut font = style
        .font_family
        .as_deref()
        .map(family_font)
        .unwrap_or(font);
    let cjk = contains_cjk(text);
    // For CJK scripts bold/italic have no dedicated faces in the OS
    // fallback set (the boot task loads only `Normal` CJK files). Degrade
    // to Normal so fallback scoring matches; Latin keeps the requested
    // weight/style.
    font.weight = if style.bold && !cjk {
        FontWeight::Bold
    } else {
        FontWeight::Normal
    };
    font.style = if style.italic && !cjk {
        FontStyle::Italic
    } else {
        FontStyle::Normal
    };
    font
}

/// Entry's font family + weight/style applied on top of base font.
/// Wrapper that preserves the old signature for callers without text
/// (e.g. preset preview); CJK detection is skipped and the requested
/// weight/style are applied verbatim.
pub(crate) fn styled_font(font: Font, style: &EntryStyle) -> Font {
    styled_font_for_text(font, style, "")
}
