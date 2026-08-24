//! CPU image inpainting with three interchangeable backends: the pure-Rust
//! Telea algorithm from the [`inpaint`] crate (the default: no model, no
//! download), the LaMa ONNX model (`lama-manga.onnx`, fixed 512) and the
//! AOT-GAN ONNX model (`inpainting_aot.onnx`, variable resolution up to
//! 1024, pad=8 — faster + lower memory than LaMa).
//!
//! [`Engine`] owns the backend chosen at build time. The ONNX backends hold
//! one shared inference session (CPU execution provider by design) and
//! inpaint image regions one at a time; the Telea backend is stateless and
//! rewrites masked pixels in place. All take the same job: an image path, a
//! selected mask rectangle and the text-box quads inside it; the mask
//! (the rectangle itself, or the quad union when quads are present) is
//! reconstructed from surrounding context — `radius` pixels of context for
//! Telea and [`LAMA_CONTEXT_PAD`] / [`AOT_CONTEXT_PAD`] pixels of real context
//! plus the existing mirror padding (LaMa) or AOT's pad-to-multiple logic
//! — and each box comes back as its own RGBA crop an app layers over the
//! original image without writing anything to disk.

use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use image::{GrayImage, Rgb, RgbImage, RgbaImage};
use imageproc::drawing::draw_polygon_mut;
use inpaint::prelude::*;
use ndarray::{Array4, ArrayD};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::TensorRef;
use scanlateit_model::Quad;
use scanlateit_settings::InpaintBackend;

/// The fixed square input size of the LaMa model.
pub const MODEL_EDGE: u32 = 512;

/// Real image pixels of surrounding context added around the selected mask
/// for the LaMa backend before the mirror padding to `MODEL_EDGE`.
/// Telea's context pad is the `radius` setting itself.
const LAMA_CONTEXT_PAD: f32 = 32.0;

/// Context pad for AOT backend (same real-pixel expansion as LaMa, but
/// AOT uses variable resolution + pad-to-multiple instead of mirror padding).
const AOT_CONTEXT_PAD: f32 = 32.0;

/// AOT pad multiple (model was trained with stride 8).
pub const AOT_PAD: u32 = 8;

/// AOT max side for inference; larger crops are scaled down preserving aspect
/// before padding (mirrors `aot_inference.py: potentially` max_size=1024).
pub const AOT_MAX_SIZE: u32 = 1024;

const MODEL_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../models");
const MODEL_FILE: &str = "lama-manga_int8.onnx";
const MODEL_FILE_AOT: &str = "inpainting_aot.onnx";

/// Cloneable handle to the shared inpainting engine: either the stateless
/// Telea backend or an ONNX session (one inference at a time, serialized
/// through the inner mutex).
#[derive(Clone)]
pub struct Engine {
    backend: InpaintBackend,
    /// Telea's interpolation radius in pixels; ignored by the ONNX backends.
    radius: i32,
    /// The shared ONNX session; `None` for the Telea backend.
    session: Option<Arc<Mutex<Session>>>,
}

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Engine")
            .field("backend", &self.backend)
            .field("radius", &self.radius)
            .finish()
    }
}

impl Engine {
    /// Builds the engine for `backend`. The Telea backend is instant and
    /// stateless; the ONNX backends load their model with a CPU-only session.
    pub fn build(backend: InpaintBackend, radius: i32) -> Result<Self, String> {
        let session = match backend {
            InpaintBackend::Telea => None,
            InpaintBackend::Lama => {
                let path = Path::new(MODEL_DIR).join(MODEL_FILE);
                let session = Session::builder()
                    .map_err(|e| format!("ORT init failed: {e}"))?
                    .with_optimization_level(GraphOptimizationLevel::Level3)
                    .map_err(|e| format!("ORT init failed: {e}"))?
                    .with_intra_threads(4)
                    .map_err(|e| format!("ORT init failed: {e}"))?
                    .with_execution_providers([ort::ep::CPU::default().build()])
                    .map_err(|e| format!("ORT init failed: {e}"))?
                    .commit_from_file(&path)
                    .map_err(|e| {
                        format!("failed to load inpainting model {}: {e}", path.display())
                    })?;
                Some(Arc::new(Mutex::new(session)))
            }
            InpaintBackend::Aot => {
                let path = Path::new(MODEL_DIR).join(MODEL_FILE_AOT);
                let session = Session::builder()
                    .map_err(|e| format!("ORT init failed: {e}"))?
                    .with_optimization_level(GraphOptimizationLevel::Level3)
                    .map_err(|e| format!("ORT init failed: {e}"))?
                    .with_intra_threads(4)
                    .map_err(|e| format!("ORT init failed: {e}"))?
                    .with_execution_providers([ort::ep::CPU::default().build()])
                    .map_err(|e| format!("ORT init failed: {e}"))?
                    .commit_from_file(&path)
                    .map_err(|e| {
                        format!("failed to load AOT inpainting model {}: {e}. Place inpainting_aot.onnx (opset 18, inputs img [B,3,H,W] + mask [B,1,H,W]) from https://github.com/zyddnys/manga-image-translator or converted ONNX.", path.display())
                    })?;
                Some(Arc::new(Mutex::new(session)))
            }
        };
        Ok(Self {
            backend,
            radius: radius.max(1),
            session,
        })
    }

    /// The backend this engine was built for; the app rebuilds the engine
    /// when the configured backend or radius changes.
    pub fn backend(&self) -> InpaintBackend {
        self.backend
    }

    /// The Telea interpolation radius this engine was built with.
    pub fn radius(&self) -> i32 {
        self.radius
    }

    /// Decodes `path` and inpaints the selected mask `rect`, masking out the
    /// given quads (or the whole `rect` when there are none) and sampling
    /// from surrounding context. The surrounding context is the Telea
    /// `radius` (`settings::inpaint_radius`) for the Telea backend and
    /// [`LAMA_CONTEXT_PAD`] / [`AOT_CONTEXT_PAD`] plus the existing padding
    /// for the ONNX backends.
    /// The original file is never modified; each returned patch is the RGBA
    /// crop of one mask quad (`[x, y, w, h]` in image pixels), with the
    /// text reconstructed by the engine's backend. For an empty quad list
    /// the single returned patch covers the (clamped) `rect` itself.
    /// The patch image has `alpha=0` outside the actual rotated quad so
    /// only the quad interior is composited. The third element is the
    /// corresponding quad (`Some` when input quads non-empty, `None` for
    /// empty-list whole-rect case).
    pub fn run_blocking(
        &self,
        path: &str,
        rect: [f32; 4],
        quads: &[Quad],
    ) -> Result<Vec<(RgbaImage, [f32; 4], Option<Quad>)>, String> {
        let image = image::ImageReader::open(path)
            .map_err(|e| format!("Failed to open {path}: {e}"))?
            .with_guessed_format()
            .map_err(|e| format!("Failed to decode {path}: {e}"))?
            .decode()
            .map_err(|e| format!("Failed to decode {path}: {e}"))?
            .into_rgba8();
        match self.backend {
            InpaintBackend::Telea => telea_inpaint_crop(&image, rect, quads, self.radius),
            InpaintBackend::Lama => {
                let mut session = self
                    .session
                    .as_ref()
                    .ok_or("LaMa engine has no session")?
                    .lock()
                    .map_err(|e| format!("Inpaint engine lock poisoned: {e}"))?;
                inpaint_crop(&mut session, &image, rect, quads)
            }
            InpaintBackend::Aot => {
                let mut session = self
                    .session
                    .as_ref()
                    .ok_or("AOT engine has no session")?
                    .lock()
                    .map_err(|e| format!("Inpaint engine lock poisoned: {e}"))?;
                aot_inpaint_crop(&mut session, &image, rect, quads)
            }
        }
    }

    /// Like [`Self::run_blocking`] but for an already-decoded [`RgbaImage`].
    /// Used for stitched canvases that span two pages (manual in-between
    /// inpaint). The `rect` and `quads` are in the passed image's pixel
    /// space. For the stitched path every backend follows Lama's 512-edge
    /// handling (window / resize) is inside `inpaint_crop` / `aot_inpaint_crop`;
    /// Telea simply expands by `radius`.
    pub fn run_on_image(
        &self,
        image: &RgbaImage,
        rect: [f32; 4],
        quads: &[Quad],
    ) -> Result<Vec<(RgbaImage, [f32; 4], Option<Quad>)>, String> {
        match self.backend {
            InpaintBackend::Telea => telea_inpaint_crop(image, rect, quads, self.radius),
            InpaintBackend::Lama => {
                let mut session = self
                    .session
                    .as_ref()
                    .ok_or("LaMa engine has no session")?
                    .lock()
                    .map_err(|e| format!("Inpaint engine lock poisoned: {e}"))?;
                inpaint_crop(&mut session, image, rect, quads)
            }
            InpaintBackend::Aot => {
                let mut session = self
                    .session
                    .as_ref()
                    .ok_or("AOT engine has no session")?
                    .lock()
                    .map_err(|e| format!("Inpaint engine lock poisoned: {e}"))?;
                aot_inpaint_crop(&mut session, image, rect, quads)
            }
        }
    }
}

/// Runs one LaMa inference on the single `input` [1,4,512,512] tensor
/// (channels 0-2 = masked RGB 0-1 zeroed, channel 3 = mask 0/1).
/// Weight-only INT8 model (`lama-manga_int8.onnx`, per-channel asymmetric UINT8
/// + DequantizeLinear axis=0) keeps compute in FP32.
fn run_session(
    session: &mut Session,
    input: Array4<f32>,
) -> Result<ArrayD<f32>, String> {
    let outputs = session
        .run(ort::inputs![
            "input" => TensorRef::from_array_view(&input).map_err(|e| format!("{e}"))?,
        ])
        .map_err(|e| format!("Inpaint inference failed: {e}"))?;
    outputs[0]
        .try_extract_array::<f32>()
        .map_err(|e| format!("Inpaint output extract failed: {e}"))
        .map(|array| array.to_owned())
}

/// Runs one AOT inference; input names are `img` + `mask` with fallback
/// to `image` + `mask` (both seen in exported variants).
fn run_session_aot(
    session: &mut Session,
    img: Array4<f32>,
    mask: Array4<f32>,
) -> Result<ArrayD<f32>, String> {
    // Prefer `img`/`mask` (canonical AOT export — aot_inference.py:180),
    // fallback to `image`/`mask` (some re-exports).
    // Each attempt is isolated in its own closure so the `SessionOutputs`
    // borrow is dropped before the next `session.run` borrow (E0499).
    let try_img: Result<ArrayD<f32>, String> = (|| {
        let outputs = session
            .run(ort::inputs![
                "img" => TensorRef::from_array_view(&img).map_err(|e| format!("{e}"))?,
                "mask" => TensorRef::from_array_view(&mask).map_err(|e| format!("{e}"))?,
            ])
            .map_err(|e| format!("{e}"))?;
        outputs[0]
            .try_extract_array::<f32>()
            .map_err(|e| format!("{e}"))
            .map(|a| a.to_owned())
    })();
    match try_img {
        Ok(arr) => Ok(arr),
        Err(e_img) => {
            let try_image: Result<ArrayD<f32>, String> = (|| {
                let outputs = session
                    .run(ort::inputs![
                        "image" => TensorRef::from_array_view(&img).map_err(|e| format!("{e}"))?,
                        "mask" => TensorRef::from_array_view(&mask).map_err(|e| format!("{e}"))?,
                    ])
                    .map_err(|e| format!("{e}"))?;
                outputs[0]
                    .try_extract_array::<f32>()
                    .map_err(|e| format!("{e}"))
                    .map(|a| a.to_owned())
            })();
            match try_image {
                Ok(arr) => Ok(arr),
                Err(e_image) => Err(format!(
                    "AOT inference failed (tried img/mask then image/mask): img/mask: {e_img}; image/mask: {e_image}"
                )),
            }
        }
    }
}

/// The clamped integer crop rect `[x, y, w, h]` for a float rect in image
/// pixels. At least 1 pixel in both dimensions and fully inside the image.
fn crop_spec(rect: [f32; 4], width: u32, height: u32) -> [u32; 4] {
    let [x0, y0, x1, y1] = rect;
    let x = x0.floor().clamp(0.0, width as f32 - 1.0) as u32;
    let y = y0.floor().clamp(0.0, height as f32 - 1.0) as u32;
    let x1 = x1.ceil().clamp(x as f32 + 1.0, width as f32);
    let y1 = y1.ceil().clamp(y as f32 + 1.0, height as f32);
    [x, y, (x1 - x as f32) as u32, (y1 - y as f32) as u32]
}

/// One dimension of [`view_window`]'s window: `(src_off, copy_len,
/// dst_off)`. Areas that fit in `edge` pixels are copied whole and placed
/// centered on the canvas; larger areas are not scaled down - a full
/// `edge`-pixel slice starting as close to the mask center as possible
/// (and clamped to the area) is copied instead.
fn window_dim(crop: i64, center: f32, edge: i64) -> (i64, i64, i64) {
    if crop <= edge {
        (0, crop, (edge - crop) / 2)
    } else {
        let src = (center - edge as f32 / 2.0).round() as i64;
        (src.clamp(0, crop - edge), edge, 0)
    }
}

/// The center of the combined mask boxes in crop-local coordinates; the
/// whole crop's center when nothing is masked. Uses the actual quad
/// centroids (mean of points) so rotated/skewed quads are centered correctly,
/// not their axis-aligned bounding boxes.
fn mask_center(crop_w: u32, crop_h: u32, quads: &[Quad], origin: [f32; 2]) -> [f32; 2] {
    if quads.is_empty() {
        return [crop_w as f32 / 2.0, crop_h as f32 / 2.0];
    }
    // For a single quad, use its centroid. For multiple, use the average
    // centroid (center of mass) — more stable than AABB for skewed quads.
    let mut sum_x = 0.0f32;
    let mut sum_y = 0.0f32;
    for quad in quads {
        let cx = (quad.points[0][0] + quad.points[1][0] + quad.points[2][0] + quad.points[3][0]) * 0.25;
        let cy = (quad.points[0][1] + quad.points[1][1] + quad.points[2][1] + quad.points[3][1]) * 0.25;
        sum_x += cx;
        sum_y += cy;
    }
    let avg_x = sum_x / quads.len() as f32;
    let avg_y = sum_y / quads.len() as f32;
    [avg_x - origin[0], avg_y - origin[1]]
}

/// The model canvas window over the area crop, as `(src_x, src_y, w, h,
/// dst_x, dst_y)`: the crop pixels `src_x..src_x+w, src_y..src_y+h` are
/// copied to the canvas at `(dst_x, dst_y)`; everything else on the canvas
/// reflects the window's edge pixels (symmetric padding, the padding the
/// LaMa model was trained with).
///
/// The window is centered on the combined mask boxes so a big area is fed
/// to the model at full resolution with only the sides around the text
/// boxes cut off; a small area is centered with symmetric padding. Either
/// way the model input is exactly `MODEL_EDGE` x `MODEL_EDGE`.
fn view_window(
    crop_w: u32,
    crop_h: u32,
    quads: &[Quad],
    origin: [f32; 2],
) -> (i64, i64, i64, i64, i64, i64) {
    let center = mask_center(crop_w, crop_h, quads, origin);
    let (sx, sw, dx) = window_dim(crop_w as i64, center[0], MODEL_EDGE as i64);
    let (sy, sh, dy) = window_dim(crop_h as i64, center[1], MODEL_EDGE as i64);
    (sx, sy, sw, sh, dx, dy)
}

/// Maps any index into the mirrored range `[0, len)`: indices inside the
/// range pass through, indices beyond it are reflected back (symmetric
/// padding, edge pixels repeated at the seam), including negative offsets.
fn reflect_index(x: i64, len: i64) -> i64 {
    let period = len * 2;
    let mut x = x % period;
    if x < 0 {
        x += period;
    }
    if x >= len {
        period - x - 1
    } else {
        x
    }
}

/// Fills `canvas` by placing `region` at `(dst_x, dst_y)` and reflecting
/// the region's edge pixels into the surrounding canvas area.
fn reflect_place_rgb(canvas: &mut RgbImage, region: &RgbImage, dst_x: i64, dst_y: i64) {
    let (canvas_w, canvas_h) = canvas.dimensions();
    let (region_w, region_h) = (region.width() as i64, region.height() as i64);
    for cy in 0..canvas_h as i64 {
        let sy = reflect_index(cy - dst_y, region_h);
        for cx in 0..canvas_w as i64 {
            let sx = reflect_index(cx - dst_x, region_w);
            canvas[(cx as u32, cy as u32)] = region[(sx as u32, sy as u32)];
        }
    }
}

/// Fills `canvas` by placing `region` at `(dst_x, dst_y)` and reflecting
/// the region's edge pixels into the surrounding canvas area.
fn reflect_place_gray(canvas: &mut GrayImage, region: &GrayImage, dst_x: i64, dst_y: i64) {
    let (canvas_w, canvas_h) = canvas.dimensions();
    let (region_w, region_h) = (region.width() as i64, region.height() as i64);
    for cy in 0..canvas_h as i64 {
        let sy = reflect_index(cy - dst_y, region_h);
        for cx in 0..canvas_w as i64 {
            let sx = reflect_index(cx - dst_x, region_w);
            canvas[(cx as u32, cy as u32)] = region[(sx as u32, sy as u32)];
        }
    }
}

/// The white/black mask of a crop: white where any quad overlaps it, black
/// elsewhere. An empty quad list masks the whole crop (nothing was detected,
/// so the user's selection itself is what gets cleaned).
fn build_mask(width: u32, height: u32, quads: &[Quad], origin: [f32; 2]) -> GrayImage {
    if quads.is_empty() {
        return GrayImage::from_pixel(width, height, image::Luma([255]));
    }
    let mut mask = GrayImage::from_pixel(width, height, image::Luma([0]));
    for quad in quads {
        let poly: Vec<imageproc::point::Point<i32>> = quad
            .points
            .iter()
            .map(|[px, py]| {
                imageproc::point::Point::new(
                    (px - origin[0]).round() as i32,
                    (py - origin[1]).round() as i32,
                )
            })
            .collect();
        draw_polygon_mut(&mut mask, &poly, image::Luma([255]));
    }
    mask
}

/// Mask for the *expanded* crop: where `quads` are non-empty the mask is
/// their union (as in [`build_mask`]); otherwise it is the original
/// `rect` (the user's selected mask) white inside the expanded black
/// context border.
fn build_mask_expanded(
    exp_w: u32,
    exp_h: u32,
    quads: &[Quad],
    rect: [f32; 4],
    exp_origin: [f32; 2],
    image_width: u32,
    image_height: u32,
) -> GrayImage {
    if !quads.is_empty() {
        return build_mask(exp_w, exp_h, quads, exp_origin);
    }
    // Empty quad list: mask == original rect (the selection) inside the expanded crop.
    let mut mask = GrayImage::from_pixel(exp_w, exp_h, image::Luma([0]));
    let [rx, ry, rw, rh] = rect;
    // Clamped original rect in image pixels.
    let [ox, oy, ow, oh] = crop_spec([rx, ry, rx + rw, ry + rh], image_width, image_height);
    let dx = (ox as f32 - exp_origin[0]).round() as i64;
    let dy = (oy as f32 - exp_origin[1]).round() as i64;
    let x0 = dx.max(0) as u32;
    let y0 = dy.max(0) as u32;
    let x1 = (dx + ow as i64).clamp(0, exp_w as i64) as u32;
    let y1 = (dy + oh as i64).clamp(0, exp_h as i64) as u32;
    if x1 > x0 && y1 > y0 {
        for y in y0..y1 {
            for x in x0..x1 {
                mask[(x, y)] = image::Luma([255]);
            }
        }
    }
    mask
}

/// Composes the single LaMa INT8 input `[1, 4, 512, 512]` f32:
/// channels 0-2 = masked RGB `canvas * (1 - mask)` in 0-1, channel 3 = mask 0/1.
/// The FP32 graph used to do `image*(1-mask)` internally (Sub+Mul+Concat);
/// the 4ch INT8 graph expects the caller to pre-pack.
fn compose_inputs(canvas: &RgbImage, mask: &GrayImage) -> Array4<f32> {
    debug_assert_eq!(canvas.dimensions(), (MODEL_EDGE, MODEL_EDGE));
    debug_assert_eq!(mask.dimensions(), (MODEL_EDGE, MODEL_EDGE));
    Array4::from_shape_fn(
        (1, 4, MODEL_EDGE as usize, MODEL_EDGE as usize),
        |(_, c, y, x)| {
            let m = mask[(x as u32, y as u32)][0] as f32 / 255.0;
            if c == 3 {
                m
            } else {
                canvas[(x as u32, y as u32)][c] as f32 / 255.0 * (1.0 - m)
            }
        },
    )
}

/// Reads the model output (`[1, 3, 512, 512]` in 0..1) back onto the area
/// crop: the canvas region that received the crop pixels (`dst_x..dst_x+w`,
/// `dst_y..dst_y+h`) maps 1:1 back to `src_x..src_x+w, src_y..src_y+h`;
/// everything else (the white padding the model never saw) is white.
fn extract_window(
    output: &ArrayD<f32>,
    crop_w: u32,
    crop_h: u32,
    src_x: i64,
    src_y: i64,
    w: i64,
    h: i64,
    dst_x: i64,
    dst_y: i64,
) -> RgbImage {
    let shape = [1usize, 3, MODEL_EDGE as usize, MODEL_EDGE as usize];
    let Ok(reshaped) = output.clone().into_shape_with_order(shape) else {
        eprintln!(
            "[inpaint::extract_window] shape mismatch expected {:?} got {:?} -> white {}x{}",
            shape,
            output.shape(),
            crop_w,
            crop_h
        );
        return RgbImage::from_pixel(crop_w, crop_h, Rgb([255, 255, 255]));
    };
    let to_u8 = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    let will_white = crop_w as i64 != w || crop_h as i64 != h;
    if will_white {
        eprintln!(
            "[inpaint::extract_window] crop {}x{} window w={} h={} src={},{} dst={},{} -> sides will stay WHITE (fitted initialized 255)",
            crop_w, crop_h, w, h, src_x, src_y, dst_x, dst_y
        );
    }
    let mut fitted = RgbImage::from_pixel(crop_w, crop_h, Rgb([255, 255, 255]));
    for py in 0..h {
        for px in 0..w {
            let cx = (dst_x + px) as usize;
            let cy = (dst_y + py) as usize;
            fitted[((src_x + px) as u32, (src_y + py) as u32)] = Rgb([
                to_u8(reshaped[[0, 0, cy, cx]]),
                to_u8(reshaped[[0, 1, cy, cx]]),
                to_u8(reshaped[[0, 2, cy, cx]]),
            ]);
        }
    }
    fitted
}

/// Helper: set alpha=0 for pixels outside `quad` inside `crop` (which is at `crop_origin` in image coords).
fn apply_quad_alpha_mask(crop: &mut RgbaImage, quad: &Quad, crop_origin: [f32; 2]) {
    let (w, h) = crop.dimensions();
    if w == 0 || h == 0 {
        return;
    }
    // Build mask of quad inside crop: white where inside, black outside.
    let mut mask = image::GrayImage::from_pixel(w, h, image::Luma([0]));
    let poly: Vec<imageproc::point::Point<i32>> = quad
        .points
        .iter()
        .map(|[px, py]| {
            imageproc::point::Point::new(
                (px - crop_origin[0]).round() as i32,
                (py - crop_origin[1]).round() as i32,
            )
        })
        .collect();
    draw_polygon_mut(&mut mask, &poly, image::Luma([255]));
    for (x, y, pixel) in crop.enumerate_pixels_mut() {
        if mask.get_pixel(x, y)[0] == 0 {
            pixel[3] = 0; // transparent outside quad
        }
    }
}

/// Splits an inpainted area patch into per-mask-box crops: one `(crop,
/// rect)` pair per quad, where `rect` is `[x, y, w, h]` in image pixels and
/// the crop covers the quad's bounding box clipped to the area. Boxes that
/// lie entirely outside the area are skipped; an empty quad list returns the
/// whole area patch (it was fully masked and cleaned).
///
/// For rotated/skewed quads the returned crop keeps the AABB dimensions but
/// pixels outside the actual quad polygon are made transparent (`alpha=0`),
/// so overlaying the patch only affects the true quad shape. Also returns
/// the quad for storage.
fn bbox_crops(patch: RgbaImage, origin: [f32; 2], quads: &[Quad]) -> Vec<(RgbaImage, [f32; 4], Option<Quad>)> {
    if quads.is_empty() {
        let (width, height) = patch.dimensions();
        return vec![(patch, [origin[0], origin[1], width as f32, height as f32], None)];
    }
    let ox = origin[0] as i64;
    let oy = origin[1] as i64;
    let max_x = ox + patch.width() as i64;
    let max_y = oy + patch.height() as i64;
    let mut crops = Vec::new();
    for quad in quads {
        let [bx0, by0, bx1, by1] = quad.bounds();
        let [ax0, ay0, ax1, ay1] = [
            origin[0],
            origin[1],
            origin[0] + patch.width() as f32,
            origin[1] + patch.height() as f32,
        ];
        // Boxes with no overlap with the area at all are left untouched.
        let ix0 = bx0.max(ax0);
        let ix1 = bx1.min(ax1);
        let iy0 = by0.max(ay0);
        let iy1 = by1.min(ay1);
        if ix0 >= ix1 || iy0 >= iy1 {
            continue;
        }
        let cx0 = (ix0.floor() as i64).clamp(ox, max_x - 1);
        let cy0 = (iy0.floor() as i64).clamp(oy, max_y - 1);
        let cx1 = (ix1.ceil() as i64).clamp(cx0 + 1, max_x);
        let cy1 = (iy1.ceil() as i64).clamp(cy0 + 1, max_y);
        let w = (cx1 - cx0) as u32;
        let h = (cy1 - cy0) as u32;
        let mut crop = image::imageops::crop_imm(
            &patch,
            (cx0 - ox) as u32,
            (cy0 - oy) as u32,
            w,
            h,
        )
        .to_image();
        // Make outside-quad pixels transparent so only the actual rotated quad is patched
        apply_quad_alpha_mask(&mut crop, quad, [cx0 as f32, cy0 as f32]);
        crops.push((crop, [cx0 as f32, cy0 as f32, w as f32, h as f32], Some(*quad)));
    }
    crops
}

/// Inpaints `rect` of `image` with the Telea algorithm from the [`inpaint`]
/// crate: the mask is `rect` itself when `quads` is empty, otherwise the
/// union of `quads` inside `rect`; the algorithm samples from the
/// surrounding context expanded by `radius` pixels around `rect`
/// (clamped to the image). Returns the same per-mask-box crops as
/// [`inpaint_crop`], with the alpha channel interpolated like every other
/// channel (opaque manga pages keep alpha at 255).
pub fn telea_inpaint_crop(
    image: &RgbaImage,
    rect: [f32; 4],
    quads: &[Quad],
    radius: i32,
) -> Result<Vec<(RgbaImage, [f32; 4], Option<Quad>)>, String> {
    let radius = radius.max(1);
    let pad = radius as f32;
    let [rx, ry, rw, rh] = rect;
    let [ex, ey, exp_w, exp_h] = crop_spec(
        [rx - pad, ry - pad, rx + rw + pad, ry + rh + pad],
        image.width(),
        image.height(),
    );
    let exp_origin = [ex as f32, ey as f32];

    let mask = build_mask_expanded(exp_w, exp_h, quads, rect, exp_origin, image.width(), image.height());
    eprintln!(
        "[inpaint::telea] rect={:?} quads={} radius={} pad={} image={}x{} exp=[{},{},{},{}] mask_sum={}",
        rect,
        quads.len(),
        radius,
        pad,
        image.width(),
        image.height(),
        ex,
        ey,
        exp_w,
        exp_h,
        mask.pixels().map(|p| p[0] as u32).sum::<u32>()
    );
    let mut crop = image::imageops::crop_imm(image, ex, ey, exp_w, exp_h).to_image();
    crop.telea_inpaint(&mask, radius)
        .map_err(|e| format!("Telea inpaint failed: {e}"))?;
    if quads.is_empty() {
        // Return only the original masked rect, not the whole expanded context border.
        let [ox, oy, ow, oh] = crop_spec([rx, ry, rx + rw, ry + rh], image.width(), image.height());
        eprintln!(
            "[inpaint::telea] quads empty -> returning sub-crop [{},{},{},{}] from exp [{},{},{},{}]",
            ox, oy, ow, oh, ex, ey, exp_w, exp_h
        );
        let sub = image::imageops::crop_imm(
            &crop,
            (ox - ex) as u32,
            (oy - ey) as u32,
            ow,
            oh,
        )
        .to_image();
        return Ok(vec![(sub, [ox as f32, oy as f32, ow as f32, oh as f32], None)]);
    }
    let out = bbox_crops(crop, exp_origin, quads);
    eprintln!("[inpaint::telea] quads={} -> {} bbox crops", quads.len(), out.len());
    Ok(out)
}

/// Inpaints `rect` of `image` with the given box quads masked out, where
/// `rect` is `[x, y, w, h]` in image pixels (the selected mask). The image
/// crop is `rect` expanded by [`LAMA_CONTEXT_PAD`] pixels in every direction
/// (clamped to the image) so the model sees real surrounding context; the
/// remaining canvas area to `MODEL_EDGE` is still mirror-padded via
/// `reflect_place_*` (the padding the LaMa model was trained with).
/// Returns one RGBA crop per mask box (or a single crop of the whole `rect`
/// when `quads` is empty), with the alpha channel copied from the original
/// pixels (transparency survives).
pub fn inpaint_crop(
    session: &mut Session,
    image: &RgbaImage,
    rect: [f32; 4],
    quads: &[Quad],
) -> Result<Vec<(RgbaImage, [f32; 4], Option<Quad>)>, String> {
    let [rx, ry, rw, rh] = rect;
    let pad = LAMA_CONTEXT_PAD;
    let [ex, ey, exp_w, exp_h] = crop_spec(
        [rx - pad, ry - pad, rx + rw + pad, ry + rh + pad],
        image.width(),
        image.height(),
    );
    let exp_origin = [ex as f32, ey as f32];

    let mask = build_mask_expanded(exp_w, exp_h, quads, rect, exp_origin, image.width(), image.height());
    eprintln!(
        "[inpaint::lama] rect={:?} quads={} pad={} image={}x{} exp=[{},{},{},{}] exp_origin={:?}",
        rect,
        quads.len(),
        pad,
        image.width(),
        image.height(),
        ex,
        ey,
        exp_w,
        exp_h,
        exp_origin
    );
    let crop = image::imageops::crop_imm(image, ex, ey, exp_w, exp_h).to_image();

    // If the expanded crop is larger than the model in either dimension we
    // resize the whole crop (and mask) to 512x512, run the model, then resize
    // the output back. This keeps the entire mask visible and avoids the
    // white `extract_window` sides (`will_white`).
    let needs_resize = exp_w > MODEL_EDGE || exp_h > MODEL_EDGE;
    let (canvas, canvas_mask, sx, sy, sw, sh, dx, dy) = if needs_resize {
        let rgb_crop = image::DynamicImage::ImageRgba8(crop.clone()).to_rgb8();
        let resized_rgb = image::imageops::resize(
            &rgb_crop,
            MODEL_EDGE,
            MODEL_EDGE,
            image::imageops::FilterType::Lanczos3,
        );
        let resized_mask = image::imageops::resize(
            &mask,
            MODEL_EDGE,
            MODEL_EDGE,
            image::imageops::FilterType::Nearest,
        );
        eprintln!(
            "[inpaint::lama] LARGE exp {}x{} > {} -> resize whole crop+mask to {}x{} (no window, no mirror)",
            exp_w, exp_h, MODEL_EDGE, MODEL_EDGE, MODEL_EDGE
        );
        // sx..dx unused in resize path; set to cover whole canvas
        (resized_rgb, resized_mask, 0, 0, MODEL_EDGE as i64, MODEL_EDGE as i64, 0, 0)
    } else {
        // Center the model window on the mask (quad union or original rect when empty).
        let (sx, sy, sw, sh, dx, dy) = if quads.is_empty() {
            // Empty quads: mask is the original rect, centered on its center in expanded-local coords.
            let [ox, oy, ow, oh] = crop_spec([rx, ry, rx + rw, ry + rh], image.width(), image.height());
            let center_x = (ox as f32 + ow as f32 / 2.0) - exp_origin[0];
            let center_y = (oy as f32 + oh as f32 / 2.0) - exp_origin[1];
            let (sx, sw, dx) = window_dim(exp_w as i64, center_x, MODEL_EDGE as i64);
            let (sy, sh, dy) = window_dim(exp_h as i64, center_y, MODEL_EDGE as i64);
            eprintln!(
                "[inpaint::lama] empty quads window: ox,oy,ow,oh=[{},{},{},{}] center=({:.1},{:.1}) sx,sy,sw,sh,dx,dy={},{},{},{},{},{} exp={}x{} will_white_h={} will_white_w={}",
                ox, oy, ow, oh, center_x, center_y, sx, sy, sw, sh, dx, dy, exp_w, exp_h, exp_h as i64 - sh, exp_w as i64 - sw
            );
            (sx, sy, sw, sh, dx, dy)
        } else {
            let win = view_window(exp_w, exp_h, quads, exp_origin);
            eprintln!(
                "[inpaint::lama] quads window: win={:?} exp={}x{} center={:?}",
                win,
                exp_w,
                exp_h,
                mask_center(exp_w, exp_h, quads, exp_origin)
            );
            win
        };
        let region = image::DynamicImage::from(
            image::imageops::crop_imm(&crop, sx as u32, sy as u32, sw as u32, sh as u32).to_image(),
        )
        .into_rgb8();
        let mut canvas = RgbImage::new(MODEL_EDGE, MODEL_EDGE);
        reflect_place_rgb(&mut canvas, &region, dx, dy);
        let region_mask =
            image::imageops::crop_imm(&mask, sx as u32, sy as u32, sw as u32, sh as u32).to_image();
        let mut canvas_mask = GrayImage::new(MODEL_EDGE, MODEL_EDGE);
        reflect_place_gray(&mut canvas_mask, &region_mask, dx, dy);
        eprintln!(
            "[inpaint::lama] canvas={}x{} region {}x{} at {},{} -> canvas {}x{} mask placed at {},{}",
            region.width(),
            region.height(),
            sw,
            sh,
            sx,
            sy,
            canvas.width(),
            canvas.height(),
            dx,
            dy
        );
        (canvas, canvas_mask, sx, sy, sw, sh, dx, dy)
    };
    let input = compose_inputs(&canvas, &canvas_mask);
    let output = run_session(session, input)?;
    eprintln!("[inpaint::lama] inference done output shape={:?}", output.shape());
    let rgb = if needs_resize {
        // Output is 512x512 representing the whole exp area → resize back to exp size
        let out_canvas = match output.clone().into_shape_with_order([1usize, 3, MODEL_EDGE as usize, MODEL_EDGE as usize]) {
            Ok(reshaped) => {
                let to_u8 = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
                let mut img = RgbImage::new(MODEL_EDGE, MODEL_EDGE);
                for y in 0..MODEL_EDGE {
                    for x in 0..MODEL_EDGE {
                        img[(x, y)] = Rgb([
                            to_u8(reshaped[[0, 0, y as usize, x as usize]]),
                            to_u8(reshaped[[0, 1, y as usize, x as usize]]),
                            to_u8(reshaped[[0, 2, y as usize, x as usize]]),
                        ]);
                    }
                }
                img
            }
            Err(_) => {
                eprintln!("[inpaint::lama] resize path shape mismatch -> white {}x{}", exp_w, exp_h);
                RgbImage::from_pixel(MODEL_EDGE, MODEL_EDGE, Rgb([255, 255, 255]))
            }
        };
        let resized_back = image::imageops::resize(
            &out_canvas,
            exp_w,
            exp_h,
            image::imageops::FilterType::Lanczos3,
        );
        eprintln!(
            "[inpaint::lama] RESIZE back {}x{} -> {}x{} (no white)",
            MODEL_EDGE, MODEL_EDGE, exp_w, exp_h
        );
        resized_back
    } else {
        let r = extract_window(&output, exp_w, exp_h, sx, sy, sw, sh, dx, dy);
        eprintln!(
            "[inpaint::lama] extract_window exp={}x{} sx,sy,sw,sh,dx,dy={},{},{},{},{},{} rgb={}x{} will_white={}",
            exp_w,
            exp_h,
            sx,
            sy,
            sw,
            sh,
            dx,
            dy,
            r.width(),
            r.height(),
            (exp_w as i64 != sw || exp_h as i64 != sh)
        );
        r
    };

    let mut patch: RgbaImage = image::DynamicImage::ImageRgb8(rgb).into_rgba8();
    for (px, src) in patch.pixels_mut().zip(crop.pixels()) {
        px[3] = src[3];
    }
    if quads.is_empty() {
        let [ox, oy, ow, oh] = crop_spec([rx, ry, rx + rw, ry + rh], image.width(), image.height());
        let sub = image::imageops::crop_imm(
            &patch,
            (ox - ex) as u32,
            (oy - ey) as u32,
            ow,
            oh,
        )
        .to_image();
        eprintln!(
            "[inpaint::lama] empty quads -> returning sub [{},{},{},{}] from patch {}x{}",
            ox, oy, ow, oh, patch.width(), patch.height()
        );
        return Ok(vec![(sub, [ox as f32, oy as f32, ow as f32, oh as f32], None)]);
    }
    let out = bbox_crops(patch, exp_origin, quads);
    eprintln!("[inpaint::lama] quads={} -> {} bbox crops", quads.len(), out.len());
    Ok(out)
}

// ---------------------------------------------------------------------------
// AOT-GAN backend
// ---------------------------------------------------------------------------

fn next_multiple(v: u32, pad: u32) -> u32 {
    if v % pad == 0 {
        v
    } else {
        ((v + pad - 1) / pad) * pad
    }
}

/// Inference dimensions for AOT: mirrors `aot_inference.py: potentially` `_next_multiple` + `max_size` logic.
fn aot_infer_dims(w: u32, h: u32, pad: u32, max_size: Option<u32>) -> (u32, u32) {
    if let Some(max) = max_size {
        if w.max(h) > max {
            let scale = max as f32 / w.max(h) as f32;
            let mut nw = (w as f32 * scale).round() as u32;
            let mut nh = (h as f32 * scale).round() as u32;
            nw = next_multiple(nw.max(1), pad);
            nh = next_multiple(nh.max(1), pad);
            return (nw, nh);
        }
    }
    (next_multiple(w, pad), next_multiple(h, pad))
}

/// Inpaints `rect` with the AOT-GAN ONNX model. Variable resolution:
/// `exp` crop (rect + `AOT_CONTEXT_PAD`) is resized to `AOT_PAD`-aligned
/// `AOT_MAX_SIZE`-capped dimensions for inference, normalized to `[-1,1]`,
/// `img*=(1-mask)`, then blended back. No mirror padding.
pub fn aot_inpaint_crop(
    session: &mut Session,
    image: &RgbaImage,
    rect: [f32; 4],
    quads: &[Quad],
) -> Result<Vec<(RgbaImage, [f32; 4], Option<Quad>)>, String> {
    let [rx, ry, rw, rh] = rect;
    let pad = AOT_CONTEXT_PAD;
    let [ex, ey, exp_w, exp_h] = crop_spec(
        [rx - pad, ry - pad, rx + rw + pad, ry + rh + pad],
        image.width(),
        image.height(),
    );
    let exp_origin = [ex as f32, ey as f32];

    let mask = build_mask_expanded(exp_w, exp_h, quads, rect, exp_origin, image.width(), image.height());
    let crop = image::imageops::crop_imm(image, ex, ey, exp_w, exp_h).to_image();
    eprintln!(
        "[inpaint::aot] rect={:?} quads={} pad={} image={}x{} exp=[{},{},{},{}] exp_origin={:?}",
        rect,
        quads.len(),
        pad,
        image.width(),
        image.height(),
        ex,
        ey,
        exp_w,
        exp_h,
        exp_origin
    );

    // Early-out: empty mask (should not happen via build_mask_expanded, but guard)
    let mask_sum: u32 = mask.pixels().map(|p| p[0] as u32).sum();
    if mask_sum == 0 {
        eprintln!("[inpaint::aot] empty mask -> returning original crop");
        if quads.is_empty() {
            let [ox, oy, ow, oh] = crop_spec([rx, ry, rx + rw, ry + rh], image.width(), image.height());
            let sub = image::imageops::crop_imm(&crop, (ox - ex) as u32, (oy - ey) as u32, ow, oh).to_image();
            return Ok(vec![(sub, [ox as f32, oy as f32, ow as f32, oh as f32], None)]);
        }
        return Ok(bbox_crops(crop, exp_origin, quads));
    }

    // Decide inference size (pad=8, max=1024) — mirrors aot_inference.py:142
    let (inf_w, inf_h) = aot_infer_dims(exp_w, exp_h, AOT_PAD, Some(AOT_MAX_SIZE));
    let needs_resize = inf_w != exp_w || inf_h != exp_h;
    eprintln!(
        "[inpaint::aot] exp {}x{} -> inf {}x{} needs_resize={} pad={} max={}",
        exp_w, exp_h, inf_w, inf_h, needs_resize, AOT_PAD, AOT_MAX_SIZE
    );

    let rgb_crop = image::DynamicImage::ImageRgba8(crop.clone()).to_rgb8();
    let (inf_rgb, inf_mask) = if needs_resize {
        let resized_rgb = image::imageops::resize(&rgb_crop, inf_w, inf_h, image::imageops::FilterType::Triangle);
        let resized_mask = image::imageops::resize(&mask, inf_w, inf_h, image::imageops::FilterType::Nearest);
        (resized_rgb, resized_mask)
    } else {
        (rgb_crop, mask.clone())
    };

    // Normalize to [-1,1] and mask*=(1-mask) exactly like aot_inference.py:165-169
    // image: (rgb/127.5 -1) * (1-mask)
    // mask: 0/1 via threshold 0.5
    let h = inf_h as usize;
    let w = inf_w as usize;
    let mut img_arr = Array4::<f32>::zeros((1, 3, h, w));
    let mut mask_arr = Array4::<f32>::zeros((1, 1, h, w));
    for y in 0..h {
        for x in 0..w {
            let m = if inf_mask[(x as u32, y as u32)][0] > 127 { 1.0 } else { 0.0 };
            mask_arr[[0, 0, y, x]] = m;
            let px = inf_rgb[(x as u32, y as u32)];
            let inv = 1.0 - m;
            img_arr[[0, 0, y, x]] = (px[0] as f32 / 127.5 - 1.0) * inv;
            img_arr[[0, 1, y, x]] = (px[1] as f32 / 127.5 - 1.0) * inv;
            img_arr[[0, 2, y, x]] = (px[2] as f32 / 127.5 - 1.0) * inv;
        }
    }

    let output = run_session_aot(session, img_arr, mask_arr)?;
    eprintln!("[inpaint::aot] inference done output shape={:?}", output.shape());

    // Output is [1,3,H,W] in [-1,1] — -> uint8 via (x+1)*127.5, clip
    let shape = [1usize, 3, h, w];
    let reshaped = output.clone().into_shape_with_order(shape).map_err(|e| {
        format!("[inpaint::aot] shape mismatch expected {:?} got {:?}: {e}", shape, output.shape())
    })?;
    let to_u8 = |v: f32| ((v.clamp(-1.0, 1.0) + 1.0) * 127.5).round().clamp(0.0, 255.0) as u8;
    let mut out_inf = RgbImage::new(inf_w, inf_h);
    for y in 0..h {
        for x in 0..w {
            out_inf[(x as u32, y as u32)] = Rgb([
                to_u8(reshaped[[0, 0, y, x]]),
                to_u8(reshaped[[0, 1, y, x]]),
                to_u8(reshaped[[0, 2, y, x]]),
            ]);
        }
    }

    let out_exp = if needs_resize {
        image::imageops::resize(&out_inf, exp_w, exp_h, image::imageops::FilterType::Triangle)
    } else {
        out_inf
    };

    // Blend: ans = inpainted*mask + original*(1-mask) — but for full-exp we
    // already have `mask` at exp resolution. Use original mask (not resized
    // thresholded) for compositing to keep sharp edges; fallback to out_exp
    // where mask==0 we keep original crop pixel.
    // For simplicity, if mask is empty we already returned. Otherwise we
    // composite: where mask white -> out_exp, else original crop rgb.
    let mut blended = RgbImage::new(exp_w, exp_h);
    for y in 0..exp_h {
        for x in 0..exp_w {
            let m = mask[(x, y)][0] > 127;
            blended[(x, y)] = if m { out_exp[(x, y)] } else { image::DynamicImage::ImageRgba8(crop.clone()).to_rgb8()[(x, y)] };
        }
    }

    let mut patch: RgbaImage = image::DynamicImage::ImageRgb8(blended).into_rgba8();
    for (px, src) in patch.pixels_mut().zip(crop.pixels()) {
        px[3] = src[3];
    }
    if quads.is_empty() {
        let [ox, oy, ow, oh] = crop_spec([rx, ry, rx + rw, ry + rh], image.width(), image.height());
        let sub = image::imageops::crop_imm(&patch, (ox - ex) as u32, (oy - ey) as u32, ow, oh).to_image();
        eprintln!(
            "[inpaint::aot] empty quads -> returning sub [{},{},{},{}] from patch {}x{}",
            ox, oy, ow, oh, patch.width(), patch.height()
        );
        return Ok(vec![(sub, [ox as f32, oy as f32, ow as f32, oh as f32], None)]);
    }
    let out = bbox_crops(patch, exp_origin, quads);
    eprintln!("[inpaint::aot] quads={} -> {} bbox crops", quads.len(), out.len());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn quad(points: [[f32; 2]; 4]) -> Quad {
        Quad { points }
    }

    #[test]
    fn telea_inpaint_crop_fills_the_masked_box() {
        let mut image = RgbaImage::from_pixel(64, 64, Rgba([0, 0, 0, 255]));
        for y in 20..40 {
            for x in 20..40 {
                image.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        let quads = [quad([[20.0, 20.0], [40.0, 20.0], [40.0, 40.0], [20.0, 40.0]])];
        let patches = telea_inpaint_crop(&image, [0.0, 0.0, 64.0, 64.0], &quads, 5).unwrap();
        assert_eq!(patches.len(), 1);
        let (patch, bounds, q) = &patches[0];
        assert_eq!(bounds, &[20.0, 20.0, 20.0, 20.0]);
        assert_eq!(patch.dimensions(), (20, 20));
        assert!(q.is_some(), "quad should be stored");
        let in_patch = |px: u32, py: u32| {
            let x = (px - 20).clamp(0, 19);
            let y = (py - 20).clamp(0, 19);
            patch.get_pixel(x, y)
        };
        assert_eq!(
            in_patch(30, 30)[0] < 128,
            true,
            "the white box center must be rewritten towards the black background"
        );
        assert_eq!(
            in_patch(22, 22)[0] < 128,
            true,
            "the box edge is reconstructed from the surrounding pixels too"
        );
    }

    #[test]
    fn telea_inpaint_crop_masks_the_whole_rect_without_quads() {
        let mut image = RgbaImage::from_pixel(32, 32, Rgba([10, 20, 30, 255]));
        for y in 0..32 {
            for x in 0..32 {
                if (x / 8 + y / 8) % 2 == 0 {
                    image.put_pixel(x, y, Rgba([200, 200, 200, 255]));
                }
            }
        }
        let patches = telea_inpaint_crop(&image, [0.0, 0.0, 32.0, 32.0], &[], 3).unwrap();
        assert_eq!(patches.len(), 1, "an empty quad list returns one whole-rect patch");
        assert_eq!(patches[0].1, [0.0, 0.0, 32.0, 32.0]);
        assert!(patches[0].2.is_none(), "empty quads should have no quad");
    }

    #[test]
    fn crop_spec_clamps_and_rounds_to_full_pixels() {
        assert_eq!(crop_spec([10.4, 20.7, 30.2, 40.3], 100, 100), [10, 20, 21, 21]);
        assert_eq!(crop_spec([-5.0, -5.0, 5.0, 5.0], 100, 100), [0, 0, 5, 5]);
        assert_eq!(crop_spec([95.0, 95.0, 500.0, 500.0], 100, 100), [95, 95, 5, 5]);
        let tiny = crop_spec([10.0, 10.0, 10.4, 10.6], 100, 100);
        assert_eq!(tiny[2], 1);
        assert_eq!(tiny[3], 1);
    }

    #[test]
    fn window_dim_pads_small_areas_centered() {
        assert_eq!(window_dim(200, 100.0, 512), (0, 200, 156));
        assert_eq!(window_dim(512, 256.0, 512), (0, 512, 0));
        assert_eq!(window_dim(3, 1.5, 512), (0, 3, 254));
    }

    #[test]
    fn window_dim_follows_the_mask_center_without_padding() {
        assert_eq!(window_dim(1024, 400.0, 512), (144, 512, 0));
        assert_eq!(window_dim(1024, 10.0, 512), (0, 512, 0), "clamped at the left edge");
        assert_eq!(window_dim(1024, 1010.0, 512), (512, 512, 0), "clamped at the right edge");
    }

    #[test]
    fn view_window_centers_on_the_combined_mask_boxes() {
        let quads = [quad([[10.0, 10.0], [30.0, 10.0], [30.0, 20.0], [10.0, 20.0]])];
        let small = view_window(100, 100, &quads, [0.0, 0.0]);
        assert_eq!(small, (0, 0, 100, 100, 206, 206), "small area is centered + padded");
        let mid = [quad([[1000.0, 1000.0], [1100.0, 1000.0], [1100.0, 1100.0], [1000.0, 1100.0]])];
        let big = view_window(2048, 2048, &mid, [0.0, 0.0]);
        assert_eq!(big, (794, 794, 512, 512, 0, 0), "window starts at the box center minus 256");
    }

    #[test]
    fn view_window_masks_the_whole_area_center_when_no_boxes() {
        let (sx, sy, w, h, dx, dy) = view_window(600, 300, &[], [0.0, 0.0]);
        assert_eq!(sx, (600 - 512) / 2, "wide area: sides are cut evenly around the center");
        assert_eq!(sy, 0, "narrow area: no cut");
        assert_eq!((w, h), (512, 300));
        assert_eq!(dx, 0);
        assert_eq!(dy, (512 - 300) / 2, "narrow area is white-padded top and bottom");
    }

    #[test]
    fn extract_window_maps_the_model_region_back_onto_the_crop() {
        let (sx, sy, sw, sh, dx, dy) = (0i64, 0i64, 100, 100, 206, 206);
        let mut output: ArrayD<f32> = Array4::<f32>::zeros((1, 3, 512, 512)).into_dyn();
        let v = output.as_slice_mut().unwrap();
        let stride = 512 * 512;
        let set = |v: &mut [f32], c: usize, y: usize, x: usize, val: f32| {
            v[c * stride + y * 512 + x] = val;
        };
        set(v, 0, 206, 206, 0.5);
        set(v, 1, 206, 206, 1.0);
        set(v, 2, 206, 206, 0.25);
        output = Array4::from_shape_vec((1, 3, 512, 512), v.to_vec())
            .unwrap()
            .into_dyn();
        let fitted = extract_window(&output, 100, 100, sx, sy, sw, sh, dx, dy);
        assert_eq!(fitted[(0, 0)], Rgb([128, 255, 64]));
        assert_eq!(fitted[(1, 1)], Rgb([0, 0, 0]), "the rest of the copied region is read 1:1");
    }

    #[test]
    fn extract_window_offsets_a_large_area_window() {
        let (sx, sy, sw, sh, dx, dy) = (144i64, 0i64, 512, 512, 0, 0);
        let mut output: ArrayD<f32> = Array4::<f32>::zeros((1, 3, 512, 512)).into_dyn();
        let v = output.as_slice_mut().unwrap();
        let stride = 512 * 512;
        let set = |v: &mut [f32], c: usize, y: usize, x: usize, val: f32| {
            v[c * stride + y * 512 + x] = val;
        };
        set(v, 0, 0, 0, 0.5);
        output = Array4::from_shape_vec((1, 3, 512, 512), v.to_vec())
            .unwrap()
            .into_dyn();
        let fitted = extract_window(&output, 1024, 512, sx, sy, sw, sh, dx, dy);
        assert_eq!(fitted[(144, 0)], Rgb([128, 0, 0]));
        assert_eq!(fitted[(143, 0)], Rgb([255, 255, 255]), "cut-off sides stay white");
    }

    #[test]
    fn mask_fills_quads_white_and_leaves_the_rest_black() {
        let origin = [0.0, 0.0];
        let mask = build_mask(
            100,
            100,
            &[quad([[10.0, 10.0], [30.0, 10.0], [30.0, 20.0], [10.0, 20.0]])],
            origin,
        );
        assert_eq!(mask[(20, 15)][0], 255, "inside the box must be masked");
        assert_eq!(mask[(5, 15)][0], 0, "left of the box must be kept");
        assert_eq!(mask[(20, 5)][0], 0, "above the box must be kept");
    }

    #[test]
    fn mask_fills_multiple_quads_into_one_union() {
        let origin = [0.0, 0.0];
        let mask = build_mask(
            100,
            100,
            &[
                quad([[10.0, 10.0], [20.0, 10.0], [20.0, 20.0], [10.0, 20.0]]),
                quad([[40.0, 40.0], [60.0, 40.0], [60.0, 50.0], [40.0, 50.0]]),
            ],
            origin,
        );
        assert_eq!(mask[(15, 15)][0], 255);
        assert_eq!(mask[(50, 45)][0], 255);
        assert_eq!(mask[(30, 30)][0], 0, "the gap between the boxes stays black");
    }

    #[test]
    fn mask_ignores_quads_outside_the_crop_without_panicking() {
        let origin = [50.0, 50.0];
        let mask = build_mask(
            10,
            10,
            &[quad([[-100.0, -100.0], [-90.0, -100.0], [-90.0, -90.0], [-100.0, -90.0]])],
            origin,
        );
        assert_eq!(mask[(0, 0)][0], 0);
    }

    #[test]
    fn empty_quad_list_masks_the_whole_crop() {
        let origin = [0.0, 0.0];
        let mask = build_mask(8, 8, &[], origin);
        assert_eq!(mask[(0, 0)][0], 255);
        assert_eq!(mask[(7, 7)][0], 255);
    }

    #[test]
    fn compose_inputs_packs_4ch_masked_rgb_plus_mask() {
        let mut canvas = RgbImage::from_pixel(MODEL_EDGE, MODEL_EDGE, Rgb([0, 128, 255]));
        canvas[(7, 3)] = Rgb([255, 0, 0]);
        let mut mask = GrayImage::from_pixel(MODEL_EDGE, MODEL_EDGE, image::Luma([0]));
        mask[(7, 3)] = image::Luma([255]);
        let input = compose_inputs(&canvas, &mask);
        assert_eq!(input.shape(), &[1, 4, MODEL_EDGE as usize, MODEL_EDGE as usize]);
        // masked pixel (7,3) has mask=1 -> rgb zeroed, mask channel 1
        assert!(input[[0, 0, 3, 7]].abs() < 1e-6);
        assert!(input[[0, 1, 3, 7]].abs() < 1e-6);
        assert!(input[[0, 2, 3, 7]].abs() < 1e-6);
        assert!((input[[0, 3, 3, 7]] - 1.0).abs() < 1e-6);
        // unmasked pixel (0,0): canvas [0,128,255], mask 0 -> rgb preserved, mask 0
        assert!(input[[0, 0, 0, 0]].abs() < 1e-6);
        assert!((input[[0, 1, 0, 0]] - 128.0 / 255.0).abs() < 1e-6);
        assert!((input[[0, 2, 0, 0]] - 1.0).abs() < 1e-6);
        assert!(input[[0, 3, 0, 0]].abs() < 1e-6);
    }

    #[test]
    fn bbox_crops_returns_the_whole_area_without_quads() {
        let patch = RgbaImage::from_pixel(8, 6, Rgba([1, 2, 3, 255]));
        let crops = bbox_crops(patch.clone(), [10.0, 20.0], &[]);
        assert_eq!(crops.len(), 1);
        assert_eq!(crops[0].1, [10.0, 20.0, 8.0, 6.0]);
        assert_eq!(crops[0].0.dimensions(), (8, 6));
        assert!(crops[0].2.is_none());
    }

    #[test]
    fn bbox_crops_splits_into_per_box_patches() {
        let patch = RgbaImage::from_pixel(100, 50, Rgba([9, 9, 9, 255]));
        let quads = [
            quad([[20.0, 10.0], [80.0, 10.0], [80.0, 40.0], [20.0, 40.0]]),
            quad([[10.0, 30.0], [30.0, 30.0], [30.0, 45.0], [10.0, 45.0]]),
        ];
        let crops = bbox_crops(patch.clone(), [0.0, 0.0], &quads);
        assert_eq!(crops.len(), 2);
        assert_eq!(crops[0].1, [20.0, 10.0, 60.0, 30.0]);
        assert_eq!(crops[0].0.dimensions(), (60, 30));
        assert_eq!(crops[1].1, [10.0, 30.0, 20.0, 15.0]);
        assert_eq!(crops[1].0.dimensions(), (20, 15));
        assert!(crops[0].2.is_some());
        assert!(crops[1].2.is_some());
        // axis-aligned quads should remain fully opaque
        assert!(crops[0].0.pixels().all(|p| p[3] == 255));
    }

    #[test]
    fn bbox_crops_clips_partial_boxes_and_skips_outside_ones() {
        let patch = RgbaImage::from_pixel(100, 50, Rgba([9, 9, 9, 255]));
        let quads = [
            quad([[95.0, 30.0], [120.0, 30.0], [120.0, 60.0], [95.0, 60.0]]),
            quad([[150.0, 150.0], [160.0, 150.0], [160.0, 160.0], [150.0, 160.0]]),
        ];
        let crops = bbox_crops(patch, [0.0, 0.0], &quads);
        assert_eq!(crops.len(), 1, "the outside box must be dropped");
        assert_eq!(crops[0].1, [95.0, 30.0, 5.0, 20.0]);
        assert_eq!(crops[0].0.dimensions(), (5, 20));
    }

    #[test]
    fn bbox_crops_makes_outside_rotated_quad_transparent() {
        let patch = RgbaImage::from_pixel(100, 100, Rgba([9, 9, 9, 255]));
        // diamond rotated 45° centered at 50,50
        let quad = quad([[50.0, 20.0], [80.0, 50.0], [50.0, 80.0], [20.0, 50.0]]);
        let crops = bbox_crops(patch, [0.0, 0.0], &[quad]);
        assert_eq!(crops.len(), 1);
        assert_eq!(crops[0].1, [20.0, 20.0, 60.0, 60.0]);
        assert_eq!(crops[0].0.dimensions(), (60, 60));
        assert!(crops[0].2.is_some());
        // corners of AABB should be transparent
        assert_eq!(crops[0].0.get_pixel(0, 0)[3], 0, "top-left corner outside diamond must be transparent");
        assert_eq!(crops[0].0.get_pixel(59, 0)[3], 0);
        assert_eq!(crops[0].0.get_pixel(0, 59)[3], 0);
        assert_eq!(crops[0].0.get_pixel(59, 59)[3], 0);
        // center should be opaque
        assert_eq!(crops[0].0.get_pixel(30, 30)[3], 255);
    }

    #[test]
    fn bbox_crops_handles_skewed_quad_transparency() {
        // skewed quad like Fig2: slanted
        let patch = RgbaImage::from_pixel(200, 100, Rgba([9, 9, 9, 255]));
        let quad = quad([[10.0, 20.0], [180.0, 0.0], [190.0, 30.0], [20.0, 50.0]]);
        let crops = bbox_crops(patch, [0.0, 0.0], &[quad]);
        assert_eq!(crops.len(), 1);
        // top-left corner of AABB should be transparent for slanted quad
        let (img, _, _) = &crops[0];
        assert_eq!(img.get_pixel(0, 0)[3], 0);
        assert_eq!(img.get_pixel(img.width() - 1, 0)[3], 0);
    }

    #[test]
    fn reflect_index_mirrors_beyond_both_edges_with_edge_repetition() {
        assert_eq!(reflect_index(0, 5), 0);
        assert_eq!(reflect_index(4, 5), 4);
        assert_eq!(reflect_index(5, 5), 4, "the edge repeats at the seam");
        assert_eq!(reflect_index(8, 5), 1);
        assert_eq!(reflect_index(9, 5), 0);
        assert_eq!(reflect_index(10, 5), 0, "full periods wrap");
        assert_eq!(reflect_index(-1, 5), 0, "negative offsets reflect too");
        assert_eq!(reflect_index(-5, 5), 4);
        assert_eq!(reflect_index(-9, 5), 1);
    }

    #[test]
    fn reflect_place_rgb_pads_the_canvas_with_mirrored_edges() {
        let mut region = RgbImage::new(2, 2);
        region[(0, 0)] = Rgb([1, 2, 3]);
        region[(1, 0)] = Rgb([4, 5, 6]);
        region[(0, 1)] = Rgb([7, 8, 9]);
        region[(1, 1)] = Rgb([10, 11, 12]);
        let mut canvas = RgbImage::new(4, 4);
        reflect_place_rgb(&mut canvas, &region, 1, 1);
        assert_eq!(canvas[(1, 1)], Rgb([1, 2, 3]), "region pixels copy through");
        assert_eq!(canvas[(2, 1)], Rgb([4, 5, 6]));
        assert_eq!(canvas[(1, 0)], Rgb([1, 2, 3]), "top padding mirrors the edge row");
        assert_eq!(canvas[(0, 0)], Rgb([1, 2, 3]), "corners reflect both edges");
        assert_eq!(canvas[(0, 3)], Rgb([7, 8, 9]), "bottom padding mirrors the bottom row");
        assert_eq!(canvas[(3, 3)], Rgb([10, 11, 12]));
    }

    #[test]
    fn reflect_place_gray_reflects_the_mask_border() {
        let mut region = GrayImage::new(2, 2);
        region[(0, 0)] = image::Luma([255]);
        region[(1, 0)] = image::Luma([0]);
        region[(0, 1)] = image::Luma([0]);
        region[(1, 1)] = image::Luma([255]);
        let mut canvas = GrayImage::new(3, 3);
        reflect_place_gray(&mut canvas, &region, 0, 0);
        assert_eq!(canvas[(0, 0)][0], 255);
        assert_eq!(canvas[(1, 0)][0], 0);
        assert_eq!(canvas[(2, 0)][0], 0, "the edge column repeats at the seam");
        assert_eq!(canvas[(0, 2)][0], 0);
        assert_eq!(canvas[(2, 2)][0], 255);
    }

    #[test]
    fn aot_next_multiple_pads_to_8() {
        assert_eq!(next_multiple(0, 8), 0);
        assert_eq!(next_multiple(1, 8), 8);
        assert_eq!(next_multiple(8, 8), 8);
        assert_eq!(next_multiple(9, 8), 16);
        assert_eq!(next_multiple(512, 8), 512);
        assert_eq!(next_multiple(513, 8), 520);
    }

    #[test]
    fn aot_infer_dims_respects_max_and_pad() {
        assert_eq!(aot_infer_dims(100, 100, 8, Some(1024)), (104, 104));
        assert_eq!(aot_infer_dims(512, 512, 8, Some(1024)), (512, 512));
        assert_eq!(aot_infer_dims(2000, 1000, 8, Some(1024)), (1024, 512));
        // 2000*0.512=1024 -> 1024 pad 8 => 1024, 1000*0.512=512 -> 512
        assert_eq!(aot_infer_dims(2048, 2048, 8, Some(1024)), (1024, 1024));
        assert_eq!(aot_infer_dims(100, 200, 8, None), (104, 200));
    }
}
