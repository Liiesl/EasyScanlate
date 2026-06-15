//! Downscaled, cached page decoding for the tile viewer.
//!
//! Every page is decoded once at a small size and retained forever: that
//! keeps rapid scrolling smooth (thumbs are always ready) at a memory cost of
//! ~`THUMB_DECODE_EDGE² * 4` bytes per page. Full-resolution pages are
//! decoded on demand for the settled viewport neighborhood and freed again
//! when the viewport moves far away.

use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use iced::widget::image::Handle;
use iced::Task;

use crate::loaded::LoadedImage;

/// Longest edge a decoded page may have, regardless of source resolution.
/// Keeps decode fast and uploads within one wgpu atlas layer.
pub const MAX_DECODE_EDGE: u32 = 2048;

/// Longest edge of the retained low-resolution tier. Small enough that a
/// whole chapter stays in memory, large enough to read page composition
/// while scrolling.
pub const THUMB_DECODE_EDGE: u32 = 128;

/// How many pages beyond the visible range a settled viewport backs with
/// full-res decodes.
pub const DECODE_PRELOAD: usize = 2;

/// How long the viewport must stop scrolling before the full-resolution
/// decode of its neighborhood kicks in.
pub const SETTLE_DEBOUNCE: Duration = Duration::from_millis(150);

/// How many pages beyond the full-backed window a full decode survives a
/// settle before it is evicted.
pub const FULL_KEEP_MARGIN: usize = 4;

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

/// Decodes `path` through the tokio blocking pool; the CPU-bound decode
/// never starves the runtime's worker threads (timers, message dispatch).
async fn decode_async(path: String, max_edge: u32) -> Result<Arc<DecodedPage>, String> {
    tokio::task::spawn_blocking(move || decode_page(&path, max_edge).map(Arc::new))
        .await
        .map_err(|e| format!("decode task cancelled: {e}"))?
}

/// The settled-viewport decode scheduler: debounces visible-range changes
/// (`TilesVisible`), backs the settled window with full-res decodes and
/// evicts far-away full caches. The app owns one of these and forwards the
/// visible-range / settle-elapsed / scroll-ended / decode-finished messages.
#[derive(Debug, Default)]
pub struct Scheduler {
    settle_seq: u64,
    pending_settle: Option<(u64, Range<usize>)>,
    settled: Option<Range<usize>>,
}

impl Scheduler {
    pub fn new() -> Self {
        Self::default()
    }

    /// The pages a settled visible range gets backed with full decodes: the
    /// range itself plus [`DECODE_PRELOAD`] pages on each side.
    pub fn full_window(len: usize, range: &Range<usize>) -> Range<usize> {
        range.start.saturating_sub(DECODE_PRELOAD)
            ..range.end.saturating_add(DECODE_PRELOAD).min(len)
    }

    /// Bumps the settle generation and spawns the debounce task; `map`
    /// turns the sequence number into the app's message.
    pub fn schedule<T>(&mut self, range: Range<usize>, map: impl Fn(u64) -> T + Send + Clone + 'static) -> Task<T>
    where
        T: Send + 'static,
    {
        self.settle_seq += 1;
        let seq = self.settle_seq;
        self.pending_settle = Some((seq, range));
        Task::perform(
            async move {
                tokio::time::sleep(SETTLE_DEBOUNCE).await;
                seq
            },
            map,
        )
    }

    /// True when `seq` is the pending generation (stale debounces no-op).
    pub fn accept_elapsed(&mut self, seq: u64) -> bool {
        self.pending_settle
            .as_ref()
            .is_some_and(|(pending_seq, _)| *pending_seq == seq)
    }

    /// The settled range, if any.
    pub fn settled(&self) -> Option<&Range<usize>> {
        self.settled.as_ref()
    }

    /// Whether `index` lies outside the settled window (needs a settle).
    pub fn needs_settle(&self, index: usize, _len: usize) -> bool {
        self.settled.as_ref().is_none_or(|range| !range.contains(&index))
    }

    /// Whether a full decode for `index` should be kept (inside the settled
    /// window + preload) — used by the `FullDecoded` handler.
    pub fn keep_full(&self, len: usize, index: usize) -> bool {
        self.settled
            .as_ref()
            .is_some_and(|range| Self::full_window(len, range).contains(&index))
    }

    /// Spawns full-res decodes for the pending settle window (visible pages
    /// first, then preload pages outward) and evicts far-away full caches.
    /// No-op when no settle is pending. `map` turns `(index, result)` into
    /// the app's message.
    pub fn settle<T>(
        &mut self,
        images: &mut [LoadedImage],
        map: impl Fn(usize, Result<Arc<DecodedPage>, String>) -> T + Send + Clone + 'static,
    ) -> Task<T>
    where
        T: Send + 'static,
    {
        let Some((_, range)) = self.pending_settle.take() else {
            return Task::none();
        };
        self.settled = Some(range.clone());
        let window = Self::full_window(images.len(), &range);
        let mut indices: Vec<usize> = window.clone().collect();
        // Spawn closest-to-visible pages first so the pages under the viewport
        // swap to full-res before the preload padding.
        let center = (range.start + range.end) as f64 / 2.0;
        indices.sort_by_key(|index| {
            let distance = (*index as f64 - center).abs();
            (distance * 1000.0) as u64
        });
        let mut tasks = Vec::new();
        for index in indices {
            let image = &mut images[index];
            if matches!(image.decode.thumb, Tier::Failed)
                || !matches!(image.decode.full, Tier::Absent)
            {
                continue;
            }
            image.decode.full = Tier::Decoding;
            let path = image.path.clone();
            let map = map.clone();
            tasks.push(Task::perform(
                decode_async(path, MAX_DECODE_EDGE),
                move |result| map(index, result),
            ));
        }
        let keep = range.start.saturating_sub(DECODE_PRELOAD + FULL_KEEP_MARGIN)
            ..range
                .end
                .saturating_add(DECODE_PRELOAD + FULL_KEEP_MARGIN)
                .min(images.len());
        for (index, image) in images.iter_mut().enumerate() {
            if index < keep.start || index >= keep.end {
                image.decode.full = Tier::Absent;
            }
        }
        if tasks.is_empty() {
            Task::none()
        } else {
            Task::batch(tasks)
        }
    }

    /// Spawns thumb decodes for every undecoded image (used on image load).
    pub fn decode_thumbs<T>(
        &mut self,
        images: &mut [LoadedImage],
        map: impl Fn(usize, Result<Arc<DecodedPage>, String>) -> T + Send + Clone + 'static,
    ) -> Task<T>
    where
        T: Send + 'static,
    {
        let tasks: Vec<Task<T>> = images
            .iter_mut()
            .enumerate()
            .map(|(index, image)| {
                image.decode.thumb = Tier::Decoding;
                let path = image.path.clone();
                let map = map.clone();
                Task::perform(
                    decode_async(path, THUMB_DECODE_EDGE),
                    move |result| map(index, result),
                )
            })
            .collect();
        Task::batch(tasks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scanlateit_model::Project;

    fn image() -> LoadedImage {
        LoadedImage {
            width: 100.0,
            height: 100.0,
            path: "x.png".to_string(),
            project: Project::new(),
            decode: PageDecode::default(),
            inpaint: Vec::new(),
        }
    }

    #[test]
    fn full_window_expands_and_clamps_at_both_ends() {
        assert_eq!(Scheduler::full_window(10, &(3..4)), 1..6);
        assert_eq!(Scheduler::full_window(10, &(0..2)), 0..4);
        assert_eq!(Scheduler::full_window(10, &(8..10)), 6..10);
        assert_eq!(Scheduler::full_window(3, &(0..3)), 0..3);
    }

    #[test]
    fn needs_settle_checks_the_settled_window() {
        let mut scheduler = Scheduler::new();
        assert!(scheduler.needs_settle(3, 10));
        scheduler.settled = Some(2..5);
        assert!(!scheduler.needs_settle(3, 10));
        assert!(!scheduler.needs_settle(2, 10));
        assert!(scheduler.needs_settle(1, 10));
        assert!(scheduler.needs_settle(5, 10));
    }

    #[test]
    fn keep_full_holds_the_settled_window_plus_preload() {
        let mut scheduler = Scheduler::new();
        scheduler.settled = Some(3..6);
        assert!(scheduler.keep_full(10, 4));
        assert!(scheduler.keep_full(10, 1));
        assert!(scheduler.keep_full(10, 7));
        assert!(!scheduler.keep_full(10, 0));
        assert!(!scheduler.keep_full(10, 8));
        let none = Scheduler::new();
        assert!(!none.keep_full(10, 4));
    }

    #[test]
    fn accept_elapsed_accepts_only_the_pending_generation() {
        let mut scheduler = Scheduler::new();
        assert!(!scheduler.accept_elapsed(0));
        scheduler.pending_settle = Some((5, 2..4));
        assert!(!scheduler.accept_elapsed(4));
        assert!(scheduler.accept_elapsed(5));
    }

    #[test]
    fn settle_takes_the_pending_range_and_records_it() {
        let mut scheduler = Scheduler::new();
        let mut images: Vec<LoadedImage> = (0..10).map(|_| image()).collect();
        scheduler.pending_settle = Some((1, 2..4));
        let _task = scheduler.settle(&mut images, |_i, _r| ());
        assert_eq!(scheduler.settled(), Some(&(2..4)));
        assert!(scheduler.pending_settle.is_none());
        // Full decodes were requested for the window around 2..4 (0..6);
        // pages beyond the keep range (0..8) stay absent.
        assert!(matches!(images[1].decode.full, Tier::Decoding));
        assert!(matches!(images[5].decode.full, Tier::Decoding));
        assert!(matches!(images[9].decode.full, Tier::Absent));
        // No pending settle left: a second settle is a no-op.
        let _ = scheduler.settle(&mut images, |_i, _r| ());
        assert_eq!(scheduler.settled(), Some(&(2..4)));
    }
}