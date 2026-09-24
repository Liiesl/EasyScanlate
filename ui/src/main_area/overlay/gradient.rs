use iced::advanced::graphics::gradient::Linear;
use iced::advanced::graphics::geometry::{self, Fill, Path, Stroke, Text};
use iced::{Color, Point, Rectangle, Vector};

use easyscanlate_model::{EntryStyle, TextGradientDir};

pub fn lerp_color(a: [u8; 4], b: [u8; 4], t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::from_rgba8(
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t).round() as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t).round() as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t).round() as u8,
        (a[3] as f32 + (b[3] as f32 - a[3] as f32) * t) / 255.0,
    )
}

/// Paint of a text stroke: solid color, or a two-stop gradient sampled
/// per glyph at the glyph's position (iced `Stroke` carries only a solid
/// color, so a gradient stroke resolves to the local gradient color).
#[derive(Debug, Clone, Copy)]
pub enum StrokePaint {
    Solid(Color),
    Gradient { angle: f32, a: [u8; 4], b: [u8; 4] },
}

impl StrokePaint {
    /// Builds the stroke paint for `style` at `width` (frame px): `None`
    /// when `width <= 0`.
    pub fn for_style(style: &EntryStyle, width: f32) -> Option<(Self, f32)> {
        if width <= 0.0 {
            return None;
        }
        if style.stroke_gradient {
            Some((
                Self::Gradient {
                    angle: style.stroke_gradient_angle,
                    a: style.stroke_gradient_a,
                    b: style.stroke_gradient_b,
                },
                width,
            ))
        } else {
            Some((
                Self::Solid(crate::color::rgba_to_color(style.stroke_color)),
                width,
            ))
        }
    }

    /// Resolves the solid stroke color at point `p` inside `box_rect`.
    pub fn color_at(self, box_rect: Rectangle, p: Point) -> Color {
        match self {
            Self::Solid(c) => c,
            Self::Gradient { angle, a, b } => {
                lerp_color(a, b, gradient_t_angle(angle, box_rect, p))
            }
        }
    }
}

/// Fill of an entry background: solid `bg_color`, or a two-stop linear
/// gradient (`bg_gradient_a/b` + `bg_gradient_angle`) when `bg_gradient`.
/// `rect` must be in the same coordinate space as the filled path (the
/// gradient endpoints are absolute points, derived from the unified angle
/// via the same `to_distance` math iced uses for angle gradients, so the
/// bg agrees with the overlay text gradient).
pub fn bg_fill_in_rect(style: &EntryStyle, rect: Rectangle) -> Fill {
    if style.bg_gradient {
        let (start, end) = gradient_start_end_angle(style.bg_gradient_angle, rect);
        Fill::from(
            Linear::new(start, end)
                .add_stop(0.0, crate::color::rgba_to_color(style.bg_gradient_a))
                .add_stop(1.0, crate::color::rgba_to_color(style.bg_gradient_b)),
        )
    } else {
        Fill::from(crate::color::rgba_to_color(style.bg_color))
    }
}

/// Text-fill gradient for `style`: `Some((angle, a, b))` when
/// `text_gradient`, else `None` (solid `text_color`).
pub fn text_fill(style: &EntryStyle) -> Option<(f32, [u8; 4], [u8; 4])> {
    style
        .text_gradient
        .then_some((style.gradient_angle, style.gradient_a, style.gradient_b))
}

/// Flow vector `(rx, ry)` for an angle in degrees under the unified
/// convention, mirroring `iced::Radians::to_distance` (`angle - 90°` gives
/// the `(cos, sin)` vector from stop 0 to stop 1; y grows downward).
fn gradient_flow(angle_deg: f32) -> (f32, f32) {
    let internal = angle_deg.to_radians() - std::f32::consts::FRAC_PI_2;
    (internal.cos(), internal.sin())
}

/// Gradient endpoints in layout coords for an angle, matching
/// `iced::Radians::to_distance` so overlay text and `Background::Gradient`
/// bgs agree.
pub fn gradient_start_end_angle(angle_deg: f32, box_rect: Rectangle) -> (Point, Point) {
    let (rx, ry) = gradient_flow(angle_deg);
    let distance = f32::max(
        f32::abs(rx * box_rect.width / 2.0),
        f32::abs(ry * box_rect.height / 2.0),
    );
    let center = Point::new(
        box_rect.x + box_rect.width / 2.0,
        box_rect.y + box_rect.height / 2.0,
    );
    (
        Point::new(center.x - rx * distance, center.y - ry * distance),
        Point::new(center.x + rx * distance, center.y + ry * distance),
    )
}

/// Normalized position `0..=1` of `p` along an angle gradient.
pub fn gradient_t_angle(angle_deg: f32, box_rect: Rectangle, p: Point) -> f32 {
    let (rx, ry) = gradient_flow(angle_deg);
    let distance = f32::max(
        f32::abs(rx * box_rect.width.max(1.0) / 2.0),
        f32::abs(ry * box_rect.height.max(1.0) / 2.0),
    )
    .max(f32::EPSILON);
    let center = Point::new(
        box_rect.x + box_rect.width / 2.0,
        box_rect.y + box_rect.height / 2.0,
    );
    let t = 0.5 + ((p.x - center.x) * rx + (p.y - center.y) * ry) / (2.0 * distance);
    t.clamp(0.0, 1.0)
}

pub fn gradient_t(dir: TextGradientDir, box_rect: Rectangle, p: Point) -> f32 {
    let w = box_rect.width.max(1.0);
    let h = box_rect.height.max(1.0);
    let x = (p.x - box_rect.x) / w;
    let y = (p.y - box_rect.y) / h;
    let t = match dir {
        TextGradientDir::TopToBottom => y,
        TextGradientDir::BottomToTop => 1.0 - y,
        TextGradientDir::LeftToRight => x,
        TextGradientDir::RightToLeft => 1.0 - x,
        TextGradientDir::TopLeftToBottomRight => (x + y) / 2.0,
        TextGradientDir::BottomRightToTopLeft => 1.0 - (x + y) / 2.0,
        TextGradientDir::TopRightToBottomLeft => ((1.0 - x) + y) / 2.0,
        TextGradientDir::BottomLeftToTopRight => 1.0 - ((1.0 - x) + y) / 2.0,
    };
    t.clamp(0.0, 1.0)
}

/// Gradient endpoints in layout coords for each direction: stop 0 (`a`) at
/// `start`, stop 1 (`b`) at `end`. Consistent with [`gradient_t`].
pub fn gradient_start_end(dir: TextGradientDir, box_rect: Rectangle) -> (Point, Point) {
    let x = box_rect.x;
    let y = box_rect.y;
    let w = box_rect.width;
    let h = box_rect.height;
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    match dir {
        TextGradientDir::TopToBottom => (Point::new(cx, y), Point::new(cx, y + h)),
        TextGradientDir::BottomToTop => (Point::new(cx, y + h), Point::new(cx, y)),
        TextGradientDir::LeftToRight => (Point::new(x, cy), Point::new(x + w, cy)),
        TextGradientDir::RightToLeft => (Point::new(x + w, cy), Point::new(x, cy)),
        TextGradientDir::TopLeftToBottomRight => (Point::new(x, y), Point::new(x + w, y + h)),
        TextGradientDir::BottomRightToTopLeft => (Point::new(x + w, y + h), Point::new(x, y)),
        TextGradientDir::TopRightToBottomLeft => (Point::new(x + w, y), Point::new(x, y + h)),
        TextGradientDir::BottomLeftToTopRight => (Point::new(x, y + h), Point::new(x + w, y)),
    }
}

fn rgba8(c: [u8; 4]) -> Color {
    Color::from_rgba8(c[0], c[1], c[2], c[3] as f32 / 255.0)
}

// Gradient text is drawn as vector glyph outlines filled with a single linear
// gradient shader, directly on the parent frame so the caller's transform
// (tile offset + quad/rotated transform) applies naturally to both the paths
// and the gradient endpoints. Banded `draft`/`paste` clipping reset the frame
// transform to identity, which dropped the tile translation (gradient
// invisible past the first image) and clipped rotated text with axis-aligned
// strips (slivers nowhere near the glyphs).
pub fn fill_gradient_text<F>(
    frame: &mut F,
    text: &Text,
    box_rect: Rectangle,
    angle: f32,
    a: [u8; 4],
    b: [u8; 4],
    stroke: Option<(StrokePaint, f32)>,
    letter_spacing: f32,
) where
    F: geometry::frame::Backend,
{
    fill_gradient_glyphs(frame, text, box_rect, angle, a, b, stroke, letter_spacing)
}

fn fill_gradient_glyphs<F>(
    frame: &mut F,
    text: &Text,
    box_rect: Rectangle,
    angle: f32,
    a: [u8; 4],
    b: [u8; 4],
    stroke: Option<(StrokePaint, f32)>,
    letter_spacing: f32,
) where
    F: geometry::frame::Backend,
{
    use iced::advanced::graphics::text::{self as gfx_text, cosmic_text};
    use iced::advanced::text::{LineHeight, Wrapping};
    use iced::Size;
    use iced::advanced::text::Alignment as TextAlignment;

    let line_factor = match text.line_height {
        LineHeight::Relative(f) => f,
        LineHeight::Absolute(px) => (f32::from(px) / text.size.0.max(1.0)).max(0.1),
    };
    let (buffer, corrected) = super::text::spaced_buffer(
        text.content.as_str(),
        text.font,
        text.size.0,
        text.max_width,
        line_factor,
        letter_spacing,
        Wrapping::WordOrGlyph,
        text.align_x,
    );
    let translation_x = match text.align_x {
        TextAlignment::Default | TextAlignment::Left | TextAlignment::Justified => text.position.x,
        TextAlignment::Center => text.position.x - corrected.width / 2.0,
        TextAlignment::Right => text.position.x - corrected.width,
    };
    let translation_y = text.position.y;
    let mut swash_cache = cosmic_text::SwashCache::new();
    let mut font_system = gfx_text::font_system().write().expect("Write font system");
    let (grad_start, grad_end) = gradient_start_end_angle(angle, box_rect);
    let gradient_fill = Fill::from(
        Linear::new(grad_start, grad_end)
            .add_stop(0.0, rgba8(a))
            .add_stop(1.0, rgba8(b)),
    );
    for run in buffer.layout_runs() {
        for glyph in run.glyphs.iter() {
            let physical_glyph = glyph.physical((0.0, 0.0), 1.0);
            let start_x = translation_x + glyph.x + glyph.x_offset;
            let start_y = translation_y + glyph.y_offset + run.line_y;
            let offset = Vector::new(start_x, start_y);
            if let Some(commands) =
                swash_cache.get_outline_commands(font_system.raw(), physical_glyph.cache_key)
            {
                let glyph_path = Path::new(|path| {
                    use cosmic_text::Command;
                    for command in commands {
                        match command {
                            Command::MoveTo(p) => path.move_to(Point::new(p.x, -p.y) + offset),
                            Command::LineTo(p) => path.line_to(Point::new(p.x, -p.y) + offset),
                            Command::CurveTo(control_a, control_b, to) => {
                                path.bezier_curve_to(
                                    Point::new(control_a.x, -control_a.y) + offset,
                                    Point::new(control_b.x, -control_b.y) + offset,
                                    Point::new(to.x, -to.y) + offset,
                                );
                            }
                            Command::QuadTo(control, to) => {
                                path.quadratic_curve_to(
                                    Point::new(control.x, -control.y) + offset,
                                    Point::new(to.x, -to.y) + offset,
                                );
                            }
                            Command::Close => path.close(),
                        }
                    }
                });
                if let Some((paint, stroke_width)) = stroke {
                    frame.stroke(
                        &glyph_path,
                        Stroke::default()
                            .with_color(paint.color_at(
                                box_rect,
                                Point::new(start_x, start_y),
                            ))
                            .with_width(stroke_width),
                    );
                }
                frame.fill(&glyph_path, gradient_fill);
            } else {
                // Color glyphs without outlines (rare): sample the gradient per
                // pixel for smoothness, modulating the lerped alpha by the
                // glyph coverage so solid `Fill`s stay export-safe.
                swash_cache.with_pixels(
                    font_system.raw(),
                    physical_glyph.cache_key,
                    cosmic_text::Color::rgba(255, 255, 255, 255),
                    |x, y, pixel| {
                        let coverage = pixel.a() as f32 / 255.0;
                        if coverage <= 0.0 {
                            return;
                        }
                        let base = lerp_color(
                            a,
                            b,
                            gradient_t_angle(angle, box_rect, Point::new(x as f32, y as f32) + offset),
                        );
                        let [r, g, bl, al] = base.into_rgba8();
                        frame.fill(
                            &Path::rectangle(
                                Point::new(x as f32, y as f32) + offset,
                                Size::new(1.0, 1.0),
                            ),
                            Fill::from(Color::from_rgba8(r, g, bl, al as f32 / 255.0 * coverage)),
                        );
                    },
                );
            }
        }
    }
}
