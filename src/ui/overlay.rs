use iced::widget::canvas::{self, Fill, Geometry, Image};
use iced::widget::image::Handle;
use iced::{Color, Font, Pixels, Point, Rectangle, Renderer, Size, Theme};

use crate::model::EntryStyle;

/// View-model entry: what the overlay draws, resolved from the model with the
/// selected profile's translation and style already applied.
pub struct OverlayEntry<'a> {
    pub text: &'a str,
    /// `[min_x, min_y, max_x, max_y]` in image pixels.
    pub bounds: [f32; 4],
    pub style: EntryStyle,
}

/// Canvas that draws the image with one box + label per overlay entry.
pub struct Overlay<'a> {
    handle: &'a Handle,
    entries: Vec<OverlayEntry<'a>>,
    font: Font,
    cache: &'a canvas::Cache,
}

impl<'a> Overlay<'a> {
    pub fn new(
        handle: &'a Handle,
        entries: Vec<OverlayEntry<'a>>,
        font: Font,
        cache: &'a canvas::Cache,
    ) -> Self {
        Self {
            handle,
            entries,
            font,
            cache,
        }
    }
}

fn to_color(rgba: [u8; 4]) -> Color {
    Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3] as f32 / 255.0)
}

impl<Message> canvas::Program<Message> for Overlay<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let geometry = self.cache.draw(renderer, bounds.size(), |frame| {
            frame.draw_image(bounds, Image::new(self.handle.clone()));
            for entry in &self.entries {
                let [min_x, min_y, max_x, max_y] = entry.bounds;
                let width = (max_x - min_x).max(0.0);
                let height = (max_y - min_y).max(0.0);
                frame.fill_rectangle(
                    Point::new(min_x, min_y),
                    Size::new(width, height),
                    Fill::from(to_color(entry.style.bg_color)),
                );
                frame.fill_text(canvas::Text {
                    content: entry.text.to_string(),
                    position: Point::new(min_x, min_y),
                    max_width: width.max(8.0),
                    size: Pixels(entry.style.font_size),
                    color: to_color(entry.style.text_color),
                    font: self.font,
                    ..canvas::Text::default()
                });
            }
        });
        vec![geometry]
    }
}