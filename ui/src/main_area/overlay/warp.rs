use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};

use iced::advanced::graphics::geometry::{self, Fill, Path, Stroke, Text};
use iced::advanced::text::{LineHeight, Paragraph as _, Wrapping};
use iced::advanced::text::Text as ParagraphText;
use iced::{alignment, Color, Font, Pixels, Point, Rectangle, Size, Vector};
use iced::advanced::text::Alignment as TextAlignment;

use scanlateit_model::TextGradientDir;

use super::cache::{FitKey, FIT_CACHE_CAP, font_hash, fnv1a};
use super::gradient::{gradient_t, lerp_color};
use crate::main_area::geometry::{fit_affine, quad_bounds, svd2};

const WARP_THRESHOLD_PX: f32 = 0.5;

#[derive(Clone)]
pub(crate) struct WarpGlyph {
    pub(crate) rect: [f32; 4],
    pub(crate) path: Path,
}

#[derive(Clone)]
pub(crate) struct WarpLayout {
    pub(crate) glyphs: Vec<WarpGlyph>,
    pub(crate) min_width: f32,
}

struct WarpCacheEntry {
    content: String,
    layout: WarpLayout,
}

struct WarpCache {
    entries: HashMap<FitKey, WarpCacheEntry>,
    order: VecDeque<FitKey>,
}

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

pub(crate) fn shape_warp_layout(text: &str, font: Font, size: f32, wrap_width: f32) -> WarpLayout {
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
        shaping: iced::advanced::text::Shaping::Auto,
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

pub fn perspective_map(quad: [[f32; 2]; 4], box_rect: Rectangle, p: Point) -> Point {
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

pub fn affine_error(quad: [[f32; 2]; 4], width: f32, height: f32) -> f32 {
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

pub fn warp_threshold() -> f32 {
    WARP_THRESHOLD_PX
}

pub fn draw_warped_text<F>(
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
