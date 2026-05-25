use iced::widget::canvas::{self, Fill, Geometry, Image};
use iced::widget::image::Handle;
use iced::{Color, Font, Pixels, Point, Rectangle, Renderer, Size, Theme};

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

/// Canvas that draws the image with one box + label per overlay entry.
#[derive(Clone)]
pub struct Overlay<'a> {
    handle: &'a Handle,
    entries: Vec<OverlayEntry<'a>>,
    font: Font,
    cache: &'a canvas::Cache,
    image_width: f32,
}

impl<'a> Overlay<'a> {
    pub fn new(
        handle: &'a Handle,
        entries: Vec<OverlayEntry<'a>>,
        font: Font,
        cache: &'a canvas::Cache,
        image_width: f32,
    ) -> Self {
        Self {
            handle,
            entries,
            font,
            cache,
            image_width,
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
            frame.draw_image(
                Rectangle::with_size(bounds.size()),
                Image::new(self.handle.clone()),
            );
            let scale = bounds.width / self.image_width;
            for entry in &self.entries {
                let [min_x, min_y, max_x, max_y] = entry.bounds;
                let width = (max_x - min_x).max(0.0) * scale;
                let height = (max_y - min_y).max(0.0) * scale;
                let position = Point::new(min_x * scale, min_y * scale);
                frame.fill_rectangle(
                    position,
                    Size::new(width, height),
                    Fill::from(to_color(entry.style.bg_color)),
                );
                frame.fill_text(canvas::Text {
                    content: entry.text.to_string(),
                    position,
                    max_width: width.max(8.0),
                    size: Pixels(entry.style.font_size * scale),
                    color: to_color(entry.style.text_color),
                    font: self.font,
                    ..canvas::Text::default()
                });
            }
        });
        vec![geometry]
    }
}