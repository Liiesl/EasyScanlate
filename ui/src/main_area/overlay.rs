use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};

use iced::advanced::graphics::geometry::{self, Fill, Path, Stroke, Text};
use iced::advanced::text::{
    Alignment as TextAlignment, LineHeight, Paragraph as _, Shaping, Text as ParagraphText,
    Wrapping,
};
use iced::border::Radius;
use iced::font::{Style as FontStyle, Weight as FontWeight};
use iced::{alignment, Color, Font, Pixels, Point, Rectangle, Size, Vector};

use scanlateit_model::{EntryId, EntryStyle, Quad, TextAlign, TextGradientDir};

/// View-model entry: what the overlay draws, resolved from the model with the
/// selected profile's translation and the per-entry style already applied.
#[derive(Clone)]
pub struct OverlayEntry<'a> {
    pub id: EntryId,
    pub text: &'a str,
    /// The entry's free-transformed box in image pixels (may be skewed).
    pub quad: Quad,
    /// `[min_x, min_y, max_x, max_y]` of [`OverlayEntry::quad`], in image
    /// pixels: the box the text is fitted to.
    pub bounds: [f32; 4],
    pub style: EntryStyle,
    /// True when this entry is the one picked in the style panel.
    pub selected: bool,
    /// True when the box is a user-adjusted view quad (move, resize, rotation
    /// or free-transform distortion) instead of the plain OCR quad: the
    /// revert-transform action is offered only then.
    pub quad_overridden: bool,
    /// True while the entry is being edited inline: only the box is drawn,
    /// the text is left to the floating text input on top.
    pub hide_text: bool,
}

/// Outline drawn around the selected entry.
const SELECTED_COLOR: Color = Color::from_rgba8(92, 190, 255, 1.0);
const SELECTED_WIDTH: f32 = 2.0;

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
        wrapping: Wrapping::WordOrGlyph,
    });
    paragraph.min_bounds()
}

/// Whether `content` fits on a single line at `size` when wrapped at `max_width`.
/// Uses the same paragraph settings as `measure_text` (and as the actual
/// circular draw) so shaping, kerning and `WordOrGlyph` wrapping match the
/// renderer—unlike the previous additive `token_width + space_width` estimate
/// which underestimated ligature/kerning widths and missed line-wrap decisions.
/// Mirrors `neverliie_iced_widgets::ellipsis_text`'s real-measurement approach
/// (`text::layout` + `min_bounds`) instead of summing isolated glyph widths.
fn line_fits(content: &str, font: Font, size: f32, max_width: f32) -> bool {
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

const MIN_FONT_SIZE: f32 = 1.0;
const FIT_ITERATIONS: u32 = 14;
/// Relative line height shared by [`measure_text`] and the circular layout.
const LINE_HEIGHT: f32 = 1.2;

/// Upper bound on the number of entries in the shared fit cache.
/// Each entry holds one string plus a size, so a few thousand are cheap;
/// when the cap is reached the least recently *inserted* entry is evicted.
const FIT_CACHE_CAP: usize = 2048;

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

type FitKey = (u64, u32, u32, u64);

/// One memoized fitted font size plus the content it was computed for and the
/// wrapped text height at that size (for vertical centering).
struct FitCacheEntry {
    content: String,
    size: f32,
    height: f32,
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
pub(crate) fn fit_font_size(text: &str, font: Font, bounds: Size) -> f32 {
    fit_font_metrics(text, font, bounds).0
}

/// Like [`fit_font_size`], also returning the wrapped text height at the
/// fitted size, used to vertically center the text inside its box.
pub(crate) fn fit_font_metrics(text: &str, font: Font, bounds: Size) -> (f32, f32) {
    if text.is_empty() || bounds.width <= 0.0 || bounds.height <= 0.0 {
        return (MIN_FONT_SIZE, 0.0);
    }

    let key = fit_key(text, font, bounds);
    let cached = with_fit_cache(|cache| {
        cache
            .entries
            .get(&key)
            .filter(|entry| entry.content == text)
            .map(|entry| (entry.size, entry.height))
    });
    if let Some(metrics) = cached {
        return metrics;
    }

    // Loose cap well above any real fitting size; the search converges on the
    // largest size that actually fits, so the cap only bounds the range.
    let mut low = MIN_FONT_SIZE;
    let mut high = (bounds.width.max(bounds.height) * 2.0).max(MIN_FONT_SIZE);
    let mut fitted_height = 0.0;
    for _ in 0..FIT_ITERATIONS {
        let mid = (low + high) / 2.0;
        let measured = measure_text(text, font, mid, bounds.width);
        if measured.width <= bounds.width && measured.height <= bounds.height {
            low = mid;
            fitted_height = measured.height;
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
                height: fitted_height,
            },
        );
    });

    (size, fitted_height)
}

/// A `Font` for the installed family `name`, memoized: iced's
/// `Font::with_name` requires a `&'static str`, so each distinct family name
/// is leaked once and cached (the set of installed families is finite).
fn family_font(name: &str) -> Font {
    static NAMES: std::sync::OnceLock<std::sync::Mutex<HashMap<String, &'static str>>> =
        std::sync::OnceLock::new();
    let names = NAMES.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut guard = names.lock().expect("font name cache poisoned");
    let leaked = guard
        .entry(name.to_owned())
        .or_insert_with(|| Box::leak(name.to_owned().into_boxed_str()));
    Font::with_name(leaked)
}

/// The entry's font family (when set), weight (bold) and style (italic)
/// applied on top of the base font.
pub(crate) fn styled_font(font: Font, style: &EntryStyle) -> Font {
    let mut font = style
        .font_family
        .as_deref()
        .map(family_font)
        .unwrap_or(font);
    font.weight = if style.bold {
        FontWeight::Bold
    } else {
        FontWeight::Normal
    };
    font.style = if style.italic {
        FontStyle::Italic
    } else {
        FontStyle::Normal
    };
    font
}

/// One laid-out line of a circular bubble: the line's text, its top y offset
/// inside the bubble (box pixels), and the ellipse chord width it was wrapped
/// to, which is also the `max_width` used to center it when drawn.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CircleLine {
    content: String,
    y: f32,
    chord: f32,
}

/// Memoized fit for a circular bubble: the fitted font size plus the lines it
/// produced. The cache mirrors [`FitCache`] (same key, eviction and
/// collision-safety) but lives in its own thread-local slot, since the two
/// caches store different payload types.
struct CircleFitCacheEntry {
    content: String,
    size: f32,
    lines: Vec<CircleLine>,
}

struct CircleFitCache {
    entries: HashMap<FitKey, CircleFitCacheEntry>,
    order: VecDeque<FitKey>,
}

fn with_circle_cache<R>(f: impl FnOnce(&mut CircleFitCache) -> R) -> R {
    thread_local! {
        static CACHE: RefCell<Option<Box<dyn std::any::Any>>> = RefCell::new(None);
    }

    CACHE.with(|cell| {
        let mut borrowed = cell.borrow_mut();
        let cache: &mut CircleFitCache = borrowed
            .get_or_insert_with(|| {
                Box::new(CircleFitCache {
                    entries: HashMap::new(),
                    order: VecDeque::new(),
                })
            })
            .downcast_mut()
            .expect("circle fit cache holds an incompatible type");

        f(cache)
    })
}

/// The ellipse chord width at the vertical position `yc` (box pixels from the
/// top), for an ellipse of the box's size.
fn chord_at(rx: f32, ry: f32, yc: f32) -> f32 {
    let t = 1.0 - ((yc - ry) / ry).powi(2);
    if t <= 0.0 {
        0.0
    } else {
        2.0 * rx * t.sqrt()
    }
}

/// Chord for a line whose centre is at `yc` (used for the vertically
/// centred block). The block is drawn with `y_offset = (bounds.height -
/// total_height)/2`, so the final `yc` is `ry + (i - (n-1)/2)*lh`,
/// not `y + lh/2` from a top-aligned layout. Using the centred `yc` avoids
/// the plateau where the top-aligned `y=0` line always sees a tiny chord
/// near the bubble tip and caps the fitted size.
fn chord_at_centered(rx: f32, ry: f32, yc: f32) -> f32 {
    chord_at(rx, ry, yc)
}

/// Splits `text` into atomic wrap units: whitespace-separated words, with any
/// word wider than `max_width` further split per grapheme (so CJK runs
/// without spaces can still wrap). Unlike the previous `sum(char_width)`
/// approximation, each candidate substring is measured as a shaped paragraph
/// (so kerning/ligatures/combining marks are accounted for), the same
/// principle `neverliie_iced_widgets::ellipsis_text` uses for truncation
/// (`measure` + `min_bounds`).
fn circle_tokens(text: &str, font: Font, size: f32, max_width: f32) -> Vec<String> {
    let mut tokens = Vec::new();
    for word in text.split_whitespace() {
        if measure_text(word, font, size, f32::INFINITY).width <= max_width {
            tokens.push(word.to_string());
            continue;
        }
        let mut sub = String::new();
        for ch in word.chars() {
            let candidate = {
                let mut c = sub.clone();
                c.push(ch);
                c
            };
            let cand_width = measure_text(&candidate, font, size, f32::INFINITY).width;
            if !sub.is_empty() && cand_width > max_width {
                tokens.push(std::mem::take(&mut sub));
                sub.push(ch);
            } else {
                sub = candidate;
            }
        }
        if !sub.is_empty() {
            tokens.push(sub);
        }
    }
    tokens
}

/// Lays `text` into lines following the ellipse of size `bounds`, at font
/// `size`. `None` when the text cannot fit: a row's chord shrinks to zero
/// (line center outside the bubble) or the block would exceed the bubble's
/// height.
///
/// The previous implementation summed `measure_text(token).width` + `space`.
/// That ignores `cosmic_text` shaping (kerning/ligatures, `glyph.x_offset`,
/// `WordOrGlyph` wrap decisions) so `width(sum) != width(joined)`. We now
/// validate each candidate line as a real paragraph (`line_fits`), exactly how
/// `neverliie_iced_widgets::ellipsis_text::truncated` validates its ellipsis
/// candidate (`measure` → `height <= max_height`). A single token that does not
/// fit its chord correctly makes the size too large (`None`), so the binary
/// search shrinks instead of drawing an overlapping overflow line.
///
/// The block is vertically centred at draw time (`y_offset =
/// (bounds.height - total_height)/2`), so chords are computed for centred
/// positions `yc = ry + (i - (n-1)/2)*lh` rather than top-aligned `y + lh/2`.
/// This prevents the plateau where a top-aligned `y=0` line always saw a tiny
/// tip chord and capped the size.
fn layout_circle_lines(
    text: &str,
    font: Font,
    size: f32,
    bounds: Size,
) -> Option<Vec<CircleLine>> {
    let rx = bounds.width / 2.0;
    let ry = bounds.height / 2.0;
    let line_height = size * LINE_HEIGHT;
    if line_height <= 0.0 {
        return None;
    }
    let tokens = circle_tokens(text, font, size, bounds.width);
    if tokens.is_empty() {
        return Some(Vec::new());
    }
    let max_lines = (bounds.height / line_height).floor() as usize;
    if max_lines == 0 {
        return None;
    }

    // Try increasing line counts, using the centred chords for that count.
    // The first `n` that can pack all tokens is the minimal (largest-chord)
    // layout, which gives the tightest fit and the largest allowable size.
    for n in 1..=max_lines {
        let chords: Vec<f32> = (0..n)
            .map(|i| {
                let yc = ry + (i as f32 - (n as f32 - 1.0) / 2.0) * line_height;
                chord_at_centered(rx, ry, yc)
            })
            .collect();

        let mut lines: Vec<CircleLine> = Vec::with_capacity(n);
        let mut idx = 0usize;
        let mut ok = true;
        for (i, &chord) in chords.iter().enumerate() {
            if idx >= tokens.len() {
                break;
            }
            if chord <= 1.0 {
                ok = false;
                break;
            }
            let mut content = String::new();
            while idx < tokens.len() {
                let candidate = if content.is_empty() {
                    tokens[idx].clone()
                } else {
                    format!("{} {}", content, tokens[idx])
                };
                if line_fits(&candidate, font, size, chord) {
                    content = candidate;
                    idx += 1;
                } else if content.is_empty() {
                    ok = false;
                    break;
                } else {
                    break;
                }
            }
            if !ok {
                break;
            }
            // `y` is kept top-aligned for the draw's `y_offset` to re-centre.
            // The stored chord is the centred one, so `max_width` at draw time
            // matches the validation.
            lines.push(CircleLine {
                content,
                y: i as f32 * line_height,
                chord,
            });
            if idx >= tokens.len() {
                break;
            }
        }
        if !ok {
            // Single token wider than its centred chord – this size can never
            // fit, even with more lines (chords only get smaller at the edges).
            // However a larger `n` gives smaller edge chords, so if it fails
            // due to a middle line (large chord) it might still fail for larger
            // `n`. We can continue to try larger `n` only if failure was not
            // due to a middle line? Simplest: continue trying larger `n`; the
            // binary search will ultimately shrink the size.
            continue;
        }
        if idx >= tokens.len() {
            return Some(lines);
        }
        // Not all tokens packed within `n` lines – need more lines.
    }
    None
}

/// Largest font size at which `text` fits `bounds` as a manhwa-style circular
/// bubble: lines are wrapped to the ellipse chord at their height and
/// centered horizontally when drawn (see [`draw_entries`]). Returns the size
/// plus the laid-out lines at that size, memoized like [`fit_font_metrics`].
pub(crate) fn fit_circle_metrics(text: &str, font: Font, bounds: Size) -> (f32, Vec<CircleLine>) {
    if text.is_empty() || bounds.width <= 0.0 || bounds.height <= 0.0 {
        return (MIN_FONT_SIZE, Vec::new());
    }

    let key = fit_key(text, font, bounds);
    let cached = with_circle_cache(|cache| {
        cache
            .entries
            .get(&key)
            .filter(|entry| entry.content == text)
            .map(|entry| (entry.size, entry.lines.clone()))
    });
    if let Some(metrics) = cached {
        return metrics;
    }

    let mut low = MIN_FONT_SIZE;
    let mut high = (bounds.width.max(bounds.height) * 2.0).max(MIN_FONT_SIZE);
    let mut best: Vec<CircleLine> = Vec::new();
    for _ in 0..FIT_ITERATIONS {
        let mid = (low + high) / 2.0;
        match layout_circle_lines(text, font, mid, bounds) {
            Some(lines) => {
                low = mid;
                best = lines;
            }
            None => high = mid,
        }
    }
    let size = low;
    let lines = if best.is_empty() {
        layout_circle_lines(text, font, MIN_FONT_SIZE, bounds).unwrap_or_default()
    } else {
        best
    };

    with_circle_cache(|cache| {
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
            CircleFitCacheEntry {
                content: text.to_owned(),
                size,
                lines: lines.clone(),
            },
        );
    });

    (size, lines)
}

/// Number of color bands a gradient text is split into (axis directions).
const GRADIENT_BANDS: u32 = 16;

fn lerp_color(a: [u8; 4], b: [u8; 4], t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::from_rgba8(
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t).round() as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t).round() as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t).round() as u8,
        (a[3] as f32 + (b[3] as f32 - a[3] as f32) * t) / 255.0,
    )
}

/// Runs `f` on a fresh draft frame clipped to `region` and pastes it back:
/// the `Backend`-trait equivalent of `Frame::with_clip`. The draft is a
/// fresh frame with an identity transform, so the caller must re-apply any
/// parent transform inside `f`.
fn with_clip<F, R>(frame: &mut F, region: Rectangle, f: impl FnOnce(&mut F) -> R) -> R
where
    F: geometry::frame::Backend,
{
    let mut draft = frame.draft(region);
    let result = f(&mut draft);
    frame.paste(draft);
    result
}

/// The gradient parameter `t` at the local point `p` inside `box_rect`.
fn gradient_t(dir: TextGradientDir, box_rect: Rectangle, p: Point) -> f32 {
    let w = box_rect.width.max(1.0);
    let h = box_rect.height.max(1.0);
    let x = (p.x - box_rect.x) / w;
    let y = (p.y - box_rect.y) / h;
    let t = match dir {
        TextGradientDir::TopToBottom => y,
        TextGradientDir::BottomToTop => 1.0 - y,
        TextGradientDir::LeftToRight => x,
        TextGradientDir::RightToLeft => 1.0 - x,
        TextGradientDir::TopLeftToBottomRight => (x + y) / 2.0,
        TextGradientDir::BottomRightToTopLeft => 1.0 - (x + y) / 2.0,
        TextGradientDir::TopRightToBottomLeft => ((1.0 - x) + y) / 2.0,
        TextGradientDir::BottomLeftToTopRight => 1.0 - ((1.0 - x) + y) / 2.0,
    };
    t.clamp(0.0, 1.0)
}

/// Draws `text` with a two-color gradient over `box_rect`. Axis directions
/// (t→b, b→t, l→r, r→l) split the box into `GRADIENT_BANDS` clipped slabs and
/// redraw the text once per slab with the band color (smooth within glyphs);
/// diagonal directions draw one fill per glyph, colored at the glyph's
/// center (their bands are rotated rectangles, which `with_clip` cannot
/// express). `transform` is re-applied inside each clipped draft (drafts are
/// fresh frames without the parent's transform); pass it only for skewed
/// quads. `stroke` is `(color, width)` when the entry has a stroke.
fn fill_gradient_text<F>(
    frame: &mut F,
    text: &Text,
    box_rect: Rectangle,
    dir: TextGradientDir,
    a: [u8; 4],
    b: [u8; 4],
    stroke: Option<(Color, f32)>,
    transform: Option<&QuadTransform>,
    position: Point,
    width: f32,
    height: f32,
) where
    F: geometry::frame::Backend,
{
    match dir {
        TextGradientDir::TopToBottom
        | TextGradientDir::BottomToTop
        | TextGradientDir::LeftToRight
        | TextGradientDir::RightToLeft => {
            let vertical = matches!(dir, TextGradientDir::TopToBottom | TextGradientDir::BottomToTop);
            let reversed = matches!(dir, TextGradientDir::BottomToTop | TextGradientDir::RightToLeft);
            for band in 0..GRADIENT_BANDS {
                let t0 = band as f32 / GRADIENT_BANDS as f32;
                let t1 = (band + 1) as f32 / GRADIENT_BANDS as f32;
                let t = if reversed { 1.0 - (t0 + t1) / 2.0 } else { (t0 + t1) / 2.0 };
                let color = lerp_color(a, b, t);
                let strip = if vertical {
                    Rectangle::new(
                        Point::new(box_rect.x, box_rect.y + t0 * box_rect.height),
                        Size::new(box_rect.width, (t1 - t0) * box_rect.height),
                    )
                } else {
                    Rectangle::new(
                        Point::new(box_rect.x + t0 * box_rect.width, box_rect.y),
                        Size::new((t1 - t0) * box_rect.width, box_rect.height),
                    )
                };
                with_clip(frame, strip, |f| {
                    if let Some(transform) = transform {
                        f.push_transform();
                        apply_quad_transform(f, transform, position, width, height);
                    }
                    let colored = Text { color, ..text.clone() };
                    if let Some((stroke_color, stroke_width)) = stroke {
                        f.stroke_text(
                            colored.clone(),
                            Stroke::default().with_color(stroke_color).with_width(stroke_width),
                        );
                    }
                    f.fill_text(colored);
                });
            }
        }
        _ => fill_gradient_glyphs(frame, text, box_rect, dir, a, b, stroke),
    }
}

/// Per-glyph gradient fill for the diagonal directions: mirrors
/// `geometry::Text::draw_with` (iced/graphics/src/geometry/text.rs:48) but
/// colors each glyph by the gradient sampled at the glyph's center.
fn fill_gradient_glyphs<F>(
    frame: &mut F,
    text: &Text,
    box_rect: Rectangle,
    dir: TextGradientDir,
    a: [u8; 4],
    b: [u8; 4],
    stroke: Option<(Color, f32)>,
) where
    F: geometry::frame::Backend,
{
    use iced::advanced::graphics::text::{self as gfx_text, cosmic_text, Paragraph as GfxParagraph};

    let paragraph = GfxParagraph::with_text(ParagraphText {
        content: text.content.as_str(),
        bounds: Size::new(text.max_width, f32::INFINITY),
        size: text.size,
        line_height: text.line_height,
        font: text.font,
        align_x: text.align_x,
        align_y: alignment::Vertical::Top,
        shaping: text.shaping,
        wrapping: Wrapping::Word,
    });
    let translation_x = match text.align_x {
        TextAlignment::Default | TextAlignment::Left | TextAlignment::Justified => text.position.x,
        TextAlignment::Center => text.position.x - paragraph.min_width() / 2.0,
        TextAlignment::Right => text.position.x - paragraph.min_width(),
    };
    let translation_y = text.position.y;
    let buffer = paragraph.buffer();
    let mut swash_cache = cosmic_text::SwashCache::new();
    let mut font_system = gfx_text::font_system().write().expect("Write font system");
    for run in buffer.layout_runs() {
        for glyph in run.glyphs.iter() {
            let physical_glyph = glyph.physical((0.0, 0.0), 1.0);
            let start_x = translation_x + glyph.x + glyph.x_offset;
            let start_y = translation_y + glyph.y_offset + run.line_y;
            let offset = Vector::new(start_x, start_y);
            let color = lerp_color(
                a,
                b,
                gradient_t(dir, box_rect, Point::new(start_x + glyph.w / 2.0, start_y + text.size.0 / 2.0)),
            );
            if let Some(commands) =
                swash_cache.get_outline_commands(font_system.raw(), physical_glyph.cache_key)
            {
                let glyph_path = Path::new(|path| {
                    use cosmic_text::Command;
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
                frame.fill(&glyph_path, Fill::from(color));
            } else {
                let [r, g, bl, al] = color.into_rgba8();
                swash_cache.with_pixels(
                    font_system.raw(),
                    physical_glyph.cache_key,
                    cosmic_text::Color::rgba(r, g, bl, al),
                    |x, y, pixel| {
                        frame.fill(
                            &Path::rectangle(
                                Point::new(x as f32, y as f32) + offset,
                                Size::new(1.0, 1.0),
                            ),
                            Fill::from(Color::from_rgba8(
                                pixel.r(),
                                pixel.g(),
                                pixel.b(),
                                pixel.a() as f32 / 255.0,
                            )),
                        );
                    },
                );
            }
        }
    }
}

/// How far (frame pixels) a quad may deviate from a parallelogram before the
/// text switches from the single-affine draw to the per-glyph warp path.
const WARP_THRESHOLD_PX: f32 = 0.5;

/// One warped glyph: its rect in paragraph-local space (x, y, w, h) plus its
/// outline path positioned at that rect (y-flipped, same convention as
/// [`fill_gradient_glyphs`]).
#[derive(Clone)]
struct WarpGlyph {
    rect: [f32; 4],
    path: Path,
}

/// The flat layout the warp path draws from: per-glyph rects and outline
/// paths in paragraph-local space plus the paragraph's min width (mirrors
/// the geometry backend's alignment translation).
#[derive(Clone)]
struct WarpLayout {
    glyphs: Vec<WarpGlyph>,
    min_width: f32,
}

struct WarpCacheEntry {
    content: String,
    layout: WarpLayout,
}

struct WarpCache {
    entries: HashMap<FitKey, WarpCacheEntry>,
    order: VecDeque<FitKey>,
}

/// Runs `f` with the shared warp layout cache.
fn with_warp_cache<R>(f: impl FnOnce(&mut WarpCache) -> R) -> R {
    thread_local! {
        static CACHE: RefCell<Option<Box<dyn std::any::Any>>> = RefCell::new(None);
    }

    CACHE.with(|cell| {
        let mut borrowed = cell.borrow_mut();
        let cache: &mut WarpCache = borrowed
            .get_or_insert_with(|| {
                Box::new(WarpCache {
                    entries: HashMap::new(),
                    order: VecDeque::new(),
                })
            })
            .downcast_mut()
            .expect("warp cache holds an incompatible type");

        f(cache)
    })
}

/// Shapes `text` flat (word-wrapped at `wrap_width`) into per-glyph rects and
/// outline paths in paragraph-local space, memoized like [`FitCache`]. Mirrors
/// [`measure_text`]'s paragraph settings so the flat layout matches what
/// `fill_text` would draw.
fn shape_warp_layout(text: &str, font: Font, size: f32, wrap_width: f32) -> WarpLayout {
    if text.is_empty() || wrap_width <= 0.0 {
        return WarpLayout {
            glyphs: Vec::new(),
            min_width: 0.0,
        };
    }
    let key = (
        fnv1a(text),
        size.to_bits(),
        wrap_width.to_bits(),
        font_hash(font),
    );
    with_warp_cache(|cache| {
        if let Some(entry) = cache.entries.get(&key).filter(|entry| entry.content == text) {
            return entry.layout.clone();
        }
        let layout = build_warp_layout(text, font, size, wrap_width);
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
            WarpCacheEntry {
                content: text.to_owned(),
                layout: layout.clone(),
            },
        );
        layout
    })
}

fn build_warp_layout(text: &str, font: Font, size: f32, wrap_width: f32) -> WarpLayout {
    use iced::advanced::graphics::text::{self as gfx_text, cosmic_text, Paragraph as GfxParagraph};

    let paragraph = GfxParagraph::with_text(ParagraphText {
        content: text,
        bounds: Size::new(wrap_width, f32::INFINITY),
        size: Pixels(size),
        line_height: LineHeight::Relative(1.2),
        font,
        align_x: TextAlignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: Shaping::Auto,
        wrapping: Wrapping::Word,
    });
    let min_width = paragraph.min_width();
    let buffer = paragraph.buffer();
    let mut swash_cache = cosmic_text::SwashCache::new();
    let mut font_system = gfx_text::font_system().write().expect("Write font system");
    let mut glyphs = Vec::new();
    for run in buffer.layout_runs() {
        for glyph in run.glyphs.iter() {
            let physical_glyph = glyph.physical((0.0, 0.0), 1.0);
            let gx = glyph.x + glyph.x_offset;
            let gy = glyph.y_offset + run.line_y;
            let gw = glyph.w;
            if gw <= 0.0 {
                continue;
            }
            let offset = Vector::new(gx, gy);
            let Some(commands) =
                swash_cache.get_outline_commands(font_system.raw(), physical_glyph.cache_key)
            else {
                continue;
            };
            // The outline is y-up from the glyph's baseline; the rect the
            // warp maps must be the glyph's actual ink box, or the affine is
            // fitted to a band shifted below the ink (and, for the last line,
            // clamped past the box's bottom edge). Track the flipped,
            // offset command extents.
            let mut min_x = f32::INFINITY;
            let mut min_y = f32::INFINITY;
            let mut max_x = f32::NEG_INFINITY;
            let mut max_y = f32::NEG_INFINITY;
            let mut track = |p: Point| {
                min_x = min_x.min(p.x);
                min_y = min_y.min(p.y);
                max_x = max_x.max(p.x);
                max_y = max_y.max(p.y);
            };
            let path = Path::new(|path| {
                use cosmic_text::Command;
                for command in commands {
                    match command {
                        Command::MoveTo(p) => {
                            let point = Point::new(p.x, -p.y) + offset;
                            track(point);
                            path.move_to(point);
                        }
                        Command::LineTo(p) => {
                            let point = Point::new(p.x, -p.y) + offset;
                            track(point);
                            path.line_to(point);
                        }
                        Command::CurveTo(control_a, control_b, to) => {
                            let point_a = Point::new(control_a.x, -control_a.y) + offset;
                            let point_b = Point::new(control_b.x, -control_b.y) + offset;
                            let point_to = Point::new(to.x, -to.y) + offset;
                            track(point_a);
                            track(point_b);
                            track(point_to);
                            path.bezier_curve_to(point_a, point_b, point_to);
                        }
                        Command::QuadTo(control, to) => {
                            let point_c = Point::new(control.x, -control.y) + offset;
                            let point_to = Point::new(to.x, -to.y) + offset;
                            track(point_c);
                            track(point_to);
                            path.quadratic_curve_to(point_c, point_to);
                        }
                        Command::Close => path.close(),
                    }
                }
            });
            if min_x.is_infinite() || min_y.is_infinite() {
                continue;
            }
            glyphs.push(WarpGlyph {
                rect: [min_x, min_y, max_x - min_x, max_y - min_y],
                path,
            });
        }
    }
    WarpLayout { glyphs, min_width }
}

/// The projective (homography) map of `box_rect` onto the quad corners
/// ordered TL/TR/BR/BL: the quad is the perspective image of the box's
/// rectangle, so `P(u, v)` with `u, v` the point's relative position inside
/// `box_rect` follows a planar projection. All horizontal lines therefore
/// share one vanishing point instead of fanning between the edge angles
/// (bilinear blending). Degenerate (parallelogram) quads fall back to the
/// affine form; the warp path only runs for quads with a real affine error,
/// so the fallback is never taken there.
fn perspective_map(quad: [[f32; 2]; 4], box_rect: Rectangle, p: Point) -> Point {
    let u = ((p.x - box_rect.x) / box_rect.width.max(1.0)).clamp(0.0, 1.0);
    let v = ((p.y - box_rect.y) / box_rect.height.max(1.0)).clamp(0.0, 1.0);
    let [x0, y0] = quad[0];
    let [x1, y1] = quad[1];
    let [x2, y2] = quad[2];
    let [x3, y3] = quad[3];
    let dx1 = x1 - x2;
    let dx2 = x3 - x2;
    let dy1 = y1 - y2;
    let dy2 = y3 - y2;
    let denom = dx1 * dy2 - dy1 * dx2;
    let (a, b, c, d, e, f, g, h) = if denom.abs() < 1e-7 {
        (x1 - x0, x3 - x0, x0, y1 - y0, y3 - y0, y0, 0.0, 0.0)
    } else {
        let sx = x0 - x1 + x2 - x3;
        let sy = y0 - y1 + y2 - y3;
        let g = (sx * dy2 - sy * dx2) / denom;
        let h = (dx1 * sy - dy1 * sx) / denom;
        (
            x1 - x0 + g * x1,
            x3 - x0 + h * x3,
            x0,
            y1 - y0 + g * y1,
            y3 - y0 + h * y3,
            y0,
            g,
            h,
        )
    };
    let w = g * u + h * v + 1.0;
    Point::new((a * u + b * v + c) / w, (d * u + e * v + f) / w)
}

/// How badly the single-affine fit ([`fit_affine`]) misses `quad`'s corners:
/// the largest distance between a quad corner and the rect corner the fitted
/// affine maps it to. Zero for parallelograms; tens of pixels for stretched
/// trapezoids. Drives the warp-vs-affine decision in [`draw_entries`].
fn affine_error(quad: [[f32; 2]; 4], width: f32, height: f32) -> f32 {
    let Some((m00, m01, m10, m11)) = fit_affine(quad, width, height) else {
        return f32::INFINITY;
    };
    let center_x = (quad[0][0] + quad[1][0] + quad[2][0] + quad[3][0]) / 4.0;
    let center_y = (quad[0][1] + quad[1][1] + quad[2][1] + quad[3][1]) / 4.0;
    let half_w = width / 2.0;
    let half_h = height / 2.0;
    let rect_corners = [
        [-half_w, -half_h],
        [half_w, -half_h],
        [half_w, half_h],
        [-half_w, half_h],
    ];
    let mut error: f32 = 0.0;
    for index in 0..4 {
        let (dx, dy) = (rect_corners[index][0], rect_corners[index][1]);
        let mapped = [
            center_x + m00 * dx + m01 * dy,
            center_y + m10 * dx + m11 * dy,
        ];
        let diff_x = mapped[0] - quad[index][0];
        let diff_y = mapped[1] - quad[index][1];
        error = error.max((diff_x * diff_x + diff_y * diff_y).sqrt());
    }
    error
}

/// Draws `text` warped through the free quad: each glyph is mapped through
/// the quad's projective field (a planar surface, so every text line shares
/// the quad's vanishing point) and drawn with its own small affine, so the
/// text tracks trapezoids and perspective shapes that no single affine can
/// follow. `quad` must be ordered TL/TR/BR/BL and `box_rect` is the box the
/// text was laid out against (its AABB). `stroke` is `(color, width)` when
/// the entry has a stroke; `gradient` colors each glyph by the gradient at
/// its center.
fn draw_warped_text<F>(
    frame: &mut F,
    text: &Text,
    box_rect: Rectangle,
    quad: [[f32; 2]; 4],
    stroke: Option<(Color, f32)>,
    gradient: Option<(TextGradientDir, [u8; 4], [u8; 4])>,
) where
    F: geometry::frame::Backend,
{
    let layout = shape_warp_layout(&text.content, text.font, text.size.0, text.max_width);
    if layout.glyphs.is_empty() {
        return;
    }
    let translation_x = match text.align_x {
        TextAlignment::Default | TextAlignment::Left | TextAlignment::Justified => text.position.x,
        TextAlignment::Center => text.position.x - layout.min_width / 2.0,
        TextAlignment::Right => text.position.x - layout.min_width,
    };
    let translation_y = text.position.y;
    for glyph in &layout.glyphs {
        let [gx, gy, gw, gh] = glyph.rect;
        let ax = translation_x + gx;
        let ay = translation_y + gy;
        let corners: [[f32; 2]; 4] = [
            perspective_map(quad, box_rect, Point::new(ax, ay)),
            perspective_map(quad, box_rect, Point::new(ax + gw, ay)),
            perspective_map(quad, box_rect, Point::new(ax + gw, ay + gh)),
            perspective_map(quad, box_rect, Point::new(ax, ay + gh)),
        ]
        .map(|p| [p.x, p.y]);
        let Some((m00, m01, m10, m11)) = fit_affine(corners, gw, gh) else {
            continue;
        };
        if m00 * m11 - m01 * m10 <= 0.0 {
            continue;
        }
        let (mut s1, mut s2, beta, alpha) = svd2(m00, m01, m10, m11);
        s1 = s1.max(0.01);
        s2 = s2.max(0.01);
        let [min_x, min_y, max_x, max_y] = quad_bounds(corners);
        let quad_center = Point::new((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
        let rect_center = Point::new(gx + gw / 2.0, gy + gh / 2.0);
        let color = match gradient {
            Some((dir, a, b)) => lerp_color(
                a,
                b,
                gradient_t(dir, box_rect, Point::new(ax + gw / 2.0, ay + gh / 2.0)),
            ),
            None => text.color,
        };
        frame.push_transform();
        frame.translate(Vector::new(quad_center.x, quad_center.y));
        frame.rotate(beta);
        frame.scale_nonuniform(Vector::new(s1, s2));
        frame.rotate(-alpha);
        frame.translate(Vector::new(-rect_center.x, -rect_center.y));
        if let Some((stroke_color, stroke_width)) = stroke {
            frame.stroke(
                &glyph.path,
                Stroke::default().with_color(stroke_color).with_width(stroke_width),
            );
        }
        frame.fill(&glyph.path, Fill::from(color));
        frame.pop_transform();
    }
}

/// Draws one translucent box + label per entry on top of the image inside
/// `frame`. Coordinates are image pixels, scaled to the frame's width. Each
/// label's font size is auto-sized to fill its bounding box (growing or
/// shrinking as needed). When the entry's [`TextAlign`] is `Circular`, the
/// text is wrapped manhwa-style: one centered line per row, each line's width
/// following the ellipse chord at its height inside the box. When `hide_text`
/// is set, the whole overlay layer is skipped per entry — box background,
/// selection outline and text (the toolbar's "Hide Text" toggle). The
/// per-entry [`OverlayEntry::hide_text`] flag (inline editing) hides only the
/// text, keeping the box background drawn under the floating editor.
pub fn draw_entries<'a, I, F>(
    frame: &mut F,
    entries: I,
    font: Font,
    image_width: f32,
    hide_text: bool,
) where
    F: geometry::frame::Backend,
    I: IntoIterator<Item = &'a OverlayEntry<'a>>,
{
    let scale = frame.width() / image_width.max(1.0);
    for entry in entries {
        if hide_text {
            continue;
        }
        let quad = entry.quad.points.map(|p| [p[0] * scale, p[1] * scale]);
        let rotated = rotated_rect_geometry(quad).and_then(|(tl, w, h, angle)| {
            let upright = angle.rem_euclid(2.0 * std::f32::consts::PI);
            let is_upright = upright.abs() < 0.01 || (upright - 2.0 * std::f32::consts::PI).abs() < 0.01;
            if is_upright {
                None
            } else {
                Some((
                    tl,
                    w,
                    h,
                    QuadTransform {
                        angle1: 0.0,
                        scale_x: 1.0005,
                        scale_y: 1.0 / 1.0005,
                        angle2: angle,
                    },
                ))
            }
        });

        let (layout_position, layout_width, layout_height, layout_transform) = match rotated {
            Some((tl, w, h, t)) => (tl, w, h, Some(t)),
            None => {
                let w_top = ((quad[1][0] - quad[0][0]).powi(2) + (quad[1][1] - quad[0][1]).powi(2)).sqrt();
                let w_bot = ((quad[2][0] - quad[3][0]).powi(2) + (quad[2][1] - quad[3][1]).powi(2)).sqrt();
                let h_left = ((quad[3][0] - quad[0][0]).powi(2) + (quad[3][1] - quad[0][1]).powi(2)).sqrt();
                let h_right = ((quad[2][0] - quad[1][0]).powi(2) + (quad[2][1] - quad[1][1]).powi(2)).sqrt();
                let w = ((w_top + w_bot) / 2.0).max(1.0);
                let h = ((h_left + h_right) / 2.0).max(1.0);
                let center_x = (quad[0][0] + quad[1][0] + quad[2][0] + quad[3][0]) / 4.0;
                let center_y = (quad[0][1] + quad[1][1] + quad[2][1] + quad[3][1]) / 4.0;
                let pos = Point::new(center_x - w / 2.0, center_y - h / 2.0);
                let transform = quad_transform(quad, w, h);
                (pos, w, h, transform)
            }
        };

        let path = match layout_transform {
            Some(_) => quad_path(quad),
            None => Path::rounded_rectangle(
                layout_position,
                Size::new(layout_width, layout_height),
                Radius::from(entry.style.bg_radius * scale),
            ),
        };
        frame.fill(&path, Fill::from(to_color(entry.style.bg_color)));
        if entry.selected {
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(SELECTED_COLOR)
                    .with_width(SELECTED_WIDTH),
            );
        }
        let wrap_width = layout_width.max(8.0);
        if entry.hide_text {
            continue;
        }
        let styled = styled_font(font, &entry.style);
        let box_rect = Rectangle::new(layout_position, Size::new(layout_width, layout_height));
        let warp = rotated.is_none()
            && layout_transform.is_some()
            && affine_error(quad, layout_width, layout_height) > WARP_THRESHOLD_PX;
        let stroke = (entry.style.stroke_width > 0.0).then(|| {
            (to_color(entry.style.stroke_color), entry.style.stroke_width * scale)
        });
        let gradient = entry.style.text_gradient.then(|| {
            (entry.style.gradient_dir, entry.style.gradient_a, entry.style.gradient_b)
        });

        if entry.style.text_align == TextAlign::Circular {
            let (size, lines) =
                fit_circle_metrics(entry.text, styled, Size::new(wrap_width, layout_height));
            let line_height = size * LINE_HEIGHT;
            let total_height = lines.last().map_or(0.0, |line| line.y + line_height);
            let y_offset = (layout_height - total_height).max(0.0) / 2.0;
            let block_rect = Rectangle::new(
                Point::new(layout_position.x, layout_position.y + y_offset),
                Size::new(wrap_width, total_height),
            );
            if warp {
                for line in &lines {
                    let text = Text {
                        content: line.content.clone(),
                        position: Point::new(
                            layout_position.x + wrap_width / 2.0,
                            layout_position.y + y_offset + line.y,
                        ),
                        max_width: line.chord,
                        size: Pixels(size),
                        color: to_color(entry.style.text_color),
                        font: styled,
                        align_x: TextAlignment::Center,
                        ..Text::default()
                    };
                    draw_warped_text(frame, &text, box_rect, quad, stroke, gradient);
                }
            } else {
                if let Some(transform) = &layout_transform {
                    frame.push_transform();
                    apply_quad_transform(
                        frame,
                        transform,
                        layout_position,
                        layout_width,
                        layout_height,
                    );
                }
                for line in &lines {
                    let text = Text {
                        content: line.content.clone(),
                        position: Point::new(
                            layout_position.x + wrap_width / 2.0,
                            layout_position.y + y_offset + line.y,
                        ),
                        max_width: line.chord,
                        size: Pixels(size),
                        color: to_color(entry.style.text_color),
                        font: styled,
                        align_x: TextAlignment::Center,
                        ..Text::default()
                    };
                    if entry.style.text_gradient {
                        fill_gradient_text(
                            frame,
                            &text,
                            block_rect,
                            entry.style.gradient_dir,
                            entry.style.gradient_a,
                            entry.style.gradient_b,
                            (entry.style.stroke_width > 0.0).then(|| {
                                (to_color(entry.style.stroke_color), entry.style.stroke_width * scale)
                            }),
                            layout_transform.as_ref(),
                            layout_position,
                            layout_width,
                            layout_height,
                        );
                    } else {
                        if entry.style.stroke_width > 0.0 {
                            frame.stroke_text(
                                text.clone(),
                                Stroke::default()
                                    .with_color(to_color(entry.style.stroke_color))
                                    .with_width(entry.style.stroke_width * scale),
                            );
                        }
                        frame.fill_text(text);
                    }
                }
                if layout_transform.is_some() {
                    frame.pop_transform();
                }
            }
            continue;
        }

        let (size, fitted_height) = fit_font_metrics(
            entry.text,
            styled,
            Size::new(wrap_width, layout_height),
        );
        let y_offset = (layout_height - fitted_height).max(0.0) / 2.0;
        let block_rect = Rectangle::new(
            Point::new(layout_position.x, layout_position.y + y_offset),
            Size::new(wrap_width, fitted_height),
        );
        let (align_x, text_x) = match entry.style.text_align {
            TextAlign::Circular => (TextAlignment::Default, layout_position.x),
            TextAlign::Left => (TextAlignment::Left, layout_position.x),
            TextAlign::Center => (TextAlignment::Center, layout_position.x + wrap_width / 2.0),
            TextAlign::Right => (TextAlignment::Right, layout_position.x + wrap_width),
        };
        let text = Text {
            content: entry.text.to_string(),
            position: Point::new(text_x, layout_position.y + y_offset),
            max_width: wrap_width,
            size: Pixels(size),
            color: to_color(entry.style.text_color),
            font: styled,
            align_x,
            ..Text::default()
        };

        if warp {
            draw_warped_text(frame, &text, box_rect, quad, stroke, gradient);
        } else {
            if let Some(transform) = &layout_transform {
                frame.push_transform();
                apply_quad_transform(
                    frame,
                    transform,
                    layout_position,
                    layout_width,
                    layout_height,
                );
            }
            if entry.style.text_gradient {
                fill_gradient_text(
                    frame,
                    &text,
                    block_rect,
                    entry.style.gradient_dir,
                    entry.style.gradient_a,
                    entry.style.gradient_b,
                    (entry.style.stroke_width > 0.0).then(|| {
                        (to_color(entry.style.stroke_color), entry.style.stroke_width * scale)
                    }),
                    layout_transform.as_ref(),
                    layout_position,
                    layout_width,
                    layout_height,
                );
            } else {
                if entry.style.stroke_width > 0.0 {
                    frame.stroke_text(
                        text.clone(),
                        Stroke::default()
                            .with_color(to_color(entry.style.stroke_color))
                            .with_width(entry.style.stroke_width * scale),
                    );
                }
                frame.fill_text(text);
            }
            if layout_transform.is_some() {
                frame.pop_transform();
            }
        }
    }
}

/// The entry's quad as a closed 4-point path, in whatever space the caller
/// provides.
fn quad_path(quad: [[f32; 2]; 4]) -> Path {
    Path::new(|builder| {
        builder.move_to(Point::new(quad[0][0], quad[0][1]));
        builder.line_to(Point::new(quad[1][0], quad[1][1]));
        builder.line_to(Point::new(quad[2][0], quad[2][1]));
        builder.line_to(Point::new(quad[3][0], quad[3][1]));
        builder.close();
    })
}

/// An affine map `M = R(angle2) * S(scale_x, scale_y) * R(angle1)` that maps
/// an axis-aligned rect onto a free-transformed quad.
#[derive(Debug, Clone, Copy)]
struct QuadTransform {
    angle1: f32,
    scale_x: f32,
    scale_y: f32,
    angle2: f32,
}

/// Rotates and skews the current frame transform so that drawing in the
/// axis-aligned rect of size `width` x `height` at `position` lands on the
/// quad the transform was fitted to.
fn apply_quad_transform<F>(
    frame: &mut F,
    transform: &QuadTransform,
    position: Point,
    width: f32,
    height: f32,
) where
    F: geometry::frame::Backend,
{
    let center = Point::new(position.x + width / 2.0, position.y + height / 2.0);
    // Backend frame transforms compose as M = M * op, so later calls apply
    // first to points: the sequence composes to
    // T(center) * R(angle2) * S * R(angle1) * T(-center), i.e. the affine map
    // applied around the box center.
    frame.translate(Vector::new(center.x, center.y));
    frame.rotate(transform.angle2);
    frame.scale_nonuniform(Vector::new(transform.scale_x, transform.scale_y));
    frame.rotate(transform.angle1);
    frame.translate(Vector::new(-center.x, -center.y));
}

/// The bounding box of a quad as `[min_x, min_y, max_x, max_y]`.
fn quad_bounds(quad: [[f32; 2]; 4]) -> [f32; 4] {
    let min_x = quad.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
    let min_y = quad.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
    let max_x = quad.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max);
    let max_y = quad.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max);
    [min_x, min_y, max_x, max_y]
}

/// Reorders the quad's points so index `0..4` matches the bounding-box
/// corners TL, TR, BR, BL, by assigning each AABB corner its nearest unused
/// quad point (correct for any convex quad).
pub(crate) fn order_quad(quad: [[f32; 2]; 4]) -> [[f32; 2]; 4] {
    Quad { points: quad }.ordered()
}

/// The quad's local rect when it is a rotated rectangle: `(tl, w, h, angle)`
/// with `w`/`h` the true edge lengths and `angle` the top-edge direction.
/// `None` for sheared/perspective quads (adjacent edges not perpendicular)
/// and degenerate boxes. Used to lay out and fit text against the box's own
/// axes instead of its axis-aligned bounding box, so rotated text keeps its
/// size and wrap width.
fn rotated_rect_geometry(quad: [[f32; 2]; 4]) -> Option<(Point, f32, f32, f32)> {
    let top = [quad[1][0] - quad[0][0], quad[1][1] - quad[0][1]];
    let bottom = [quad[2][0] - quad[3][0], quad[2][1] - quad[3][1]];
    let left = [quad[3][0] - quad[0][0], quad[3][1] - quad[0][1]];
    let right = [quad[2][0] - quad[1][0], quad[2][1] - quad[1][1]];
    let w = (top[0] * top[0] + top[1] * top[1]).sqrt();
    let h = (left[0] * left[0] + left[1] * left[1]).sqrt();
    if w <= f32::EPSILON || h <= f32::EPSILON {
        return None;
    }
    let w_bot = (bottom[0] * bottom[0] + bottom[1] * bottom[1]).sqrt();
    let h_right = (right[0] * right[0] + right[1] * right[1]).sqrt();
    if (w - w_bot).abs() / w.max(w_bot) > 0.05 || (h - h_right).abs() / h.max(h_right) > 0.05 {
        return None;
    }
    let dot = top[0] * left[0] + top[1] * left[1];
    if dot.abs() / (w * h) > 0.05 {
        return None;
    }
    let angle = top[1].atan2(top[0]);
    let center_x = (quad[0][0] + quad[1][0] + quad[2][0] + quad[3][0]) / 4.0;
    let center_y = (quad[0][1] + quad[1][1] + quad[2][1] + quad[3][1]) / 4.0;
    let unrotated_pos = Point::new(center_x - w / 2.0, center_y - h / 2.0);
    Some((unrotated_pos, w, h, angle))
}

/// The affine 2x2 matrix (least squares) mapping the corners of the
/// `width` x `height` rect onto the quad: returns
/// `(m00, m01, m10, m11)` with `x' = m00*x + m01*y` etc. Exact for
/// parallelograms; best fit for perspective quads.
///
/// The rect corners are relative to the rect center and the quad points
/// relative to the quad's bounding-box center, so the returned matrix needs
/// no translation.
fn fit_affine(quad: [[f32; 2]; 4], width: f32, height: f32) -> Option<(f32, f32, f32, f32)> {
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let half_w = width / 2.0;
    let half_h = height / 2.0;
    let center_x = (quad[0][0] + quad[1][0] + quad[2][0] + quad[3][0]) / 4.0;
    let center_y = (quad[0][1] + quad[1][1] + quad[2][1] + quad[3][1]) / 4.0;
    let lx = [-half_w, half_w, half_w, -half_w];
    let ly = [-half_h, -half_h, half_h, half_h];
    let mut m00 = 0.0;
    let mut m10 = 0.0;
    let mut m01 = 0.0;
    let mut m11 = 0.0;
    for index in 0..4 {
        let (qx, qy) = (quad[index][0] - center_x, quad[index][1] - center_y);
        m00 += lx[index] * qx;
        m10 += lx[index] * qy;
        m01 += ly[index] * qx;
        m11 += ly[index] * qy;
    }
    m00 /= width * width;
    m10 /= width * width;
    m01 /= height * height;
    m11 /= height * height;
    Some((m00, m01, m10, m11))
}

/// Singular value decomposition of a 2x2 matrix: `A = U * S * V^T` with
/// `U = R(beta)` and `V = R(alpha)`. Returns `(s1, s2, beta, alpha)`.
fn svd2(m00: f32, m01: f32, m10: f32, m11: f32) -> (f32, f32, f32, f32) {
    // A^T A = [[a, b], [b, c]]
    let a = m00 * m00 + m10 * m10;
    let b = m00 * m01 + m10 * m11;
    let c = m01 * m01 + m11 * m11;
    let trace = a + c;
    let discriminant = ((a - c) * (a - c) + 4.0 * b * b).sqrt();
    let lambda1 = (trace + discriminant) / 2.0;
    let lambda2 = (trace - discriminant) / 2.0;
    let s1 = lambda1.sqrt();
    let s2 = lambda2.sqrt();
    // Eigenvector of the larger eigenvalue: (b, lambda1 - a).
    let (v1x, v1y) = if b.abs() > f32::EPSILON {
        let len = (b * b + (lambda1 - a) * (lambda1 - a)).sqrt();
        (b / len, (lambda1 - a) / len)
    } else {
        (1.0, 0.0)
    };
    let alpha = v1y.atan2(v1x);
    // U columns: u1 = A·v1 / s1, u2 = A·v2 / s2 with v2 = (-v1y, v1x).
    let u1x = (m00 * v1x + m01 * v1y) / s1;
    let u1y = (m10 * v1x + m11 * v1y) / s1;
    let mut u2x = (-m00 * v1y + m01 * v1x) / s2;
    let mut u2y = (-m10 * v1y + m11 * v1x) / s2;
    if u1x * u2y - u1y * u2x < 0.0 {
        u2x = -u2x;
        u2y = -u2y;
    }
    let beta = u1y.atan2(u1x);
    (s1, s2, beta, alpha)
}

/// The affine transform mapping the `width` x `height` rect onto `quad` in
/// the same space, as rotate/scale/rotate factors the frame can apply.
/// `None` when the quad is axis-aligned (keep the exact rounded-rect draw
/// path), degenerate, or mirrored.
fn quad_transform(quad: [[f32; 2]; 4], width: f32, height: f32) -> Option<QuadTransform> {
    let (m00, m01, m10, m11) = fit_affine(quad, width, height)?;
    if m00 * m11 - m01 * m10 <= 0.0 {
        return None;
    }
    if (m00 - 1.0).abs() < 1e-3 && (m11 - 1.0).abs() < 1e-3 && m01.abs() < 1e-3 && m10.abs() < 1e-3 {
        return None;
    }
    let (mut s1, mut s2, beta, alpha) = svd2(m00, m01, m10, m11);
    s1 = s1.max(0.01);
    s2 = s2.max(0.01);
    let stretch = 1.0005;
    Some(QuadTransform {
        angle1: -alpha,
        scale_x: s1 * stretch,
        scale_y: s2 / stretch,
        angle2: beta,
    })
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
    fn chord_at_is_full_at_center_and_zero_at_edges() {
        assert!((chord_at(50.0, 25.0, 25.0) - 100.0).abs() < 1e-3);
        assert!(chord_at(50.0, 25.0, 0.0) < 1e-3);
        assert!(chord_at(50.0, 25.0, 50.0) < 1e-3);
        // At quarter height the chord is sqrt(0.75) of the full width.
        let quarter = chord_at(50.0, 25.0, 12.5);
        assert!((quarter - 100.0 * 0.75f32.sqrt()).abs() < 1e-2);
    }

    #[test]
    fn circle_lines_follow_the_chords() {
        let bounds = Size::new(300.0, 150.0);
        let (size, lines) = fit_circle_metrics(
            "hello world this is a longer bubble line for manhwa",
            Font::DEFAULT,
            bounds,
        );
        assert!(lines.len() >= 2, "expected several lines, got {}", lines.len());
        assert!(size > 8.0, "size {size} too small for a big bubble");
        let line_height = size * LINE_HEIGHT;
        for line in &lines {
            let measured = measure_text(&line.content, Font::DEFAULT, size, f32::INFINITY).width;
            // The first token of a row may overflow its chord, everything
            // else must stay inside the ellipse.
            assert!(
                measured <= line.chord + 0.5 || !line.content.contains(' '),
                "line {:?} width {measured} exceeds chord {}",
                line.content,
                line.chord
            );
            assert!(line.y + line_height <= bounds.height + 0.5);
        }
    }

    #[test]
    fn circle_fit_shrinks_to_fit_small_bubble() {
        let text = "hello world this is a longer bubble line for manhwa";
        let big = fit_circle_metrics(text, Font::DEFAULT, Size::new(300.0, 150.0)).0;
        let small = fit_circle_metrics(text, Font::DEFAULT, Size::new(120.0, 60.0)).0;
        assert!(small < big, "small bubble must fit smaller text: {small} >= {big}");
    }

    #[test]
    fn circle_fit_is_cached_and_consistent() {
        let bounds = Size::new(200.0, 100.0);
        let text = "cached circle text goes here";
        let first = fit_circle_metrics(text, Font::DEFAULT, bounds);
        let second = fit_circle_metrics(text, Font::DEFAULT, bounds);
        assert_eq!(first.0, second.0);
        assert_eq!(first.1, second.1);
    }

    #[test]
    fn circle_wraps_unspaced_runs() {
        // A single unspaced run wider than the bubble must still lay out in
        // per-character lines instead of overflowing or failing.
        let bounds = Size::new(120.0, 120.0);
        let long = "aaaaaaaaaa".repeat(6);
        let (_, lines) = fit_circle_metrics(&long, Font::DEFAULT, bounds);
        assert!(!lines.is_empty());
        assert!(lines.iter().all(|line| !line.content.is_empty()));
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

    #[test]
    fn axis_aligned_quad_has_no_transform() {
        let quad = [[0.0, 0.0], [100.0, 0.0], [100.0, 50.0], [0.0, 50.0]];
        assert!(quad_transform(quad, 100.0, 50.0).is_none());

        let quad = [[10.0, 20.0], [110.0, 20.0], [110.0, 70.0], [10.0, 70.0]];
        assert!(quad_transform(quad, 100.0, 50.0).is_none());
    }

    #[test]
    fn svd2_reconstructs_the_matrix() {
        // A = R(beta) . S(1.6, 0.5) . R(-alpha) with alpha = -0.9, beta = 0.4
        let (s1, s2, beta, alpha) = (1.6f32, 0.5f32, 0.4f32, -0.9f32);
        let (ca, sa) = (alpha.cos(), alpha.sin());
        let (cb, sb) = (beta.cos(), beta.sin());
        let m00 = cb * s1 * ca + sb * s2 * sa;
        let m01 = -cb * s1 * sa + sb * s2 * ca;
        let m10 = -sb * s1 * ca + cb * s2 * sa;
        let m11 = sb * s1 * sa + cb * s2 * ca;

        let (got_s1, got_s2, got_beta, got_alpha) = svd2(m00, m01, m10, m11);
        assert!((got_s1 - s1).abs() < 1e-3, "s1: {got_s1} != {s1}");
        assert!((got_s2 - s2).abs() < 1e-3, "s2: {got_s2} != {s2}");
        // The eigenvector sign is free, so only the round trip
        // A = R(got_beta) . S . R(-got_alpha) is asserted.
        let (cga, sga) = (got_alpha.cos(), got_alpha.sin());
        let (cgb, sgb) = (got_beta.cos(), got_beta.sin());
        let got00 = cgb * got_s1 * cga + sgb * got_s2 * sga;
        let got01 = cgb * got_s1 * sga - sgb * got_s2 * cga;
        let got10 = sgb * got_s1 * cga - cgb * got_s2 * sga;
        let got11 = sgb * got_s1 * sga + cgb * got_s2 * cga;
        assert!((got00 - m00).abs() < 1e-3, "m00: {got00} != {m00}");
        assert!((got01 - m01).abs() < 1e-3, "m01: {got01} != {m01}");
        assert!((got10 - m10).abs() < 1e-3, "m10: {got10} != {m10}");
        assert!((got11 - m11).abs() < 1e-3, "m11: {got11} != {m11}");
    }

    #[test]
    fn transform_maps_box_corners_onto_the_skewed_quad() {
        // A skewed quad: top edge tilted, bottom edge straight. The app
        // fits the quad's AABB (the text box spans the AABB), so the rect
        // mirrors the box: width/height come from the bounds.
        let quad = [[0.0, 0.0], [200.0, 30.0], [180.0, 100.0], [-20.0, 70.0]];
        let [min_x, min_y, max_x, max_y] = quad_bounds(quad);
        let width = max_x - min_x;
        let height = max_y - min_y;
        let transform = quad_transform(quad, width, height).expect("skewed quad transforms");

        // Apply T(c) . R(angle2) . S . R(angle1) . T(-c) to the rect corners
        // and check they land on the quad corners (the transform composes
        // with pre-concatenation, same as the backends).
        let apply = |x: f32, y: f32| -> [f32; 2] {
            let [min_x, min_y, max_x, max_y] = quad_bounds(quad);
            let center = [(min_x + max_x) / 2.0, (min_y + max_y) / 2.0];
            let (mut lx, mut ly) = (x - center[0], y - center[1]);
            let (a1, a2) = (transform.angle1, transform.angle2);
            let (c1, s1) = (a1.cos(), a1.sin());
            (lx, ly) = (
                c1 * lx - s1 * ly,
                s1 * lx + c1 * ly,
            );
            (lx, ly) = (lx * transform.scale_x, ly * transform.scale_y);
            let (c2, s2) = (a2.cos(), a2.sin());
            (lx, ly) = (c2 * lx - s2 * ly, s2 * lx + c2 * ly);
            [lx + center[0], ly + center[1]]
        };

        let corners = [[min_x, min_y], [max_x, min_y], [max_x, max_y], [min_x, max_y]];
        for (mapped, expected) in corners.iter().zip(quad.iter()) {
            let got = apply(mapped[0], mapped[1]);
            assert!(
                (got[0] - expected[0]).abs() < 1.0 && (got[1] - expected[1]).abs() < 1.0,
                "mapped {mapped:?} -> {got:?}, expected {expected:?}"
            );
        }
    }

    #[test]
    fn mirrored_quad_falls_back() {
        // Self-crossing / mirroring quad must not produce a flipped text map.
        let quad = [[0.0, 0.0], [200.0, 0.0], [0.0, 100.0], [200.0, 100.0]];
        assert!(quad_transform(quad, 200.0, 100.0).is_none());
    }

    #[test]
    fn lerp_color_endpoints_and_midpoint() {
        let a = [0, 0, 0, 255];
        let b = [255, 255, 255, 255];
        assert_eq!(lerp_color(a, b, 0.0), Color::from_rgba8(0, 0, 0, 1.0));
        assert_eq!(lerp_color(a, b, 1.0), Color::from_rgba8(255, 255, 255, 1.0));
        assert_eq!(lerp_color(a, b, 0.5), Color::from_rgba8(128, 128, 128, 1.0));
        // Out-of-range t clamps.
        assert_eq!(lerp_color(a, b, -1.0), Color::from_rgba8(0, 0, 0, 1.0));
        assert_eq!(lerp_color(a, b, 2.0), Color::from_rgba8(255, 255, 255, 1.0));
    }

    #[test]
    fn gradient_t_at_box_corners_for_all_directions() {
        let box_rect = Rectangle::new(Point::new(10.0, 20.0), Size::new(100.0, 50.0));
        let tl = Point::new(10.0, 20.0);
        let tr = Point::new(110.0, 20.0);
        let bl = Point::new(10.0, 70.0);
        let br = Point::new(110.0, 70.0);

        let t = |dir, p| gradient_t(dir, box_rect, p);
        assert!((t(TextGradientDir::TopToBottom, tl) - 0.0).abs() < 1e-6);
        assert!((t(TextGradientDir::TopToBottom, bl) - 1.0).abs() < 1e-6);
        assert!((t(TextGradientDir::BottomToTop, tl) - 1.0).abs() < 1e-6);
        assert!((t(TextGradientDir::BottomToTop, bl) - 0.0).abs() < 1e-6);
        assert!((t(TextGradientDir::LeftToRight, tl) - 0.0).abs() < 1e-6);
        assert!((t(TextGradientDir::LeftToRight, tr) - 1.0).abs() < 1e-6);
        assert!((t(TextGradientDir::RightToLeft, tl) - 1.0).abs() < 1e-6);
        assert!((t(TextGradientDir::RightToLeft, tr) - 0.0).abs() < 1e-6);
        assert!((t(TextGradientDir::TopLeftToBottomRight, tl) - 0.0).abs() < 1e-6);
        assert!((t(TextGradientDir::TopLeftToBottomRight, br) - 1.0).abs() < 1e-6);
        assert!((t(TextGradientDir::BottomRightToTopLeft, br) - 0.0).abs() < 1e-6);
        assert!((t(TextGradientDir::BottomRightToTopLeft, tl) - 1.0).abs() < 1e-6);
        assert!((t(TextGradientDir::TopRightToBottomLeft, tr) - 0.0).abs() < 1e-6);
        assert!((t(TextGradientDir::TopRightToBottomLeft, bl) - 1.0).abs() < 1e-6);
        assert!((t(TextGradientDir::BottomLeftToTopRight, bl) - 0.0).abs() < 1e-6);
        assert!((t(TextGradientDir::BottomLeftToTopRight, tr) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn perspective_map_maps_box_corners_onto_quad_corners() {
        // A stretched trapezoid: top edge wider than the bottom and shifted
        // right. The homography must map the box's four corners onto the
        // quad's ordered TL/TR/BR/BL corners.
        let quad = [[100.0, 50.0], [500.0, 50.0], [420.0, 200.0], [180.0, 200.0]];
        let box_rect = Rectangle::new(Point::new(100.0, 50.0), Size::new(400.0, 150.0));
        let tl = perspective_map(quad, box_rect, Point::new(100.0, 50.0));
        let tr = perspective_map(quad, box_rect, Point::new(500.0, 50.0));
        let br = perspective_map(quad, box_rect, Point::new(500.0, 200.0));
        let bl = perspective_map(quad, box_rect, Point::new(100.0, 200.0));
        assert!((tl.x - 100.0).abs() < 1e-3 && (tl.y - 50.0).abs() < 1e-3);
        assert!((tr.x - 500.0).abs() < 1e-3 && (tr.y - 50.0).abs() < 1e-3);
        assert!((br.x - 420.0).abs() < 1e-3 && (br.y - 200.0).abs() < 1e-3);
        assert!((bl.x - 180.0).abs() < 1e-3 && (bl.y - 200.0).abs() < 1e-3);
    }

    #[test]
    fn perspective_map_shares_one_vanishing_point_between_lines() {
        // A keystone quad: its top and bottom edges are not parallel, so a
        // bilinear field would fan the lines between the two edge angles
        // (top line slants like the top edge, last line like the bottom
        // edge). The projective map must make every horizontal line's image
        // pass through the vanishing point of the top and bottom edges.
        let quad = [[100.0, 50.0], [500.0, 50.0], [460.0, 230.0], [140.0, 210.0]];
        let box_rect = Rectangle::new(Point::new(100.0, 50.0), Size::new(400.0, 180.0));
        // The top edge is y = 50; the bottom edge goes through (140, 210)
        // with direction (320, 20), so it meets y = 50 at x = -2420.
        let vp = Point::new(-2420.0, 50.0);
        for v in [0.25, 0.5, 0.75] {
            let y = box_rect.y + v * box_rect.height;
            let a = perspective_map(
                quad,
                box_rect,
                Point::new(box_rect.x + 0.2 * box_rect.width, y),
            );
            let b = perspective_map(
                quad,
                box_rect,
                Point::new(box_rect.x + 0.8 * box_rect.width, y),
            );
            let area = (b.x - a.x) * (vp.y - a.y) - (b.y - a.y) * (vp.x - a.x);
            assert!(
                area.abs() < 1.0,
                "line image at v={v} misses the vanishing point (area {area})"
            );
        }
    }

    #[test]
    fn rotated_rect_geometry_detects_rotated_boxes_and_rejects_shear() {
        // A 100x30 box spun 0.5 rad: true edge lengths, exact angle.
        let rotated = [[0.0, 0.0], [87.758256, 47.942554], [73.37549, 74.27003], [-14.382766, 26.327477]];
        let (tl, w, h, angle) = rotated_rect_geometry(rotated).unwrap();
        assert_eq!((tl.x, tl.y), (0.0, 0.0));
        assert!((w - 100.0).abs() < 1e-3 && (h - 30.0).abs() < 1e-3);
        assert!((angle - 0.5).abs() < 1e-3);

        // An axis-aligned box: geometry reported, upright angle.
        let upright = [[0.0, 0.0], [100.0, 0.0], [100.0, 30.0], [0.0, 30.0]];
        let (_, w, h, angle) = rotated_rect_geometry(upright).unwrap();
        assert!((w - 100.0).abs() < 1e-3 && (h - 30.0).abs() < 1e-3);
        assert!(angle.abs() < 1e-3);

        // A sheared parallelogram: not a rectangle, must be rejected.
        let sheared = [[0.0, 0.0], [100.0, 0.0], [90.0, 30.0], [10.0, 30.0]];
        assert!(rotated_rect_geometry(sheared).is_none());
    }

    #[test]
    fn affine_error_is_zero_for_parallelogram_and_large_for_trapezoid() {
        // A parallelogram: the affine fit is exact.
        let parallelogram = [[100.0, 50.0], [500.0, 50.0], [440.0, 200.0], [40.0, 200.0]];
        let [min_x, min_y, max_x, max_y] = quad_bounds(parallelogram);
        let width = max_x - min_x;
        let height = max_y - min_y;
        assert!(
            affine_error(parallelogram, width, height) < 0.01,
            "parallelogram must fit exactly"
        );
        // The design doc's stretched trapezoid: tens of pixels of deviation.
        let trapezoid = [[302.75, 257.02], [785.25, 257.02], [815.2, 376.0], [302.75, 313.79]];
        let [min_x, min_y, max_x, max_y] = quad_bounds(trapezoid);
        let width = max_x - min_x;
        let height = max_y - min_y;
        let error = affine_error(trapezoid, width, height);
        assert!(error > 5.0, "trapezoid must deviate, got {error}");
    }

    #[test]
    fn warp_transform_round_trips_glyph_rect() {
        // Map a small glyph rect through a trapezoid's projective field, fit
        // a per-glyph affine and check that applying it lands the rect
        // corners back on the mapped corners (the "close enough" guarantee).
        let quad = [[100.0, 50.0], [500.0, 50.0], [420.0, 200.0], [180.0, 200.0]];
        let box_rect = Rectangle::new(Point::new(100.0, 50.0), Size::new(400.0, 150.0));
        // A small glyph: the affine error scales with glyph size, so a
        // realistic glyph stays well under a pixel even in a strongly
        // tapered quad.
        let rect = [224.0, 92.0, 8.0, 9.0];
        let corners: [[f32; 2]; 4] = [
            perspective_map(quad, box_rect, Point::new(rect[0], rect[1])),
            perspective_map(quad, box_rect, Point::new(rect[0] + rect[2], rect[1])),
            perspective_map(quad, box_rect, Point::new(rect[0] + rect[2], rect[1] + rect[3])),
            perspective_map(quad, box_rect, Point::new(rect[0], rect[1] + rect[3])),
        ]
        .map(|p| [p.x, p.y]);
        let (m00, m01, m10, m11) = fit_affine(corners, rect[2], rect[3]).expect("fit");
        let [min_x, min_y, max_x, max_y] = quad_bounds(corners);
        let quad_center = [(min_x + max_x) / 2.0, (min_y + max_y) / 2.0];
        let rect_center = [rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0];
        let apply = |x: f32, y: f32| -> [f32; 2] {
            let (lx, ly) = (x - rect_center[0], y - rect_center[1]);
            [
                quad_center[0] + m00 * lx + m01 * ly,
                quad_center[1] + m10 * lx + m11 * ly,
            ]
        };
        let local = [
            [rect[0], rect[1]],
            [rect[0] + rect[2], rect[1]],
            [rect[0] + rect[2], rect[1] + rect[3]],
            [rect[0], rect[1] + rect[3]],
        ];
        for (mapped, expected) in corners.iter().zip(local.iter()) {
            let got = apply(expected[0], expected[1]);
            assert!(
                (got[0] - mapped[0]).abs() < 0.5 && (got[1] - mapped[1]).abs() < 0.5,
                "glyph corner {expected:?} -> {got:?}, expected {mapped:?}"
            );
        }
    }

    #[test]
    fn warp_layout_shapes_glyphs() {
        let layout = shape_warp_layout("hello world", Font::DEFAULT, 20.0, 200.0);
        assert!(
            layout.glyphs.len() >= 6,
            "expected glyphs, got {}",
            layout.glyphs.len()
        );
        assert!(layout.min_width > 50.0, "min_width {}", layout.min_width);
        let empty = shape_warp_layout("", Font::DEFAULT, 20.0, 200.0);
        assert!(empty.glyphs.is_empty());
    }

    #[test]
    fn warp_layout_glyph_rects_stay_inside_the_paragraph() {
        // The per-glyph rect must be the glyph's ink box. A rect anchored at
        // the baseline and sized by the font size hangs below the paragraph
        // for the last line (and clamps the projective map to the box's
        // bottom edge), so every rect must fit inside the measured paragraph
        // height.
        let text = "hello world this wraps into several lines";
        let size = 20.0;
        let wrap_width = 120.0;
        let fitted = measure_text(text, Font::DEFAULT, size, wrap_width);
        let layout = shape_warp_layout(text, Font::DEFAULT, size, wrap_width);
        assert!(
            layout.glyphs.len() >= 2,
            "expected several lines of glyphs, got {}",
            layout.glyphs.len()
        );
        for glyph in &layout.glyphs {
            let [gx, gy, gw, gh] = glyph.rect;
            assert!(gx >= -1.0, "glyph rect left {gx} out of bounds");
            assert!(gy >= -1.0, "glyph rect top {gy} out of bounds");
            assert!(
                gy + gh <= fitted.height + 1.0,
                "glyph rect bottom {} exceeds paragraph height {}",
                gy + gh,
                fitted.height
            );
        }
    }
}
