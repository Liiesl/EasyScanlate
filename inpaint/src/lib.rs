//! CPU-only image inpainting with the LaMa ONNX model (`lama-manga.onnx`).
//!
//! [`Engine`] holds one shared inference session (built for the CPU
//! execution provider by design) and inpaints image regions one at a time:
//! the caller supplies an image path, a rectangle and the text-box quads
//! inside it; the box pixels are masked out (black/white mask) and
//! reconstructed by LaMa, and each box comes back as its own RGBA crop an
//! app layers over the original image without writing anything to disk.

use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use image::{GrayImage, Rgb, RgbImage, RgbaImage};
use imageproc::drawing::draw_polygon_mut;
use ndarray::{Array4, ArrayD};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::TensorRef;
use scanlateit_model::Quad;

/// The fixed square input size of the LaMa model.
pub const MODEL_EDGE: u32 = 512;

const MODEL_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../models");
const MODEL_FILE: &str = "lama-manga.onnx";

/// Cloneable handle to the shared inpainting session. Only one inference
/// runs at a time; runs are serialized through the inner mutex.
#[derive(Clone)]
pub struct Engine(Arc<Mutex<Session>>);

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Engine")
    }
}

impl Engine {
    /// Loads the LaMa model with a CPU-only session.
    pub fn build() -> Result<Self, String> {
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
            .map_err(|e| format!("failed to load inpainting model {}: {e}", path.display()))?;
        Ok(Self(Arc::new(Mutex::new(session))))
    }

    /// Decodes `path` and inpaints the area `rect`, masking out the given
    /// quads (or the whole area when there are none). The original file is
    /// never modified; each returned patch is the RGBA crop of one mask quad
    /// (`[x, y, w, h]` in image pixels), with the text reconstructed by LaMa.
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
        let mut session = self
            .0
            .lock()
            .map_err(|e| format!("Inpaint engine lock poisoned: {e}"))?;
        inpaint_crop(&mut session, &image, rect, quads)
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
        return RgbImage::from_pixel(crop_w, crop_h, Rgb([255, 255, 255]));
    };
    let to_u8 = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
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

/// Inpaints `rect` of `image` with the given box quads masked out, where
/// `rect` is `[x, y, w, h]` in image pixels. Returns one RGBA crop per mask
/// box (or a single crop of the whole rect when `quads` is empty), with the
/// alpha channel copied from the original pixels (transparency survives).
pub fn inpaint_crop(
    session: &mut Session,
    image: &RgbaImage,
    rect: [f32; 4],
    quads: &[Quad],
) -> Result<Vec<(RgbaImage, [f32; 4])>, String> {
    let [rx, ry, rw, rh] = rect;
    let [x, y, crop_w, crop_h] =
        crop_spec([rx, ry, rx + rw, ry + rh], image.width(), image.height());
    let origin = [x as f32, y as f32];

    let mask = build_mask(crop_w, crop_h, quads, origin);
    let crop = image::imageops::crop_imm(image, x, y, crop_w, crop_h).to_image();

    let (sx, sy, sw, sh, dx, dy) = view_window(crop_w, crop_h, quads, origin);
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

    let (image, mask) = compose_inputs(&canvas, &canvas_mask);
    let output = run_session(session, image, mask)?;
    let rgb = extract_window(&output, crop_w, crop_h, sx, sy, sw, sh, dx, dy);

    let mut patch: RgbaImage = image::DynamicImage::ImageRgb8(rgb).into_rgba8();
    for (px, src) in patch.pixels_mut().zip(crop.pixels()) {
        px[3] = src[3];
    }
    Ok(bbox_crops(patch, origin, quads))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn quad(points: [[f32; 2]; 4]) -> Quad {
        Quad { points }
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
