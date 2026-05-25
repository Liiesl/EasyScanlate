use iced::advanced::graphics::geometry::{self, Fill, Text};
use iced::{Color, Font, Pixels, Point, Size};

use crate::model::EntryStyle;

/// View-model entry: what the overlay draws, resolved from the model with the
/// selected profile's translation and style already applied.
#[derive(Clone)]
pub struct OverlayEntry<'a> {
    pub text: &'a str,
    /// `[min_x, min_y, max_x, max_y]` in image pixels.
    pub bounds: [f32; 4],
    pub style: EntryStyle,
}

fn to_color(rgba: [u8; 4]) -> Color {
    Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3] as f32 / 255.0)
}

/// Draws one translucent box + label per entry on top of the image inside
/// `frame`. Coordinates are image pixels, scaled to the frame's width.
pub fn draw_entries<F>(
    frame: &mut F,
    entries: &[OverlayEntry<'_>],
    font: Font,
    image_width: f32,
) where
    F: geometry::frame::Backend,
{
    let scale = frame.width() / image_width.max(1.0);
    for entry in entries {
        let [min_x, min_y, max_x, max_y] = entry.bounds;
        let width = (max_x - min_x).max(0.0) * scale;
        let height = (max_y - min_y).max(0.0) * scale;
        let position = Point::new(min_x * scale, min_y * scale);
        frame.fill_rectangle(
            position,
            Size::new(width, height),
            Fill::from(to_color(entry.style.bg_color)),
        );
        frame.fill_text(Text {
            content: entry.text.to_string(),
            position,
            max_width: width.max(8.0),
            size: Pixels(entry.style.font_size * scale),
            color: to_color(entry.style.text_color),
            font,
            ..Text::default()
        });
    }
}
