//! CPU image inpainting with two interchangeable backends: the pure-Rust
//! Telea algorithm from the [`inpaint`] crate (the default: no model, no
//! download) and the LaMa ONNX model (`lama-manga.onnx`).
//!
//! [`Engine`] owns the backend chosen at build time. The LaMa backend holds
//! one shared inference session (CPU execution provider by design) and
//! inpaints image regions one at a time; the Telea backend is stateless and
//! rewrites masked pixels in place. Both take the same job: an image path, a
//! selected mask rectangle and the text-box quads inside it; the mask
//! (the rectangle itself, or the quad union when quads are present) is
//! reconstructed from surrounding context — `radius` pixels of context for
//! Telea and [`LAMA_CONTEXT_PAD`] pixels of real context plus the existing
//! mirror padding to `MODEL_EDGE` for LaMa — and each box comes back as
//! its own RGBA crop an app layers over the original image without writing
//! anything to disk.

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

const MODEL_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../models");
const MODEL_FILE: &str = "lama-manga.onnx";

/// Cloneable handle to the shared inpainting engine: either the stateless
/// Telea backend or the LaMa session (one inference at a time, serialized
/// through the inner mutex).
#[derive(Clone)]
pub struct Engine {
    backend: InpaintBackend,
    /// Telea's interpolation radius in pixels; ignored by the LaMa backend.
    radius: i32,
    /// The shared LaMa session; `None` for the Telea backend.
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
    /// stateless; the LaMa backend loads its model with a CPU-only session.
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
    /// [`LAMA_CONTEXT_PAD`] plus the existing mirror padding for LaMa.
    /// The original file is never modified; each returned patch is the RGBA
    /// crop of one mask quad (`[x, y, w, h]` in image pixels), with the
    /// text reconstructed by the engine's backend. For an empty quad list
    /// the single returned patch covers the (clamped) `rect` itself.
    pub fn run_blocking(
        &self,
        path: &str,
        rect: [f32; 4],
        quads: &[Quad],
    ) -> Result<Vec<(RgbaImage, [f32; 4])>, String> {
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
        }
    }
}

/// Runs one LaMa inference on the `image` and `mask` inputs.
fn run_session(
    session: &mut Session,
    image: Array4<f32>,
    mask: Array4<f32>,
) -> Result<ArrayD<f32>, String> {
    let outputs = session
        .run(ort::inputs![
            "image" => TensorRef::from_array_view(&image).map_err(|e| format!("{e}"))?,
            "mask" => TensorRef::from_array_view(&mask).map_err(|e| format!("{e}"))?,
        ])
        .map_err(|e| format!("Inpaint inference failed: {e}"))?;
    outputs[0]
        .try_extract_array::<f32>()
        .map_err(|e| format!("Inpaint output extract failed: {e}"))
        .map(|array| array.to_owned())
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
/// whole crop's center when nothing is masked.
fn mask_center(crop_w: u32, crop_h: u32, quads: &[Quad], origin: [f32; 2]) -> [f32; 2] {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for quad in quads {
        let [bx0, by0, bx1, by1] = quad.bounds();
        min_x = min_x.min(bx0);
        min_y = min_y.min(by0);
        max_x = max_x.max(bx1);
        max_y = max_y.max(by1);
    }
    if min_x.is_infinite() {
        return [crop_w as f32 / 2.0, crop_h as f32 / 2.0];
    }
    [
        (min_x + max_x) / 2.0 - origin[0],
        (min_y + max_y) / 2.0 - origin[1],
    ]
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

/// Composes the model inputs: `image` as `[1, 3, 512, 512]` f32 RGB in
/// 0..1 and `mask` as `[1, 1, 512, 512]` f32 (1 = inpaint, 0 = keep).
fn compose_inputs(canvas: &RgbImage, mask: &GrayImage) -> (Array4<f32>, Array4<f32>) {
    debug_assert_eq!(canvas.dimensions(), (MODEL_EDGE, MODEL_EDGE));
    debug_assert_eq!(mask.dimensions(), (MODEL_EDGE, MODEL_EDGE));
    let image = Array4::from_shape_fn(
        (1, 3, MODEL_EDGE as usize, MODEL_EDGE as usize),
        |(_, c, y, x)| canvas[(x as u32, y as u32)][c] as f32 / 255.0,
    );
    let mask = Array4::from_shape_fn(
        (1, 1, MODEL_EDGE as usize, MODEL_EDGE as usize),
        |(_, _, y, x)| mask[(x as u32, y as u32)][0] as f32 / 255.0,
    );
    (image, mask)
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

/// Splits an inpainted area patch into per-mask-box crops: one `(crop,
/// rect)` pair per quad, where `rect` is `[x, y, w, h]` in image pixels and
/// the crop covers the quad's bounding box clipped to the area. Boxes that
/// lie entirely outside the area are skipped; an empty quad list returns the
/// whole area patch (it was fully masked and cleaned).
fn bbox_crops(patch: RgbaImage, origin: [f32; 2], quads: &[Quad]) -> Vec<(RgbaImage, [f32; 4])> {
    if quads.is_empty() {
        let (width, height) = patch.dimensions();
        return vec![(patch, [origin[0], origin[1], width as f32, height as f32])];
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
        let crop = image::imageops::crop_imm(
            &patch,
            (cx0 - ox) as u32,
            (cy0 - oy) as u32,
            w,
            h,
        )
        .to_image();
        crops.push((crop, [cx0 as f32, cy0 as f32, w as f32, h as f32]));
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
) -> Result<Vec<(RgbaImage, [f32; 4])>, String> {
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
        return Ok(vec![(sub, [ox as f32, oy as f32, ow as f32, oh as f32])]);
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
) -> Result<Vec<(RgbaImage, [f32; 4])>, String> {
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
    let (image_tensor, mask_tensor) = compose_inputs(&canvas, &canvas_mask);
    let output = run_session(session, image_tensor, mask_tensor)?;
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
        return Ok(vec![(sub, [ox as f32, oy as f32, ow as f32, oh as f32])]);
    }
    let out = bbox_crops(patch, exp_origin, quads);
    eprintln!("[inpaint::lama] quads={} -> {} bbox crops", quads.len(), out.len());
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
        let (patch, bounds) = &patches[0];
        assert_eq!(bounds, &[20.0, 20.0, 20.0, 20.0]);
        assert_eq!(patch.dimensions(), (20, 20));
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
    fn compose_inputs_split_rgb_and_mask_into_two_tensors() {
        let mut canvas = RgbImage::from_pixel(MODEL_EDGE, MODEL_EDGE, Rgb([0, 128, 255]));
        canvas[(7, 3)] = Rgb([255, 0, 0]);
        let mut mask = GrayImage::from_pixel(MODEL_EDGE, MODEL_EDGE, image::Luma([0]));
        mask[(7, 3)] = image::Luma([255]);
        let (image, mask) = compose_inputs(&canvas, &mask);
        assert_eq!(image.shape(), &[1, 3, MODEL_EDGE as usize, MODEL_EDGE as usize]);
        assert_eq!(mask.shape(), &[1, 1, MODEL_EDGE as usize, MODEL_EDGE as usize]);
        assert!((image[[0, 0, 3, 7]] - 1.0).abs() < 1e-6);
        assert!(image[[0, 1, 3, 7]].abs() < 1e-6);
        assert!(image[[0, 2, 3, 7]].abs() < 1e-6);
        assert!((mask[[0, 0, 3, 7]] - 1.0).abs() < 1e-6);
        assert!(mask[[0, 0, 0, 0]].abs() < 1e-6);
    }

    #[test]
    fn bbox_crops_returns_the_whole_area_without_quads() {
        let patch = RgbaImage::from_pixel(8, 6, Rgba([1, 2, 3, 255]));
        let crops = bbox_crops(patch.clone(), [10.0, 20.0], &[]);
        assert_eq!(crops.len(), 1);
        assert_eq!(crops[0].1, [10.0, 20.0, 8.0, 6.0]);
        assert_eq!(crops[0].0.dimensions(), (8, 6));
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
}
