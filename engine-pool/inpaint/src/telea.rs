//! Telea backend — pure-Rust algorithm from the [`inpaint`] crate.
//!
//! The default backend: no model, no download, works instantly on CPU.
//! The mask is `rect` itself when `quads` is empty, otherwise the union of
//! `quads` inside `rect`; the algorithm samples from the surrounding context
//! expanded by `radius` pixels around `rect` (clamped to the image).

use image::RgbaImage;

use easyscanlate_model::Quad;

use crate::common::{bbox_crops, build_mask_expanded, crop_spec, InpaintResult};

/// Inpaints `rect` of `image` with the Telea algorithm from the [`inpaint`]
/// crate: the mask is `rect` itself when `quads` is empty, otherwise the
/// union of `quads` inside `rect`; the algorithm samples from the
/// surrounding context expanded by `radius` pixels around `rect`
/// (clamped to the image). Returns per-mask-box crops with the alpha
/// channel interpolated like every other channel (opaque manga pages keep
/// alpha at 255).
pub fn telea_inpaint_crop(
    image: &RgbaImage,
    rect: [f32; 4],
    quads: &[Quad],
    radius: i32,
) -> InpaintResult {
    use inpaint::prelude::*;
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
            ox - ex,
            oy - ey,
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
}
