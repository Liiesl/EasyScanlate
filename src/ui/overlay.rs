use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};

use iced::advanced::graphics::geometry::{self, Fill, Text};
use iced::advanced::text::{
    Alignment as TextAlignment, LineHeight, Paragraph as _, Shaping, Text as ParagraphText,
    Wrapping,
};
use iced::{alignment, Color, Font, Pixels, Point, Size};

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

/// Rendered size of `text` at `size` points, wrapped at `max_width`. Mirrors
/// what `fill_text` lays out (word wrapping at the box width).
fn measure_text(text: &str, font: Font, size: f32, max_width: f32) -> Size {
    let paragraph = iced::advanced::graphics::text::Paragraph::with_text(ParagraphText {
        content: text,
        bounds: Size::new(max_width, f32::INFINITY),
        size: Pixels(size),
        line_height: LineHeight::Relative(1.2),
        font,
        align_x: TextAlignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: Shaping::Auto,
        wrapping: Wrapping::Word,
    });
    paragraph.min_bounds()
}

const MIN_FONT_SIZE: f32 = 1.0;
const FIT_ITERATIONS: u32 = 14;

/// Upper bound on the number of entries in the shared fit cache.
/// Each entry holds one string plus a size, so a few thousand are cheap;
/// when the cap is reached the least recently *inserted* entry is evicted.
const FIT_CACHE_CAP: usize = 2048;

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

type FitKey = (u64, u32, u32, u64);

/// One memoized fitted font size plus the content it was computed for.
struct FitCacheEntry {
    content: String,
    size: f32,
}

/// Shared, bounded cache of fitted font sizes, keyed by a content hash plus
/// the exact fitting parameters (max size, box width/height and font).
///
/// The content is stored alongside the hash and compared on lookup, so a hash
/// collision can never yield a wrong size. The cache is thread-local (the UI
/// thread lays out and draws the whole tree), so it survives the per-frame
/// `OverlayEntry` rebuilds: scrolling re-draws the same entries every frame,
/// and the cache turns those frames into a hash lookup instead of 12
/// paragraph re-shapes per entry.
struct FitCache {
    entries: HashMap<FitKey, FitCacheEntry>,
    /// Insertion order of the keys, used to evict the oldest entry.
    order: VecDeque<FitKey>,
}

/// Runs `f` with the shared fit cache.
///
/// `thread_local!` expands to a named item that cannot capture outer generic
/// parameters, so the cache is stored type-erased and downcast on access.
fn with_fit_cache<R>(f: impl FnOnce(&mut FitCache) -> R) -> R {
    thread_local! {
        static CACHE: RefCell<Option<Box<dyn std::any::Any>>> = RefCell::new(None);
    }

    CACHE.with(|cell| {
        let mut borrowed = cell.borrow_mut();
        let cache: &mut FitCache = borrowed
            .get_or_insert_with(|| {
                Box::new(FitCache {
                    entries: HashMap::new(),
                    order: VecDeque::new(),
                })
            })
            .downcast_mut()
            .expect("fit cache holds an incompatible type");

        f(cache)
    })
}

fn fnv1a(content: &str) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in content.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn font_hash(font: Font) -> u64 {
    let mut hasher = DefaultHasher::new();
    font.hash(&mut hasher);
    hasher.finish()
}

fn fit_key(text: &str, font: Font, bounds: Size) -> FitKey {
    (
        fnv1a(text),
        bounds.width.to_bits(),
        bounds.height.to_bits(),
        font_hash(font),
    )
}

/// Largest font size at which `text` fits inside `bounds` (measured with word
/// wrapping at the box's width). The text always grows *or* shrinks to fill
/// the box: the search ranges from [`MIN_FONT_SIZE`] up to a bound derived
/// from the box size and converges on the largest size that fits, so the
/// result is driven by the bounding rect, not by any style font size.
///
/// Results are memoized in a shared bounded cache keyed by content and the
/// exact box size, so steady-state frames and re-scrolled tiles hit the cache
/// instead of re-shaping the text.
fn fit_font_size(text: &str, font: Font, bounds: Size) -> f32 {
    if text.is_empty() || bounds.width <= 0.0 || bounds.height <= 0.0 {
        return MIN_FONT_SIZE;
    }

    let key = fit_key(text, font, bounds);
    let cached = with_fit_cache(|cache| {
        cache
            .entries
            .get(&key)
            .filter(|entry| entry.content == text)
            .map(|entry| entry.size)
    });
    if let Some(size) = cached {
        return size;
    }

    // Loose cap well above any real fitting size; the search converges on the
    // largest size that actually fits, so the cap only bounds the range.
    let mut low = MIN_FONT_SIZE;
    let mut high = (bounds.width.max(bounds.height) * 2.0).max(MIN_FONT_SIZE);
    for _ in 0..FIT_ITERATIONS {
        let mid = (low + high) / 2.0;
        let measured = measure_text(text, font, mid, bounds.width);
        if measured.width <= bounds.width && measured.height <= bounds.height {
            low = mid;
        } else {
            high = mid;
        }
    }
    let size = low;

    with_fit_cache(|cache| {
        if !cache.entries.contains_key(&key) {
            if cache.entries.len() >= FIT_CACHE_CAP {
                if let Some(evicted) = cache.order.pop_front() {
                    cache.entries.remove(&evicted);
                }
            }
            cache.order.push_back(key);
        }

        cache.entries.insert(
            key,
            FitCacheEntry {
                content: text.to_owned(),
                size,
            },
        );
    });

    size
}

/// Draws one translucent box + label per entry on top of the image inside
/// `frame`. Coordinates are image pixels, scaled to the frame's width. Each
/// label's font size is auto-sized to fill its bounding box (growing or
/// shrinking as needed).
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
        let wrap_width = width.max(8.0);
        let size = fit_font_size(entry.text, font, Size::new(wrap_width, height));
        frame.fill_text(Text {
            content: entry.text.to_string(),
            position,
            max_width: wrap_width,
            size: Pixels(size),
            color: to_color(entry.style.text_color),
            font,
            ..Text::default()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measure_text_is_sane() {
        let size = measure_text("hello world", Font::DEFAULT, 20.0, 400.0);
        assert!(size.width > 30.0, "width {} too small", size.width);
        assert!(size.width < 300.0, "width {} too large", size.width);
        assert!(size.height > 15.0, "height {} too small", size.height);
        assert!(size.height < 45.0, "height {} too large", size.height);
    }

    #[test]
    fn fit_grows_to_fill_big_box() {
        // A big box must yield a size far above the old 14px style default,
        // and the measured text must actually fit the box.
        let bounds = Size::new(400.0, 200.0);
        let size = fit_font_size("hello world", Font::DEFAULT, bounds);
        assert!(size > 40.0, "expected grown size, got {size}");
        let measured = measure_text("hello world", Font::DEFAULT, size, bounds.width);
        assert!(
            measured.width <= bounds.width && measured.height <= bounds.height,
            "size {size} does not fit: {measured:?}"
        );
    }

    #[test]
    fn fit_shrinks_to_fit_small_box() {
        let bounds = Size::new(60.0, 20.0);
        let size = fit_font_size("hello world", Font::DEFAULT, bounds);
        let measured = measure_text("hello world", Font::DEFAULT, size, bounds.width);
        assert!(
            measured.width <= bounds.width && measured.height <= bounds.height,
            "size {size} does not fit: {measured:?}"
        );
    }

    #[test]
    fn cache_returns_consistent_results() {
        let bounds = Size::new(300.0, 100.0);
        let first = fit_font_size("hello world", Font::DEFAULT, bounds);
        let second = fit_font_size("hello world", Font::DEFAULT, bounds);
        assert_eq!(first, second);

        let wider = Size::new(600.0, 100.0);
        let grown = fit_font_size("hello world", Font::DEFAULT, wider);
        assert!(grown > first, "wider box should fit larger text: {grown} <= {first}");
    }
}
