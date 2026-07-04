use scanlateit_model::{EntryId, EntryStyle, Quad};

/// View-model entry: what the overlay draws, resolved from the model with the
/// selected profile's translation and the per-entry style already applied.
#[derive(Clone)]
pub struct OverlayEntry<'a> {
    pub id: EntryId,
    pub text: &'a str,
    /// The entry's free-transformed box in image pixels (may be skewed).
    pub quad: Quad,
    /// `[min_x, min_y, max_x, max_y]` of [`OverlayEntry::quad`], in image
    /// pixels: the box the text is fitted to.
    pub bounds: [f32; 4],
    pub style: EntryStyle,
    /// True when this entry is the one picked in the style panel.
    pub selected: bool,
    /// True when the box is a user-adjusted view quad instead of the plain OCR quad.
    pub quad_overridden: bool,
    /// True while the entry is being edited inline: only the box is drawn.
    pub hide_text: bool,
}

impl<'a> OverlayEntry<'a> {
    /// Convenience: recomputed bounds from `quad` (single source; `bounds` field
    /// is kept for backwards compat but this is the canonical derived value).
    pub fn computed_bounds(&self) -> [f32; 4] {
        self.quad.bounds()
    }
}
