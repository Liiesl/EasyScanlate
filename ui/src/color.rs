//! Small color helpers for the app's ui crate.

use iced::Color;

/// Converts an RGBA byte color to an iced [`Color`].
pub fn rgba_to_color(rgba: [u8; 4]) -> Color {
    Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3] as f32 / 255.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_to_color_maps_channels_and_alpha() {
        assert_eq!(
            rgba_to_color([10, 20, 30, 255]),
            Color::from_rgba8(10, 20, 30, 1.0)
        );
        assert_eq!(
            rgba_to_color([255, 0, 128, 128]),
            Color::from_rgba8(255, 0, 128, 128.0 / 255.0)
        );
    }
}