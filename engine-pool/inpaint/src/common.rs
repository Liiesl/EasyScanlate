//! Shared crop/mask/compose helpers for all inpainting backends.
//!
//! Extracted verbatim from `lib.rs`: every backend (Telea, LaMa, AOT-GAN,
//! ShiftMap, Harmonic) expands the selected mask `rect` by a context pad,
//! builds a mask over the expanded crop, runs its fill, then splits the
//! result into per-mask-box crops via [`bbox_crops`]. This module owns that
//! shared plumbing so the backend modules only contain their own fill logic.

use image::{GrayImage, RgbImage, RgbaImage};
use imageproc::drawing::draw_polygon_mut;

use easyscanlate_model::Quad;

/// One inpainted patch: RGBA crop, `[x, y, w, h]` bounds in image pixels,
/// and the source quad (`None` for whole-rect case).
pub type InpaintPatch = (RgbaImage, [f32; 4], Option<Quad>);
/// Result of inpainting one rect: one [`InpaintPatch`] per mask box.
pub type InpaintResult = Result<Vec<InpaintPatch>, String>;

/// Context pad for the ShiftMap backend: real-pixel expansion around the
/// selection (same value as the ONNX backends; the MRF stage additionally
/// caps its long edge at `shiftmap::MAX_EDGE`).
pub(crate) const SHIFTMAP_CONTEXT_PAD: f32 = 32.0;

/// The clamped integer crop rect `[x, y, w, h]` for a float rect in image
/// pixels. At least 1 pixel in both dimensions and fully inside the image.
pub(crate) fn crop_spec(rect: [f32; 4], width: u32, height: u32) -> [u32; 4] {
    let [x0, y0, x1, y1] = rect;
    let x = x0.floor().clamp(0.0, width as f32 - 1.0) as u32;
    let y = y0.floor().clamp(0.0, height as f32 - 1.0) as u32;
    let x1 = x1.ceil().clamp(x as f32 + 1.0, width as f32);
    let y1 = y1.ceil().clamp(y as f32 + 1.0, height as f32);
    [x, y, (x1 - x as f32) as u32, (y1 - y as f32) as u32]
}

/// The white/black mask of a crop: white where any quad overlaps it, black
/// elsewhere. An empty quad list masks the whole crop (nothing was detected,
/// so the user's selection itself is what gets cleaned).
pub(crate) fn build_mask(width: u32, height: u32, quads: &[Quad], origin: [f32; 2]) -> GrayImage {
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
pub(crate) fn build_mask_expanded(
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

/// Helper: set alpha=0 for pixels outside `quad` inside `crop` (which is at `crop_origin` in image coords).
pub(crate) fn apply_quad_alpha_mask(crop: &mut RgbaImage, quad: &Quad, crop_origin: [f32; 2]) {
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
pub(crate) fn bbox_crops(patch: RgbaImage, origin: [f32; 2], quads: &[Quad]) -> Vec<InpaintPatch> {
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

/// Runs one diffusion/MRF crop through `fill`: expands `rect` by `pad`,
/// builds the expanded mask, converts to RGB, fills, converts back to RGBA
/// (alpha copied from the source), then returns whole-rect or per-quad
/// crops exactly like the Telea path.
pub(crate) fn diffusion_inpaint_crop(
    log_tag: &str,
    image: &RgbaImage,
    rect: [f32; 4],
    quads: &[Quad],
    pad: f32,
    fill: impl FnOnce(&RgbImage, &GrayImage) -> RgbImage,
) -> InpaintResult {
    let [rx, ry, rw, rh] = rect;
    let [ex, ey, exp_w, exp_h] = crop_spec(
        [rx - pad, ry - pad, rx + rw + pad, ry + rh + pad],
        image.width(),
        image.height(),
    );
    let exp_origin = [ex as f32, ey as f32];
    let mask = build_mask_expanded(exp_w, exp_h, quads, rect, exp_origin, image.width(), image.height());
    eprintln!(
        "[inpaint::{log_tag}] rect={:?} quads={} pad={} image={}x{} exp=[{},{},{},{}] mask_sum={}",
        rect,
        quads.len(),
        pad,
        image.width(),
        image.height(),
        ex,
        ey,
        exp_w,
        exp_h,
        mask.pixels().map(|p| p[0] as u32).sum::<u32>()
    );
    let crop: RgbaImage = image::imageops::crop_imm(image, ex, ey, exp_w, exp_h).to_image();
    let rgb_crop = image::DynamicImage::ImageRgba8(crop.clone()).to_rgb8();
    let filled_rgb = fill(&rgb_crop, &mask);
    let mut filled: RgbaImage = image::DynamicImage::ImageRgb8(filled_rgb).into_rgba8();
    for (px, src) in filled.pixels_mut().zip(crop.pixels()) {
        px[3] = src[3];
    }
    if quads.is_empty() {
        let [ox, oy, ow, oh] = crop_spec([rx, ry, rx + rw, ry + rh], image.width(), image.height());
        let sub = image::imageops::crop_imm(&filled, ox - ex, oy - ey, ow, oh).to_image();
        return Ok(vec![(sub, [ox as f32, oy as f32, ow as f32, oh as f32], None)]);
    }
    let out = bbox_crops(filled, exp_origin, quads);
    eprintln!("[inpaint::{log_tag}] quads={} -> {} bbox crops", quads.len(), out.len());
    Ok(out)
}

/// Inpaints `rect` of `image` with the ShiftMap backend (He & Sun 2012 +
/// Poisson blend): best for textured regions (screentone, scenery). The
/// crop is `rect` expanded by [`SHIFTMAP_CONTEXT_PAD`] (the MRF stage caps
/// its long edge at `shiftmap::MAX_EDGE`, giant masks fall back to
/// harmonic diffusion). Same per-mask-box crop contract as the Telea path.
pub fn shiftmap_inpaint_crop(
    image: &RgbaImage,
    rect: [f32; 4],
    quads: &[Quad],
) -> InpaintResult {
    diffusion_inpaint_crop("shiftmap", image, rect, quads, SHIFTMAP_CONTEXT_PAD, |rgb, mask| {
        crate::shiftmap::shiftmap_inpaint_rgb(rgb, mask)
    })
}

/// Inpaints `rect` of `image` with the Harmonic (Laplace) backend: solves
/// `Δf = 0` inside the mask with Dirichlet boundary. Best for texture-free
/// regions (speech bubbles, smooth gradients). Context pad is the Telea
/// `radius`, like the Telea path. Same per-mask-box crop contract.
pub fn harmonic_inpaint_crop(
    image: &RgbaImage,
    rect: [f32; 4],
    quads: &[Quad],
    radius: i32,
) -> InpaintResult {
    let pad = radius.max(1) as f32;
    diffusion_inpaint_crop("harmonic", image, rect, quads, pad, |rgb, mask| {
        crate::harmonic::harmonic_inpaint_rgb(rgb, mask)
    })
}

/// Manual multi oversized: square crop per spec Q3.
/// Larger side decides full image dim, square centered on selection.
/// If selection is full width/height, expands other direction to make square of that full side (600x135 full width on 600x1600 -> 600x600).
/// Side is clamped to `min(img_w, img_h)` to fit without excessive mirror, otherwise mirror pad will be used.
/// Returns `(side, sx, sy)` where `side` is square side and `sx,sy` is top-left in image.
pub fn manual_square_params(sel_x0: u32, sel_y0: u32, sel_w: u32, sel_h: u32, img_w: u32, img_h: u32) -> (u32, u32, u32) {
    let larger_is_w = sel_w >= sel_h;
    let side_full = if larger_is_w { img_w } else { img_h };
    let mut side = side_full;
    let min_dim = img_w.min(img_h);
    if side > min_dim {
        side = min_dim;
    }
    // If side still smaller than selection's larger side (e.g., 400x700 on 600x1600 -> side 600 <700), expand to selection's larger side and allow mirror.
    let sel_larger = sel_w.max(sel_h);
    if side < sel_larger {
        side = sel_larger;
    }
    let cx = sel_x0 as f32 + sel_w as f32 * 0.5;
    let cy = sel_y0 as f32 + sel_h as f32 * 0.5;
    let mut sx = (cx - side as f32 * 0.5).round() as i32;
    let mut sy = (cy - side as f32 * 0.5).round() as i32;
    // Clamp, but if side > img dim, keep 0 (mirror will pad)
    if side <= img_w {
        sx = sx.clamp(0, img_w as i32 - side as i32).max(0);
    } else {
        sx = 0;
    }
    if side <= img_h {
        sy = sy.clamp(0, img_h as i32 - side as i32).max(0);
    } else {
        sy = 0;
    }
    (side, sx as u32, sy as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use easyscanlate_model::Quad;
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
    fn manual_square_full_width_expands_to_square() {
        // Q3: 600x1600 image, selection 600x135 full width at (0,700) -> square 600x600
        let (side, sx, sy) = manual_square_params(0, 700, 600, 135, 600, 1600);
        assert_eq!(side, 600, "full width 600 -> square side 600");
        assert_eq!(sx, 0, "full width -> sx 0");
        // center cy = 700+67.5=767.5, side 600 => sy 467.5 round 468, clamped to 0..1000
        assert_eq!(sy, 468);
        // Selection at top edge should clamp sy 0
        let (side2, _sx2, sy2) = manual_square_params(0, 0, 600, 135, 600, 1600);
        assert_eq!(side2, 600);
        assert_eq!(sy2, 0, "top edge selection clamped to 0");
        // Selection at bottom edge
        let (side3, _, sy3) = manual_square_params(0, 1465, 600, 135, 600, 1600);
        assert_eq!(sy3, 1000, "bottom edge clamped to 1600-600=1000");
    }

    #[test]
    fn manual_square_oversized_larger_side_takes_full_dim() {
        // 690x1600 image, selection 400x550 (h larger) -> full dim based on h is 1600 -> side min 690 -> side 690
        let (side, _, _) = manual_square_params(100, 500, 400, 550, 690, 1600);
        assert_eq!(side, 690, "larger is h, full h 1600 -> clamped to min 690");
        // 1000x1000 square image, selection 600x400 larger w 600 -> full w 1000 -> side 1000
        let (side2, sx2, sy2) = manual_square_params(200, 300, 600, 400, 1000, 1000);
        assert_eq!(side2, 1000);
        assert_eq!(sx2, 0, "centered square 1000 on 1000x1000 -> 0");
        assert_eq!(sy2, 0);
        // Small image 300x300, selection 100x80 -> side full w 300 -> side 300
        let (side3, _, _) = manual_square_params(50, 50, 100, 80, 300, 300);
        assert_eq!(side3, 300);
    }
}
