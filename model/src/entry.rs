/// Stable identifier for an OCR entry. Assigned by [`OcrResult`] on append,
/// never reused, survives soft-deletes.
///
/// [`OcrResult`]: crate::OcrResult
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryId(pub u64);

/// Where an entry's text came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntrySource {
    /// Produced by the automatic OCR engine over the whole image.
    AutoOcr,
    /// Manually added by the user (future feature).
    #[allow(dead_code)]
    Manual,
}

/// A quadrilateral region in image pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    pub points: [[f32; 2]; 4],
}

impl Quad {
    /// Axis-aligned bounding box as `[min_x, min_y, max_x, max_y]`.
    pub fn bounds(&self) -> [f32; 4] {
        let min_x = self.points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
        let min_y = self.points.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
        let max_x = self.points.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max);
        let max_y = self.points.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max);
        [min_x, min_y, max_x, max_y]
    }
}

/// A single OCR result line. Entries are immutable once appended; edits are
/// represented by soft-deleting (see [`OcrResult::soft_delete`]).
#[derive(Debug, Clone)]
pub struct OcrEntry {
    pub id: EntryId,
    /// Auto or manual OCR. Not read by the UI yet.
    #[allow(dead_code)]
    pub source: EntrySource,
    pub text: String,
    pub score: f32,
    pub quad: Quad,
    pub deleted: bool,
}

/// Payload for appending a new entry. Identity is assigned by the store.
#[derive(Debug, Clone)]
pub struct NewEntry {
    pub source: EntrySource,
    pub text: String,
    pub score: f32,
    pub quad: Quad,
}
