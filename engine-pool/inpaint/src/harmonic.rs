//! Harmonic (Laplace) inpainting — pure-Rust port of `harmonic_algo.py`.
//!
//! Solves `Δf = 0` inside the mask with Dirichlet boundary pinned to known
//! pixels. Best for texture-free regions (speech bubbles, skies, smooth
//! shading gradients); for textured regions prefer ShiftMap.
//!
//! Matrix `A` is `-Δ_h` (positive-definite): diagonal = `deg` = number of
//! in-image neighbours (2..=4), off-diagonal `-1` for in-hole neighbours.
//! Out-of-image neighbours are dropped (Neumann), never pinned.
//! `rhs[k]` = sum of known (non-hole, in-image) neighbour pixel values.
//! Solved per channel (`f64`) via the shared [`crate::cg::cg_solve`].
//! Deterministic, no RNG. Empty mask returns a copy; solver failure keeps
//! the original pixels.

use image::{GrayImage, RgbImage};

/// Fills `mask` (`>127` = hole) of `crop` by harmonic diffusion.
///
/// Returns a new image; `crop` is never modified. Dimension mismatch or an
/// empty mask returns a clone of `crop`.
pub fn harmonic_inpaint_rgb(crop: &RgbImage, mask: &GrayImage) -> RgbImage {
    if crop.dimensions() != mask.dimensions() {
        return crop.clone();
    }
    let (w, h) = crop.dimensions();
    let w = w as usize;
    let h = h as usize;
    let hole = |x: usize, y: usize| mask.get_pixel(x as u32, y as u32)[0] > 127;

    // Linear index of each hole pixel.
    let mut idx = vec![-1i64; w * h];
    let mut ys: Vec<usize> = Vec::new();
    let mut xs: Vec<usize> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            if hole(x, y) {
                idx[y * w + x] = ys.len() as i64;
                ys.push(y);
                xs.push(x);
            }
        }
    }
    let n = ys.len();
    if n == 0 {
        return crop.clone();
    }

    const DIRS: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    let mut deg = vec![0.0f64; n];
    let mut nbrs: Vec<Vec<u32>> = vec![Vec::new(); n];
    let mut rhs = vec![[0.0f64; 3]; n];
    for (k, (&y, &x)) in ys.iter().zip(xs.iter()).enumerate() {
        let mut d = 0u32;
        for (dy, dx) in DIRS {
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny < 0 || nx < 0 || ny >= h as isize || nx >= w as isize {
                continue; // Neumann: drop out-of-image neighbours
            }
            d += 1;
            let j = idx[ny as usize * w + nx as usize];
            if j >= 0 {
                nbrs[k].push(j as u32);
            } else {
                let px = crop.get_pixel(nx as u32, ny as u32);
                rhs[k][0] += px[0] as f64;
                rhs[k][1] += px[1] as f64;
                rhs[k][2] += px[2] as f64;
            }
        }
        deg[k] = d as f64;
    }

    let mut out = crop.clone();
    for c in 0..3 {
        let b: Vec<f64> = rhs.iter().map(|r| r[c]).collect();
        let sol = match crate::cg::cg_solve(&deg, &nbrs, &b) {
            Some(s) => s,
            None => continue, // keep original pixels on solver failure
        };
        for (k, (&y, &x)) in ys.iter().zip(xs.iter()).enumerate() {
            let px = out.get_pixel_mut(x as u32, y as u32);
            px[c] = sol[k].round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// Diffusion fallback used when ShiftMap finds no feasible label: returns
/// the harmonic fill as full RGB.
pub(crate) fn harmonic_fallback_rgb(crop: &RgbImage, mask: &GrayImage) -> RgbImage {
    harmonic_inpaint_rgb(crop, mask)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Luma, Rgb};

    fn mask_with_hole(w: u32, h: u32, x0: u32, y0: u32, x1: u32, y1: u32) -> GrayImage {
        let mut m = GrayImage::from_pixel(w, h, Luma([0]));
        for y in y0..y1 {
            for x in x0..x1 {
                m.put_pixel(x, y, Luma([255]));
            }
        }
        m
    }

    #[test]
    fn empty_mask_returns_copy() {
        let img = RgbImage::from_pixel(8, 8, Rgb([10, 20, 30]));
        let mask = GrayImage::from_pixel(8, 8, Luma([0]));
        assert_eq!(harmonic_inpaint_rgb(&img, &mask), img);
    }

    #[test]
    fn fills_hole_toward_surroundings() {
        // Black page with a white box punched as the hole: the fill must
        // diffuse back toward the black background.
        let mut img = RgbImage::from_pixel(32, 32, Rgb([0, 0, 0]));
        for y in 10..22 {
            for x in 10..22 {
                img.put_pixel(x, y, Rgb([255, 255, 255]));
            }
        }
        let mask = mask_with_hole(32, 32, 10, 10, 22, 22);
        let out = harmonic_inpaint_rgb(&img, &mask);
        // Unmasked pixels untouched.
        assert_eq!(*out.get_pixel(0, 0), Rgb([0, 0, 0]));
        // Hole interior: harmonic of a constant-black boundary is black.
        // (Boundary here is black outside the box; the white box interior
        // is fully masked so no white pins remain.)
        let c = out.get_pixel(16, 16);
        assert!(c[0] < 128 && c[1] < 128 && c[2] < 128, "center must diffuse to dark: {c:?}");
    }

    #[test]
    fn single_pixel_hole_equals_neighbour_mean() {
        let mut img = RgbImage::from_pixel(5, 5, Rgb([100, 100, 100]));
        img.put_pixel(2, 2, Rgb([255, 255, 255]));
        let mask = mask_with_hole(5, 5, 2, 2, 3, 3);
        let out = harmonic_inpaint_rgb(&img, &mask);
        assert_eq!(*out.get_pixel(2, 2), Rgb([100, 100, 100]));
    }

    #[test]
    fn linear_gradient_hole_stays_smooth() {
        // Vertical gradient; a 1-row hole must interpolate, not band.
        let mut img = RgbImage::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                let v = (y * 32) as u8;
                img.put_pixel(x, y, Rgb([v, v, v]));
            }
        }
        let mask = mask_with_hole(8, 8, 0, 4, 8, 5);
        let out = harmonic_inpaint_rgb(&img, &mask);
        let above = out.get_pixel(3, 3)[0] as i32;
        let mid = out.get_pixel(3, 4)[0] as i32;
        let below = out.get_pixel(3, 5)[0] as i32;
        assert!(mid > above && mid < below, "must interpolate: {above} {mid} {below}");
        assert!((mid - (above + below) / 2).abs() <= 2, "must be near-linear: {above} {mid} {below}");
    }
}
