/// Per-entry rendering style for the text overlay and future image export.
///
/// Stored as a delta inside a [`Profile`]; `Default` is the fallback when the
/// profile has no delta for an entry.
///
/// [`Profile`]: crate::model::Profile
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntryStyle {
    pub font_size: f32,
    /// RGBA.
    pub text_color: [u8; 4],
    /// RGBA.
    pub bg_color: [u8; 4],
}

impl Default for EntryStyle {
    fn default() -> Self {
        Self {
            font_size: 14.0,
            text_color: [255, 230, 90, 255],
            bg_color: [20, 20, 31, 140],
        }
    }
}
