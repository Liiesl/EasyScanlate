//! Downscaled, cached page decoding for the tile viewer.

use std::sync::Arc;

use bytes::Bytes;
use iced::widget::image::Handle;

/// Longest edge a decoded page may have, regardless of source resolution.
/// Keeps decode fast and uploads within one wgpu atlas layer.
pub const MAX_DECODE_EDGE: u32 = 2048;

/// A page decoded at display resolution, ready for GPU upload.
#[derive(Debug, Clone)]
pub struct DecodedPage {
    pub handle: Handle,
    pub width: u32,
    pub height: u32,
}

/// Per-page decode state, owned by the app so decoded buffers survive
/// widget rebuilds and scrolling away/back (no blank pages).
pub enum PageDecode {
    Pending,
    Decoding,
    Ready(Arc<DecodedPage>),
    Failed,
}

/// Decodes `path`, downscaling so the longest edge is at most `max_edge`.
pub fn decode_page(path: &str, max_edge: u32) -> Result<DecodedPage, String> {
    let img = image::ImageReader::open(path)
        .map_err(|e| format!("Failed to open {path}: {e}"))?
        .with_guessed_format()
        .map_err(|e| format!("Failed to decode {path}: {e}"))?
        .decode()
        .map_err(|e| format!("Failed to decode {path}: {e}"))?;
    let (source_width, source_height) = (img.width(), img.height());
    let longest = source_width.max(source_height);
    let (width, height) = if longest > max_edge {
        let scale = max_edge as f64 / longest as f64;
        (
            ((source_width as f64 * scale).round() as u32).max(1),
            ((source_height as f64 * scale).round() as u32).max(1),
        )
    } else {
        (source_width, source_height)
    };
    let rgba = img.thumbnail(width, height).into_rgba8();
    let pixels = Bytes::from(rgba.into_raw());
    Ok(DecodedPage {
        handle: Handle::from_rgba(width, height, pixels),
        width,
        height,
    })
}
