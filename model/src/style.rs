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
/// The default font family for new entries/presets. Always bundled.
pub const DEFAULT_FONT_FAMILY: &str = ANIME_ACE_FAMILY;
/// All families that are bundled in the binary (no install needed).
pub const BUNDLED_FONTS: &[&str] = &[ANIME_ACE_FAMILY, AUGIE_FAMILY];

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
}
