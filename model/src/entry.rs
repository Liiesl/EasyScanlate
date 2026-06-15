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

    /// Whether this quad's AABB overlaps `rect` (`[x, y, w, h]` in the same
    /// coordinate space).
    pub fn intersects_rect(&self, rect: [f32; 4]) -> bool {
        let [x0, y0, x1, y1] = self.bounds();
        !(x1 <= rect[0] || x0 >= rect[0] + rect[2] || y1 <= rect[1] || y0 >= rect[1] + rect[3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(x0: f32, y0: f32, x1: f32, y1: f32) -> Quad {
        Quad {
            points: [[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
        }
    }

    #[test]
    fn intersects_rect_overlaps() {
        let q = quad(0.0, 0.0, 100.0, 100.0);
        assert!(q.intersects_rect([50.0, 50.0, 100.0, 100.0]));
    }

    #[test]
    fn intersects_rect_disjoint() {
        let q = quad(0.0, 0.0, 100.0, 100.0);
        assert!(!q.intersects_rect([200.0, 200.0, 10.0, 10.0]));
    }

    #[test]
    fn intersects_rect_edge_touching_is_not_overlap() {
        let q = quad(0.0, 0.0, 100.0, 100.0);
        assert!(!q.intersects_rect([100.0, 0.0, 10.0, 100.0]), "x1 <= rect[0]");
        assert!(!q.intersects_rect([0.0, 100.0, 100.0, 10.0]), "y1 <= rect[1]");
    }

    #[test]
    fn intersects_rect_fully_contained() {
        let q = quad(10.0, 10.0, 90.0, 90.0);
        assert!(q.intersects_rect([0.0, 0.0, 100.0, 100.0]));
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
