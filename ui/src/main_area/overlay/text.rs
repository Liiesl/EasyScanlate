use iced::advanced::text::{
    Alignment as TextAlignment, LineHeight, Paragraph as _, Shaping, Text as ParagraphText,
    Wrapping,
};
use iced::{alignment, Font, Pixels, Size};

/// Relative line height shared by measure and circular layout.
pub const LINE_HEIGHT: f32 = 1.2;

/// Rendered size of `text` at `size` points, wrapped at `max_width`.
pub fn measure_text(text: &str, font: Font, size: f32, max_width: f32) -> Size {
    let paragraph = iced::advanced::graphics::text::Paragraph::with_text(ParagraphText {
        content: text,
        bounds: Size::new(max_width, f32::INFINITY),
        size: Pixels(size),
        line_height: LineHeight::Relative(1.2),
        font,
        align_x: TextAlignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: Shaping::Auto,
        wrapping: Wrapping::WordOrGlyph,
    });
    paragraph.min_bounds()
}

/// Whether `content` fits on a single line at `size` when wrapped at `max_width`.
pub fn line_fits(content: &str, font: Font, size: f32, max_width: f32) -> bool {
    if content.is_empty() || max_width <= 0.0 {
        return false;
    }
    let paragraph = iced::advanced::graphics::text::Paragraph::with_text(ParagraphText {
        content,
        bounds: Size::new(max_width, f32::INFINITY),
        size: Pixels(size),
        line_height: LineHeight::Relative(LINE_HEIGHT),
        font,
        align_x: TextAlignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: Shaping::Auto,
        wrapping: Wrapping::WordOrGlyph,
    });
    let b = paragraph.min_bounds();
    b.width <= max_width + 0.5 && b.height <= size * LINE_HEIGHT + 0.5
}
