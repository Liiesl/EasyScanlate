use iced::advanced::text::{Alignment as TextAlignment, LineHeight, Shaping, Wrapping};
use iced::{alignment, Font, Size};

/// Relative line height shared by measure and circular layout.
/// Default for entries; per-entry `EntryStyle::line_height` overrides it.
pub const LINE_HEIGHT: f32 = 1.2;

/// cosmic-text takes letter spacing (tracking) in EM, while the entry style
/// stores it in frame px. Convert here so measure and draw share one mapping.
fn spacing_em(size: f32, letter_spacing: f32) -> Option<f32> {
    if letter_spacing <= 0.0 || size <= 0.0 {
        return None;
    }
    Some(letter_spacing / size)
}

/// Attributes for `font` with real letter spacing applied. Falls back to the
/// plain iced mapping when spacing is off so zero-spacing output is unchanged.
fn spaced_attrs(font: Font, size: f32, letter_spacing: f32) -> CosmicAttrs<'static> {
    use iced::advanced::graphics::text as gfx_text;
    let attrs = gfx_text::to_attributes(font);
    match spacing_em(size, letter_spacing) {
        Some(em) => attrs.letter_spacing(em),
        None => attrs,
    }
}

// Concrete cosmic-text Attrs type without spelling the lifetime at call sites.
use iced::advanced::graphics::text::cosmic_text as cosmic_crate;
type CosmicAttrs<'a> = cosmic_crate::Attrs<'a>;

/// Build a shaped cosmic-text buffer with real letter spacing.
///
/// Returns the owned buffer plus the corrected (ink, no trailing spacing)
/// minimum bounds. cosmic-text adds the tracking advance to every glyph,
/// including the line-trailing one, so the raw `min_bounds.width` overhangs
/// by one `letter_spacing` per (max) line; subtracting it keeps centering,
/// right alignment and fit conservative-but-tight like the old
/// `chars-1 gaps` stub while the glyph positions themselves carry the real
/// visual spread.
pub fn spaced_buffer(
    text: &str,
    font: Font,
    size: f32,
    max_width: f32,
    line_height: f32,
    letter_spacing: f32,
    wrapping: Wrapping,
    align_x: TextAlignment,
) -> (cosmic_crate::Buffer, Size) {
    use iced::advanced::graphics::text as gfx_text;
    let mut font_system = gfx_text::font_system().write().expect("Write font system");
    let mut buffer = cosmic_crate::Buffer::new(
        font_system.raw(),
        cosmic_crate::Metrics::new(size, size * line_height),
    );
    buffer.set_size(font_system.raw(), Some(max_width), Some(f32::INFINITY));
    buffer.set_wrap(font_system.raw(), gfx_text::to_wrap(wrapping));
    let attrs = spaced_attrs(font, size, letter_spacing);
    buffer.set_text(
        font_system.raw(),
        text,
        &attrs,
        gfx_text::to_shaping(Shaping::Auto, text),
        None,
    );
    let raw = gfx_text::align(&mut buffer, font_system.raw(), align_x);
    let corrected = if letter_spacing > 0.0 && !text.is_empty() {
        Size::new((raw.width - letter_spacing).max(0.0), raw.height)
    } else {
        raw
    };
    // `buffer` is owned; layout runs stay usable after the lock is dropped.
    (buffer, corrected)
}

/// Corrected minimum width of `text` with real letter spacing. Used for
/// Center/Right translation so the trailing tracking advance does not shift
/// the visual center.
pub fn spaced_min_width(
    text: &str,
    font: Font,
    size: f32,
    max_width: f32,
    line_height: f32,
    letter_spacing: f32,
    wrapping: Wrapping,
    align_x: TextAlignment,
) -> f32 {
    spaced_buffer(
        text,
        font,
        size,
        max_width,
        line_height,
        letter_spacing,
        wrapping,
        align_x,
    )
    .1
    .width
}

/// Rendered size of `text` at `size` points, wrapped at `max_width`.
pub fn measure_text(
    text: &str,
    font: Font,
    size: f32,
    max_width: f32,
    line_height: f32,
    letter_spacing: f32,
) -> Size {
    if text.is_empty() {
        return Size::new(0.0, 0.0);
    }
    spaced_buffer(
        text,
        font,
        size,
        max_width,
        line_height,
        letter_spacing,
        Wrapping::WordOrGlyph,
        TextAlignment::Default,
    )
    .1
}

/// Whether `content` fits on a single line at `size` when wrapped at `max_width`.
pub fn line_fits(
    content: &str,
    font: Font,
    size: f32,
    max_width: f32,
    line_height: f32,
    letter_spacing: f32,
) -> bool {
    if content.is_empty() || max_width <= 0.0 {
        return false;
    }
    let b = spaced_buffer(
        content,
        font,
        size,
        max_width,
        line_height,
        letter_spacing,
        Wrapping::WordOrGlyph,
        TextAlignment::Default,
    )
    .1;
    b.width <= max_width + 0.5 && b.height <= size * line_height + 0.5
}

/// Draw `text` with real letter spacing as vector glyph outlines.
///
/// iced 0.14 `Text` has no letter-spacing field, so solid/stroke text goes
/// through the same glyph-outline loop the gradient and warp paths already
/// use. `translation` centers/right-aligns with the corrected (no-trailing)
/// width so Center/Right stay visually centered.
pub fn draw_spaced_text<F>(
    frame: &mut F,
    text: &iced::advanced::graphics::geometry::Text,
    letter_spacing: f32,
    stroke: Option<(iced::Color, f32)>,
) where
    F: iced::advanced::graphics::geometry::frame::Backend,
{
    use iced::advanced::graphics::geometry::{Fill, Path, Stroke};
    use iced::advanced::graphics::text as gfx_text;
    use iced::{Point, Vector};

    if text.content.is_empty() {
        return;
    }
    let (buffer, corrected) = spaced_buffer(
        text.content.as_str(),
        text.font,
        text.size.0,
        text.max_width,
        match text.line_height {
            LineHeight::Relative(f) => f,
            LineHeight::Absolute(px) => (f32::from(px) / text.size.0.max(1.0)).max(0.1),
        },
        letter_spacing,
        Wrapping::WordOrGlyph,
        text.align_x,
    );
    let translation_x = match text.align_x {
        TextAlignment::Default | TextAlignment::Left | TextAlignment::Justified => text.position.x,
        TextAlignment::Center => text.position.x - corrected.width / 2.0,
        TextAlignment::Right => text.position.x - corrected.width,
    };
    let translation_y = match text.align_y {
        alignment::Vertical::Top => text.position.y,
        alignment::Vertical::Center => text.position.y - corrected.height / 2.0,
        alignment::Vertical::Bottom => text.position.y - corrected.height,
    };
    let mut swash_cache = cosmic_crate::SwashCache::new();
    let mut font_system = gfx_text::font_system().write().expect("Write font system");
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
                    use cosmic_crate::Command;
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
                frame.fill(&glyph_path, Fill::from(text.color));
            } else {
                let [r, g, b, a] = text.color.into_rgba8();
                swash_cache.with_pixels(
                    font_system.raw(),
                    physical_glyph.cache_key,
                    cosmic_crate::Color::rgba(r, g, b, a),
                    |x, y, color| {
                        frame.fill(
                            &Path::rectangle(
                                Point::new(x as f32, y as f32) + offset,
                                Size::new(1.0, 1.0),
                            ),
                            Fill::from(iced::Color::from_rgba8(
                                color.r(),
                                color.g(),
                                color.b(),
                                color.a() as f32 / 255.0,
                            )),
                        );
                    },
                );
            }
        }
    }
}


