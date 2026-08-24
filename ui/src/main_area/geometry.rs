//! Shared geometry helpers for the main area.
//!
//! Single source for quad math, scale and layout helpers used by both the
//! viewer (hit-testing, interaction) and the overlay (painting, warping).
//! All quad ordering delegates to [`scanlateit_model::Quad::ordered`] so
//! `overlay` and `viewer` never re-implement it.

use iced::advanced::graphics::geometry::{self, Path};
use iced::{Point, Rectangle, Vector};

use scanlateit_model::Quad;

/// Viewport/content scale helper: `frame_width / source_width`.
#[inline]
pub fn tile_scale(frame_width: f32, source_width: f32) -> f32 {
    frame_width / source_width.max(1.0)
}

/// Width available to tile content: scrollbar gutter reserved on the right.
#[inline]
pub fn content_width(width: f32, scrollbar_width: f32, scrollbar_margin: f32) -> f32 {
    (width - scrollbar_width - scrollbar_margin).max(0.0)
}

/// The bounding box of a quad as `[min_x, min_y, max_x, max_y]`.
pub fn quad_bounds(quad: [[f32; 2]; 4]) -> [f32; 4] {
    let min_x = quad.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
    let min_y = quad.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
    let max_x = quad.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max);
    let max_y = quad.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max);
    [min_x, min_y, max_x, max_y]
}

/// Reorders quad points to TL/TR/BR/BL. Delegates to [`Quad::ordered`].
pub fn order_quad(quad: [[f32; 2]; 4]) -> [[f32; 2]; 4] {
    Quad { points: quad }.ordered()
}

/// The quad's local rect when it is a rotated rectangle: `(tl, w, h, angle)`.
pub fn rotated_rect_geometry(quad: [[f32; 2]; 4]) -> Option<(Point, f32, f32, f32)> {
    let top = [quad[1][0] - quad[0][0], quad[1][1] - quad[0][1]];
    let bottom = [quad[2][0] - quad[3][0], quad[2][1] - quad[3][1]];
    let left = [quad[3][0] - quad[0][0], quad[3][1] - quad[0][1]];
    let right = [quad[2][0] - quad[1][0], quad[2][1] - quad[1][1]];
    let w = (top[0] * top[0] + top[1] * top[1]).sqrt();
    let h = (left[0] * left[0] + left[1] * left[1]).sqrt();
    if w <= f32::EPSILON || h <= f32::EPSILON {
        return None;
    }
    let w_bot = (bottom[0] * bottom[0] + bottom[1] * bottom[1]).sqrt();
    let h_right = (right[0] * right[0] + right[1] * right[1]).sqrt();
    if (w - w_bot).abs() / w.max(w_bot) > 0.05 || (h - h_right).abs() / h.max(h_right) > 0.05 {
        return None;
    }
    let dot = top[0] * left[0] + top[1] * left[1];
    if dot.abs() / (w * h) > 0.05 {
        return None;
    }
    let angle = top[1].atan2(top[0]);
    let center_x = (quad[0][0] + quad[1][0] + quad[2][0] + quad[3][0]) / 4.0;
    let center_y = (quad[0][1] + quad[1][1] + quad[2][1] + quad[3][1]) / 4.0;
    let unrotated_pos = Point::new(center_x - w / 2.0, center_y - h / 2.0);
    Some((unrotated_pos, w, h, angle))
}

/// The quad as a closed 4-point path.
pub fn quad_path(quad: [[f32; 2]; 4]) -> Path {
    Path::new(|builder| {
        builder.move_to(Point::new(quad[0][0], quad[0][1]));
        builder.line_to(Point::new(quad[1][0], quad[1][1]));
        builder.line_to(Point::new(quad[2][0], quad[2][1]));
        builder.line_to(Point::new(quad[3][0], quad[3][1]));
        builder.close();
    })
}

/// Affine map `M = R(angle2) * S * R(angle1)` that maps an axis-aligned rect onto a quad.
#[derive(Debug, Clone, Copy)]
pub struct QuadTransform {
    pub angle1: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub angle2: f32,
}

/// Rotates/skews current frame so drawing in the axis-aligned rect lands on the quad.
pub fn apply_quad_transform<F>(
    frame: &mut F,
    transform: &QuadTransform,
    position: Point,
    width: f32,
    height: f32,
) where
    F: geometry::frame::Backend,
{
    let center = Point::new(position.x + width / 2.0, position.y + height / 2.0);
    frame.translate(Vector::new(center.x, center.y));
    frame.rotate(transform.angle2);
    frame.scale_nonuniform(Vector::new(transform.scale_x, transform.scale_y));
    frame.rotate(transform.angle1);
    frame.translate(Vector::new(-center.x, -center.y));
}

/// Least-squares affine 2x2 mapping rect corners onto quad.
pub fn fit_affine(quad: [[f32; 2]; 4], width: f32, height: f32) -> Option<(f32, f32, f32, f32)> {
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let half_w = width / 2.0;
    let half_h = height / 2.0;
    let center_x = (quad[0][0] + quad[1][0] + quad[2][0] + quad[3][0]) / 4.0;
    let center_y = (quad[0][1] + quad[1][1] + quad[2][1] + quad[3][1]) / 4.0;
    let lx = [-half_w, half_w, half_w, -half_w];
    let ly = [-half_h, -half_h, half_h, half_h];
    let mut m00 = 0.0;
    let mut m10 = 0.0;
    let mut m01 = 0.0;
    let mut m11 = 0.0;
    for index in 0..4 {
        let (qx, qy) = (quad[index][0] - center_x, quad[index][1] - center_y);
        m00 += lx[index] * qx;
        m10 += lx[index] * qy;
        m01 += ly[index] * qx;
        m11 += ly[index] * qy;
    }
    m00 /= width * width;
    m10 /= width * width;
    m01 /= height * height;
    m11 /= height * height;
    Some((m00, m01, m10, m11))
}

/// SVD of 2x2: `A = R(beta) * S * R(-alpha)`.
pub fn svd2(m00: f32, m01: f32, m10: f32, m11: f32) -> (f32, f32, f32, f32) {
    let a = m00 * m00 + m10 * m10;
    let b = m00 * m01 + m10 * m11;
    let c = m01 * m01 + m11 * m11;
    let trace = a + c;
    let discriminant = ((a - c) * (a - c) + 4.0 * b * b).sqrt();
    let lambda1 = (trace + discriminant) / 2.0;
    let lambda2 = (trace - discriminant) / 2.0;
    let s1 = lambda1.sqrt();
    let s2 = lambda2.sqrt();
    let (v1x, v1y) = if b.abs() > f32::EPSILON {
        let len = (b * b + (lambda1 - a) * (lambda1 - a)).sqrt();
        (b / len, (lambda1 - a) / len)
    } else {
        (1.0, 0.0)
    };
    let alpha = v1y.atan2(v1x);
    let u1x = (m00 * v1x + m01 * v1y) / s1;
    let u1y = (m10 * v1x + m11 * v1y) / s1;
    let beta = u1y.atan2(u1x);
    (s1, s2, beta, alpha)
}

/// Perspective map from `box_rect` (uv) onto `quad`, identical to warp::perspective_map.
/// Maps a point in `box_rect` coordinates to its perspective position on `quad`.
pub fn perspective_map(quad: [[f32; 2]; 4], box_rect: Rectangle, p: Point) -> Point {
    let u = ((p.x - box_rect.x) / box_rect.width.max(1.0)).clamp(0.0, 1.0);
    let v = ((p.y - box_rect.y) / box_rect.height.max(1.0)).clamp(0.0, 1.0);
    let [x0, y0] = quad[0];
    let [x1, y1] = quad[1];
    let [x2, y2] = quad[2];
    let [x3, y3] = quad[3];
    let dx1 = x1 - x2;
    let dx2 = x3 - x2;
    let dy1 = y1 - y2;
    let dy2 = y3 - y2;
    let denom = dx1 * dy2 - dy1 * dx2;
    let (a, b, c, d, e, f, g, h) = if denom.abs() < 1e-7 {
        (x1 - x0, x3 - x0, x0, y1 - y0, y3 - y0, y0, 0.0, 0.0)
    } else {
        let sx = x0 - x1 + x2 - x3;
        let sy = y0 - y1 + y2 - y3;
        let g = (sx * dy2 - sy * dx2) / denom;
        let h = (dx1 * sy - dy1 * sx) / denom;
        (
            x1 - x0 + g * x1,
            x3 - x0 + h * x3,
            x0,
            y1 - y0 + g * y1,
            y3 - y0 + h * y3,
            y0,
            g,
            h,
        )
    };
    let w = g * u + h * v + 1.0;
    Point::new((a * u + b * v + c) / w, (d * u + e * v + f) / w)
}

/// Perspective-correct rounded rectangle path: a `radius`-rounded `rect`
/// projected onto `quad` via the homography. Straight edges stay straight;
/// quarter-circle corners are tessellated into line segments (8 per corner)
/// before projection, so the curve is perspective-correct.
pub fn perspective_rounded_rect_path(
    quad: [[f32; 2]; 4],
    rect: Rectangle,
    radius: f32,
) -> Path {
    let w = rect.width;
    let h = rect.height;
    if w <= 0.0 || h <= 0.0 {
        return quad_path(quad);
    }
    let r = radius.clamp(0.0, w.min(h) / 2.0);
    if r <= 0.5 {
        return quad_path(quad);
    }
    let x = rect.x;
    let y = rect.y;
    let cx1 = x + r;
    let cx2 = x + w - r;
    let cy1 = y + r;
    let cy2 = y + h - r;
    const SEGMENTS: usize = 8;
    Path::new(|builder| {
        let map = |p: Point| perspective_map(quad, rect, p);
        // Helper to emit mapped point: first point uses move_to.
        let mut first = true;
        let mut push = |pt: Point| {
            let mp = map(pt);
            if first {
                builder.move_to(mp);
                first = false;
            } else {
                builder.line_to(mp);
            }
        };
        // Top edge
        push(Point::new(x + r, y));
        push(Point::new(x + w - r, y));
        // Top-right arc: center (cx2, cy1), angle -90° -> 0°
        for i in 1..=SEGMENTS {
            let t = i as f32 / SEGMENTS as f32;
            let ang = -std::f32::consts::FRAC_PI_2 + t * std::f32::consts::FRAC_PI_2;
            push(Point::new(cx2 + r * ang.cos(), cy1 + r * ang.sin()));
        }
        // Right edge
        push(Point::new(x + w, y + h - r));
        // Bottom-right arc: center (cx2, cy2), 0° -> 90°
        for i in 1..=SEGMENTS {
            let t = i as f32 / SEGMENTS as f32;
            let ang = t * std::f32::consts::FRAC_PI_2;
            push(Point::new(cx2 + r * ang.cos(), cy2 + r * ang.sin()));
        }
        // Bottom edge
        push(Point::new(x + r, y + h));
        // Bottom-left arc: center (cx1, cy2), 90° -> 180°
        for i in 1..=SEGMENTS {
            let t = i as f32 / SEGMENTS as f32;
            let ang = std::f32::consts::FRAC_PI_2 + t * std::f32::consts::FRAC_PI_2;
            push(Point::new(cx1 + r * ang.cos(), cy2 + r * ang.sin()));
        }
        // Left edge
        push(Point::new(x, y + r));
        // Top-left arc: center (cx1, cy1), 180° -> 270°
        for i in 1..=SEGMENTS {
            let t = i as f32 / SEGMENTS as f32;
            let ang = std::f32::consts::PI + t * std::f32::consts::FRAC_PI_2;
            push(Point::new(cx1 + r * ang.cos(), cy1 + r * ang.sin()));
        }
        builder.close();
    })
}

/// Affine transform mapping `width x height` rect onto `quad`.
pub fn quad_transform(quad: [[f32; 2]; 4], width: f32, height: f32) -> Option<QuadTransform> {
    let (m00, m01, m10, m11) = fit_affine(quad, width, height)?;
    if m00 * m11 - m01 * m10 <= 0.0 {
        return None;
    }
    if (m00 - 1.0).abs() < 1e-3 && (m11 - 1.0).abs() < 1e-3 && m01.abs() < 1e-3 && m10.abs() < 1e-3 {
        return None;
    }
    let (mut s1, mut s2, beta, alpha) = svd2(m00, m01, m10, m11);
    s1 = s1.max(0.01);
    s2 = s2.max(0.01);
    let stretch = 1.0005;
    Some(QuadTransform {
        angle1: -alpha,
        scale_x: s1 * stretch,
        scale_y: s2 / stretch,
        angle2: beta,
    })
}
