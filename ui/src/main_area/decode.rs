//! Downscaled, cached page decoding for the tile viewer.
//!
//! Every page is decoded once at a small size and retained forever: that
//! keeps rapid scrolling smooth (thumbs are always ready) at a memory cost of
//! ~`THUMB_DECODE_EDGE² * 4` bytes per page. Full-resolution pages are
//! decoded on demand for the settled viewport neighborhood and freed again
//! when the viewport moves far away.

use std::sync::Arc;

use bytes::Bytes;
use iced::widget::image::Handle;

/// Longest edge a decoded page may have, regardless of source resolution.
/// Keeps decode fast and uploads within one wgpu atlas layer.
pub const MAX_DECODE_EDGE: u32 = 2048;

/// Longest edge of the retained low-resolution tier. Small enough that a
/// whole chapter stays in memory, large enough to read page composition
/// while scrolling.
pub const THUMB_DECODE_EDGE: u32 = 128;

/// A page decoded at display resolution, ready for GPU upload.
#[derive(Debug, Clone)]
pub struct DecodedPage {
    pub handle: Handle,
    pub width: u32,
    pub height: u32,
}

/// One decode tier of a page.
pub enum Tier {
    /// No decode has been requested yet.
    Absent,
    /// A decode task is in flight.
    Decoding,
    /// Decoded; the buffer survives as long as this tier keeps it.
    Ready(Arc<DecodedPage>),
    Failed,
}

/// Per-page decode state, owned by the app so decoded buffers survive
/// widget rebuilds and scrolling away/back (no blank pages).
///
/// The thumb tier is decoded once and never freed; the full tier is decoded
/// near the settled viewport and evicted when it scrolls far away.
pub struct PageDecode {
    pub thumb: Tier,
    pub full: Tier,
}

impl Default for PageDecode {
    fn default() -> Self {
        Self {
            thumb: Tier::Absent,
            full: Tier::Absent,
        }
    }
}

impl PageDecode {
    /// The best page currently available for drawing: the full tier when
    /// ready, otherwise the thumb tier.
    pub fn image(&self) -> Option<&Arc<DecodedPage>> {
        match &self.full {
            Tier::Ready(page) => Some(page),
            _ => match &self.thumb {
                Tier::Ready(page) => Some(page),
                _ => None,
            },
        }
    }

    /// Whether the retained tier failed to load; the page is broken and no
    /// full decode will ever be requested for it either.
    pub fn thumb_failed(&self) -> bool {
        matches!(self.thumb, Tier::Failed)
    }
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