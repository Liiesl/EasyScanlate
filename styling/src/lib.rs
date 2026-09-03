//! Text style classification with the ONNX styling model
//! (`text_styling_model.onnx`). CPU-only — the model uses ops incompatible
//! with the DirectML execution provider. Given a `160x64` RGB crop of a text
//! region (from the ONNX Text Styling Classification project), the model
//! predicts five binary style flags, the background type, and several colors
//! in a single forward pass.
//!
//! [`Engine`] holds one shared inference session (CPU-only),
//! mirroring the inpainting crate's pattern. Callers pass an image
//! path plus an entry's [`Quad`]; the quad's bounding box is squished to the
//! model's fixed `160x64` input (the model was trained on stretched crops, so
//! no aspect-ratio letterboxing), normalized, and classified.

use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use image::RgbImage;
use ndarray::{Array4, ArrayD};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::TensorRef;
use scanlateit_model::{EntryStyle, Quad};

pub mod tracker;

pub use tracker::JobTracker;

/// The fixed input size of the styling model: width x height.
pub const MODEL_WIDTH: u32 = 160;
pub const MODEL_HEIGHT: u32 = 64;

const MODEL_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../models");
const MODEL_FILE: &str = "text_styling_model.onnx";

/// ImageNet normalization constants the model was trained with.
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

/// The type of background the model classified a crop as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BgType {
    Solid,
    Gradient,
    Artwork,
}

impl BgType {
    fn from_index(index: usize) -> Self {
        match index {
            0 => BgType::Solid,
            1 => BgType::Gradient,
            _ => BgType::Artwork,
        }
    }
}

/// The styling predictions of one crop. Colors are RGB in 0..255. Shadow and
/// glow are detected but have no [`EntryStyle`] field yet, so they are kept
/// here for callers without being persisted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StylePrediction {
    pub bold: bool,
    pub italic: bool,
    pub stroke: bool,
    pub shadow: bool,
    pub glow: bool,
    pub bg_type: BgType,
    pub text_color: [u8; 3],
    pub effect_color: [u8; 3],
    /// Meaningful only for a solid background.
    pub bg_color: [u8; 3],
    /// Meaningful only for a gradient background (start color).
    pub bg_color_a: [u8; 3],
    /// Meaningful only for a gradient background (end color).
    pub bg_color_b: [u8; 3],
    /// `[sin(θ), cos(θ)]` of the gradient direction; meaningful only for a
    /// gradient background.
    pub bg_direction: [f32; 2],
}

/// Cloneable handle to the shared styling session. Only one inference runs at
/// a time; runs are serialized through the inner mutex.
#[derive(Clone)]
pub struct Engine(Arc<Mutex<Session>>);

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Engine")
    }
}

impl Engine {
    /// Loads the styling model (CPU-only; incompatible with DirectML).
    pub fn build() -> Result<Self, String> {
        let path = scanlateit_settings::resolve_model_path(MODEL_FILE);
        let session = Session::builder()
            .map_err(|e| format!("ORT init failed: {e}"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| format!("ORT init failed: {e}"))?
            .with_intra_threads(4)
            .map_err(|e| format!("ORT init failed: {e}"))?
            .with_execution_providers([ort::ep::CPU::default().build()])
            .map_err(|e| format!("ORT init failed: {e}"))?
            .commit_from_file(&path)
            .map_err(|e| format!("failed to load styling model {}: {e}", path.display()))?;
        Ok(Self(Arc::new(Mutex::new(session))))
    }

    /// Decodes `path`, squishes `quad`'s bounding box to the model's `160x64`
    /// input, and classifies the crop. The model was trained on stretched
    /// crops, so no aspect-ratio padding is applied.
    pub fn predict_entry(
        &self,
        path: &str,
        quad: &Quad,
    ) -> Result<StylePrediction, String> {
        let image = image::ImageReader::open(path)
            .map_err(|e| format!("Failed to open {path}: {e}"))?
            .with_guessed_format()
            .map_err(|e| format!("Failed to decode {path}: {e}"))?
            .decode()
            .map_err(|e| format!("Failed to decode {path}: {e}"))?
            .to_rgb8();
        let mut session = self
            .0
            .lock()
            .map_err(|e| format!("Styling engine lock poisoned: {e}"))?;
        let crop = crop_quad(&image, quad);
        let resized = resize_to_input(&crop);
        let input = compose_input(&resized);
        let outputs = run_session(&mut session, input)?;
        Ok(parse_outputs(&outputs))
    }

    /// Decodes `path`, classifies the `quad` crop and maps the prediction
    /// onto an [`EntryStyle`] in one call (the app's auto-detect job).
    pub fn classify_entry(&self, path: &str, quad: &Quad) -> Result<EntryStyle, String> {
        self.predict_entry(path, quad)
            .map(|pred| pred.to_entry_style(EntryStyle::default()))
    }

    /// Like [`Self::classify_entry`] but also returns the raw [`StylePrediction`]
    /// so the caller can route `BgType`-aware post-processing (auto-inpaint).
    pub fn classify_entry_with_prediction(
        &self,
        path: &str,
        quad: &Quad,
    ) -> Result<(EntryStyle, StylePrediction), String> {
        let pred = self.predict_entry(path, quad)?;
        let style = pred.to_entry_style(EntryStyle::default());
        Ok((style, pred))
    }
}

/// Crops the axis-aligned bounding box of `quad` from `image`, clamped to the
/// image. Degenerate (empty) quads fall back to the whole image.
fn crop_quad(image: &RgbImage, quad: &Quad) -> RgbImage {
    let [min_x, min_y, max_x, max_y] = quad.bounds();
    let (width, height) = image.dimensions();
    let x0 = min_x.floor().clamp(0.0, width as f32 - 1.0) as u32;
    let y0 = min_y.floor().clamp(0.0, height as f32 - 1.0) as u32;
    let x1 = max_x.ceil().clamp((x0 + 1) as f32, width as f32) as u32;
    let y1 = max_y.ceil().clamp((y0 + 1) as f32, height as f32) as u32;
    image::imageops::crop_imm(image, x0, y0, x1 - x0, y1 - y0).to_image()
}

/// Squishes the crop to `160x64` (bicubic), exactly like the training
/// pipeline. No letterboxing: the crop fills the whole input.
fn resize_to_input(crop: &RgbImage) -> RgbImage {
    image::imageops::resize(
        crop,
        MODEL_WIDTH,
        MODEL_HEIGHT,
        image::imageops::FilterType::CatmullRom,
    )
}

/// Normalizes the resized RGB crop to the `[1, 3, 64, 160]` float tensor the
/// model expects (ImageNet normalization, values in 0..1).
fn compose_input(resized: &RgbImage) -> Array4<f32> {
    debug_assert_eq!(resized.dimensions(), (MODEL_WIDTH, MODEL_HEIGHT));
    Array4::from_shape_fn(
        (1, 3, MODEL_HEIGHT as usize, MODEL_WIDTH as usize),
        |(_, c, y, x)| {
            let px = resized[(x as u32, y as u32)];
            (px[c] as f32 / 255.0 - MEAN[c]) / STD[c]
        },
    )
}

/// Runs one inference on the single model input; returns the ordered output
/// tensors: flags, bg_type, text_color, effect_color, bg_color, bg_color_a,
/// bg_color_b, bg_direction.
fn run_session(session: &mut Session, input: Array4<f32>) -> Result<Vec<ArrayD<f32>>, String> {
    let outputs = session
        .run(ort::inputs![
            TensorRef::from_array_view(&input).map_err(|e| format!("{e}"))?,
        ])
        .map_err(|e| format!("Styling inference failed: {e}"))?;
    outputs
        .iter()
        .map(|(_, value)| {
            value
                .try_extract_array::<f32>()
                .map_err(|e| format!("Styling output extract failed: {e}"))
                .map(|array| array.to_owned())
        })
        .collect()
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// Reads a flat RGB triplet from an output tensor, mapped from normalized
/// 0..1 to 8-bit.
fn read_rgb(out: &ArrayD<f32>) -> [u8; 3] {
    let flat: Vec<f32> = out.iter().copied().collect();
    let mut rgb = [0u8; 3];
    for (i, channel) in rgb.iter_mut().enumerate() {
        let v = flat.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
        // The model regresses colors with L1 loss, so it converges to the
        // median of a color cluster (~0.96-0.98 for white, ~0.02-0.04 for
        // black) and almost never saturates to the exact corner. Snap the
        // extremes so pure-white / pure-black come out as 255 / 0.
        let v = if v >= 0.95 {
            1.0
        } else if v <= 0.05 {
            0.0
        } else {
            v
        };
        *channel = (v * 255.0).round() as u8;
    }
    rgb
}

/// Reads the binary style flags (sigmoid over a 5-logit tensor, threshold 0.5)
/// in order bold, italic, stroke, shadow, glow.
fn read_flags(out: &ArrayD<f32>) -> [bool; 5] {
    let flat: Vec<f32> = out.iter().copied().collect();
    (0..5)
        .map(|i| sigmoid(flat.get(i).copied().unwrap_or(0.0)) > 0.5)
        .collect::<Vec<_>>()
        .try_into()
        .expect("five flag logits")
}

/// Parses the eight ordered model outputs into a [`StylePrediction`]. Missing
/// outputs (unexpected model) fall back to neutral values instead of panicking.
fn parse_outputs(outputs: &[ArrayD<f32>]) -> StylePrediction {
    let get = |i: usize| outputs.get(i).cloned().unwrap_or_else(|| ArrayD::zeros(vec![0]));

    let [bold, italic, stroke, shadow, glow] = read_flags(&get(0));
    let bg_type_index = get(1)
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(index, _)| index)
        .unwrap_or(0);

    let direction: Vec<f32> = get(7).iter().copied().collect();
    StylePrediction {
        bold,
        italic,
        stroke,
        shadow,
        glow,
        bg_type: BgType::from_index(bg_type_index),
        text_color: read_rgb(&get(2)),
        effect_color: read_rgb(&get(3)),
        bg_color: read_rgb(&get(4)),
        bg_color_a: read_rgb(&get(5)),
        bg_color_b: read_rgb(&get(6)),
        bg_direction: [
            direction.first().copied().unwrap_or(0.0),
            direction.get(1).copied().unwrap_or(0.0),
        ],
    }
}

impl StylePrediction {
    /// Maps the prediction onto an [`EntryStyle`] the app can store.
    ///
    /// - `bold`/`italic` map to the matching flags.
    /// - A stroke turns on a default stroke width and uses the effect color.
    /// - The text color is applied with full alpha.
    /// - A solid background's color is applied to `bg_color`. An artwork
    ///   background becomes fully transparent (alpha 0) so the original
    ///   artwork shows through. A gradient background has no single
    ///   representable color and is left untouched.
    /// - `shadow`/`glow` are detected but have no style field, so they are
    ///   dropped here.
    pub fn to_entry_style(&self, base: EntryStyle) -> EntryStyle {
        let mut style = base;
        style.bold = self.bold;
        style.italic = self.italic;
        style.text_color = [self.text_color[0], self.text_color[1], self.text_color[2], 255];
        if self.stroke {
            style.stroke_color = [
                self.effect_color[0],
                self.effect_color[1],
                self.effect_color[2],
                255,
            ];
            style.stroke_width = style.stroke_width.max(2.0);
        }
        match self.bg_type {
            BgType::Solid => {
                style.bg_color = [self.bg_color[0], self.bg_color[1], self.bg_color[2], 255];
            }
            BgType::Artwork => {
                // No flat color on art: drop the background so the artwork
                // behind the text stays visible.
                style.bg_color = [0, 0, 0, 0];
            }
            BgType::Gradient => {}
        }
        style
    }

    /// Like [`Self::to_entry_style`] but for the **auto pipeline** the gradient
    /// background is also made transparent so the subsequent bg-aware inpaint
    /// can reconstruct it (Telea). Artwork is already transparent.
    pub fn to_entry_style_for_auto(&self, base: EntryStyle) -> EntryStyle {
        let mut style = self.to_entry_style(base);
        if self.bg_type == BgType::Gradient {
            style.bg_color = [0, 0, 0, 0];
        }
        style
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    #[test]
    fn crop_quad_clamps_to_image_bounds() {
        let image = RgbImage::from_pixel(100, 50, Rgb([1, 2, 3]));
        let quad = Quad {
            points: [[-10.0, -10.0], [200.0, -10.0], [200.0, 300.0], [-10.0, 300.0]],
        };
        let crop = crop_quad(&image, &quad);
        assert_eq!(crop.dimensions(), (100, 50), "clamped to the whole image");
    }

    #[test]
    fn crop_quad_takes_the_bounding_box() {
        let mut image = RgbImage::from_pixel(100, 50, Rgb([0, 0, 0]));
        image[(30, 20)] = Rgb([9, 9, 9]);
        let quad = Quad {
            points: [[10.0, 10.0], [40.0, 10.0], [40.0, 30.0], [10.0, 30.0]],
        };
        let crop = crop_quad(&image, &quad);
        assert_eq!(crop.dimensions(), (30, 20));
        assert_eq!(crop[(20, 10)], Rgb([9, 9, 9]));
        assert_eq!(crop[(0, 0)], Rgb([0, 0, 0]));
    }

    #[test]
    fn resize_squishes_to_input_size() {
        let crop = RgbImage::from_pixel(300, 100, Rgb([1, 2, 3]));
        let resized = resize_to_input(&crop);
        assert_eq!(resized.dimensions(), (MODEL_WIDTH, MODEL_HEIGHT));
    }

    #[test]
    fn compose_input_normalizes_image_net() {
        let resized = RgbImage::from_pixel(MODEL_WIDTH, MODEL_HEIGHT, Rgb([0, 0, 0]));
        let input = compose_input(&resized);
        assert_eq!(
            input.shape(),
            &[1, 3, MODEL_HEIGHT as usize, MODEL_WIDTH as usize]
        );
        assert!((input[[0, 0, 0, 0]] - (0.0 - MEAN[0]) / STD[0]).abs() < 1e-6);
        assert!((input[[0, 1, 0, 0]] - (0.0 - MEAN[1]) / STD[1]).abs() < 1e-6);
        let white = RgbImage::from_pixel(MODEL_WIDTH, MODEL_HEIGHT, Rgb([255, 255, 255]));
        let input = compose_input(&white);
        assert!((input[[0, 0, 0, 0]] - (1.0 - MEAN[0]) / STD[0]).abs() < 1e-6);
    }

    #[test]
    fn read_flags_thresholds_sigmoid_at_0_5() {
        let out: ArrayD<f32> =
            ArrayD::from_shape_vec(vec![1, 5], vec![0.0, 1.0, -1.0, 10.0, -10.0])
                .unwrap();
        assert_eq!(read_flags(&out), [false, true, false, true, false]);
    }

    #[test]
    fn read_rgb_maps_normalized_to_8bit() {
        let out: ArrayD<f32> =
            ArrayD::from_shape_vec(vec![1, 3], vec![0.0, 0.5, 1.0]).unwrap();
        assert_eq!(read_rgb(&out), [0, 128, 255]);
    }

    #[test]
    fn parse_outputs_picks_argmax_bg_type() {
        let mut outputs = Vec::new();
        let mk = |shape: &[usize], vals: Vec<f32>| -> ArrayD<f32> {
            ArrayD::from_shape_vec(shape.to_vec(), vals).unwrap()
        };
        outputs.push(mk(&[1, 5], vec![1.0, -1.0, 1.0, -1.0, -1.0]));
        outputs.push(mk(&[1, 3], vec![0.1, 0.2, 0.7])); // artwork
        outputs.push(mk(&[1, 3], vec![0.0, 1.0, 0.0]));
        outputs.push(mk(&[1, 3], vec![1.0, 0.0, 0.0]));
        outputs.push(mk(&[1, 3], vec![0.0, 0.0, 1.0]));
        outputs.push(mk(&[1, 3], vec![0.5, 0.5, 0.0]));
        outputs.push(mk(&[1, 3], vec![0.0, 0.0, 0.5]));
        outputs.push(mk(&[1, 2], vec![0.0, 1.0]));

        let prediction = parse_outputs(&outputs);
        assert_eq!(prediction.bg_type, BgType::Artwork);
        assert!(prediction.bold);
        assert!(!prediction.italic);
        assert!(prediction.stroke);
        assert_eq!(prediction.text_color, [0, 255, 0]);
        assert_eq!(prediction.bg_color, [0, 0, 255]);
        assert_eq!(prediction.bg_color_a, [128, 128, 0]);
        assert_eq!(prediction.bg_direction, [0.0, 1.0]);
    }

    #[test]
    fn to_entry_style_maps_flags_and_colors() {
        let base = EntryStyle::default();
        let prediction = StylePrediction {
            bold: true,
            italic: false,
            stroke: true,
            shadow: true,
            glow: false,
            bg_type: BgType::Solid,
            text_color: [10, 20, 30],
            effect_color: [40, 50, 60],
            bg_color: [70, 80, 90],
            bg_color_a: [0, 0, 0],
            bg_color_b: [0, 0, 0],
            bg_direction: [0.0, 0.0],
        };
        let style = prediction.to_entry_style(base);
        assert!(style.bold);
        assert!(!style.italic);
        assert_eq!(style.stroke_color, [40, 50, 60, 255]);
        assert!(style.stroke_width > 0.0);
        assert_eq!(style.text_color, [10, 20, 30, 255]);
        assert_eq!(style.bg_color, [70, 80, 90, 255]);
    }

    #[test]
    fn to_entry_style_leaves_gradient_background_untouched() {
        let base = EntryStyle::default();
        let prediction = StylePrediction {
            bold: false,
            italic: false,
            stroke: false,
            shadow: false,
            glow: false,
            bg_type: BgType::Gradient,
            text_color: [1, 2, 3],
            effect_color: [0, 0, 0],
            bg_color: [255, 255, 255],
            bg_color_a: [5, 6, 7],
            bg_color_b: [8, 9, 10],
            bg_direction: [0.0, 1.0],
        };
        let style = prediction.to_entry_style(base.clone());
        assert_eq!(style.bg_color, base.bg_color, "no single color for gradients");
        assert_eq!(style.text_color, [1, 2, 3, 255]);
    }

    #[test]
    fn to_entry_style_makes_artwork_background_transparent() {
        let base = EntryStyle::default();
        assert_eq!(base.bg_color, [255, 255, 255, 255]);
        let prediction = StylePrediction {
            bold: false,
            italic: false,
            stroke: false,
            shadow: false,
            glow: false,
            bg_type: BgType::Artwork,
            text_color: [1, 2, 3],
            effect_color: [0, 0, 0],
            bg_color: [255, 255, 255],
            bg_color_a: [0, 0, 0],
            bg_color_b: [0, 0, 0],
            bg_direction: [0.0, 0.0],
        };
        let style = prediction.to_entry_style(base);
        assert_eq!(style.bg_color, [0, 0, 0, 0], "artwork bg must be transparent");
        assert_eq!(style.text_color, [1, 2, 3, 255]);
    }
}