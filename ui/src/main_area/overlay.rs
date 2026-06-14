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
use iced::{alignment, Color, Font, Pixels, Point, Size, Vector};

use scanlateit_model::{EntryId, EntryStyle, Quad};

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
    /// True while the entry is being edited inline: only the box is drawn,
    /// the text is left to the floating text input on top.
    pub hide_text: bool,
}

/// Outline drawn around the selected entry.
const SELECTED_COLOR: Color = Color::from_rgba8(92, 190, 255, 1.0);
const SELECTED_WIDTH: f32 = 2.0;

/// Global manhwa-style rendering: overlay text is laid out in centered lines
/// that follow the curve of the entry's box (each line's width matches the
/// ellipse chord at its height), like text inside a manhwa speech bubble.
/// The box background itself is unchanged. Set to `false` to restore plain
/// rectangular wrapping.
pub(crate) const CIRCULAR_OVERLAYS: bool = true;

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

/// The base font with the entry's weight (bold) and style (italic) applied.
pub(crate) fn styled_font(font: Font, style: &EntryStyle) -> Font {
    Font {
        weight: if style.bold {
            FontWeight::Bold
        } else {
            FontWeight::Normal
        },
        style: if style.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
        ..font
    }
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

/// Splits `text` into atomic wrap units: whitespace-separated words, with any
/// word wider than `max_width` further split per character (so CJK runs
/// without spaces can still wrap).
fn circle_tokens(text: &str, font: Font, size: f32, max_width: f32) -> Vec<String> {
    let mut tokens = Vec::new();
    for word in text.split_whitespace() {
        if measure_text(word, font, size, f32::INFINITY).width <= max_width {
            tokens.push(word.to_string());
            continue;
        }
        let mut sub = String::new();
        let mut sub_width = 0.0;
        for ch in word.chars() {
            let char_width = measure_text(&ch.to_string(), font, size, f32::INFINITY).width;
            if !sub.is_empty() && sub_width + char_width > max_width {
                tokens.push(std::mem::take(&mut sub));
                sub_width = 0.0;
            }
            sub.push(ch);
            sub_width += char_width;
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
fn layout_circle_lines(
    text: &str,
    font: Font,
    size: f32,
    bounds: Size,
) -> Option<Vec<CircleLine>> {
    let rx = bounds.width / 2.0;
    let ry = bounds.height / 2.0;
    let line_height = size * LINE_HEIGHT;
    let tokens = circle_tokens(text, font, size, bounds.width);
    let widths: Vec<f32> = tokens
        .iter()
        .map(|token| measure_text(token, font, size, f32::INFINITY).width)
        .collect();
    let space_width = measure_text(" ", font, size, f32::INFINITY).width;

    let mut lines = Vec::new();
    let mut y = 0.0;
    let mut index = 0;
    while index < tokens.len() {
        let chord = chord_at(rx, ry, y + line_height / 2.0);
        if chord <= 0.0 {
            return None;
        }
        let mut content = String::new();
        let mut width = 0.0;
        // The first token of a row may overflow the chord (it can only be
        // narrower than the box); later tokens must fit.
        while index < tokens.len() {
            let add = widths[index] + if content.is_empty() { 0.0 } else { space_width };
            if !content.is_empty() && width + add > chord {
                break;
            }
            if !content.is_empty() {
                content.push(' ');
            }
            content.push_str(&tokens[index]);
            width += add;
            index += 1;
        }
        lines.push(CircleLine { content, y, chord });
        y += line_height;
        if y > bounds.height {
            return None;
        }
    }
    Some(lines)
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

/// Draws one translucent box + label per entry on top of the image inside
/// `frame`. Coordinates are image pixels, scaled to the frame's width. Each
/// label's font size is auto-sized to fill its bounding box (growing or
/// shrinking as needed). When `circular` is set, the text is wrapped
/// manhwa-style: one centered line per row, each line's width following the
/// ellipse chord at its height inside the box.
pub fn draw_entries<'a, I, F>(
    frame: &mut F,
    entries: I,
    font: Font,
    image_width: f32,
    circular: bool,
) where
    F: geometry::frame::Backend,
    I: IntoIterator<Item = &'a OverlayEntry<'a>>,
{
    let scale = frame.width() / image_width.max(1.0);
    for entry in entries {
        let [min_x, min_y, max_x, max_y] = entry.bounds;
        let width = (max_x - min_x).max(0.0) * scale;
        let height = (max_y - min_y).max(0.0) * scale;
        let position = Point::new(min_x * scale, min_y * scale);
        // The entry's quad scaled to viewport pixels. When it is not an
        // axis-aligned box (free transform), the box is the quad itself and
        // the text is mapped onto it by an affine transform, so the text
        // skews and rotates with the box.
        let quad = entry.quad.points.map(|p| [p[0] * scale, p[1] * scale]);
        let transform = quad_transform(quad, width, height);
        // The quad path is already in screen space at the box's real
        // corners: it must be drawn without the text transform, or the
        // polygon gets pushed through the rect->quad map a second time.
        let path = match transform {
            Some(_) => quad_path(quad),
            None => Path::rounded_rectangle(
                position,
                Size::new(width, height),
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
        let wrap_width = width.max(8.0);
        if entry.hide_text {
            continue;
        }
        let styled = styled_font(font, &entry.style);
        if circular {
            let (size, lines) =
                fit_circle_metrics(entry.text, styled, Size::new(wrap_width, height));
            let line_height = size * LINE_HEIGHT;
            let total_height = lines.last().map_or(0.0, |line| line.y + line_height);
            // Vertically center the whole block inside the ellipse.
            let y_offset = (height - total_height).max(0.0) / 2.0;
            if let Some(transform) = &transform {
                frame.push_transform();
                apply_quad_transform(frame, transform, position, width, height);
            }
            for line in &lines {
                // `align_x: Center` treats the position's x as the line's
                // center (not its left edge), so it must be the bubble's
                // horizontal center, not the box's left corner.
                let text = Text {
                    content: line.content.clone(),
                    position: Point::new(
                        position.x + wrap_width / 2.0,
                        position.y + y_offset + line.y,
                    ),
                    max_width: line.chord,
                    size: Pixels(size),
                    color: to_color(entry.style.text_color),
                    font: styled,
                    align_x: TextAlignment::Center,
                    ..Text::default()
                };
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
            if transform.is_some() {
                frame.pop_transform();
            }
            continue;
        }
        let (size, fitted_height) = fit_font_metrics(
            entry.text,
            styled,
            Size::new(wrap_width, height),
        );
        // Vertically center the wrapped text block inside the box.
        let y_offset = (height - fitted_height).max(0.0) / 2.0;
        let text = Text {
            content: entry.text.to_string(),
            position: Point::new(position.x, position.y + y_offset),
            max_width: wrap_width,
            size: Pixels(size),
            color: to_color(entry.style.text_color),
            font: styled,
            ..Text::default()
        };
        // Only the text lives in the axis-aligned rect space and needs the
        // map onto the skewed quad.
        if let Some(transform) = &transform {
            frame.push_transform();
            apply_quad_transform(frame, transform, position, width, height);
        }
        // DEBUG markers: green square = text position drawn WITHOUT the
        // transform (raw AABB coords); red squares = same local rect under
        // the transform (must land exactly on the quad's envelope).
        if entry.selected {
            frame.fill_rectangle(
                Point::new(text.position.x - 4.0, text.position.y - 4.0),
                Size::new(8.0, 8.0),
                Fill::from(Color::from_rgba8(0, 255, 0, 1.0)),
            );
            if transform.is_some() {
                frame.fill_rectangle(
                    Point::new(text.position.x - 4.0, text.position.y - 4.0),
                    Size::new(8.0, 8.0),
                    Fill::from(Color::from_rgba8(255, 0, 0, 1.0)),
                );
                frame.fill_rectangle(
                    Point::new(text.position.x + wrap_width - 4.0, text.position.y + fitted_height - 4.0),
                    Size::new(8.0, 8.0),
                    Fill::from(Color::from_rgba8(255, 0, 0, 1.0)),
                );
            }
        }
        if entry.style.stroke_width > 0.0 {
            frame.stroke_text(
                text.clone(),
                Stroke::default()
                    .with_color(to_color(entry.style.stroke_color))
                    .with_width(entry.style.stroke_width * scale),
            );
        }
        frame.fill_text(text);
        if transform.is_some() {
            frame.pop_transform();
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
    let [min_x, min_y, max_x, max_y] = quad_bounds(quad);
    let corners = [
        [min_x, min_y],
        [max_x, min_y],
        [max_x, max_y],
        [min_x, max_y],
    ];
    let mut used = [false; 4];
    let mut ordered = [[0.0; 2]; 4];
    for (corner_index, corner) in corners.iter().enumerate() {
        let mut best = None;
        for (index, point) in quad.iter().enumerate() {
            if used[index] {
                continue;
            }
            let dx = point[0] - corner[0];
            let dy = point[1] - corner[1];
            if best.is_none_or(|(_, best_d2)| dx * dx + dy * dy < best_d2) {
                best = Some((index, dx * dx + dy * dy));
            }
        }
        let (index, _) = best.expect("quad has four points");
        used[index] = true;
        ordered[corner_index] = quad[index];
    }
    ordered
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
    let [min_x, min_y, max_x, max_y] = quad_bounds(quad);
    let center = ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
    // Rect corners relative to the rect center: (-hw,-hh), (hw,-hh), (hw,hh),
    // (-hw,hh). Because they are symmetric, the normal matrix is diagonal:
    // sum(lx*lx) = width^2 and sum(ly*ly) = height^2, so the least-squares
    // solution is the average of the corner ratios along each axis.
    let lx = [-half_w, half_w, half_w, -half_w];
    let ly = [-half_h, -half_h, half_h, half_h];
    let mut m00 = 0.0;
    let mut m10 = 0.0;
    let mut m01 = 0.0;
    let mut m11 = 0.0;
    for index in 0..4 {
        let (qx, qy) = (quad[index][0] - center.0, quad[index][1] - center.1);
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
    let ordered = order_quad(quad);
    let [min_x, min_y, max_x, max_y] = quad_bounds(quad);
    let corners = [[min_x, min_y], [max_x, min_y], [max_x, max_y], [min_x, max_y]];
    let axis_aligned = ordered
        .iter()
        .zip(corners.iter())
        .all(|(point, corner)| (point[0] - corner[0]).abs() < 0.5 && (point[1] - corner[1]).abs() < 0.5);
    if axis_aligned {
        return None;
    }
    let (m00, m01, m10, m11) = fit_affine(ordered, width, height)?;
    // Mirroring maps (det <= 0) would flip the text; the rounded-rect path
    // with axis-aligned text is the safer fallback.
    if m00 * m11 - m01 * m10 <= 0.0 {
        return None;
    }
    let (mut s1, mut s2, beta, alpha) = svd2(m00, m01, m10, m11);
    s1 = s1.max(0.01);
    s2 = s2.max(0.01);
    // Text backends only rasterize glyphs through the transform when it is
    // not a pure scale+translation: a pure rotation (s1 == s2) would render
    // glyphs axis-aligned at a rotated position. The 0.05% anisotropy is
    // invisible and forces the transform-aware glyph path.
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
        assert!((got_alpha - alpha).abs() < 1e-3, "alpha: {got_alpha} != {alpha}");
        assert!((got_beta - beta).abs() < 1e-3, "beta: {got_beta} != {beta}");
    }

    #[test]
    fn transform_maps_box_corners_onto_the_skewed_quad() {
        // A skewed quad: top edge tilted, bottom edge straight.
        let quad = [[0.0, 0.0], [200.0, 30.0], [180.0, 100.0], [-20.0, 70.0]];
        let width = 200.0;
        let height = 100.0;
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

        let corners = [[0.0, 0.0], [width, 0.0], [width, height], [0.0, height]];
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
}
