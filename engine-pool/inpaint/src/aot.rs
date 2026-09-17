//! AOT-GAN ONNX backend (`inpainting_aot.onnx`).
//!
//! Variable resolution: the expanded crop is resized to `AOT_PAD`-aligned,
//! `AOT_MAX_SIZE`-capped dimensions for inference, normalized to `[-1,1]`.
//! Pure helpers (`next_multiple`, `aot_infer_dims`) are always compiled;
//! everything touching `ort`/`ndarray` is gated behind the crate's `onnx`
//! feature so the crate builds without ORT for fast pure-Rust testing.

// Without `onnx` the pure helpers below are only exercised by tests.
#![cfg_attr(not(feature = "onnx"), allow(dead_code))]

#[cfg(feature = "onnx")]
use std::path::Path;

#[cfg(feature = "onnx")]
use image::{Rgb, RgbImage, RgbaImage};

#[cfg(feature = "onnx")]
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

/// Context pad for AOT backend (same real-pixel expansion as LaMa, but
/// AOT uses variable resolution + pad-to-multiple instead of mirror padding).
pub(crate) const AOT_CONTEXT_PAD: f32 = 32.0;

/// AOT pad multiple (model was trained with stride 8).
pub const AOT_PAD: u32 = 8;

/// AOT max side for inference; larger crops are scaled down preserving aspect
/// before padding (mirrors `aot_inference.py: potentially` max_size=1024).
pub const AOT_MAX_SIZE: u32 = 1024;

pub(crate) const MODEL_FILE_AOT: &str = "inpainting_aot.onnx";

/// Builds the shared AOT ONNX session: DirectML on Windows (feature
/// `directml`) with CPU fallback.
#[cfg(feature = "onnx")]
pub(crate) fn build_session() -> Result<Session, String> {
    let path = easyscanlate_settings::resolve_model_path(MODEL_FILE_AOT);
    // Helper to build a CPU session for a given model path.
    let build_cpu = |path: &Path| -> Result<Session, String> {
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
                format!("failed to load AOT inpainting model {}: {e}. Place inpainting_aot.onnx (opset 18, inputs img [B,3,H,W] + mask [B,1,H,W]) from https://huggingface.co/Liiesl/aot-inpainting-onnx/serve/main/inpainting_aot.onnx?download=true.", path.display())
            })
    };
    #[cfg(all(feature = "directml", target_os = "windows"))]
    let build_directml = |path: &Path| -> Result<Session, String> {
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
                format!("failed to load AOT inpainting model {}: {e}. Place inpainting_aot.onnx (opset 18, inputs img [B,3,H,W] + mask [B,1,H,W]) from https://huggingface.co/Liiesl/aot-inpainting-onnx/serve/main/inpainting_aot.onnx?download=true.", path.display())
            })
    };
    #[cfg(all(feature = "directml", target_os = "windows"))]
    let session = {
        match build_directml(&path) {
            Ok(s) => {
                eprintln!("[inpaint::aot] DirectML EP active for {}", path.display());
                s
            }
            Err(e) => {
                eprintln!(
                    "[inpaint::aot] DirectML EP init failed for {}: {e} – falling back to CPU",
                    path.display()
                );
                build_cpu(&path)?
            }
        }
    };
    #[cfg(any(not(feature = "directml"), not(target_os = "windows")))]
    let session = build_cpu(&path)?;
    Ok(session)
}

/// Runs one AOT inference; input names are `img` + `mask` with fallback
/// to `image` + `mask` (both seen in exported variants).
#[cfg(feature = "onnx")]
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

pub(crate) fn next_multiple(v: u32, pad: u32) -> u32 {
    if v.is_multiple_of(pad) {
        v
    } else {
        v.div_ceil(pad) * pad
    }
}

/// Inference dimensions for AOT: mirrors `aot_inference.py: potentially` `_next_multiple` + `max_size` logic.
pub(crate) fn aot_infer_dims(w: u32, h: u32, pad: u32, max_size: Option<u32>) -> (u32, u32) {
    if let Some(max) = max_size
        && w.max(h) > max {
            let scale = max as f32 / w.max(h) as f32;
            let mut nw = (w as f32 * scale).round() as u32;
            let mut nh = (h as f32 * scale).round() as u32;
            nw = next_multiple(nw.max(1), pad);
            nh = next_multiple(nh.max(1), pad);
            return (nw, nh);
        }
    (next_multiple(w, pad), next_multiple(h, pad))
}

/// Inpaints `rect` with the AOT-GAN ONNX model. Variable resolution:
/// `exp` crop (rect + `AOT_CONTEXT_PAD`) is resized to `AOT_PAD`-aligned
/// `AOT_MAX_SIZE`-capped dimensions for inference, normalized to `[-1,1]`,
/// `img*=(1-mask)`, then blended back. No mirror padding.
#[cfg(feature = "onnx")]
pub fn aot_inpaint_crop(
    session: &mut Session,
    image: &RgbaImage,
    rect: [f32; 4],
    quads: &[Quad],
) -> InpaintResult {
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
            let sub = image::imageops::crop_imm(&crop, ox - ex, oy - ey, ow, oh).to_image();
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
        let sub = image::imageops::crop_imm(&patch, ox - ex, oy - ey, ow, oh).to_image();
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
