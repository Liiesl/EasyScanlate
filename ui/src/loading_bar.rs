//! Void → Cerulean loading bar.
//! Matches the HTML/CSS specification:
//! - Track: 4px transparent, rounded pill caps
//! - Bar: linear gradient #111827 (Void) → #0ea5e9 (Cerulean) → #111827 (Void)
//! - background-size: 200% 100%
//! - Animations:
//!     - snap-asym: 3s ease-in-out infinite (fast stretch 15%, slow snap 35%)
//!     - pan-grad: 2s linear infinite
//! - Optional label: uppercase text in Slate-400 (#94a3b8)

use iced::alignment::Horizontal;
use iced::widget::canvas::{self, Cache, Frame, Geometry, Path};
use iced::widget::{column, text};
use iced::{Color, Element, Length, Point, Rectangle, Size, Theme, mouse};

use crate::event::UiEvent;
use crate::scale;

// Colors matching the HTML/CSS snippet
const VOID: Color = Color::from_rgb8(17, 24, 39);          // #111827
const CERULEAN: Color = Color::from_rgb8(14, 165, 233);    // #0ea5e9
const LABEL_COLOR: Color = Color::from_rgb8(148, 163, 184); // #94a3b8 (Slate-400)

// --- Perceptual sRGB Color Blending (Fixes GPU linear-gamma dark falloff) ---
#[inline]
fn lerp_f32(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Blends between VOID (0.0) and CERULEAN (1.0) with sRGB perceptual gamma correction.
fn blend_void_cerulean(t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    // Apply gamma curve to maintain vibrant cerulean body matching CSS browser interpolation
    let t_adj = t.powf(0.65);
    Color::from_rgb(
        lerp_f32(VOID.r, CERULEAN.r, t_adj),
        lerp_f32(VOID.g, CERULEAN.g, t_adj),
        lerp_f32(VOID.b, CERULEAN.b, t_adj),
    )
}

// --- Cubic Bezier helper for CSS ease-in-out: cubic-bezier(0.42, 0, 0.58, 1) ---
fn bezier_x(p1x: f32, p2x: f32, t: f32) -> f32 {
    let o = 1.0 - t;
    3.0 * o * o * t * p1x + 3.0 * o * t * t * p2x + t * t * t
}

fn bezier_dx(p1x: f32, p2x: f32, t: f32) -> f32 {
    let o = 1.0 - t;
    3.0 * o * (1.0 - 3.0 * t) * p1x + 3.0 * t * (2.0 - 3.0 * t) * p2x + 3.0 * t * t
}

fn bezier_y(p1y: f32, p2y: f32, t: f32) -> f32 {
    let o = 1.0 - t;
    3.0 * o * o * t * p1y + 3.0 * o * t * t * p2y + t * t * t
}

fn cubic_bezier(p1x: f32, p1y: f32, p2x: f32, p2y: f32, time: f32) -> f32 {
    let mut t = time;
    for _ in 0..8 {
        let x = bezier_x(p1x, p2x, t) - time;
        let dx = bezier_dx(p1x, p2x, t);
        if dx.abs() < 0.0001 {
            break;
        }
        t = (t - x / dx).clamp(0.0, 1.0);
    }
    bezier_y(p1y, p2y, t)
}

#[inline]
fn ease_in_out(t: f32) -> f32 {
    cubic_bezier(0.42, 0.0, 0.58, 1.0, t)
}

/// snap-asym 3s keyframes → (left, right) fractions 0..1
fn snap_asym(phase: f32) -> (f32, f32) {
    let t = phase.rem_euclid(3.0);

    if t < 0.45 {
        // 0% -> 15% (0.0s -> 0.45s): Left anchored, right snaps open to 100%
        let seg = t / 0.45;
        let e = ease_in_out(seg);
        let left = 0.0;
        let right = lerp_f32(0.95, 0.0, e);
        (left, right)
    } else if t < 1.50 {
        // 15% -> 50% (0.45s -> 1.50s): Right anchored, left catches up (slow snap)
        let seg = (t - 0.45) / 1.05;
        let e = ease_in_out(seg);
        let left = lerp_f32(0.0, 0.95, e);
        let right = 0.0;
        (left, right)
    } else if t < 1.95 {
        // 50% -> 65% (1.50s -> 1.95s): Left anchored, right expands back across
        let seg = (t - 1.50) / 0.45;
        let e = ease_in_out(seg);
        let left = lerp_f32(0.95, 0.0, e);
        let right = 0.0;
        (left, right)
    } else {
        // 65% -> 100% (1.95s -> 3.00s): Left anchored, right contracts back to 5%
        let seg = (t - 1.95) / 1.05;
        let e = ease_in_out(seg);
        let left = 0.0;
        let right = lerp_f32(0.0, 0.95, e);
        (left, right)
    }
}

#[derive(Debug, Clone)]
pub struct LoadingBar {
    phase: f32,
    label: Option<String>,
}

impl LoadingBar {
    pub fn new(phase: f32) -> Self {
        Self {
            phase,
            label: None,
        }
    }

    /// Attach an optional label like "LOADING" above the bar.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn view(self) -> Element<'static, UiEvent> {
        let h = scale::s(4.0);
        let bar_canvas = iced::widget::canvas(LoadingBarCanvas { phase: self.phase })
            .width(Length::Fill)
            .height(Length::Fixed(h));

        if let Some(label_text) = self.label {
            let label = text(label_text.to_uppercase())
                .size(scale::s(13.6)) // ~0.85rem
                .color(LABEL_COLOR)
                .align_x(Horizontal::Center)
                .width(Length::Fill);

            column![label, bar_canvas]
                .spacing(scale::s(12.0)) // margin-bottom: 12px
                .width(Length::Fill)
                .into()
        } else {
            bar_canvas.into()
        }
    }
}

struct LoadingBarCanvas {
    phase: f32,
}

impl canvas::Program<UiEvent> for LoadingBarCanvas {
    type State = Cache;

    fn draw(
        &self,
        _cache: &Self::State,
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        let w = bounds.width;
        let h = bounds.height;
        if w < 1.0 || h < 1.0 {
            return vec![frame.into_geometry()];
        }

        // 1. Calculate bar position and width
        let (left_frac, right_frac) = snap_asym(self.phase);
        let left_px = left_frac * w;
        let right_px = right_frac * w;
        let bar_w = (w - left_px - right_px).max(0.0);
        let bar_x = left_px;

        if bar_w < 0.5 {
            return vec![frame.into_geometry()];
        }

        // 2. Continuous gradient panning (pan-grad 2s linear)
        // In CSS: background-size: 200% 100%, period P = 2.0 * bar_w
        let period = 2.0 * bar_w;
        let pan_norm = (self.phase.rem_euclid(2.0)) / 2.0; // 0..1 linear
        let shift = pan_norm * period;

        // 3-period linear gradient spanning [bar_x + shift - period, bar_x + shift + 2*period]
        let grad_start = Point::new(bar_x + shift - period, 0.0);
        let grad_end = Point::new(bar_x + shift + 2.0 * period, 0.0);

        let mid_glow = blend_void_cerulean(0.70);
        let outer_glow = blend_void_cerulean(0.35);

        let gradient = {
            use iced::widget::canvas::Gradient;
            use iced::widget::canvas::gradient::Linear as GLinear;

            let mut linear = GLinear::new(grad_start, grad_end);

            // Add packed, multi-stop wave cycles so Cerulean stays vibrant without wide black dips
            for p in 0..3 {
                let base = p as f32 / 3.0;
                let step = 1.0 / 3.0;

                linear = linear
                    .add_stop(base + step * 0.00, VOID)
                    .add_stop(base + step * 0.20, outer_glow)
                    .add_stop(base + step * 0.35, mid_glow)
                    .add_stop(base + step * 0.50, CERULEAN)
                    .add_stop(base + step * 0.65, mid_glow)
                    .add_stop(base + step * 0.80, outer_glow)
                    .add_stop(base + step * 1.00, VOID);
            }

            Gradient::Linear(linear)
        };

        // 3. Draw rounded pill bar
        let radius = (h * 0.5).min(bar_w * 0.5);
        let path = Path::rounded_rectangle(
            Point::new(bar_x, 0.0),
            Size::new(bar_w, h),
            radius.into(),
        );

        frame.fill(&path, gradient);

        vec![frame.into_geometry()]
    }
}