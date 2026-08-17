pub mod cache;
pub mod circle;
pub mod entry;
pub mod fit;
pub mod gradient;
pub mod style;
pub mod text;
pub mod warp;

pub use entry::OverlayEntry;
pub(crate) use circle::fit_circle_metrics;
#[allow(unused_imports)]
pub(crate) use fit::{fit_font_metrics, fit_font_size};
pub(crate) use style::styled_font;
pub use crate::main_area::geometry::order_quad;

use iced::advanced::graphics::geometry::{self, Fill, Path, Stroke, Text};
use iced::advanced::text::Alignment as TextAlignment;
use iced::{Color, Font, Pixels, Point, Rectangle, Size};

use scanlateit_model::TextAlign;

use crate::color::rgba_to_color;
use crate::main_area::geometry::{
    apply_quad_transform, quad_path, quad_transform, rotated_rect_geometry, QuadTransform,
};

use self::gradient::fill_gradient_text;
use self::text::LINE_HEIGHT;
use self::warp::{affine_error, draw_warped_text};

const SELECTED_COLOR: Color = Color::from_rgba8(92, 190, 255, 1.0);
const SELECTED_WIDTH: f32 = 2.0;

/// Draws one translucent box + label per entry on top of the image inside `frame`.
pub fn draw_entries<'a, I, F>(
    frame: &mut F,
    entries: I,
    font: Font,
    image_width: f32,
    hide_text: bool,
) where
    F: geometry::frame::Backend,
    I: IntoIterator<Item = &'a OverlayEntry<'a>>,
{
    let scale = frame.width() / image_width.max(1.0);
    for entry in entries {
        if hide_text {
            continue;
        }
        let quad = entry.quad.points.map(|p| [p[0] * scale, p[1] * scale]);
        let rotated = rotated_rect_geometry(quad).and_then(|(tl, w, h, angle)| {
            let upright = angle.rem_euclid(2.0 * std::f32::consts::PI);
            let is_upright = upright.abs() < 0.01 || (upright - 2.0 * std::f32::consts::PI).abs() < 0.01;
            if is_upright {
                None
            } else {
                Some((
                    tl,
                    w,
                    h,
                    QuadTransform {
                        angle1: 0.0,
                        scale_x: 1.0005,
                        scale_y: 1.0 / 1.0005,
                        angle2: angle,
                    },
                ))
            }
        });

        let (layout_position, layout_width, layout_height, layout_transform) = match rotated {
            Some((tl, w, h, t)) => (tl, w, h, Some(t)),
            None => {
                let w_top = ((quad[1][0] - quad[0][0]).powi(2) + (quad[1][1] - quad[0][1]).powi(2)).sqrt();
                let w_bot = ((quad[2][0] - quad[3][0]).powi(2) + (quad[2][1] - quad[3][1]).powi(2)).sqrt();
                let h_left = ((quad[3][0] - quad[0][0]).powi(2) + (quad[3][1] - quad[0][1]).powi(2)).sqrt();
                let h_right = ((quad[2][0] - quad[1][0]).powi(2) + (quad[2][1] - quad[1][1]).powi(2)).sqrt();
                let w = ((w_top + w_bot) / 2.0).max(1.0);
                let h = ((h_left + h_right) / 2.0).max(1.0);
                let center_x = (quad[0][0] + quad[1][0] + quad[2][0] + quad[3][0]) / 4.0;
                let center_y = (quad[0][1] + quad[1][1] + quad[2][1] + quad[3][1]) / 4.0;
                let pos = Point::new(center_x - w / 2.0, center_y - h / 2.0);
                let transform = quad_transform(quad, w, h);
                (pos, w, h, transform)
            }
        };

        let path = match layout_transform {
            Some(_) => quad_path(quad),
            None => Path::rounded_rectangle(
                layout_position,
                Size::new(layout_width, layout_height),
                iced::border::Radius::from(entry.style.bg_radius * scale),
            ),
        };
        frame.fill(&path, Fill::from(rgba_to_color(entry.style.bg_color)));
        if entry.selected {
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(SELECTED_COLOR)
                    .with_width(SELECTED_WIDTH),
            );
        }
        let wrap_width = layout_width.max(8.0);
        if entry.hide_text {
            continue;
        }
        let styled = styled_font(font, &entry.style);
        let box_rect = Rectangle::new(layout_position, Size::new(layout_width, layout_height));
        let warp = rotated.is_none()
            && layout_transform.is_some()
            && affine_error(quad, layout_width, layout_height) > warp::warp_threshold();
        let stroke = (entry.style.stroke_width > 0.0).then(|| {
            (rgba_to_color(entry.style.stroke_color), entry.style.stroke_width * scale)
        });
        let gradient = entry.style.text_gradient.then(|| {
            (entry.style.gradient_dir, entry.style.gradient_a, entry.style.gradient_b)
        });

        if entry.style.text_align == TextAlign::Circular {
            let (size, lines) =
                fit_circle_metrics(entry.text, styled, Size::new(wrap_width, layout_height));
            let line_height = size * LINE_HEIGHT;
            let total_height = lines.last().map_or(0.0, |line| line.y + line_height);
            let y_offset = (layout_height - total_height).max(0.0) / 2.0;
            let block_rect = Rectangle::new(
                Point::new(layout_position.x, layout_position.y + y_offset),
                Size::new(wrap_width, total_height),
            );
            if warp {
                for line in &lines {
                    let text = Text {
                        content: line.content.clone(),
                        position: Point::new(
                            layout_position.x + wrap_width / 2.0,
                            layout_position.y + y_offset + line.y,
                        ),
                        max_width: line.chord,
                        size: Pixels(size),
                        color: rgba_to_color(entry.style.text_color),
                        font: styled,
                        align_x: TextAlignment::Center,
                        ..Text::default()
                    };
                    draw_warped_text(frame, &text, box_rect, quad, stroke, gradient);
                }
            } else {
                if let Some(transform) = &layout_transform {
                    frame.push_transform();
                    apply_quad_transform(
                        frame,
                        transform,
                        layout_position,
                        layout_width,
                        layout_height,
                    );
                }
                for line in &lines {
                    let text = Text {
                        content: line.content.clone(),
                        position: Point::new(
                            layout_position.x + wrap_width / 2.0,
                            layout_position.y + y_offset + line.y,
                        ),
                        max_width: line.chord,
                        size: Pixels(size),
                        color: rgba_to_color(entry.style.text_color),
                        font: styled,
                        align_x: TextAlignment::Center,
                        ..Text::default()
                    };
                    if entry.style.text_gradient {
                        fill_gradient_text(
                            frame,
                            &text,
                            block_rect,
                            entry.style.gradient_dir,
                            entry.style.gradient_a,
                            entry.style.gradient_b,
                            (entry.style.stroke_width > 0.0).then(|| {
                                (rgba_to_color(entry.style.stroke_color), entry.style.stroke_width * scale)
                            }),
                            layout_transform.as_ref(),
                            layout_position,
                            layout_width,
                            layout_height,
                        );
                    } else {
                        if entry.style.stroke_width > 0.0 {
                            frame.stroke_text(
                                text.clone(),
                                Stroke::default()
                                    .with_color(rgba_to_color(entry.style.stroke_color))
                                    .with_width(entry.style.stroke_width * scale),
                            );
                        }
                        frame.fill_text(text);
                    }
                }
                if layout_transform.is_some() {
                    frame.pop_transform();
                }
            }
            continue;
        }

        let (size, fitted_height) = fit_font_metrics(
            entry.text,
            styled,
            Size::new(wrap_width, layout_height),
        );
        let y_offset = (layout_height - fitted_height).max(0.0) / 2.0;
        let block_rect = Rectangle::new(
            Point::new(layout_position.x, layout_position.y + y_offset),
            Size::new(wrap_width, fitted_height),
        );
        let (align_x, text_x) = match entry.style.text_align {
            TextAlign::Circular => (TextAlignment::Default, layout_position.x),
            TextAlign::Left => (TextAlignment::Left, layout_position.x),
            TextAlign::Center => (TextAlignment::Center, layout_position.x + wrap_width / 2.0),
            TextAlign::Right => (TextAlignment::Right, layout_position.x + wrap_width),
        };
        let text = Text {
            content: entry.text.to_string(),
            position: Point::new(text_x, layout_position.y + y_offset),
            max_width: wrap_width,
            size: Pixels(size),
            color: rgba_to_color(entry.style.text_color),
            font: styled,
            align_x,
            ..Text::default()
        };

        if warp {
            draw_warped_text(frame, &text, box_rect, quad, stroke, gradient);
        } else {
            if let Some(transform) = &layout_transform {
                frame.push_transform();
                apply_quad_transform(
                    frame,
                    transform,
                    layout_position,
                    layout_width,
                    layout_height,
                );
            }
            if entry.style.text_gradient {
                fill_gradient_text(
                    frame,
                    &text,
                    block_rect,
                    entry.style.gradient_dir,
                    entry.style.gradient_a,
                    entry.style.gradient_b,
                    (entry.style.stroke_width > 0.0).then(|| {
                        (rgba_to_color(entry.style.stroke_color), entry.style.stroke_width * scale)
                    }),
                    layout_transform.as_ref(),
                    layout_position,
                    layout_width,
                    layout_height,
                );
            } else {
                if entry.style.stroke_width > 0.0 {
                    frame.stroke_text(
                        text.clone(),
                        Stroke::default()
                            .with_color(rgba_to_color(entry.style.stroke_color))
                            .with_width(entry.style.stroke_width * scale),
                    );
                }
                frame.fill_text(text);
            }
            if layout_transform.is_some() {
                frame.pop_transform();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::gradient::{gradient_t, lerp_color};
    use super::warp::{affine_error, perspective_map, shape_warp_layout, warp_threshold};
    use super::circle::fit_circle_metrics;
    use super::fit::{fit_font_metrics, fit_font_size};
    use super::text::{measure_text, LINE_HEIGHT};
    use crate::main_area::geometry::{quad_bounds, quad_transform, rotated_rect_geometry, svd2};
    use iced::{alignment, Color, Font, Point, Rectangle, Size};

    #[test]
    fn measure_text_is_sane() {
        let size = measure_text("hello world", Font::DEFAULT, 20.0, 400.0);
        assert!(size.width > 30.0, "width {} too small", size.width);
        assert!(size.width < 300.0, "width {} too large", size.width);
        assert!(size.height > 15.0, "height {} too small", size.height);
        assert!(size.height < 45.0, "height {} too large", size.height);
    }

    #[test]
    fn circle_lines_follow_the_chords() {
        let bounds = Size::new(300.0, 150.0);
        let (size, lines) = fit_circle_metrics(
            "hello world this is a longer bubble line for manhwa",
            Font::DEFAULT,
            bounds,
        );
        assert!(lines.len() >= 2, "expected several lines, got {}", lines.len());
        assert!(size > 8.0, "size {size} too small for a big bubble");
        let line_height = size * LINE_HEIGHT;
        for line in &lines {
            let measured = measure_text(&line.content, Font::DEFAULT, size, f32::INFINITY).width;
            assert!(
                measured <= line.chord + 0.5 || !line.content.contains(' '),
                "line {:?} width {measured} exceeds chord {}",
                line.content,
                line.chord
            );
            assert!(line.y + line_height <= bounds.height + 0.5);
        }
    }

    #[test]
    fn circle_fit_shrinks_to_fit_small_bubble() {
        let text = "hello world this is a longer bubble line for manhwa";
        let big = fit_circle_metrics(text, Font::DEFAULT, Size::new(300.0, 150.0)).0;
        let small = fit_circle_metrics(text, Font::DEFAULT, Size::new(120.0, 60.0)).0;
        assert!(small < big, "small bubble must fit smaller text: {small} >= {big}");
    }

    #[test]
    fn circle_fit_is_cached_and_consistent() {
        let bounds = Size::new(200.0, 100.0);
        let text = "cached circle text goes here";
        let first = fit_circle_metrics(text, Font::DEFAULT, bounds);
        let second = fit_circle_metrics(text, Font::DEFAULT, bounds);
        assert_eq!(first.0, second.0);
        assert_eq!(first.1, second.1);
    }

    #[test]
    fn circle_wraps_unspaced_runs() {
        let bounds = Size::new(120.0, 120.0);
        let long = "aaaaaaaaaa".repeat(6);
        let (_, lines) = fit_circle_metrics(&long, Font::DEFAULT, bounds);
        assert!(!lines.is_empty());
        assert!(lines.iter().all(|line| !line.content.is_empty()));
    }

    #[test]
    fn fit_grows_to_fill_big_box() {
        let bounds = Size::new(400.0, 200.0);
        let size = fit_font_size("hello world", Font::DEFAULT, bounds);
        assert!(size > 40.0, "expected grown size, got {size}");
        let measured = measure_text("hello world", Font::DEFAULT, size, bounds.width);
        assert!(
            measured.width <= bounds.width && measured.height <= bounds.height,
            "size {size} does not fit: {measured:?}"
        );
    }

    #[test]
    fn fit_shrinks_to_fit_small_box() {
        let bounds = Size::new(60.0, 20.0);
        let size = fit_font_size("hello world", Font::DEFAULT, bounds);
        let measured = measure_text("hello world", Font::DEFAULT, size, bounds.width);
        assert!(
            measured.width <= bounds.width && measured.height <= bounds.height,
            "size {size} does not fit: {measured:?}"
        );
    }

    #[test]
    fn cache_returns_consistent_results() {
        let bounds = Size::new(300.0, 100.0);
        let first = fit_font_size("hello world", Font::DEFAULT, bounds);
        let second = fit_font_size("hello world", Font::DEFAULT, bounds);
        assert_eq!(first, second);

        let wider = Size::new(600.0, 100.0);
        let grown = fit_font_size("hello world", Font::DEFAULT, wider);
        assert!(grown > first, "wider box should fit larger text: {grown} <= {first}");
    }

    #[test]
    fn axis_aligned_quad_has_no_transform() {
        let quad = [[0.0, 0.0], [100.0, 0.0], [100.0, 50.0], [0.0, 50.0]];
        assert!(quad_transform(quad, 100.0, 50.0).is_none());

        let quad = [[10.0, 20.0], [110.0, 20.0], [110.0, 70.0], [10.0, 70.0]];
        assert!(quad_transform(quad, 100.0, 50.0).is_none());
    }

    #[test]
    fn svd2_reconstructs_the_matrix() {
        let (s1, s2, beta, alpha) = (1.6f32, 0.5f32, 0.4f32, -0.9f32);
        let (ca, sa) = (alpha.cos(), alpha.sin());
        let (cb, sb) = (beta.cos(), beta.sin());
        let m00 = cb * s1 * ca + sb * s2 * sa;
        let m01 = -cb * s1 * sa + sb * s2 * ca;
        let m10 = -sb * s1 * ca + cb * s2 * sa;
        let m11 = sb * s1 * sa + cb * s2 * ca;

        let (got_s1, got_s2, got_beta, got_alpha) = svd2(m00, m01, m10, m11);
        assert!((got_s1 - s1).abs() < 1e-3, "s1: {got_s1} != {s1}");
        assert!((got_s2 - s2).abs() < 1e-3, "s2: {got_s2} != {s2}");
        let (cga, sga) = (got_alpha.cos(), got_alpha.sin());
        let (cgb, sgb) = (got_beta.cos(), got_beta.sin());
        let got00 = cgb * got_s1 * cga + sgb * got_s2 * sga;
        let got01 = cgb * got_s1 * sga - sgb * got_s2 * cga;
        let got10 = sgb * got_s1 * cga - cgb * got_s2 * sga;
        let got11 = sgb * got_s1 * sga + cgb * got_s2 * cga;
        assert!((got00 - m00).abs() < 1e-3, "m00: {got00} != {m00}");
        assert!((got01 - m01).abs() < 1e-3, "m01: {got01} != {m01}");
        assert!((got10 - m10).abs() < 1e-3, "m10: {got10} != {m10}");
        assert!((got11 - m11).abs() < 1e-3, "m11: {got11} != {m11}");
    }

    #[test]
    fn transform_maps_box_corners_onto_the_skewed_quad() {
        let quad = [[0.0, 0.0], [200.0, 30.0], [180.0, 100.0], [-20.0, 70.0]];
        let [min_x, min_y, max_x, max_y] = quad_bounds(quad);
        let width = max_x - min_x;
        let height = max_y - min_y;
        let transform = quad_transform(quad, width, height).expect("skewed quad transforms");
        let apply = |x: f32, y: f32| -> [f32; 2] {
            let [min_x, min_y, max_x, max_y] = quad_bounds(quad);
            let center = [(min_x + max_x) / 2.0, (min_y + max_y) / 2.0];
            let (mut lx, mut ly) = (x - center[0], y - center[1]);
            let (a1, a2) = (transform.angle1, transform.angle2);
            let (c1, s1) = (a1.cos(), a1.sin());
            (lx, ly) = (c1 * lx - s1 * ly, s1 * lx + c1 * ly);
            (lx, ly) = (lx * transform.scale_x, ly * transform.scale_y);
            let (c2, s2) = (a2.cos(), a2.sin());
            (lx, ly) = (c2 * lx - s2 * ly, s2 * lx + c2 * ly);
            [lx + center[0], ly + center[1]]
        };
        let corners = [[min_x, min_y], [max_x, min_y], [max_x, max_y], [min_x, max_y]];
        for (mapped, expected) in corners.iter().zip(quad.iter()) {
            let got = apply(mapped[0], mapped[1]);
            assert!(
                (got[0] - expected[0]).abs() < 1.0 && (got[1] - expected[1]).abs() < 1.0,
                "mapped {mapped:?} -> {got:?}, expected {expected:?}"
            );
        }
    }

    #[test]
    fn mirrored_quad_falls_back() {
        let quad = [[0.0, 0.0], [200.0, 0.0], [0.0, 100.0], [200.0, 100.0]];
        assert!(quad_transform(quad, 200.0, 100.0).is_none());
    }

    #[test]
    fn lerp_color_endpoints_and_midpoint() {
        let a = [0, 0, 0, 255];
        let b = [255, 255, 255, 255];
        assert_eq!(lerp_color(a, b, 0.0), Color::from_rgba8(0, 0, 0, 1.0));
        assert_eq!(lerp_color(a, b, 1.0), Color::from_rgba8(255, 255, 255, 1.0));
        assert_eq!(lerp_color(a, b, 0.5), Color::from_rgba8(128, 128, 128, 1.0));
        assert_eq!(lerp_color(a, b, -1.0), Color::from_rgba8(0, 0, 0, 1.0));
        assert_eq!(lerp_color(a, b, 2.0), Color::from_rgba8(255, 255, 255, 1.0));
    }

    #[test]
    fn gradient_t_at_box_corners_for_all_directions() {
        use scanlateit_model::TextGradientDir;
        let box_rect = Rectangle::new(Point::new(10.0, 20.0), Size::new(100.0, 50.0));
        let tl = Point::new(10.0, 20.0);
        let tr = Point::new(110.0, 20.0);
        let bl = Point::new(10.0, 70.0);
        let br = Point::new(110.0, 70.0);
        let t = |dir, p| gradient_t(dir, box_rect, p);
        assert!((t(TextGradientDir::TopToBottom, tl) - 0.0).abs() < 1e-6);
        assert!((t(TextGradientDir::TopToBottom, bl) - 1.0).abs() < 1e-6);
        assert!((t(TextGradientDir::BottomToTop, tl) - 1.0).abs() < 1e-6);
        assert!((t(TextGradientDir::BottomToTop, bl) - 0.0).abs() < 1e-6);
        assert!((t(TextGradientDir::LeftToRight, tl) - 0.0).abs() < 1e-6);
        assert!((t(TextGradientDir::LeftToRight, tr) - 1.0).abs() < 1e-6);
        assert!((t(TextGradientDir::RightToLeft, tl) - 1.0).abs() < 1e-6);
        assert!((t(TextGradientDir::RightToLeft, tr) - 0.0).abs() < 1e-6);
        assert!((t(TextGradientDir::TopLeftToBottomRight, tl) - 0.0).abs() < 1e-6);
        assert!((t(TextGradientDir::TopLeftToBottomRight, br) - 1.0).abs() < 1e-6);
        assert!((t(TextGradientDir::BottomRightToTopLeft, br) - 0.0).abs() < 1e-6);
        assert!((t(TextGradientDir::BottomRightToTopLeft, tl) - 1.0).abs() < 1e-6);
        assert!((t(TextGradientDir::TopRightToBottomLeft, tr) - 0.0).abs() < 1e-6);
        assert!((t(TextGradientDir::TopRightToBottomLeft, bl) - 1.0).abs() < 1e-6);
        assert!((t(TextGradientDir::BottomLeftToTopRight, bl) - 0.0).abs() < 1e-6);
        assert!((t(TextGradientDir::BottomLeftToTopRight, tr) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn perspective_map_maps_box_corners_onto_quad_corners() {
        let quad = [[100.0, 50.0], [500.0, 50.0], [420.0, 200.0], [180.0, 200.0]];
        let box_rect = Rectangle::new(Point::new(100.0, 50.0), Size::new(400.0, 150.0));
        let tl = perspective_map(quad, box_rect, Point::new(100.0, 50.0));
        let tr = perspective_map(quad, box_rect, Point::new(500.0, 50.0));
        let br = perspective_map(quad, box_rect, Point::new(500.0, 200.0));
        let bl = perspective_map(quad, box_rect, Point::new(100.0, 200.0));
        assert!((tl.x - 100.0).abs() < 1e-3 && (tl.y - 50.0).abs() < 1e-3);
        assert!((tr.x - 500.0).abs() < 1e-3 && (tr.y - 50.0).abs() < 1e-3);
        assert!((br.x - 420.0).abs() < 1e-3 && (br.y - 200.0).abs() < 1e-3);
        assert!((bl.x - 180.0).abs() < 1e-3 && (bl.y - 200.0).abs() < 1e-3);
    }

    #[test]
    fn perspective_map_shares_one_vanishing_point_between_lines() {
        let quad = [[100.0, 50.0], [500.0, 50.0], [460.0, 230.0], [140.0, 210.0]];
        let box_rect = Rectangle::new(Point::new(100.0, 50.0), Size::new(400.0, 180.0));
        let vp = Point::new(-2420.0, 50.0);
        for v in [0.25, 0.5, 0.75] {
            let y = box_rect.y + v * box_rect.height;
            let a = perspective_map(
                quad,
                box_rect,
                Point::new(box_rect.x + 0.2 * box_rect.width, y),
            );
            let b = perspective_map(
                quad,
                box_rect,
                Point::new(box_rect.x + 0.8 * box_rect.width, y),
            );
            let area = (b.x - a.x) * (vp.y - a.y) - (b.y - a.y) * (vp.x - a.x);
            assert!(
                area.abs() < 1.0,
                "line image at v={v} misses the vanishing point (area {area})"
            );
        }
    }

    #[test]
    fn rotated_rect_geometry_detects_rotated_boxes_and_rejects_shear() {
        let rotated = [[0.0, 0.0], [87.758256, 47.942554], [73.37549, 74.27003], [-14.382766, 26.327477]];
        let (tl, w, h, angle) = rotated_rect_geometry(rotated).unwrap();
        assert_eq!((tl.x, tl.y), (0.0, 0.0));
        assert!((w - 100.0).abs() < 1e-3 && (h - 30.0).abs() < 1e-3);
        assert!((angle - 0.5).abs() < 1e-3);
        let upright = [[0.0, 0.0], [100.0, 0.0], [100.0, 30.0], [0.0, 30.0]];
        let (_, w, h, angle) = rotated_rect_geometry(upright).unwrap();
        assert!((w - 100.0).abs() < 1e-3 && (h - 30.0).abs() < 1e-3);
        assert!(angle.abs() < 1e-3);
        let sheared = [[0.0, 0.0], [100.0, 0.0], [90.0, 30.0], [10.0, 30.0]];
        assert!(rotated_rect_geometry(sheared).is_none());
    }

    #[test]
    fn affine_error_is_zero_for_parallelogram_and_large_for_trapezoid() {
        let parallelogram = [[100.0, 50.0], [500.0, 50.0], [440.0, 200.0], [40.0, 200.0]];
        let [min_x, min_y, max_x, max_y] = quad_bounds(parallelogram);
        let width = max_x - min_x;
        let height = max_y - min_y;
        assert!(
            affine_error(parallelogram, width, height) < 0.01,
            "parallelogram must fit exactly"
        );
        let trapezoid = [[302.75, 257.02], [785.25, 257.02], [815.2, 376.0], [302.75, 313.79]];
        let [min_x, min_y, max_x, max_y] = quad_bounds(trapezoid);
        let width = max_x - min_x;
        let height = max_y - min_y;
        let error = affine_error(trapezoid, width, height);
        assert!(error > 5.0, "trapezoid must deviate, got {error}");
    }

    #[test]
    fn warp_transform_round_trips_glyph_rect() {
        let quad = [[100.0, 50.0], [500.0, 50.0], [420.0, 200.0], [180.0, 200.0]];
        let box_rect = Rectangle::new(Point::new(100.0, 50.0), Size::new(400.0, 150.0));
        let rect = [224.0, 92.0, 8.0, 9.0];
        let corners: [[f32; 2]; 4] = [
            perspective_map(quad, box_rect, Point::new(rect[0], rect[1])),
            perspective_map(quad, box_rect, Point::new(rect[0] + rect[2], rect[1])),
            perspective_map(quad, box_rect, Point::new(rect[0] + rect[2], rect[1] + rect[3])),
            perspective_map(quad, box_rect, Point::new(rect[0], rect[1] + rect[3])),
        ]
        .map(|p| [p.x, p.y]);
        let (m00, m01, m10, m11) = crate::main_area::geometry::fit_affine(corners, rect[2], rect[3]).expect("fit");
        let [min_x, min_y, max_x, max_y] = quad_bounds(corners);
        let quad_center = [(min_x + max_x) / 2.0, (min_y + max_y) / 2.0];
        let rect_center = [rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0];
        let apply = |x: f32, y: f32| -> [f32; 2] {
            let (lx, ly) = (x - rect_center[0], y - rect_center[1]);
            [
                quad_center[0] + m00 * lx + m01 * ly,
                quad_center[1] + m10 * lx + m11 * ly,
            ]
        };
        let local = [
            [rect[0], rect[1]],
            [rect[0] + rect[2], rect[1]],
            [rect[0] + rect[2], rect[1] + rect[3]],
            [rect[0], rect[1] + rect[3]],
        ];
        for (mapped, expected) in corners.iter().zip(local.iter()) {
            let got = apply(expected[0], expected[1]);
            assert!(
                (got[0] - mapped[0]).abs() < 0.5 && (got[1] - mapped[1]).abs() < 0.5,
                "glyph corner {expected:?} -> {got:?}, expected {mapped:?}"
            );
        }
    }

    #[test]
    fn warp_layout_shapes_glyphs() {
        let layout = shape_warp_layout("hello world", Font::DEFAULT, 20.0, 200.0);
        assert!(
            layout.glyphs.len() >= 6,
            "expected glyphs, got {}",
            layout.glyphs.len()
        );
        assert!(layout.min_width > 50.0, "min_width {}", layout.min_width);
        let empty = shape_warp_layout("", Font::DEFAULT, 20.0, 200.0);
        assert!(empty.glyphs.is_empty());
    }

    #[test]
    fn warp_layout_glyph_rects_stay_inside_the_paragraph() {
        let text = "hello world this wraps into several lines";
        let size = 20.0;
        let wrap_width = 120.0;
        let fitted = measure_text(text, Font::DEFAULT, size, wrap_width);
        let layout = shape_warp_layout(text, Font::DEFAULT, size, wrap_width);
        assert!(
            layout.glyphs.len() >= 2,
            "expected several lines of glyphs, got {}",
            layout.glyphs.len()
        );
        for glyph in &layout.glyphs {
            let [gx, gy, gw, gh] = glyph.rect;
            assert!(gx >= -1.0, "glyph rect left {gx} out of bounds");
            assert!(gy >= -1.0, "glyph rect top {gy} out of bounds");
            assert!(
                gy + gh <= fitted.height + 1.0,
                "glyph rect bottom {} exceeds paragraph height {}",
                gy + gh,
                fitted.height
            );
        }
    }

    #[test]
    fn chord_at_is_full_at_center_and_zero_at_edges() {
        let rx = 50.0;
        let ry = 25.0;
        assert!((super::circle::chord_at(rx, ry, 25.0) - 100.0).abs() < 1e-3);
        assert!(super::circle::chord_at(rx, ry, 0.0) < 1e-3);
        assert!(super::circle::chord_at(rx, ry, 50.0) < 1e-3);
    }
}
