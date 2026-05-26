/// Per-entry rendering style for the text overlay and future image export.
///
/// Stored as a delta inside a [`Profile`]; `Default` is the fallback when the
/// profile has no delta for an entry.
///
/// [`Profile`]: crate::model::Profile
#[derive(Debug, Clone, Copy, PartialEq)]
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
        }
    }
}
