//! Poisson (seamless-clone) blending — pure-Rust port of
//! `shiftmap_algo._poisson_blend` (Perez et al. 2003).
//!
//! Clones `src` into `dst` under `mask` preserving the source gradient field
//! with Dirichlet boundary from `dst`. Used as the final ShiftMap compose
//! step to hide seams.
//!
//! Conventions (verified against the Python reference):
//! * Matrix `A` is `-Δ_h` (diag = `deg` over in-image neighbours only,
//!   off-diag `-1`); Neumann at image borders (out-of-image neighbours
//!   dropped).
//! * Guidance MUST be `-Δ_h(src)`, i.e. `deg*c - Σneighbours` — implemented
//!   here as `div = 4*c - (up+dn+lf+rt)` with edge replication. Using the
//!   plain Laplacian (`+Δ`, sign flip) inverts curvature and produces a
//!   dark-bowl artifact.
//! * Solved per channel (`f64`) via [`crate::cg::cg_solve`]; solver failure
//!   returns the `src` copy (never scaled garbage).

use image::{GrayImage, RgbImage};

/// Masks larger than this skip the seamless solve and return `src` as-is
/// (mirrors the Python `max_pixels` guard; there is no `seamlessClone`
/// fallback in Rust).
pub const MAX_PIXELS: usize = 120_000;

/// Seamless-clones `src` into `dst` under `mask` (`>127` = blend).
///
/// All three images must share dimensions; mismatch returns a copy of
/// `src`. Empty mask returns a copy of `dst`.
pub fn poisson_blend(src: &RgbImage, dst: &RgbImage, mask: &GrayImage) -> RgbImage {
    if src.dimensions() != dst.dimensions() || src.dimensions() != mask.dimensions() {
        return src.clone();
    }
    let (w, h) = src.dimensions();
    let w = w as usize;
    let h = h as usize;
    let hole = |x: usize, y: usize| mask.get_pixel(x as u32, y as u32)[0] > 127;

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
        return dst.clone();
    }
    if n > MAX_PIXELS {
        eprintln!("[inpaint::poisson] mask {n}px > {MAX_PIXELS} -> skipping seamless solve");
        return src.clone();
    }

    // Guidance div = -Δ_h(src) per channel, with edge replication:
    // up[0]=c[0], dn[-1]=c[-1], lf[:,0]=c[:,0], rt[:,-1]=c[:,-1].
    let at = |img: &RgbImage, x: isize, y: isize, c: usize| -> f64 {
        let x = x.clamp(0, w as isize - 1) as u32;
        let y = y.clamp(0, h as isize - 1) as u32;
        img.get_pixel(x, y)[c] as f64
    };
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
                continue; // Neumann at image borders
            }
            d += 1;
            let j = idx[ny as usize * w + nx as usize];
            if j >= 0 {
                nbrs[k].push(j as u32);
            } else {
                let dp = dst.get_pixel(nx as u32, ny as u32);
                rhs[k][0] += dp[0] as f64;
                rhs[k][1] += dp[1] as f64;
                rhs[k][2] += dp[2] as f64;
            }
        }
        deg[k] = d as f64;
        for c in 0..3 {
            // -Δ_h(src): 4*c - (up+dn+lf+rt), replicated at borders.
            let div = 4.0 * at(src, x as isize, y as isize, c)
                - at(src, x as isize, y as isize - 1, c)
                - at(src, x as isize, y as isize + 1, c)
                - at(src, x as isize - 1, y as isize, c)
                - at(src, x as isize + 1, y as isize, c);
            rhs[k][c] += div;
        }
    }

    let mut out = dst.clone();
    for c in 0..3 {
        let b: Vec<f64> = rhs.iter().map(|r| r[c]).collect();
        let sol = match crate::cg::cg_solve(&deg, &nbrs, &b) {
            Some(s) => s,
            None => return src.clone(),
        };
        for (k, (&y, &x)) in ys.iter().zip(xs.iter()).enumerate() {
            let px = out.get_pixel_mut(x as u32, y as u32);
            px[c] = sol[k].round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Luma, Rgb};

    fn full_mask(w: u32, h: u32) -> GrayImage {
        GrayImage::from_pixel(w, h, Luma([255]))
    }

    #[test]
    fn empty_mask_returns_dst() {
        let src = RgbImage::from_pixel(8, 8, Rgb([200, 0, 0]));
        let dst = RgbImage::from_pixel(8, 8, Rgb([0, 0, 200]));
        let mask = GrayImage::from_pixel(8, 8, Luma([0]));
        assert_eq!(poisson_blend(&src, &dst, &mask), dst);
    }

    #[test]
    fn constant_clone_preserves_color() {
        // Constant src over constant dst: result must equal the constants
        // (catches the +Δ sign-flip dark-bowl bug: wrong-sign guidance
        // bends the interior away from the boundary value).
        let src = RgbImage::from_pixel(16, 16, Rgb([180, 180, 180]));
        let dst = RgbImage::from_pixel(16, 16, Rgb([50, 50, 50]));
        let mut mask = GrayImage::from_pixel(16, 16, Luma([0]));
        for y in 4..12 {
            for x in 4..12 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }
        let out = poisson_blend(&src, &dst, &mask);
        assert_eq!(*out.get_pixel(0, 0), Rgb([50, 50, 50]), "outside stays dst");
        for (x, y) in [(8, 8), (5, 5), (10, 10)] {
            let p = out.get_pixel(x, y);
            for c in 0..3 {
                assert!(
                    (p[c] as i32 - 180).abs() <= 2,
                    "constant blend must hold 180, got {p:?} at ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn gradient_clone_matches_boundary() {
        // Linear ramp in dst, constant src patch: the blend must meet the
        // dst boundary continuously (edge pixels near the boundary value,
        // not the raw src value).
        let mut dst = RgbImage::new(16, 16);
        for y in 0..16 {
            for x in 0..16 {
                let v = (x * 16) as u8;
                dst.put_pixel(x, y, Rgb([v, v, v]));
            }
        }
        let src = RgbImage::from_pixel(16, 16, Rgb([10, 10, 10]));
        let mut mask = GrayImage::from_pixel(16, 16, Luma([0]));
        for y in 6..10 {
            for x in 6..10 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }
        let out = poisson_blend(&src, &dst, &mask);
        // Just outside the mask the dst ramp is untouched.
        assert_eq!(out.get_pixel(5, 7)[0], 5 * 16);
        // Just inside, the solution is pulled toward the boundary ramp
        // (96..160 across x=6..9), not the flat src value 10.
        let inside = out.get_pixel(6, 7)[0] as i32;
        assert!(inside > 40, "must follow boundary gradient, got {inside}");
    }

    #[test]
    fn dim_mismatch_returns_src() {
        let src = RgbImage::from_pixel(8, 8, Rgb([1, 2, 3]));
        let dst = RgbImage::from_pixel(4, 4, Rgb([4, 5, 6]));
        let mask = full_mask(8, 8);
        assert_eq!(poisson_blend(&src, &dst, &mask), src);
    }
}
