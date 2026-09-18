use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use iced::font::{Family as FontFamily, Stretch as FontStretch, Style as FontStyle, Weight as FontWeight};
use iced::Font;

use easyscanlate_model::EntryStyle;

use super::fallback::contains_cjk;

/// A `Font` for the installed family `name`, memoized: iced's `Font::with_name` requires `&'static str`.
pub(crate) fn family_font(name: &str) -> Font {
    static NAMES: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let names = NAMES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = names.lock().expect("font name cache poisoned");
    let leaked = guard
        .entry(name.to_owned())
        .or_insert_with(|| Box::leak(name.to_owned().into_boxed_str()));
    Font::with_name(leaked)
}

/// Per-family Bold/Italic face availability in the live renderer DB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontSupport {
    pub has_bold: bool,
    pub has_italic: bool,
    pub has_bold_italic: bool,
}

impl FontSupport {
    fn full() -> Self {
        Self {
            has_bold: true,
            has_italic: true,
            has_bold_italic: true,
        }
    }

    /// True when Bold and Italic exist separately but no combined
    /// BoldItalic face exists: the two toggles must act as an exclusive
    /// switcher (enabling one clears the other).
    pub fn is_exclusive(&self) -> bool {
        self.has_bold && self.has_italic && !self.has_bold_italic
    }
}

/// Bold/Italic availability for the selected family (`None` = default font).
///
/// Scans the live renderer font DB like [`nearest_available_weight`]:
/// weight `>= 600` (Semibold and above) counts as Bold, `Italic`/`Oblique`
/// counts as Italic. Unknown families (not yet lazily loaded) optimistically
/// report full support and are *not* cached, so a later `font::load`
/// (bumped [`Version`]) is picked up. `None` also reports full support.
pub fn font_support(family: Option<&str>) -> FontSupport {
    use iced::advanced::graphics::text::{Version, font_system};

    let Some(family) = family else {
        return FontSupport::full();
    };
    static SUPPORT: OnceLock<Mutex<HashMap<(Version, String), FontSupport>>> =
        OnceLock::new();
    let cache = SUPPORT.get_or_init(|| Mutex::new(HashMap::new()));
    let version = font_system()
        .read()
        .expect("Read font system")
        .version();
    let key = (version, family.to_ascii_lowercase());
    if let Some(&hit) = cache.lock().expect("support cache poisoned").get(&key) {
        return hit;
    }
    let Some(hit) = scan_font_support(family) else {
        // Unknown family (not in DB yet): optimistic, no cache.
        return FontSupport::full();
    };
    cache
        .lock()
        .expect("support cache poisoned")
        .insert(key, hit);
    hit
}

/// Single scan of the live renderer DB for Bold/Italic faces.
/// Returns `None` when the family has no faces at all (unknown / not loaded).
fn scan_font_support(family: &str) -> Option<FontSupport> {
    use iced::advanced::graphics::text::{cosmic_text, font_system};

    let mut guard = font_system().write().expect("Write font system");
    let mut found = false;
    let mut has_bold = false;
    let mut has_italic = false;
    let mut has_bold_italic = false;
    for face in guard.raw().db().faces() {
        if !face
            .families
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(family))
        {
            continue;
        }
        found = true;
        let bold = face.weight.0 >= 600;
        let italic = matches!(
            face.style,
            cosmic_text::fontdb::Style::Italic | cosmic_text::fontdb::Style::Oblique
        );
        has_bold |= bold;
        has_italic |= italic;
        has_bold_italic |= bold && italic;
    }
    found.then_some(FontSupport {
        has_bold,
        has_italic,
        has_bold_italic,
    })
}

/// Entry's font family + weight/style applied on top of base font, with
/// CJK-aware degradation: when `text` contains Hangul/Han/Kana and bold or
/// italic is requested, the weight/style are forced to `Normal` so that
/// cosmic-text's fallback (Malgun Gothic / Noto) finds the glyph. Otherwise
/// the requested weight/style are preserved, except when the selected family
/// lacks the face (see [`font_support`]): the stored [`EntryStyle`] flags are
/// kept intact (so auto-detected values survive a font switch) but rendering
/// degrades to an available face instead of silently falling back to another
/// family. This prevents `□` tofu and
/// avoids expensive fallback scans for missing `Bold Italic` CJK faces
/// (issue #24).
pub fn styled_font_for_text(font: Font, style: &EntryStyle, text: &str) -> Font {
    let mut font = style
        .font_family
        .as_deref()
        .map(family_font)
        .unwrap_or(font);
    let cjk = contains_cjk(text);
    // Availability of the resolved family; generic families report full.
    let support = match font.family {
        FontFamily::Name(name) => font_support(Some(name)),
        _ => FontSupport::full(),
    };
    // For CJK scripts bold/italic have no dedicated faces in the OS
    // fallback set (the boot task loads only `Normal` CJK files). Degrade
    // to Normal so fallback scoring matches; Latin keeps the requested
    // weight/style unless the family lacks the face.
    let mut want_italic = style.italic && !cjk && support.has_italic;
    let want_bold = style.bold && !cjk && support.has_bold;
    // Exclusive families (Bold + Italic but no BoldItalic): requesting both
    // would miss the family gate and fall back to another family. Keep Bold,
    // the more visible emphasis for manhwa shouting.
    if support.is_exclusive() && want_bold && want_italic {
        want_italic = false;
    }
    font.style = if want_italic {
        FontStyle::Italic
    } else {
        FontStyle::Normal
    };
    let want = if want_bold {
        FontWeight::Bold
    } else {
        FontWeight::Normal
    };
    // Clamp named families to a weight the renderer actually has (see below).
    // Generic families resolve through cosmic-text's locale fallback table
    // and need no exact weight.
    font.weight = match font.family {
        FontFamily::Name(name) => nearest_available_weight(name, want, font.style, font.stretch),
        _ => want,
    };
    font
}

/// Numeric CSS weight for an iced [`FontWeight`], mirroring iced's own
/// `to_weight` mapping (Thin=100 … Black=900).
fn weight_value(weight: FontWeight) -> u16 {
    match weight {
        FontWeight::Thin => 100,
        FontWeight::ExtraLight => 200,
        FontWeight::Light => 300,
        FontWeight::Normal => 400,
        FontWeight::Medium => 500,
        FontWeight::Semibold => 600,
        FontWeight::Bold => 700,
        FontWeight::ExtraBold => 800,
        FontWeight::Black => 900,
    }
}

/// Nearest named [`FontWeight`] step for a raw weight value (ties go lighter).
fn weight_from_value(value: u16) -> FontWeight {
    const STEPS: [(u16, FontWeight); 9] = [
        (100, FontWeight::Thin),
        (200, FontWeight::ExtraLight),
        (300, FontWeight::Light),
        (400, FontWeight::Normal),
        (500, FontWeight::Medium),
        (600, FontWeight::Semibold),
        (700, FontWeight::Bold),
        (800, FontWeight::ExtraBold),
        (900, FontWeight::Black),
    ];
    let mut best = STEPS[3];
    for step in STEPS {
        if value.abs_diff(step.0) < value.abs_diff(best.0) {
            best = step;
        }
    }
    best.1
}

/// Closest weight the renderer's font DB actually provides for `family`
/// under the requested style/stretch.
///
/// cosmic-text only keeps the requested family when a face matches the
/// requested weight *exactly* (`font_weight_diff == 0` in its
/// `default_font_match_key`); otherwise it silently falls back to another
/// family (e.g. Segoe UI). Single-weight families whose only face is not
/// 400 — like Minecraft (500) — therefore never render when asked for
/// Normal/Bold. Clamping the request to the nearest available weight lets
/// the family gate pass so the real face is used.
fn nearest_available_weight(
    family: &str,
    want: FontWeight,
    style: FontStyle,
    stretch: FontStretch,
) -> FontWeight {
    use iced::advanced::graphics::text::{Version, font_system};

    static RESOLVED: OnceLock<
        Mutex<HashMap<(Version, String, FontWeight, FontStyle, FontStretch), FontWeight>>,
    > = OnceLock::new();

    let cache = RESOLVED.get_or_init(|| Mutex::new(HashMap::new()));
    let version = font_system()
        .read()
        .expect("Read font system")
        .version();
    let key = (version, family.to_owned(), want, style, stretch);
    if let Some(&hit) = cache.lock().expect("weight cache poisoned").get(&key) {
        return hit;
    }
    let hit = scan_nearest_weight(family, want, style, stretch).unwrap_or(want);
    cache
        .lock()
        .expect("weight cache poisoned")
        .insert(key, hit);
    hit
}

/// Single scan of the live renderer DB for the nearest face weight.
/// Only positive hits are cached by the caller: an unknown family is
/// re-scanned so a font that finishes loading afterwards is picked up.
fn scan_nearest_weight(
    family: &str,
    want: FontWeight,
    style: FontStyle,
    stretch: FontStretch,
) -> Option<FontWeight> {
    use iced::advanced::graphics::text::{cosmic_text, font_system};

    let want_value = weight_value(want);
    let want_style = match style {
        FontStyle::Normal => cosmic_text::fontdb::Style::Normal,
        FontStyle::Italic => cosmic_text::fontdb::Style::Italic,
        FontStyle::Oblique => cosmic_text::fontdb::Style::Oblique,
    };
    let want_stretch = match stretch {
        FontStretch::UltraCondensed => cosmic_text::fontdb::Stretch::UltraCondensed,
        FontStretch::ExtraCondensed => cosmic_text::fontdb::Stretch::ExtraCondensed,
        FontStretch::Condensed => cosmic_text::fontdb::Stretch::Condensed,
        FontStretch::SemiCondensed => cosmic_text::fontdb::Stretch::SemiCondensed,
        FontStretch::Normal => cosmic_text::fontdb::Stretch::Normal,
        FontStretch::SemiExpanded => cosmic_text::fontdb::Stretch::SemiExpanded,
        FontStretch::Expanded => cosmic_text::fontdb::Stretch::Expanded,
        FontStretch::ExtraExpanded => cosmic_text::fontdb::Stretch::ExtraExpanded,
        FontStretch::UltraExpanded => cosmic_text::fontdb::Stretch::UltraExpanded,
    };
    // Same pre-filter cosmic-text applies before family matching
    // (`Attrs::matches`): style and stretch must agree exactly.
    let mut guard = font_system().write().expect("Write font system");
    let mut best: Option<u16> = None;
    for face in guard.raw().db().faces() {
        if face.style != want_style || face.stretch != want_stretch {
            continue;
        }
        if !face
            .families
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(family))
        {
            continue;
        }
        let weight = face.weight.0;
        let closer = match best {
            Some(current) => weight.abs_diff(want_value) < current.abs_diff(want_value),
            None => true,
        };
        if closer {
            best = Some(weight);
        }
    }
    best.map(weight_from_value)
}

/// Entry's font family + weight/style applied on top of base font.
/// Wrapper that preserves the old signature for callers without text
/// (e.g. preset preview); CJK detection is skipped and the requested
/// weight/style are applied verbatim.
pub fn styled_font(font: Font, style: &EntryStyle) -> Font {
    styled_font_for_text(font, style, "")
}

/// Preview `Font` for the font-picker dropdown: the family's own face at
/// plain Normal, with the weight clamped like the overlay does, so
/// single-weight bitmap families whose only face is not 400 (e.g.
/// Minecraft at 500) render their real face instead of silently falling
/// back to another family.
pub fn preview_font(name: &str) -> Font {
    let mut font = family_font(name);
    font.weight = match font.family {
        FontFamily::Name(n) => {
            nearest_available_weight(n, FontWeight::Normal, font.style, font.stretch)
        }
        _ => font.weight,
    };
    font
}
