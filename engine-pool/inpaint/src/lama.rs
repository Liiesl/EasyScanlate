//! LaMa ONNX backend (`lama-manga_int8.onnx`, fixed 512).
//!
//! Pure-Rust geometry (window/mirror helpers) is always compiled; everything
//! touching `ort`/`ndarray` is gated behind the crate's `onnx` feature so
//! the crate builds without ORT for fast pure-Rust testing.

// Without `onnx` the pure geometry helpers below are only exercised by tests.
#![cfg_attr(not(feature = "onnx"), allow(dead_code))]

#[cfg(feature = "onnx")]
use std::path::Path;

use image::{GrayImage, RgbImage};
#[cfg(any(test, feature = "onnx"))]
use image::Rgb;
#[cfg(feature = "onnx")]
use image::RgbaImage;

use easyscanlate_model::Quad;

#[cfg(feature = "onnx")]
use ndarray::{Array4, ArrayD};
#[cfg(feature = "onnx")]
use ort::session::builder::GraphOptimizationLevel;
#[cfg(feature = "onnx")]
use ort::session::Session;
#[cfg(feature = "onnx")]
use ort::value::TensorRef;

#[cfg(feature = "onnx")]
use crate::common::{bbox_crops, build_mask_expanded, crop_spec, InpaintResult};

/// The fixed square input size of the LaMa model.
pub const MODEL_EDGE: u32 = 512;

/// Real image pixels of surrounding context added around the selected mask
/// for the LaMa backend before the mirror padding to `MODEL_EDGE`.
/// Telea's context pad is the `radius` setting itself.
pub(crate) const LAMA_CONTEXT_PAD: f32 = 32.0;

pub(crate) const MODEL_FILE: &str = "lama-manga_int8.onnx";

/// Builds the shared LaMa ONNX session: DirectML on Windows (feature
/// `directml`) with CPU fallback.
#[cfg(feature = "onnx")]
pub(crate) fn build_session() -> Result<Session, String> {
    let path = easyscanlate_settings::resolve_model_path(MODEL_FILE);
    // Helper to build a CPU session for a given model path.
    let build_cpu = |path: &Path, _label: &str| -> Result<Session, String> {
        Session::builder()
            .map_err(|e| format!("ORT init failed: {e}"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| format!("ORT init failed: {e}"))?
            .with_intra_threads(4)
            .map_err(|e| format!("ORT init failed: {e}"))?
            .with_execution_providers([ort::ep::CPU::default().build()])
            .map_err(|e| format!("ORT init failed: {e}"))?
            .commit_from_file(path)
            .map_err(|e| {
                format!("failed to load inpainting model {}: {e}", path.display())
            })
    };
    #[cfg(all(feature = "directml", target_os = "windows"))]
    let build_directml = |path: &Path, _label: &str| -> Result<Session, String> {
        Session::builder()
            .map_err(|e| format!("ORT init failed: {e}"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| format!("ORT init failed: {e}"))?
            .with_intra_threads(4)
            .map_err(|e| format!("ORT init failed: {e}"))?
            .with_execution_providers([ort::ep::DirectML::default()
                .build()
                .error_on_failure()])
            .map_err(|e| format!("ORT init failed: {e}"))?
            .commit_from_file(path)
            .map_err(|e| {
                format!("failed to load inpainting model {}: {e}", path.display())
            })
    };
    #[cfg(all(feature = "directml", target_os = "windows"))]
    let session = {
        match build_directml(&path, "lama") {
            Ok(s) => {
                eprintln!("[inpaint::lama] DirectML EP active for {}", path.display());
                s
            }
            Err(e) => {
                eprintln!(
                    "[inpaint::lama] DirectML EP init failed for {}: {e} – falling back to CPU",
                    path.display()
                );
                build_cpu(&path, "lama")?
            }
        }
    };
    #[cfg(any(not(feature = "directml"), not(target_os = "windows")))]
    let session = build_cpu(&path, "lama")?;
    Ok(session)
}

/// Runs one LaMa inference on the single `input` [1,4,512,512] tensor
/// (channels 0-2 = masked RGB 0-1 zeroed, channel 3 = mask 0/1).
/// Weight-only INT8 model (`lama-manga_int8.onnx`, per-channel asymmetric UINT8
/// + DequantizeLinear axis=0) keeps compute in FP32.
#[cfg(feature = "onnx")]
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

/// One dimension of [`view_window`]'s window: `(src_off, copy_len,
/// dst_off)`. Areas that fit in `edge` pixels are copied whole and placed
/// centered on the canvas; larger areas are not scaled down - a full
/// `edge`-pixel slice starting as close to the mask center as possible
/// (and clamped to the area) is copied instead.
pub(crate) fn window_dim(crop: i64, center: f32, edge: i64) -> (i64, i64, i64) {
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
pub(crate) fn mask_center(crop_w: u32, crop_h: u32, quads: &[Quad], origin: [f32; 2]) -> [f32; 2] {
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
pub(crate) fn view_window(
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
pub(crate) fn reflect_index(x: i64, len: i64) -> i64 {
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
pub(crate) fn reflect_place_rgb(canvas: &mut RgbImage, region: &RgbImage, dst_x: i64, dst_y: i64) {
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
pub(crate) fn reflect_place_gray(canvas: &mut GrayImage, region: &GrayImage, dst_x: i64, dst_y: i64) {
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

/// Composes the single LaMa INT8 input `[1, 4, 512, 512]` f32:
/// channels 0-2 = masked RGB `canvas * (1 - mask)` in 0-1, channel 3 = mask 0/1.
/// The FP32 graph used to do `image*(1-mask)` internally (Sub+Mul+Concat);
/// the 4ch INT8 graph expects the caller to pre-pack.
#[cfg(feature = "onnx")]
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
#[cfg(feature = "onnx")]
#[allow(clippy::too_many_arguments)]
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

/// Inpaints `rect` of `image` with the given box quads masked out, where
/// `rect` is `[x, y, w, h]` in image pixels (the selected mask). The image
/// crop is `rect` expanded by [`LAMA_CONTEXT_PAD`] pixels in every direction
/// (clamped to the image) so the model sees real surrounding context; the
/// remaining canvas area to `MODEL_EDGE` is still mirror-padded via
/// `reflect_place_*` (the padding the LaMa model was trained with).
/// Returns one RGBA crop per mask box (or a single crop of the whole `rect`
/// when `quads` is empty), with the alpha channel copied from the original
/// pixels (transparency survives).
#[cfg(feature = "onnx")]
pub fn inpaint_crop(
    session: &mut Session,
    image: &RgbaImage,
    rect: [f32; 4],
    quads: &[Quad],
) -> InpaintResult {
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
            ox - ex,
            oy - ey,
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

#[cfg(test)]
mod tests {
    use super::*;
    use easyscanlate_model::Quad;

    fn quad(points: [[f32; 2]; 4]) -> Quad {
        Quad { points }
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
    fn manual_canvas_mirror_pad_small_image_needs_reflect() {
        // 300x300 image smaller than 512, any window will need mirror pad
        // Verify that reflect_index correctly mirrors and that manual window would be centered
        let img_w = 300;
        let img_h = 300;
        let w_src = MODEL_EDGE.min(img_w); // 300
        let h_src = MODEL_EDGE.min(img_h); // 300
        assert_eq!(w_src, 300);
        assert_eq!(h_src, 300);
        // view_window for small image should center with pad 106 each side (512-300)/2=106
        let (sx, sy, sw, sh, dx, dy) = view_window(img_w, img_h, &[], [0.0, 0.0]);
        assert_eq!(sw, 300);
        assert_eq!(sh, 300);
        assert_eq!(dx, 106);
        assert_eq!(dy, 106);
        // Verify reflect place fills 512x512 via mirror
        let mut region = RgbImage::new(300, 300);
        for y in 0..300 {
            for x in 0..300 {
                region[(x, y)] = Rgb([x as u8, y as u8, 0]);
            }
        }
        let mut canvas = RgbImage::new(MODEL_EDGE, MODEL_EDGE);
        reflect_place_rgb(&mut canvas, &region, dx, dy);
        // Center pixel should be from region center
        assert_eq!(canvas[(256, 256)], region[(150, 150)]);
        // Corner top-left should be mirrored from region via reflect_index(-106,300)=105
        assert_eq!(canvas[(0, 0)], region[(105, 105)], "top-left mirrors correctly via reflect_index");
    }

    #[cfg(feature = "onnx")]
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

    #[cfg(feature = "onnx")]
    #[test]
    fn extract_window_maps_the_model_region_back_onto_the_crop() {
        use ndarray::{Array4, ArrayD};
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

    #[cfg(feature = "onnx")]
    #[test]
    fn extract_window_offsets_a_large_area_window() {
        use ndarray::{Array4, ArrayD};
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
}
