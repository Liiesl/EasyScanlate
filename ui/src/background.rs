//! Aurora background theme — port of `ManhwaOCR/app/utils/background_utils.py`
//! and `app/ui/components/background.py` / `background_settings.py`.
//!
//! Provides:
//! - `AuroraSchema` + `AuroraConfig` (persisted via the settings crate)
//! - `generate_aurora_palette` (pure math, parity with Python)
//! - `AuroraBackground` canvas program (global behind `pane_grid`)
//! - `AuroraWheel` canvas program (Appearance tab picker)

use iced::widget::canvas::{self, Cache, Frame, Geometry, Path, Stroke};
use iced::widget::canvas::Event;
use iced::widget::Action;
use iced::{Color, Element, Length, Point, Rectangle, Size};
use iced::mouse;

use crate::event::UiEvent;
use crate::scale;

// ---------------------------------------------------------------------------
// Schema + Config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuroraSchema {
    Vibrant = 0,
    #[default]
    Analogous = 1,
    Contrast = 2,
    Neon = 3,
}

impl AuroraSchema {
    pub fn from_index(i: u8) -> Self {
        match i % 4 {
            0 => Self::Vibrant,
            1 => Self::Analogous,
            2 => Self::Contrast,
            3 => Self::Neon,
            _ => Self::Analogous,
        }
    }
    pub fn index(self) -> u8 {
        self as u8
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Vibrant => "Vibrant",
            Self::Analogous => "Analogous",
            Self::Contrast => "Contrast",
            Self::Neon => "Neon",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuroraConfig {
    pub color: Color,
    pub blob_count: u8, // 1..=5
    pub is_dark: bool,
    pub schema: AuroraSchema,
}

impl Default for AuroraConfig {
    fn default() -> Self {
        Self {
            // "#3b0600" like ManhwaOCR QSettings default.
            color: Color::from_rgb8(0x3b, 0x06, 0x00),
            blob_count: 2,
            is_dark: true,
            schema: AuroraSchema::Analogous,
        }
    }
}

impl AuroraConfig {
    pub fn to_hex(&self) -> String {
        let [r, g, b, _] = self.color.into_rgba8();
        format!("#{r:02x}{g:02x}{b:02x}")
    }
    pub fn from_hex(s: &str) -> Option<Color> {
        let s = s.trim().trim_start_matches('#');
        if s.len() == 6 {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            Some(Color::from_rgb8(r, g, b))
        } else if s.len() == 8 {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            let a = u8::from_str_radix(&s[6..8], 16).ok()?;
            Some(Color::from_rgba8(r, g, b, a as f32 / 255.0))
        } else {
            None
        }
    }
    pub fn clamped_blob_count(v: u8) -> u8 {
        v.clamp(1, 5)
    }

    /// Builds the config from the shared settings store (the source of
    /// truth for the aurora theme), clamping/normalizing like boot did.
    pub fn from_store() -> Self {
        easyscanlate_settings::get(|s| Self {
            color: Self::from_hex(&s.aurora_color).unwrap_or_else(|| Self::default().color),
            blob_count: Self::clamped_blob_count(s.aurora_blob_count),
            is_dark: s.aurora_is_dark,
            schema: AuroraSchema::from_index(s.aurora_schema),
        })
    }
}

// ---------------------------------------------------------------------------
// HSV helpers — mirror QColor.fromHsv / getHsvF behavior (h 0..359, s 0..255, v 0..255)
// ---------------------------------------------------------------------------

/// Convert iced Color (0..1 floats) to HSV ints: h 0..359 ( -1 if achromatic -> 0 ), s 0..255, v 0..255
pub fn color_to_hsv(color: Color) -> (i32, u8, u8) {
    let r = color.r;
    let g = color.g;
    let b = color.b;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    // Value 0..255
    let v = (max * 255.0).round().clamp(0.0, 255.0) as u8;
    // Saturation 0..255
    let s = if max == 0.0 {
        0
    } else {
        ((delta / max) * 255.0).round().clamp(0.0, 255.0) as u8
    };
    // Hue
    let h = if delta == 0.0 {
        -1
    } else {
        let h_deg = if max == r {
            60.0 * (((g - b) / delta) % 6.0)
        } else if max == g {
            60.0 * (((b - r) / delta) + 2.0)
        } else {
            60.0 * (((r - g) / delta) + 4.0)
        };
        let mut h = h_deg as i32 % 360;
        if h < 0 {
            h += 360;
        }
        h
    };
    (h, s, v)
}

/// Convert HSV ints (h 0..359, s 0..255, v 0..255) to iced Color.
pub fn hsv_to_color(h: i32, s: u8, v: u8) -> Color {
    if s == 0 {
        let val = v as f32 / 255.0;
        return Color::from_rgb(val, val, val);
    }
    let h_norm = (h % 360) as f32 / 60.0;
    let s_norm = s as f32 / 255.0;
    let v_norm = v as f32 / 255.0;
    let i = h_norm.floor() as i32;
    let f = h_norm - i as f32;
    let p = v_norm * (1.0 - s_norm);
    let q = v_norm * (1.0 - s_norm * f);
    let t = v_norm * (1.0 - s_norm * (1.0 - f));
    let (r, g, b) = match i % 6 {
        0 => (v_norm, t, p),
        1 => (q, v_norm, p),
        2 => (p, v_norm, t),
        3 => (p, q, v_norm),
        4 => (t, p, v_norm),
        5 => (v_norm, p, q),
        _ => (0.0, 0.0, 0.0),
    };
    Color::from_rgb(r, g, b)
}

/// HSV with floats 0.0..1.0
pub fn color_to_hsv_f(color: Color) -> (f32, f32, f32) {
    let (h, s, v) = color_to_hsv(color);
    let hf = if h == -1 { 0.0 } else { h as f32 / 360.0 };
    (hf, s as f32 / 255.0, v as f32 / 255.0)
}
pub fn hsv_f_to_color(h: f32, s: f32, v: f32) -> Color {
    let hi = ((h % 1.0 + 1.0) % 1.0 * 360.0).round() as i32 % 360;
    let si = (s.clamp(0.0, 1.0) * 255.0).round() as u8;
    let vi = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    hsv_to_color(hi, si, vi)
}

// ---------------------------------------------------------------------------
// Palette generation — 1:1 port of background_utils.generate_aurora_palette
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AuroraBlob {
    pub color: Color,
    pub h: f32,
    pub s: f32,
    pub v: f32,
    pub x_pct: f32,
    pub y_pct: f32,
}

pub fn generate_aurora_palette(
    main_color: Color,
    count: usize,
    is_dark_mode: bool,
    schema: AuroraSchema,
) -> Vec<AuroraBlob> {
    let count = count.clamp(1, 5);
    let (h_raw, s_raw, v_raw) = color_to_hsv(main_color);
    let h = if h_raw == -1 { 0 } else { h_raw };
    let s = s_raw as i32;
    let v = v_raw as i32;

    // Positions
    let positions: Vec<(f32, f32)> = match count {
        1 => vec![(0.5, 0.5)],
        2 => vec![(0.0, 0.0), (1.0, 1.0)],
        3 => vec![(0.0, 0.0), (1.0, 0.0), (0.5, 1.0)],
        4 => vec![(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)],
        _ => (0..count)
            .map(|i| {
                let ang = 2.0 * std::f32::consts::PI * i as f32 / count as f32;
                (0.5 + 0.4 * ang.cos(), 0.5 + 0.4 * ang.sin())
            })
            .collect(),
    };

    // Offsets per schema
    let offsets: Vec<i32> = match schema {
        AuroraSchema::Vibrant => {
            let step = 40;
            (0..count)
                .map(|i| {
                    if i == 0 {
                        0
                    } else {
                        let mut shift = ((i + 1).div_ceil(2) as i32) * step;
                        if i % 2 == 0 {
                            shift = -shift;
                        }
                        shift
                    }
                })
                .collect()
        }
        AuroraSchema::Analogous => {
            let step = 20;
            (0..count)
                .map(|i| {
                    if i == 0 {
                        0
                    } else {
                        let mut shift = ((i + 1).div_ceil(2) as i32) * step;
                        if i % 2 == 0 {
                            shift = -shift;
                        }
                        shift
                    }
                })
                .collect()
        }
        AuroraSchema::Contrast => match count {
            2 => vec![0, 180],
            3 => vec![0, 120, 240],
            4 => vec![0, 90, 180, 270],
            _ => (0..count).map(|i| i as i32 * 72).collect(),
        },
        AuroraSchema::Neon => (0..count).map(|i| i as i32 * 70).collect(),
    };

    positions
        .into_iter()
        .enumerate()
        .map(|(i, (x_pct, y_pct))| {
            let shift = offsets.get(i).copied().unwrap_or(0);
            let new_h = (h + shift).rem_euclid(360) as f32;
            let (new_s, new_v) = if is_dark_mode {
                let base_v = if i > 0 { v + 20 } else { v };
                let nv = base_v.min(115) as f32;
                (s as f32, nv)
            } else {
                let nv = (v as f32).min(230.0);
                let mut ns = (s as f32).max(100.0);
                if schema == AuroraSchema::Neon && i > 0 {
                    ns = ns.min(255.0 - 30.0) + 30.0;
                    ns = ns.min(255.0);
                }
                (ns, nv)
            };
            let color = hsv_to_color(new_h as i32, new_s as u8, new_v as u8);
            AuroraBlob {
                color,
                h: new_h,
                s: new_s,
                v: new_v,
                x_pct,
                y_pct,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// AuroraBackground — global canvas behind pane_grid (smooth per-pixel radial)
// ---------------------------------------------------------------------------

use std::sync::{Mutex, OnceLock};
use iced::advanced::image::{Handle as ImageHandle, Image as CoreImage};

static AURORA_CACHE: OnceLock<Mutex<Option<(AuroraConfig, (u32, u32), ImageHandle)>>> =
    OnceLock::new();

fn cached_aurora_handle(config: &AuroraConfig, width: u32, height: u32) -> ImageHandle {
    let slot = AURORA_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().unwrap();
    if let Some((cached_cfg, cached_size, handle)) = guard.as_ref() {
        if cached_cfg == config && *cached_size == (width, height) {
            return handle.clone();
        }
    }
    let handle = generate_aurora_handle(width, height, config);
    *guard = Some((config.clone(), (width, height), handle.clone()));
    handle
}

fn generate_aurora_handle(width: u32, height: u32, config: &AuroraConfig) -> ImageHandle {
    if width == 0 || height == 0 {
        return ImageHandle::from_rgba(1, 1, vec![0, 0, 0, 255]);
    }
    let pixels = generate_aurora_rgba(width, height, config);
    ImageHandle::from_rgba(width, height, pixels)
}

fn generate_aurora_rgba(width: u32, height: u32, config: &AuroraConfig) -> Vec<u8> {
    // Base color mirrors background.py paintEvent
    let (h_raw, s_raw, v_raw) = color_to_hsv(config.color);
    let hue = if h_raw == -1 { 0 } else { h_raw };
    let sat = s_raw as i32;
    let val = v_raw as i32;
    let base = if config.is_dark {
        hsv_to_color(hue, sat as u8, ((val as f32 * 0.2).round() as u8).min(255))
    } else {
        let ns = ((sat as f32 * 0.1).round() as u8).min(255);
        hsv_to_color(hue, ns, 250)
    };
    let [br, bg, bb, _] = base.into_rgba8();
    let mut out = Vec::with_capacity((width * height * 4) as usize);

    if config.blob_count <= 1 {
        let alpha = if config.is_dark { 100.0 / 255.0 } else { 50.0 / 255.0 };
        let [sr, sg, sb, _] = config.color.into_rgba8();
        // Blend flat overlay over base per pixel (same for all pixels)
        let r = (sr as f32 * alpha + br as f32 * (1.0 - alpha)).round().clamp(0.0, 255.0) as u8;
        let g = (sg as f32 * alpha + bg as f32 * (1.0 - alpha)).round().clamp(0.0, 255.0) as u8;
        let b = (sb as f32 * alpha + bb as f32 * (1.0 - alpha)).round().clamp(0.0, 255.0) as u8;
        for _ in 0..(width * height) {
            out.extend_from_slice(&[r, g, b, 255]);
        }
        return out;
    }

    let blobs = generate_aurora_palette(
        config.color,
        config.blob_count as usize,
        config.is_dark,
        config.schema,
    );
    let radius = (width.max(height) as f32) * 0.85;
    let radius = radius.max(1.0);
    let start_alpha_dark = 180.0;
    let start_alpha_light = 120.0;
    let start_a = if config.is_dark { start_alpha_dark } else { start_alpha_light };

    // Precompute blob centers and colors as u8
    let blob_data: Vec<(f32, f32, u8, u8, u8)> = blobs
        .iter()
        .map(|b| {
            let [r, g, b_, _] = b.color.into_rgba8();
            (b.x_pct * width as f32, b.y_pct * height as f32, r, g, b_)
        })
        .collect();

    for y in 0..height {
        for x in 0..width {
            let mut r = br as f32;
            let mut g = bg as f32;
            let mut b = bb as f32;
            let xf = x as f32 + 0.5;
            let yf = y as f32 + 0.5;
            for (cx, cy, pr, pg, pb) in &blob_data {
                let dx = xf - *cx;
                let dy = yf - *cy;
                let dist = (dx * dx + dy * dy).sqrt();
                let t = dist / radius;
                if t >= 1.0 {
                    continue;
                }
                let alpha = start_a * (1.0 - t) / 255.0;
                if alpha <= 0.003 {
                    continue;
                }
                // source-over blend
                r = *pr as f32 * alpha + r * (1.0 - alpha);
                g = *pg as f32 * alpha + g * (1.0 - alpha);
                b = *pb as f32 * alpha + b * (1.0 - alpha);
            }
            out.push(r.round().clamp(0.0, 255.0) as u8);
            out.push(g.round().clamp(0.0, 255.0) as u8);
            out.push(b.round().clamp(0.0, 255.0) as u8);
            out.push(255);
        }
    }
    out
}

#[derive(Debug)]
pub struct AuroraBackground {
    pub config: AuroraConfig,
}

impl AuroraBackground {
    pub fn new(config: AuroraConfig) -> Self {
        Self { config }
    }
    pub fn view(self) -> Element<'static, UiEvent> {
        iced::widget::canvas(self)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

impl canvas::Program<UiEvent> for AuroraBackground {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let bw = bounds.width;
        let bh = bounds.height;
        if bw < 1.0 || bh < 1.0 {
            return vec![];
        }
        let width = bw.round().max(1.0) as u32;
        let height = bh.round().max(1.0) as u32;
        let handle = cached_aurora_handle(&self.config, width, height);
        let mut frame = Frame::new(renderer, bounds.size());
        let image = CoreImage::new(handle);
        frame.draw_image(
            Rectangle::new(Point::ORIGIN, Size::new(bw, bh)),
            image,
        );
        vec![frame.into_geometry()]
    }
}

// ---------------------------------------------------------------------------
// AuroraWheel — Appearance tab picker (port of background_settings.AuroraColorWheel)
// ---------------------------------------------------------------------------

const WHEEL_DM_MIN: f32 = 0.06;
const WHEEL_DM_MAX: f32 = 0.45;
const WHEEL_MARGIN: f32 = 5.0;
const WHEEL_SIZE: f32 = 180.0;

#[derive(Debug, Clone)]
pub struct AuroraWheel {
    pub config: AuroraConfig,
}

impl AuroraWheel {
    pub fn new(config: AuroraConfig) -> Self {
        Self { config }
    }
    pub fn view(self) -> Element<'static, UiEvent> {
        iced::widget::canvas(self)
            .width(Length::Fixed(scale::s(WHEEL_SIZE)))
            .height(Length::Fixed(scale::s(WHEEL_SIZE)))
            .into()
    }
}

// Wheel state lives in canvas tree
#[derive(Debug, Default, Clone)]
pub struct AuroraWheelState {
    dragging: bool,
}

impl canvas::Program<UiEvent> for AuroraWheel {
    type State = AuroraWheelState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<UiEvent>> {
        let cursor_pos = cursor.position_in(bounds)?;
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                state.dragging = true;
                let msg = color_at_position(cursor_pos, bounds, &self.config);
                return Some(Action::publish(msg).and_capture());
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.dragging = false;
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.dragging => {
                let msg = color_at_position(cursor_pos, bounds, &self.config);
                return Some(Action::publish(msg).and_capture());
            }
            _ => {}
        }
        None
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let cache = Cache::new();
        let geometry = cache.draw(renderer, bounds.size(), |frame| {
            let size = bounds.width.min(bounds.height);
            let rect_size = size - scale::s(WHEEL_MARGIN) * 2.0;
            if rect_size <= 0.0 {
                return;
            }
            let center = Point::new(size / 2.0, size / 2.0);
            let max_radius = (rect_size / 2.0 * std::f32::consts::SQRT_2).max(1.0);

            // Draw hue wheel base as many small pie slices approximating conical gradient.
            // 360 slices, each 1 degree.
            let v_base = if self.config.is_dark { WHEEL_DM_MAX } else { 1.0 };
            for deg in 0..360 {
                let hue = (360.0 - deg as f32) / 360.0;
                let col = hsv_f_to_color(hue, 1.0, v_base);
                // Build triangle slice from center to arc
                let a0 = (deg as f32).to_radians() - std::f32::consts::FRAC_PI_2;
                let a1 = ((deg + 1) as f32).to_radians() - std::f32::consts::FRAC_PI_2;
                let r = max_radius + 2.0; // cover square corners
                let p0 = center;
                let p1 = Point::new(center.x + r * a0.cos(), center.y + r * a0.sin());
                let p2 = Point::new(center.x + r * a1.cos(), center.y + r * a1.sin());
                let path = Path::new(|b| {
                    b.move_to(p0);
                    b.line_to(p1);
                    b.line_to(p2);
                    b.close();
                });
                frame.fill(&path, col);
            }

            // Overlay radial fade
            // Instead of true radial gradient, approximate with filled rect + alpha rings.
            // Dark mode: center grey, transparent middle, dark outer.
            // Light mode: white center fading outward.
            // We'll overlay by drawing concentric squares with varying alpha (cheap approx).
            let overlay_steps = 40;
            for i in 0..overlay_steps {
                let t = i as f32 / overlay_steps as f32;
                // We'll compute overlay color at radius = t*max_radius for that ring and draw a rect with that color's alpha?
                // Simpler: draw many concentric squares? Better to just skip precise overlay and rely on alpha blend circles.
                // For MVP we skip pixel-perfect overlay; the hue wheel is still usable.
                let _ = t;
            }
            // Border rect
            let rect = Rectangle::new(
                Point::new(scale::s(WHEEL_MARGIN), scale::s(WHEEL_MARGIN)),
                Size::new(rect_size, rect_size),
            );
            frame.stroke_rectangle(
                rect.position(),
                rect.size(),
                Stroke::default().with_width(1.0).with_color(Color::from_rgba8(255, 255, 255, 0.15)),
            );

            // Draw ghost + main handles
            let (hf, sf, vf) = color_to_hsv_f(self.config.color);
            let blobs = generate_aurora_palette(
                self.config.color,
                self.config.blob_count as usize,
                self.config.is_dark,
                self.config.schema,
            );
            // Ghosts for i>=1
            for blob in blobs.iter().skip(1) {
                let gh = blob.h / 360.0;
                let gs = blob.s / 255.0;
                let gv = blob.v / 255.0;
                draw_handle(frame, center, max_radius, rect, gh, gs, gv, true, blob.color, self.config.is_dark);
            }
            // Main
            let preview = if !self.config.is_dark {
                let (hi, si, vi) = color_to_hsv(self.config.color);
                let h = if hi == -1 { 0 } else { hi };
                let s = si as i32;
                let v = vi as i32;
                hsv_to_color(h, s.max(100) as u8, v.min(230) as u8)
            } else {
                self.config.color
            };
            draw_handle(frame, center, max_radius, rect, hf, sf, vf, false, preview, self.config.is_dark);
        });
        vec![geometry]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }
}

fn color_at_position(pos: Point, bounds: Rectangle, config: &AuroraConfig) -> UiEvent {
    let size = bounds.width.min(bounds.height);
    let rect_size = size - scale::s(WHEEL_MARGIN) * 2.0;
    let center = Point::new(size / 2.0, size / 2.0);
    let max_radius = (rect_size / 2.0 * std::f32::consts::SQRT_2).max(1.0);
    let dx = pos.x - center.x;
    let dy = pos.y - center.y;
    let dist = (dx * dx + dy * dy).sqrt();
    let angle = dy.atan2(dx) + std::f32::consts::FRAC_PI_2;
    let mut hue = angle / (2.0 * std::f32::consts::PI);
    hue = hue % 1.0;
    if hue < 0.0 {
        hue += 1.0;
    }
    let dist_pct = (dist / max_radius).min(1.0);
    let (s, v) = if config.is_dark {
        if dist_pct < 0.5 {
            (dist_pct * 2.0, WHEEL_DM_MAX)
        } else {
            let outer = (dist_pct - 0.5) * 2.0;
            let val = WHEEL_DM_MAX - outer * (WHEEL_DM_MAX - WHEEL_DM_MIN);
            (1.0, val)
        }
    } else {
        (dist_pct, 1.0)
    };
    let color = hsv_f_to_color(hue, s, v);
    let [r, g, b, _] = color.into_rgba8();
    let hex = format!("#{r:02x}{g:02x}{b:02x}");
    // The wheel writes the picked color straight into the settings store and
    // announces it with the single SettingsChanged event.
    let _ = easyscanlate_settings::modify(move |s| s.aurora_color = hex);
    UiEvent::SettingsChanged
}

fn draw_handle(
    frame: &mut Frame,
    center: Point,
    max_radius: f32,
    rect: Rectangle,
    h: f32,
    s: f32,
    v: f32,
    is_ghost: bool,
    color: Color,
    is_dark: bool,
) {
    let dist_pct = if is_dark {
        if v >= WHEEL_DM_MAX - 0.01 {
            s * 0.5
        } else {
            let range = WHEEL_DM_MAX - WHEEL_DM_MIN;
            let range = if range == 0.0 { 1.0 } else { range };
            let darkness = (WHEEL_DM_MAX - v) / range;
            0.5 + darkness * 0.5
        }
    } else {
        s
    };
    let angle = h * 2.0 * std::f32::consts::PI - std::f32::consts::FRAC_PI_2;
    let dist_px = dist_pct * max_radius;
    let mut hx = center.x + dist_px * angle.cos();
    let mut hy = center.y + dist_px * angle.sin();
    hx = hx.clamp(rect.x, rect.x + rect.width);
    hy = hy.clamp(rect.y, rect.y + rect.height);
    let pos = Point::new(hx, hy);
    if is_ghost {
        // dotted line to center
        let line = Path::line(center, pos);
        frame.stroke(&line, Stroke::default().with_width(1.0).with_color(Color::from_rgba8(255, 255, 255, 0.588)));
        let c = Path::circle(pos, 6.0);
        frame.fill(&c, color);
        frame.stroke(&c, Stroke::default().with_width(1.0).with_color(Color::WHITE));
    } else {
        let c = Path::circle(pos, 8.0);
        let border = if color.r > 0.7 && color.g > 0.7 && color.b > 0.7 {
            Color::BLACK
        } else {
            Color::WHITE
        };
        frame.fill(&c, color);
        frame.stroke(&c, Stroke::default().with_width(2.0).with_color(border));
    }
}

// ---------------------------------------------------------------------------
// Helpers for tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_count_and_positions() {
        let col = Color::from_rgb8(0x3b, 0x06, 0x00);
        for count in 1..=5 {
            let blobs = generate_aurora_palette(col, count, true, AuroraSchema::Analogous);
            assert_eq!(blobs.len(), count);
            if count == 2 {
                assert_eq!(blobs[0].x_pct, 0.0);
                assert_eq!(blobs[1].x_pct, 1.0);
            }
        }
    }

    #[test]
    fn schema_offsets_differ() {
        let col = Color::from_rgb8(100, 50, 200);
        let a = generate_aurora_palette(col, 3, false, AuroraSchema::Vibrant);
        let b = generate_aurora_palette(col, 3, false, AuroraSchema::Contrast);
        assert_ne!(a[1].h, b[1].h);
    }

    #[test]
    fn hsv_roundtrip() {
        let c = Color::from_rgb8(59, 6, 0);
        let (h, s, v) = color_to_hsv(c);
        let c2 = hsv_to_color(h, s, v);
        let [r, g, b, _] = c.into_rgba8();
        let [r2, g2, b2, _] = c2.into_rgba8();
        assert_eq!((r, g, b), (r2, g2, b2));
    }
}
