use iced::advanced::graphics::gradient::Linear;
use iced::advanced::graphics::geometry::{self, Fill, Path, Stroke, Text};
use iced::{Color, Point, Rectangle, Vector};

use easyscanlate_model::TextGradientDir;

pub fn lerp_color(a: [u8; 4], b: [u8; 4], t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::from_rgba8(
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t).round() as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t).round() as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t).round() as u8,
        (a[3] as f32 + (b[3] as f32 - a[3] as f32) * t) / 255.0,
    )
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
    dir: TextGradientDir,
    a: [u8; 4],
    b: [u8; 4],
    stroke: Option<(Color, f32)>,
) where
    F: geometry::frame::Backend,
{
    fill_gradient_glyphs(frame, text, box_rect, dir, a, b, stroke)
}

fn fill_gradient_glyphs<F>(
    frame: &mut F,
    text: &Text,
    box_rect: Rectangle,
    dir: TextGradientDir,
    a: [u8; 4],
    b: [u8; 4],
    stroke: Option<(Color, f32)>,
) where
    F: geometry::frame::Backend,
{
    use iced::advanced::graphics::text::{self as gfx_text, cosmic_text, Paragraph as GfxParagraph};
    use iced::advanced::text::{Paragraph as _, Wrapping};
    use iced::advanced::text::Text as ParagraphText;
    use iced::{alignment, Size};
    use iced::advanced::text::Alignment as TextAlignment;

    let paragraph = GfxParagraph::with_text(ParagraphText {
        content: text.content.as_str(),
        bounds: Size::new(text.max_width, f32::INFINITY),
        size: text.size,
        line_height: text.line_height,
        font: text.font,
        align_x: text.align_x,
        align_y: alignment::Vertical::Top,
        shaping: text.shaping,
        wrapping: Wrapping::Word,
    });
    let translation_x = match text.align_x {
        TextAlignment::Default | TextAlignment::Left | TextAlignment::Justified => text.position.x,
        TextAlignment::Center => text.position.x - paragraph.min_width() / 2.0,
        TextAlignment::Right => text.position.x - paragraph.min_width(),
    };
    let translation_y = text.position.y;
    let buffer = paragraph.buffer();
    let mut swash_cache = cosmic_text::SwashCache::new();
    let mut font_system = gfx_text::font_system().write().expect("Write font system");
    let (grad_start, grad_end) = gradient_start_end(dir, box_rect);
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
                if let Some((stroke_color, stroke_width)) = stroke {
                    frame.stroke(
                        &glyph_path,
                        Stroke::default().with_color(stroke_color).with_width(stroke_width),
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
                            gradient_t(dir, box_rect, Point::new(x as f32, y as f32) + offset),
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
