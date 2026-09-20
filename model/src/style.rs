use serde::{Deserialize, Serialize};

/// Per-entry text alignment mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextAlign {
    /// Manhwa-style bubble text: lines follow the ellipse chords of the box.
    Circular,
    Left,
    Center,
    Right,
}

impl TextAlign {
    pub const LABELS: [&'static str; 4] = ["Circular", "Left", "Center", "Right"];

    pub fn label(self) -> &'static str {
        match self {
            TextAlign::Circular => "Circular",
            TextAlign::Left => "Left",
            TextAlign::Center => "Center",
            TextAlign::Right => "Right",
        }
    }

    pub fn from_label(label: &str) -> Self {
        match label {
            "Left" => TextAlign::Left,
            "Center" => TextAlign::Center,
            "Right" => TextAlign::Right,
            _ => TextAlign::Circular,
        }
    }
}

/// Direction of the two-color text gradient.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextGradientDir {
    TopToBottom,
    BottomToTop,
    TopLeftToBottomRight,
    BottomRightToTopLeft,
    TopRightToBottomLeft,
    BottomLeftToTopRight,
    LeftToRight,
    RightToLeft,
}

impl TextGradientDir {
    pub const LABELS: [&'static str; 8] = [
        "Top → Bottom",
        "Bottom → Top",
        "Top-Left → Bottom-Right",
        "Bottom-Right → Top-Left",
        "Top-Right → Bottom-Left",
        "Bottom-Left → Top-Right",
        "Left → Right",
        "Right → Left",
    ];

    pub fn label(self) -> &'static str {
        match self {
            TextGradientDir::TopToBottom => "Top → Bottom",
            TextGradientDir::BottomToTop => "Bottom → Top",
            TextGradientDir::TopLeftToBottomRight => "Top-Left → Bottom-Right",
            TextGradientDir::BottomRightToTopLeft => "Bottom-Right → Top-Left",
            TextGradientDir::TopRightToBottomLeft => "Top-Right → Bottom-Left",
            TextGradientDir::BottomLeftToTopRight => "Bottom-Left → Top-Right",
            TextGradientDir::LeftToRight => "Left → Right",
            TextGradientDir::RightToLeft => "Right → Left",
        }
    }

    pub fn from_label(label: &str) -> Self {
        match label {
            "Bottom → Top" => TextGradientDir::BottomToTop,
            "Top-Left → Bottom-Right" => TextGradientDir::TopLeftToBottomRight,
            "Bottom-Right → Top-Left" => TextGradientDir::BottomRightToTopLeft,
            "Top-Right → Bottom-Left" => TextGradientDir::TopRightToBottomLeft,
            "Bottom-Left → Top-Right" => TextGradientDir::BottomLeftToTopRight,
            "Left → Right" => TextGradientDir::LeftToRight,
            "Right → Left" => TextGradientDir::RightToLeft,
            _ => TextGradientDir::TopToBottom,
        }
    }
}

/// Bundled font families shipped with the app (embedded at compile time via
/// `include_bytes!` in the binary — no system install required, see
/// `src/main.rs` and `src/app.rs` merging).
pub const ANIME_ACE_FAMILY: &str = "Anime Ace";
pub const AUGIE_FAMILY: &str = "augie";
pub const FUZZY_BUBBLES_FAMILY: &str = "Fuzzy Bubbles";
pub const KOMIKA_BOO_FAMILY: &str = "Komika Boo";
pub const KOMIKA_HAND_FAMILY: &str = "Komika Hand";
pub const KOMIKA_JAM_FAMILY: &str = "Komika Jam";
pub const KOMIKA_SLICK_FAMILY: &str = "Komika Slick";
pub const KOMIKA_SLIM_FAMILY: &str = "Komika Slim";
pub const NANUM_PEN_FAMILY: &str = "Nanum Pen";
/// The default font family for new entries/presets. Always bundled.
pub const DEFAULT_FONT_FAMILY: &str = ANIME_ACE_FAMILY;
/// All families that are bundled in the binary (no install needed).
pub const BUNDLED_FONTS: &[&str] = &[
    ANIME_ACE_FAMILY,
    NANUM_PEN_FAMILY,
    AUGIE_FAMILY,
    FUZZY_BUBBLES_FAMILY,
    KOMIKA_BOO_FAMILY,
    KOMIKA_HAND_FAMILY,
    KOMIKA_JAM_FAMILY,
    KOMIKA_SLICK_FAMILY,
    KOMIKA_SLIM_FAMILY,
];

/// Section titles of the font-picker dropdown.
pub const FEATURED_FONTS_LABEL: &str = "Featured";
pub const ALL_FONTS_LABEL: &str = "All fonts";

/// Split `installed` (the sorted, deduped union of system + bundled families)
/// into `(featured, rest)` index lists for the font-picker dropdown.
///
/// - `featured` holds indices into `installed` in the curated
///   [`BUNDLED_FONTS`] order, matched case-insensitively; bundled names
///   missing from `installed` are skipped. Indices borrow from the caller's
///   slice so dropdown labels can borrow them for the widget lifetime.
/// - `rest` holds the remaining indices in `installed` order (the "All
///   fonts" section is deduped against Featured).
pub fn partition_featured_fonts(installed: &[String]) -> (Vec<usize>, Vec<usize>) {
    let mut featured = Vec::with_capacity(BUNDLED_FONTS.len());
    for bundled in BUNDLED_FONTS {
        if let Some(index) = installed.iter().position(|n| n.eq_ignore_ascii_case(bundled)) {
            featured.push(index);
        }
    }
    let rest: Vec<usize> = installed
        .iter()
        .enumerate()
        .filter(|(_, n)| !BUNDLED_FONTS.iter().any(|b| n.eq_ignore_ascii_case(b)))
        .map(|(index, _)| index)
        .collect();
    (featured, rest)
}

/// Per-entry rendering style for the text overlay and future image export.
///
/// Stored as a per-entry map inside [`Project`] (`Project::styles`), **shared
/// by every profile** — not a per-profile delta. `Default` is the fallback
/// when no override is stored. The `Profile` delta only holds translated text.
///
/// [`Profile`]: crate::Profile
/// [`Project`]: crate::Project
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntryStyle {
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
    /// RGBA.
    pub text_color: [u8; 4],
    /// RGBA.
    pub stroke_color: [u8; 4],
    /// Stroke thickness in image pixels; `0` disables the stroke.
    pub stroke_width: f32,
    /// RGBA.
    pub bg_color: [u8; 4],
    /// Corner radius of the background in image pixels.
    pub bg_radius: f32,
    /// Installed font family name for this entry's text; `None` = the app's
    /// default overlay font.
    pub font_family: Option<String>,
    /// How the text is laid out inside its box.
    pub text_align: TextAlign,
    /// When true, the text fill is a two-color gradient instead of
    /// `text_color`.
    pub text_gradient: bool,
    /// RGBA; gradient start color, used when `text_gradient`.
    pub gradient_a: [u8; 4],
    /// RGBA; gradient end color, used when `text_gradient`.
    pub gradient_b: [u8; 4],
    /// Gradient direction, used when `text_gradient`.
    pub gradient_dir: TextGradientDir,
}

impl Default for EntryStyle {
    fn default() -> Self {
        Self {
            font_size: 14.0,
            bold: false,
            italic: false,
            text_color: [0, 0, 0, 255],
            stroke_color: [0, 0, 0, 255],
            stroke_width: 0.0,
            bg_color: [255, 255, 255, 255],
            bg_radius: 0.0,
            font_family: Some(DEFAULT_FONT_FAMILY.to_string()),
            text_align: TextAlign::Circular,
            text_gradient: false,
            gradient_a: [0, 0, 0, 255],
            gradient_b: [0, 0, 0, 255],
            gradient_dir: TextGradientDir::TopToBottom,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_style_round_trips_all_fields() {
        let style = EntryStyle::default();
        assert_eq!(style.bold, false);
        assert_eq!(style.italic, false);
        assert_eq!(style.stroke_color, [0, 0, 0, 255]);
        assert_eq!(style.stroke_width, 0.0);
        assert_eq!(style.bg_radius, 0.0);
        assert_eq!(style.font_family.as_deref(), Some(DEFAULT_FONT_FAMILY));
        assert_eq!(style.text_align, TextAlign::Circular);
        assert!(!style.text_gradient);
    }

    #[test]
    fn bundled_fonts_default_to_anime_ace() {
        assert_eq!(DEFAULT_FONT_FAMILY, ANIME_ACE_FAMILY);
        assert!(BUNDLED_FONTS.contains(&ANIME_ACE_FAMILY));
        assert!(BUNDLED_FONTS.contains(&AUGIE_FAMILY));
        assert!(BUNDLED_FONTS.contains(&FUZZY_BUBBLES_FAMILY));
        assert!(BUNDLED_FONTS.contains(&KOMIKA_BOO_FAMILY));
        assert!(BUNDLED_FONTS.contains(&KOMIKA_HAND_FAMILY));
        assert!(BUNDLED_FONTS.contains(&KOMIKA_JAM_FAMILY));
        assert!(BUNDLED_FONTS.contains(&KOMIKA_SLICK_FAMILY));
        assert!(BUNDLED_FONTS.contains(&KOMIKA_SLIM_FAMILY));
        assert!(BUNDLED_FONTS.contains(&NANUM_PEN_FAMILY));
        let style = EntryStyle::default();
        assert_eq!(style.font_family.as_deref(), Some(ANIME_ACE_FAMILY));
    }

    #[test]
    fn text_align_labels_round_trip() {
        for label in TextAlign::LABELS {
            assert_eq!(TextAlign::from_label(label).label(), label);
        }
    }

    #[test]
    fn gradient_dir_labels_round_trip() {
        for label in TextGradientDir::LABELS {
            assert_eq!(TextGradientDir::from_label(label).label(), label);
        }
    }

    #[test]
    fn partition_keeps_bundled_order_and_dedupes_rest() {
        let installed = ["Arial", "Anime Ace", "augie", "Verdana"]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let (featured, rest) = partition_featured_fonts(&installed);
        let names = |indices: &[usize]| {
            indices
                .iter()
                .map(|&i| installed[i].clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&featured), vec!["Anime Ace".to_string(), "augie".to_string()]);
        assert_eq!(names(&rest), vec!["Arial".to_string(), "Verdana".to_string()]);
    }

    #[test]
    fn partition_skips_missing_bundled_and_matches_case_insensitively() {
        let installed = ["ANIME ACE", "Arial"]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let (featured, rest) = partition_featured_fonts(&installed);
        assert_eq!(featured, vec![0]);
        assert_eq!(rest, vec![1]);
    }
}
