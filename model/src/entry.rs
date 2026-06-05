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

    /// Move every point by `(dx, dy)`.
    pub fn translate(mut self, dx: f32, dy: f32) -> Self {
        for point in &mut self.points {
            point[0] += dx;
            point[1] += dy;
        }
        self
    }

    /// Refit the quad inside `new_bounds` by scaling each point's offset from
    /// `old_bounds`' top-left, so the shape's proportions are preserved.
    pub fn refit(mut self, old: [f32; 4], new: [f32; 4]) -> Self {
        let scale_x = (new[2] - new[0]) / (old[2] - old[0]).max(f32::EPSILON);
        let scale_y = (new[3] - new[1]) / (old[3] - old[1]).max(f32::EPSILON);
        for point in &mut self.points {
            point[0] = new[0] + (point[0] - old[0]) * scale_x;
            point[1] = new[1] + (point[1] - old[1]) * scale_y;
        }
        self
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
