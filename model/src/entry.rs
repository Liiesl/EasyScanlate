/// Stable identifier for an image within a chapter `Project`. Assigned by
/// [`Project`] when an image is added, never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct ImageId(pub u64);

/// Metadata for one image in a chapter `Project`. Immutable after `Project`
/// creation (images are append-only, like `OcrResult` entries); the pixels and
/// decode cache live outside the model.
///
/// At runtime `path` is an absolute filesystem path (original file or
/// extracted temp dir after `.mmtl` load). Inside `.mmtl`/`project.xml` it is
/// stored zip-relative as `images/<id>_<basename>` and resolved to absolute on
/// load (`mmtl/src/zip.rs`).
#[derive(Debug, Clone)]
pub struct ImageMeta {
    pub id: ImageId,
    pub path: String,
    pub width: f32,
    pub height: f32,
}

/// Stable identifier for an OCR entry. Assigned by [`OcrResult`] on append,
/// never reused, survives soft-deletes. Unique within the whole `Project`
/// (chapter), not per image.
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
    /// Upright-snap tolerance: quads tilted less than this collapse to their
    /// AABB so near-flat detector jitter doesn't produce rotated rendering,
    /// styling crops, or inpaint masks. 5 degrees (~0.087 rad).
    pub const SNAP_ANGLE_RAD: f32 = 0.087_266_46;
    /// Corner-deviation floor (px) for the upright snap.
    pub const SNAP_CORNER_PX: f32 = 1.5;
    /// Corner-deviation allowance as a fraction of the quad's short side.
    pub const SNAP_CORNER_RATIO: f32 = 0.02;

    /// Axis-aligned quad from `[min_x, min_y, max_x, max_y]` bounds.
    pub fn from_xyxy(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self {
            points: [[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
        }
    }

    /// Whether this quad is within snap tolerance of flat-upright: its
    /// corners sit on its AABB corners (within `max(1.5px, 2% of short
    /// side)`) and its top edge is within 5 degrees of horizontal.
    pub fn is_near_upright(&self) -> bool {
        let [min_x, min_y, max_x, max_y] = self.bounds();
        if !(min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite()) {
            return true;
        }
        let w = max_x - min_x;
        let h = max_y - min_y;
        if w <= 0.0 || h <= 0.0 {
            return true;
        }
        let ordered = self.ordered();
        let corners = [
            [min_x, min_y],
            [max_x, min_y],
            [max_x, max_y],
            [min_x, max_y],
        ];
        let mut max_dev2 = 0.0f32;
        for (point, corner) in ordered.iter().zip(corners) {
            let dx = point[0] - corner[0];
            let dy = point[1] - corner[1];
            max_dev2 = max_dev2.max(dx * dx + dy * dy);
        }
        let tol = Self::SNAP_CORNER_PX.max(Self::SNAP_CORNER_RATIO * w.min(h));
        if max_dev2 > tol * tol {
            return false;
        }
        let dx = ordered[1][0] - ordered[0][0];
        let dy = ordered[1][1] - ordered[0][1];
        if dx <= 0.0 {
            return false;
        }
        let mut angle = dy.atan2(dx).abs();
        if angle > std::f32::consts::FRAC_PI_2 {
            angle = std::f32::consts::PI - angle;
        }
        angle <= Self::SNAP_ANGLE_RAD
    }

    /// Collapse to the AABB when [`Quad::is_near_upright`], else `self`.
    /// Single choke point so auto-OCR rendering, styling, and inpaint agree
    /// on what counts as upright.
    pub fn snap_if_near_upright(self) -> Self {
        if self.is_near_upright() {
            let [min_x, min_y, max_x, max_y] = self.bounds();
            Self::from_xyxy(min_x, min_y, max_x, max_y)
        } else {
            self
        }
    }

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

    /// Reorder the points so index `0..4` matches the bounding-box corners
    /// TL, TR, BR, BL, by assigning each AABB corner its nearest unused point
    /// (correct for any convex quad).
    pub fn ordered(self) -> [[f32; 2]; 4] {
        let [min_x, min_y, max_x, max_y] = self.bounds();
        let corners = [
            [min_x, min_y],
            [max_x, min_y],
            [max_x, max_y],
            [min_x, max_y],
        ];
        let mut used = [false; 4];
        let mut ordered = [[0.0; 2]; 4];
        for (corner_index, corner) in corners.iter().enumerate() {
            let mut best = None;
            for (index, point) in self.points.iter().enumerate() {
                if used[index] {
                    continue;
                }
                let dx = point[0] - corner[0];
                let dy = point[1] - corner[1];
                if best.is_none_or(|(_, best_d2)| dx * dx + dy * dy < best_d2) {
                    best = Some((index, dx * dx + dy * dy));
                }
            }
            let (index, _) = best.expect("quad has four points");
            used[index] = true;
            ordered[corner_index] = self.points[index];
        }
        ordered
    }

    /// Rotate every point around `center` by `angle` (radians, counter-
    /// clockwise in the image coordinate space).
    pub fn rotate(mut self, center: [f32; 2], angle: f32) -> Self {
        let (sin, cos) = angle.sin_cos();
        for point in &mut self.points {
            let dx = point[0] - center[0];
            let dy = point[1] - center[1];
            point[0] = center[0] + dx * cos - dy * sin;
            point[1] = center[1] + dx * sin + dy * cos;
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
    fn ordered_matches_aabb_corners() {
        let q = quad(10.0, 20.0, 90.0, 80.0);
        let ordered = q.ordered();
        assert_eq!(ordered[0], [10.0, 20.0]);
        assert_eq!(ordered[1], [90.0, 20.0]);
        assert_eq!(ordered[2], [90.0, 80.0]);
        assert_eq!(ordered[3], [10.0, 80.0]);
    }

    #[test]
    fn ordered_survives_rotation() {
        let q = quad(0.0, 0.0, 100.0, 100.0).rotate([50.0, 50.0], 0.7);
        let ordered = q.ordered();
        let [min_x, min_y, max_x, max_y] = q.bounds();
        let corners = [
            [min_x, min_y],
            [max_x, min_y],
            [max_x, max_y],
            [min_x, max_y],
        ];
        for (point, corner) in ordered.iter().zip(corners) {
            assert!((point[0] - corner[0]).abs() < 1.0 || (point[1] - corner[1]).abs() < 1.0);
        }
        assert!((ordered[1][0] - ordered[0][0]).abs() > 0.0 || (ordered[1][1] - ordered[0][1]).abs() > 0.0);
    }

    #[test]
    fn rotate_spins_points_around_the_center() {
        let q = quad(0.0, 0.0, 100.0, 100.0);
        let rotated = q.rotate([50.0, 50.0], std::f32::consts::PI / 2.0);
        for (point, expected) in rotated.points.iter().zip([
            [100.0, 0.0],
            [100.0, 100.0],
            [0.0, 100.0],
            [0.0, 0.0],
        ]) {
            assert!((point[0] - expected[0]).abs() < 1e-3, "x: {point:?}");
            assert!((point[1] - expected[1]).abs() < 1e-3, "y: {point:?}");
        }
    }

    #[test]
    fn rotate_keeps_the_center_stationary() {
        let q = quad(10.0, 20.0, 90.0, 80.0);
        let center = [50.0, 50.0];
        let rotated = q.rotate(center, 0.37);
        let centroid = |points: [[f32; 2]; 4]| {
            [
                points.iter().map(|p| p[0]).sum::<f32>() / 4.0,
                points.iter().map(|p| p[1]).sum::<f32>() / 4.0,
            ]
        };
        let c = centroid(rotated.points);
        assert!((c[0] - center[0]).abs() < 1e-3);
        assert!((c[1] - center[1]).abs() < 1e-3);
        let restored = rotated.rotate(center, -0.37);
        for (point, original) in restored.points.iter().zip(q.points) {
            assert!((point[0] - original[0]).abs() < 1e-3);
            assert!((point[1] - original[1]).abs() < 1e-3);
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

    #[test]
    fn upright_quad_snaps_to_aabb() {
        let q = quad(10.0, 20.0, 90.0, 80.0);
        assert!(q.is_near_upright());
        assert_eq!(q.snap_if_near_upright(), q);
    }

    #[test]
    fn small_jitter_snaps_to_upright() {
        // ~2deg tilt on an 80x60 box: corner drift ~1.4px, inside tolerance.
        let q = quad(10.0, 20.0, 90.0, 80.0).rotate([50.0, 50.0], 0.035);
        assert!(q.is_near_upright(), "2deg jitter should snap: {q:?}");
        let snapped = q.snap_if_near_upright();
        let [x0, y0, x1, y1] = snapped.bounds();
        assert_eq!(snapped.points, [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]);
    }

    #[test]
    fn five_degree_tilt_is_the_boundary() {
        let center = [50.0, 50.0];
        let just_inside = quad(0.0, 20.0, 100.0, 80.0).rotate(center, 0.08);
        let just_outside = quad(0.0, 20.0, 100.0, 80.0).rotate(center, 0.12);
        assert!(just_inside.is_near_upright(), "4.6deg should snap");
        assert!(!just_outside.is_near_upright(), "6.9deg should stay rotated");
        assert_eq!(
            just_outside.snap_if_near_upright().points,
            just_outside.points,
            "rotated quad must be preserved as-is"
        );
    }

    #[test]
    fn clearly_rotated_quad_is_preserved() {
        let q = quad(0.0, 0.0, 100.0, 50.0).rotate([50.0, 25.0], 0.5);
        assert!(!q.is_near_upright());
        assert_eq!(q.snap_if_near_upright().points, q.points);
    }
}

/// A single OCR result line. Entries are immutable once appended; edits are
/// represented by soft-deleting (see [`OcrResult::soft_delete`]).
#[derive(Debug, Clone)]
pub struct OcrEntry {
    pub id: EntryId,
    /// Which image this entry belongs to (chapter-wide `Project`).
    pub image_id: ImageId,
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
