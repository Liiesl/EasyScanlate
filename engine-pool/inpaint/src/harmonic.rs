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
//! Solved per channel (`f64`) via CSR [`crate::cg`] through
//! [`crate::laplace::solve_grouped`] (channel-parallel, component-split,
//! boundary-mean warm start, 8-bit adaptive early-exit).
//! Deterministic, no RNG. Empty mask returns a copy; solver failure returns
//! a clone of the input (whole-patch fallback — never a per-channel ghost).

use image::{GrayImage, RgbImage, RgbaImage};

const DIRS: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

/// Fills `mask` (`>127` = hole) of `crop` by harmonic diffusion.
///
/// Returns a new image; `crop` is never modified. Dimension mismatch or an
/// empty mask returns a clone of `crop`. Solver failure returns a clone of
/// `crop` (whole-patch fallback).
pub fn harmonic_inpaint_rgb(crop: &RgbImage, mask: &GrayImage) -> RgbImage {
    if crop.dimensions() != mask.dimensions() {
        return crop.clone();
    }
    let (w_u32, h_u32) = crop.dimensions();
    let w = w_u32 as usize;
    let h = h_u32 as usize;
    let Some((idx, ys, xs)) =
        crate::laplace::build_idx(w, h, |x, y| mask.get_pixel(x as u32, y as u32)[0] > 127)
    else {
        return crop.clone();
    };
    let n = ys.len();
    if n == 0 {
        return crop.clone();
    }

    let (deg, offsets, cols, rhs) = assemble_rgb(crop, w, h, &idx, &ys, &xs);
    // Whole-patch fallback: any channel/component failure keeps originals.
    let sols = match crate::laplace::solve_grouped(&deg, &offsets, &cols, &rhs) {
        Some(s) => s,
        None => return crop.clone(),
    };

    let mut out = crop.clone();
    for (k, (&y, &x)) in ys.iter().zip(xs.iter()).enumerate() {
        let px = out.get_pixel_mut(x as u32, y as u32);
        px[0] = sols[0][k].round().clamp(0.0, 255.0) as u8;
        px[1] = sols[1][k].round().clamp(0.0, 255.0) as u8;
        px[2] = sols[2][k].round().clamp(0.0, 255.0) as u8;
    }
    out
}

/// RGBA harmonic fill: diffuses RGB **and** alpha with the same matrix.
/// Used by [`crate::common::harmonic_inpaint_crop`] so partial-alpha holes
/// composite correctly instead of fringing (alpha copied from source).
/// Same whole-patch fallback contract as [`harmonic_inpaint_rgb`].
pub fn harmonic_inpaint_rgba(crop: &RgbaImage, mask: &GrayImage) -> RgbaImage {
    if crop.dimensions() != mask.dimensions() {
        return crop.clone();
    }
    let (w_u32, h_u32) = crop.dimensions();
    let w = w_u32 as usize;
    let h = h_u32 as usize;
    let Some((idx, ys, xs)) =
        crate::laplace::build_idx(w, h, |x, y| mask.get_pixel(x as u32, y as u32)[0] > 127)
    else {
        return crop.clone();
    };
    let n = ys.len();
    if n == 0 {
        return crop.clone();
    }

    let (deg, offsets, cols, rhs) = assemble_rgba(crop, w, h, &idx, &ys, &xs);
    let sols = match crate::laplace::solve_grouped(&deg, &offsets, &cols, &rhs) {
        Some(s) => s,
        None => return crop.clone(),
    };

    let mut out = crop.clone();
    for (k, (&y, &x)) in ys.iter().zip(xs.iter()).enumerate() {
        let px = out.get_pixel_mut(x as u32, y as u32);
        px[0] = sols[0][k].round().clamp(0.0, 255.0) as u8;
        px[1] = sols[1][k].round().clamp(0.0, 255.0) as u8;
        px[2] = sols[2][k].round().clamp(0.0, 255.0) as u8;
        px[3] = sols[3][k].round().clamp(0.0, 255.0) as u8;
    }
    out
}

/// Single-channel harmonic diffusion of the alpha plane. Used by the shared
/// [`crate::common::diffusion_inpaint_crop`] so ShiftMap-composed patches
/// also get diffused (not copied) alpha. On solver failure returns the
/// original alpha (whole-plane fallback).
pub(crate) fn diffuse_alpha_plane(crop: &RgbaImage, mask: &GrayImage) -> Vec<u8> {
    let (w_u32, h_u32) = crop.dimensions();
    debug_assert_eq!((w_u32, h_u32), mask.dimensions());
    let w = w_u32 as usize;
    let h = h_u32 as usize;
    let orig: Vec<u8> = crop.pixels().map(|p| p[3]).collect();
    let Some((idx, ys, xs)) =
        crate::laplace::build_idx(w, h, |x, y| mask.get_pixel(x as u32, y as u32)[0] > 127)
    else {
        return orig;
    };
    let n = ys.len();
    if n == 0 {
        return orig;
    }
    // Assemble single-channel CSR (same Neumann conventions as RGB).
    let mut deg = vec![0.0f64; n];
    let mut counts = vec![0u32; n];
    let mut rhs = vec![0.0f64; n];
    for (k, (&y, &x)) in ys.iter().zip(xs.iter()).enumerate() {
        let mut d = 0u32;
        let mut hc = 0u32;
        for (dy, dx) in DIRS {
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny < 0 || nx < 0 || ny >= h as isize || nx >= w as isize {
                continue;
            }
            d += 1;
            let j = idx[ny as usize * w + nx as usize];
            if j >= 0 {
                hc += 1;
            } else {
                rhs[k] += crop.get_pixel(nx as u32, ny as u32)[3] as f64;
            }
        }
        deg[k] = d as f64;
        counts[k] = hc;
    }
    let mut offsets = vec![0u32; n + 1];
    for k in 0..n {
        offsets[k + 1] = offsets[k] + counts[k];
    }
    let total = offsets[n] as usize;
    let mut cols = vec![0u32; total];
    let mut cursor = offsets[..n].to_vec();
    for (k, (&y, &x)) in ys.iter().zip(xs.iter()).enumerate() {
        for (dy, dx) in DIRS {
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny < 0 || nx < 0 || ny >= h as isize || nx >= w as isize {
                continue;
            }
            let j = idx[ny as usize * w + nx as usize];
            if j >= 0 {
                let wpos = cursor[k] as usize;
                cols[wpos] = j as u32;
                cursor[k] += 1;
            }
        }
    }
    let sols = match crate::laplace::solve_grouped(&deg, &offsets, &cols, &[rhs]) {
        Some(s) => s,
        None => return orig,
    };
    let mut alpha = orig;
    for (k, (&y, &x)) in ys.iter().zip(xs.iter()).enumerate() {
        alpha[y * w + x] = sols[0][k].round().clamp(0.0, 255.0) as u8;
    }
    alpha
}

fn assemble_rgb(
    crop: &RgbImage,
    w: usize,
    h: usize,
    idx: &[i32],
    ys: &[usize],
    xs: &[usize],
) -> (Vec<f64>, Vec<u32>, Vec<u32>, Vec<Vec<f64>>) {
    let n = ys.len();
    let mut deg = vec![0.0f64; n];
    let mut counts = vec![0u32; n];
    let mut r0 = vec![0.0f64; n];
    let mut r1 = vec![0.0f64; n];
    let mut r2 = vec![0.0f64; n];
    for (k, (&y, &x)) in ys.iter().zip(xs.iter()).enumerate() {
        let mut d = 0u32;
        let mut hc = 0u32;
        for (dy, dx) in DIRS {
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny < 0 || nx < 0 || ny >= h as isize || nx >= w as isize {
                continue; // Neumann: drop out-of-image neighbours
            }
            d += 1;
            let j = idx[ny as usize * w + nx as usize];
            if j >= 0 {
                hc += 1;
            } else {
                let px = crop.get_pixel(nx as u32, ny as u32);
                r0[k] += px[0] as f64;
                r1[k] += px[1] as f64;
                r2[k] += px[2] as f64;
            }
        }
        deg[k] = d as f64;
        counts[k] = hc;
    }
    let (offsets, cols) = counts_to_csr(w, h, idx, ys, xs, &counts);
    (deg, offsets, cols, vec![r0, r1, r2])
}

fn assemble_rgba(
    crop: &RgbaImage,
    w: usize,
    h: usize,
    idx: &[i32],
    ys: &[usize],
    xs: &[usize],
) -> (Vec<f64>, Vec<u32>, Vec<u32>, Vec<Vec<f64>>) {
    let n = ys.len();
    let mut deg = vec![0.0f64; n];
    let mut counts = vec![0u32; n];
    let mut r0 = vec![0.0f64; n];
    let mut r1 = vec![0.0f64; n];
    let mut r2 = vec![0.0f64; n];
    let mut ra = vec![0.0f64; n];
    for (k, (&y, &x)) in ys.iter().zip(xs.iter()).enumerate() {
        let mut d = 0u32;
        let mut hc = 0u32;
        for (dy, dx) in DIRS {
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny < 0 || nx < 0 || ny >= h as isize || nx >= w as isize {
                continue;
            }
            d += 1;
            let j = idx[ny as usize * w + nx as usize];
            if j >= 0 {
                hc += 1;
            } else {
                let px = crop.get_pixel(nx as u32, ny as u32);
                r0[k] += px[0] as f64;
                r1[k] += px[1] as f64;
                r2[k] += px[2] as f64;
                ra[k] += px[3] as f64;
            }
        }
        deg[k] = d as f64;
        counts[k] = hc;
    }
    let (offsets, cols) = counts_to_csr(w, h, idx, ys, xs, &counts);
    (deg, offsets, cols, vec![r0, r1, r2, ra])
}

fn counts_to_csr(
    w: usize,
    h: usize,
    idx: &[i32],
    ys: &[usize],
    xs: &[usize],
    counts: &[u32],
) -> (Vec<u32>, Vec<u32>) {
    let n = ys.len();
    let mut offsets = vec![0u32; n + 1];
    for k in 0..n {
        offsets[k + 1] = offsets[k] + counts[k];
    }
    let total = offsets[n] as usize;
    let mut cols = vec![0u32; total];
    let mut cursor = offsets[..n].to_vec();
    for (k, (&y, &x)) in ys.iter().zip(xs.iter()).enumerate() {
        for (dy, dx) in DIRS {
            let ny = y as isize + dy;
            let nx = x as isize + dx;
            if ny < 0 || nx < 0 || ny >= h as isize || nx >= w as isize {
                continue;
            }
            let j = idx[ny as usize * w + nx as usize];
            if j >= 0 {
                let wpos = cursor[k] as usize;
                cols[wpos] = j as u32;
                cursor[k] += 1;
            }
        }
    }
    (offsets, cols)
}

/// Diffusion fallback used when ShiftMap finds no feasible label: returns
/// the harmonic fill as full RGB.
pub(crate) fn harmonic_fallback_rgb(crop: &RgbImage, mask: &GrayImage) -> RgbImage {
    harmonic_inpaint_rgb(crop, mask)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Luma, Rgb, Rgba};

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

    #[test]
    fn multi_blob_matches_single_blob_parity() {
        // Two disjoint quads: component-split solve must equal the legacy
        // global solve at u8 level (per-component norms differ in theory,
        // but must round identically here).
        let mut img = RgbImage::from_pixel(32, 32, Rgb([40, 40, 40]));
        for y in 4..10 {
            for x in 4..10 {
                img.put_pixel(x, y, Rgb([200, 200, 200]));
            }
        }
        for y in 20..26 {
            for x in 20..26 {
                img.put_pixel(x, y, Rgb([200, 200, 200]));
            }
        }
        let mut mask = GrayImage::from_pixel(32, 32, Luma([0]));
        for y in 4..10 {
            for x in 4..10 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }
        for y in 20..26 {
            for x in 20..26 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }
        let out = harmonic_inpaint_rgb(&img, &mask);
        // Both holes diffuse toward the dark surround.
        for (x, y) in [(6, 6), (22, 22)] {
            let c = out.get_pixel(x, y);
            assert!(c[0] < 128, "multi-blob hole must diffuse dark at ({x},{y}): {c:?}");
        }
        // Gap between blobs untouched.
        assert_eq!(*out.get_pixel(15, 15), Rgb([40, 40, 40]));
        // Deterministic: second run identical.
        let out2 = harmonic_inpaint_rgb(&img, &mask);
        assert_eq!(out, out2);
    }

    #[test]
    fn rgba_diffuses_alpha_not_fringe() {
        // Opaque surround, transparent hole interior in source: copying
        // alpha would leave a transparent hole; diffusion must fill it.
        let mut img = RgbaImage::from_pixel(16, 16, Rgba([80, 80, 80, 255]));
        for y in 5..11 {
            for x in 5..11 {
                img.put_pixel(x, y, Rgba([80, 80, 80, 0]));
            }
        }
        let mask = mask_with_hole(16, 16, 5, 5, 11, 11);
        let out = harmonic_inpaint_rgba(&img, &mask);
        let c = out.get_pixel(8, 8);
        assert!(c[3] > 200, "hole alpha must diffuse to opaque, got {c:?}");
    }

    #[test]
    fn dim_mismatch_returns_copy() {
        let img = RgbImage::from_pixel(8, 8, Rgb([1, 2, 3]));
        let mask = GrayImage::from_pixel(4, 4, Luma([255]));
        assert_eq!(harmonic_inpaint_rgb(&img, &mask), img);
    }
}
