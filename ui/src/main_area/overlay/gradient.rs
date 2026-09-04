use iced::advanced::graphics::geometry::{self, Fill, Path, Stroke, Text};
use iced::{Color, Point, Rectangle, Size, Vector};

use easyscanlate_model::TextGradientDir;

use crate::main_area::geometry::QuadTransform;

/// Number of color bands a gradient text is split into.
const GRADIENT_BANDS: u32 = 16;

pub fn lerp_color(a: [u8; 4], b: [u8; 4], t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::from_rgba8(
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t).round() as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t).round() as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t).round() as u8,
        (a[3] as f32 + (b[3] as f32 - a[3] as f32) * t) / 255.0,
    )
}

fn with_clip<F, R>(frame: &mut F, region: Rectangle, f: impl FnOnce(&mut F) -> R) -> R
where
    F: geometry::frame::Backend,
{
    let mut draft = frame.draft(region);
    let result = f(&mut draft);
    frame.paste(draft);
    result
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

// Gradient text needs every band param at the draw call; a params struct would
// add indirection for a single call site.
#[allow(clippy::too_many_arguments)]
pub fn fill_gradient_text<F>(
    frame: &mut F,
    text: &Text,
    box_rect: Rectangle,
    dir: TextGradientDir,
    a: [u8; 4],
    b: [u8; 4],
    stroke: Option<(Color, f32)>,
    transform: Option<&QuadTransform>,
    position: Point,
    width: f32,
    height: f32,
) where
    F: geometry::frame::Backend,
{
    match dir {
        TextGradientDir::TopToBottom
        | TextGradientDir::BottomToTop
        | TextGradientDir::LeftToRight
        | TextGradientDir::RightToLeft => {
            let vertical = matches!(dir, TextGradientDir::TopToBottom | TextGradientDir::BottomToTop);
            let reversed = matches!(dir, TextGradientDir::BottomToTop | TextGradientDir::RightToLeft);
            for band in 0..GRADIENT_BANDS {
                let t0 = band as f32 / GRADIENT_BANDS as f32;
                let t1 = (band + 1) as f32 / GRADIENT_BANDS as f32;
                let t = if reversed { 1.0 - (t0 + t1) / 2.0 } else { (t0 + t1) / 2.0 };
                let color = lerp_color(a, b, t);
                let strip = if vertical {
                    Rectangle::new(
                        Point::new(box_rect.x, box_rect.y + t0 * box_rect.height),
                        Size::new(box_rect.width, (t1 - t0) * box_rect.height),
                    )
                } else {
                    Rectangle::new(
                        Point::new(box_rect.x + t0 * box_rect.width, box_rect.y),
                        Size::new((t1 - t0) * box_rect.width, box_rect.height),
                    )
                };
                with_clip(frame, strip, |f| {
                    if let Some(transform) = transform {
                        f.push_transform();
                        crate::main_area::geometry::apply_quad_transform(f, transform, position, width, height);
                    }
                    let colored = Text { color, ..text.clone() };
                    if let Some((stroke_color, stroke_width)) = stroke {
                        f.stroke_text(
                            colored.clone(),
                            Stroke::default().with_color(stroke_color).with_width(stroke_width),
                        );
                    }
                    f.fill_text(colored);
                });
            }
        }
        _ => fill_gradient_glyphs(frame, text, box_rect, dir, a, b, stroke),
    }
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
    for run in buffer.layout_runs() {
        for glyph in run.glyphs.iter() {
            let physical_glyph = glyph.physical((0.0, 0.0), 1.0);
            let start_x = translation_x + glyph.x + glyph.x_offset;
            let start_y = translation_y + glyph.y_offset + run.line_y;
            let offset = Vector::new(start_x, start_y);
            let color = lerp_color(
                a,
                b,
                gradient_t(dir, box_rect, Point::new(start_x + glyph.w / 2.0, start_y + text.size.0 / 2.0)),
            );
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
                frame.fill(&glyph_path, Fill::from(color));
            } else {
                let [r, g, bl, al] = color.into_rgba8();
                swash_cache.with_pixels(
                    font_system.raw(),
                    physical_glyph.cache_key,
                    cosmic_text::Color::rgba(r, g, bl, al),
                    |x, y, pixel| {
                        frame.fill(
                            &Path::rectangle(
                                Point::new(x as f32, y as f32) + offset,
                                Size::new(1.0, 1.0),
                            ),
                            Fill::from(Color::from_rgba8(
                                pixel.r(),
                                pixel.g(),
                                pixel.b(),
                                pixel.a() as f32 / 255.0,
                            )),
                        );
                    },
                );
            }
        }
    }
}
