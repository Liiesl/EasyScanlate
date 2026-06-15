/// Per-entry text alignment mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Per-entry rendering style for the text overlay and future image export.
///
/// Stored as a delta inside a [`Profile`]; `Default` is the fallback when the
/// profile has no delta for an entry.
///
/// [`Profile`]: crate::Profile
#[derive(Debug, Clone, PartialEq)]
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
            font_family: None,
            text_align: TextAlign::Circular,
            text_gradient: false,
            gradient_a: [0, 0, 0, 255],
            gradient_b: [0, 0, 0, 255],
            gradient_dir: TextGradientDir::TopToBottom,
        }
    }
}

/// How many preset slots the app starts with: five built-in styles plus
/// three empty slots.
pub const INITIAL_PRESET_SLOTS: usize = 8;

/// The style-preset slot list shown in the styling panel, in memory only:
/// `None` = empty slot. "+" fills the first empty slot or appends; clicking
/// a filled swatch applies its style; right-click replaces or empties.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StylePresets(Vec<Option<EntryStyle>>);

impl StylePresets {
    /// The five seeded variants (white bg/black text, inverse, transparent
    /// bg/black text, transparent bg/white text, red bg/white text) followed
    /// by three empty slots.
    pub fn default_presets() -> Self {
        let mut presets = Vec::with_capacity(INITIAL_PRESET_SLOTS);
        let mut preset = EntryStyle::default();
        presets.push(Some(preset.clone()));
        preset.bg_color = [0, 0, 0, 255];
        preset.text_color = [255, 255, 255, 255];
        presets.push(Some(preset.clone()));
        preset.bg_color = [0, 0, 0, 0];
        preset.text_color = [0, 0, 0, 255];
        presets.push(Some(preset.clone()));
        preset.text_color = [255, 255, 255, 255];
        presets.push(Some(preset.clone()));
        preset.bg_color = [255, 0, 0, 255];
        presets.push(Some(preset));
        presets.resize(INITIAL_PRESET_SLOTS, None);
        Self(presets)
    }

    /// The style of slot `index`, or `None` for an empty slot / out of range.
    pub fn get(&self, index: usize) -> Option<EntryStyle> {
        self.0.get(index).cloned().flatten()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn as_slice(&self) -> &[Option<EntryStyle>] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Fills the first empty slot, or appends when all are full.
    pub fn add(&mut self, style: EntryStyle) {
        if let Some(slot) = self.0.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(style);
        } else {
            self.0.push(Some(style));
        }
    }

    /// Overwrites slot `index` (no-op when out of range).
    pub fn replace(&mut self, index: usize, style: EntryStyle) {
        if let Some(slot) = self.0.get_mut(index) {
            *slot = Some(style);
        }
    }

    /// Empties slot `index` (no-op when out of range).
    pub fn remove(&mut self, index: usize) {
        if let Some(slot) = self.0.get_mut(index) {
            *slot = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_presets_cover_the_expected_variants() {
        let presets = StylePresets::default_presets();
        assert_eq!(presets.len(), INITIAL_PRESET_SLOTS);
        let filled: Vec<&EntryStyle> = presets.as_slice().iter().flatten().collect();
        assert_eq!(filled.len(), 5);
        assert_eq!(filled[0].bg_color, [255, 255, 255, 255], "white bg");
        assert_eq!(filled[0].text_color, [0, 0, 0, 255], "black text");
        assert_eq!(filled[1].bg_color, [0, 0, 0, 255], "inverse: black bg");
        assert_eq!(filled[1].text_color, [255, 255, 255, 255], "inverse: white text");
        assert_eq!(filled[2].bg_color, [0, 0, 0, 0], "transparent bg");
        assert_eq!(filled[2].text_color, [0, 0, 0, 255], "black text");
        assert_eq!(filled[3].bg_color, [0, 0, 0, 0], "transparent bg");
        assert_eq!(filled[3].text_color, [255, 255, 255, 255], "white text");
        assert_eq!(filled[4].bg_color, [255, 0, 0, 255], "red bg");
        assert_eq!(filled[4].text_color, [255, 255, 255, 255], "white text");
        assert!(presets.as_slice()[5..].iter().all(|slot| slot.is_none()), "last slots empty");
    }

    #[test]
    fn default_style_round_trips_all_fields() {
        let style = EntryStyle::default();
        assert_eq!(style.bold, false);
        assert_eq!(style.italic, false);
        assert_eq!(style.stroke_color, [0, 0, 0, 255]);
        assert_eq!(style.stroke_width, 0.0);
        assert_eq!(style.bg_radius, 0.0);
        assert_eq!(style.font_family, None);
        assert_eq!(style.text_align, TextAlign::Circular);
        assert!(!style.text_gradient);
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
    fn add_fills_the_first_empty_slot() {
        let mut presets = StylePresets::default_presets();
        let style = EntryStyle { bg_color: [9, 9, 9, 255], ..EntryStyle::default() };
        presets.add(style.clone());
        presets.add(style.clone());
        presets.add(style.clone());

        assert_eq!(presets.len(), INITIAL_PRESET_SLOTS);
        assert_eq!(presets.get(5), Some(style.clone()));
        assert_eq!(presets.get(6), Some(style.clone()));
        assert_eq!(presets.get(7), Some(style));
    }

    #[test]
    fn add_appends_when_all_slots_are_full() {
        let mut presets = StylePresets::default_presets();
        for i in 0..INITIAL_PRESET_SLOTS {
            let style = EntryStyle { text_color: [i as u8, 0, 0, 255], ..EntryStyle::default() };
            presets.replace(i, style);
        }
        let style = EntryStyle { bg_color: [1, 2, 3, 255], ..EntryStyle::default() };
        presets.add(style.clone());

        assert_eq!(presets.len(), INITIAL_PRESET_SLOTS + 1);
        assert_eq!(presets.get(INITIAL_PRESET_SLOTS), Some(style));
    }

    #[test]
    fn add_refills_an_emptied_slot_before_appending() {
        let mut presets = StylePresets::default_presets();
        presets.remove(2);
        let style = EntryStyle { text_color: [7, 7, 7, 255], ..EntryStyle::default() };
        presets.add(style.clone());

        assert_eq!(presets.len(), INITIAL_PRESET_SLOTS);
        assert_eq!(presets.get(2), Some(style));
    }

    #[test]
    fn replace_overwrites_filled_and_empty_slots() {
        let mut presets = StylePresets::default_presets();
        let style = EntryStyle { text_color: [42, 0, 0, 255], ..EntryStyle::default() };
        presets.replace(1, style.clone());
        assert_eq!(presets.get(1), Some(style.clone()));
        presets.replace(6, style.clone());
        assert_eq!(presets.get(6), Some(style.clone()));
        presets.replace(999, style);
        assert_eq!(presets.len(), INITIAL_PRESET_SLOTS);
    }

    #[test]
    fn remove_empties_the_slot() {
        let mut presets = StylePresets::default_presets();
        presets.remove(0);
        presets.remove(999);

        assert!(presets.get(0).is_none());
        assert_eq!(presets.len(), INITIAL_PRESET_SLOTS);
    }

    #[test]
    fn get_returns_none_for_empty_slots_and_out_of_range() {
        let presets = StylePresets::default_presets();
        assert!(presets.get(5).is_none());
        assert!(presets.get(999).is_none());
    }
}
