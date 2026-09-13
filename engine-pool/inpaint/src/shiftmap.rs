//! Shift-Map inpainting (He & Sun 2012, ECCV) — pure-Rust port of
//! `shiftmap_algo.py`: `shiftmap_inpaint()` / `_shiftmap_core()`.
//!
//! Best for textured regions (screentone, scenery); for smooth gradients
//! prefer [`crate::harmonic`].
//!
//! Pipeline (mirrors the Python reference):
//! 1. Harmonic-diffused seed → translation-only PatchMatch NNF, quantised
//!    to `N_LABELS` dominant *validated* shifts (sources fully known).
//! 2. MRF over hole pixels solved by alpha-expansion graph cut with exact
//!    proposal-evaluated seam costs (He & Sun / Kwatra) + Rother truncation
//!    + Hammer construction, over the external [`push_relabel`] s-t solver.
//! 3. Compose from the original image + [`crate::poisson`] blend.
//!
//! Critical conventions (verified by cut enumeration in tests):
//! * [`push_relabel::PushRelabel::min_cut`] returns source-reachability:
//!   `reachable == true` means SOURCE set = KEEP current label,
//!   `reachable == false` means SINK set = TAKE alpha.
//! * Data edges: `source -> node` cap `D_alpha`, `node -> sink` cap `D_cur`.
//!   Swapping these silently inverts the data term.
//! * Pairwise `E(0,0)=A..E(1,1)=D` via Hammer; requires `A+D<=B+C`,
//!   enforced with Rother truncation `A=min(A,B+C)`.
//! * Sink-side nodes take `alpha` ONLY if `data[k,alpha]` is finite
//!   (infeasible labels are pinned by inf cost).
//! * RNG is seeded (`StdRng::seed_from_u64(2)`) for reproducibility.
//! * Shift quantisation uses floor division (`div_euclid`), matching
//!   Python `//` for negative offsets.

use std::collections::HashMap;

use image::{GrayImage, RgbImage};
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;

/// Patch side length (odd). Mirrors `patch=9`.
pub const PATCH: usize = 9;
/// Number of dominant shifts kept as MRF labels. Mirrors `n_labels=24`.
pub const N_LABELS: usize = 24;
/// Long edge cap for the MRF stage; larger crops are downscaled first and
/// the result is rescaled + Poisson-blended at full resolution.
pub const MAX_EDGE: u32 = 600;
/// Seam-cost truncation. Mirrors `smooth_trunc=4000.0`.
pub const SMOOTH_TRUNC: f64 = 4000.0;
/// Seam-cost weight. Mirrors `smooth_w=1.0`.
pub const SMOOTH_W: f64 = 1.0;
/// Infinite data cost marking infeasible (hole-copying) labels.
const INF: f64 = 1e9;
/// PatchMatch sweeps. Mirrors `iters=5`.
const PM_ITERS: usize = 5;
/// Alpha-expansion rounds. Mirrors `iters=3`.
const EXPANSION_ITERS: usize = 3;
/// Above this many hole pixels the MRF stage is skipped in favour of
/// harmonic diffusion (s-t cut cost grows super-linearly; giant masks are
/// diffusion-friendly anyway).
const MAX_HOLE_PIXELS: usize = 50_000;
/// Integer capacity scale for the external max-flow solver.
const FLOW_SCALE: f64 = 64.0;
/// Capacity clamp: `INF * FLOW_SCALE` (6.4e10) fits with large headroom in
/// `i64`, keeping total-flow sums far from overflow.
const FLOW_CAP: i64 = 1_000_000_000_000;

/// Shift-Map + Poisson inpainting: fills `mask` (`>127` = hole) of `img`.
///
/// Dimension mismatch returns a clone of `img`; empty mask returns a copy.
pub fn shiftmap_inpaint_rgb(img: &RgbImage, mask: &GrayImage) -> RgbImage {
    if img.dimensions() != mask.dimensions() {
        return img.clone();
    }
    if !mask.pixels().any(|p| p[0] > 127) {
        return img.clone();
    }
    let (w, h) = img.dimensions();
    let s = (MAX_EDGE as f32 / w.max(h) as f32).min(1.0);
    if s >= 1.0 {
        return shiftmap_core(img, mask);
    }
    let nw = ((w as f32 * s).round() as u32).max(1);
    let nh = ((h as f32 * s).round() as u32).max(1);
    let img_s = image::imageops::resize(img, nw, nh, image::imageops::FilterType::Triangle);
    let mask_s = image::imageops::resize(mask, nw, nh, image::imageops::FilterType::Nearest);
    let mut out_s = shiftmap_core(&img_s, &mask_s);
    // Rescale to full resolution, re-pin known pixels, Poisson-blend.
    out_s = image::imageops::resize(&out_s, w, h, image::imageops::FilterType::Triangle);
    for (x, y, px) in out_s.enumerate_pixels_mut() {
        if mask.get_pixel(x, y)[0] <= 127 {
            *px = *img.get_pixel(x, y);
        }
    }
    crate::poisson::poisson_blend(&out_s, img, mask)
}

fn shiftmap_core(img: &RgbImage, mask: &GrayImage) -> RgbImage {
    let (w_u32, h_u32) = img.dimensions();
    let w = w_u32 as usize;
    let h = h_u32 as usize;
    let r = PATCH / 2;

    let hole_at = |x: usize, y: usize| mask.get_pixel(x as u32, y as u32)[0] > 127;
    let mut hy: Vec<usize> = Vec::new();
    let mut hx: Vec<usize> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            if hole_at(x, y) {
                hy.push(y);
                hx.push(x);
            }
        }
    }
    if hy.is_empty() {
        return img.clone();
    }
    if hy.len() > MAX_HOLE_PIXELS {
        eprintln!(
            "[inpaint::shiftmap] hole {}px > {MAX_HOLE_PIXELS} -> harmonic fallback",
            hy.len()
        );
        return crate::harmonic::harmonic_fallback_rgb(img, mask);
    }

    // Seed canvas: boundary-local harmonic diffusion (never the global
    // mean — it biases matching toward dark donors).
    let seed = crate::harmonic::harmonic_inpaint_rgb(img, mask);
    let mut canvas = vec![[0.0f32; 3]; w * h];
    for y in 0..h {
        for x in 0..w {
            let p = seed.get_pixel(x as u32, y as u32);
            canvas[y * w + x] = [p[0] as f32, p[1] as f32, p[2] as f32];
        }
    }

    let known: Vec<bool> = (0..h)
        .flat_map(|y| (0..w).map(move |x| !hole_at(x, y)))
        .collect();
    let mut valid = valid_source_mask(&known, w, h, PATCH);
    if r > 0 {
        for y in 0..h {
            for x in 0..w {
                if y < r || y + r >= h || x < r || x + r >= w {
                    valid[y * w + x] = false;
                }
            }
        }
    }

    let mut rng = StdRng::seed_from_u64(2);
    let nnf = patchmatch_nnf(&canvas, w, h, &known, &valid, r, PM_ITERS, &mut rng);

    let offs: Vec<(i32, i32)> = hy
        .iter()
        .zip(hx.iter())
        .map(|(&y, &x)| nnf[y * w + x])
        .collect();
    let shifts = top_shifts(&offs, &hy, &hx, &valid, w, h, r, N_LABELS);
    let nl = shifts.len();

    // Data term: mean SSD of the seed patch vs the shifted source patch;
    // infeasible (out-of-range / not-fully-known) labels stay INF.
    let n = hy.len();
    let mut data = vec![INF; n * nl];
    let mut feasible = vec![false; n * nl];
    for (li, &(dy, dx)) in shifts.iter().enumerate() {
        for (k, (&y, &x)) in hy.iter().zip(hx.iter()).enumerate() {
            let sy = y as isize + dy as isize;
            let sx = x as isize + dx as isize;
            if sy < r as isize
                || sx < r as isize
                || sy + r as isize >= h as isize
                || sx + r as isize >= w as isize
            {
                continue;
            }
            let (sy, sx) = (sy as usize, sx as usize);
            if !valid[sy * w + sx] {
                continue;
            }
            feasible[k * nl + li] = true;
            data[k * nl + li] = ssd_full(&canvas, w, h, y, x, sy, sx, r, f64::INFINITY);
        }
    }
    let keep: Vec<usize> = (0..nl).filter(|&li| (0..n).any(|k| feasible[k * nl + li])).collect();
    if keep.is_empty() {
        eprintln!("[inpaint::shiftmap] no feasible label -> harmonic fallback");
        return crate::harmonic::harmonic_fallback_rgb(img, mask);
    }
    let old_nl = nl;
    let shifts: Vec<(i32, i32)> = keep.iter().map(|&li| shifts[li]).collect();
    let nl = shifts.len();
    // Compact the data term to the kept labels: new_data[k][j] = data[k][keep[j]].
    let mut data_new = vec![INF; n * nl];
    for k in 0..n {
        for (j, &li) in keep.iter().enumerate() {
            data_new[k * nl + j] = data[k * old_nl + li];
        }
    }
    let data = data_new;

    let edges = neighbour_edges(&hy, &hx, w, h);

    let init_energy = total_energy(&canvas, w, h, &hy, &hx, &shifts, &data, nl, &edges);
    let labels = alpha_expansion(&canvas, w, h, &hy, &hx, &shifts, &data, &edges, EXPANSION_ITERS);
    let labels = {
        let e = total_energy_with(&canvas, w, h, &hy, &hx, &shifts, &data, nl, &edges, &labels);
        if e > init_energy {
            eprintln!("[inpaint::shiftmap] expansion energy {e:.1} > init {init_energy:.1} -> ICM fallback");
            icm(&canvas, w, h, &hy, &hx, &shifts, &data, &edges, 6)
        } else {
            labels
        }
    };

    let mut comp = img.clone();
    for (k, (&y, &x)) in hy.iter().zip(hx.iter()).enumerate() {
        let (dy, dx) = shifts[labels[k] as usize];
        let sy = (y as i32 + dy).clamp(0, h as i32 - 1) as u32;
        let sx = (x as i32 + dx).clamp(0, w as i32 - 1) as u32;
        comp.put_pixel(x as u32, y as u32, *img.get_pixel(sx, sy));
    }
    crate::poisson::poisson_blend(&comp, img, mask)
}

// ---------------------------------------------------------------------------
// Source-validity mask
// ---------------------------------------------------------------------------

/// True where a patch centred here lies fully in `known`, via integral image.
fn valid_source_mask(known: &[bool], w: usize, h: usize, patch: usize) -> Vec<bool> {
    // Integral image with 1-cell padding.
    let mut integ = vec![0u32; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0u32;
        for x in 0..w {
            if known[y * w + x] {
                row += 1;
            }
            integ[(y + 1) * (w + 1) + (x + 1)] = integ[y * (w + 1) + (x + 1)] + row;
        }
    }
    let r = patch / 2;
    let mut out = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            let x0 = x.saturating_sub(r);
            let y0 = y.saturating_sub(r);
            let x1 = (x + r).min(w - 1);
            let y1 = (y + r).min(h - 1);
            let full_w = x1 - x0 + 1;
            let full_h = y1 - y0 + 1;
            if full_w < patch || full_h < patch {
                continue; // patch would leave the image
            }
            let sum = integ[(y1 + 1) * (w + 1) + (x1 + 1)] + integ[y0 * (w + 1) + x0]
                - integ[y0 * (w + 1) + (x1 + 1)]
                - integ[(y1 + 1) * (w + 1) + x0];
            out[y * w + x] = sum >= (patch * patch) as u32;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Patch distance
// ---------------------------------------------------------------------------

/// Mean SSD over the FULL patch on the canvas estimate. Out-of-range
/// centres return infinity. `bound` enables exact early exit: partial sums
/// already exceeding `bound * count` can never beat `bound`.
fn ssd_full(
    canvas: &[[f32; 3]],
    w: usize,
    h: usize,
    ty: usize,
    tx: usize,
    sy: usize,
    sx: usize,
    r: usize,
    bound: f64,
) -> f64 {
    if ty < r || tx < r || sy < r || sx < r {
        return f64::INFINITY;
    }
    if ty + r >= h || tx + r >= w || sy + r >= h || sx + r >= w {
        return f64::INFINITY;
    }
    let side = 2 * r + 1;
    let count = (side * side * 3) as f64;
    let limit = bound * count;
    let mut acc = 0.0f64;
    for dy in 0..side {
        for dx in 0..side {
            let t = canvas[(ty + dy - r) * w + (tx + dx - r)];
            let s = canvas[(sy + dy - r) * w + (sx + dx - r)];
            for c in 0..3 {
                let d = (t[c] - s[c]) as f64;
                acc += d * d;
                if acc >= limit {
                    return acc / count; // exact early exit, same comparison outcome
                }
            }
        }
    }
    acc / count
}

// ---------------------------------------------------------------------------
// PatchMatch
// ---------------------------------------------------------------------------

/// NNF offsets `(dy,dx)` per pixel; valid on hole pixels only. `known` is
/// currently used for the empty-donor fast path (all sampling is gated by
/// `valid`).
fn patchmatch_nnf(
    canvas: &[[f32; 3]],
    w: usize,
    h: usize,
    known: &[bool],
    valid: &[bool],
    r: usize,
    iters: usize,
    rng: &mut StdRng,
) -> Vec<(i32, i32)> {
    let mut nnf = vec![(0i32, 0i32); w * h];
    let mut best = vec![f64::INFINITY; w * h];

    // Donor pool: fully-known patch centres (with r-border cleared by caller).
    let donors: Vec<(usize, usize)> = (0..h)
        .flat_map(|y| (0..w).filter(move |&x| valid[y * w + x]).map(move |x| (y, x)))
        .collect();
    let donors: Vec<(usize, usize)> = if donors.is_empty() {
        // Degenerate: fall back to any known pixel (mirrors Python).
        (0..h)
            .flat_map(|y| (0..w).filter(move |&x| known[y * w + x]).map(move |x| (y, x)))
            .collect()
    } else {
        donors
    };
    if donors.is_empty() {
        return nnf;
    }
    let nv = donors.len();

    // Random init on hole pixels.
    for y in 0..h {
        for x in 0..w {
            if known[y * w + x] {
                continue;
            }
            let (sy, sx) = donors[rng.random_range(0..nv)];
            nnf[y * w + x] = (sy as i32 - y as i32, sx as i32 - x as i32);
            best[y * w + x] = ssd_full(canvas, w, h, y, x, sy, sx, r, f64::INFINITY);
        }
    }

    for it in 0..iters {
        let fwd = it % 2 == 0;
        let y_range: Vec<usize> = if fwd { (0..h).collect() } else { (0..h).rev().collect() };
        let x_range: Vec<usize> = if fwd { (0..w).collect() } else { (0..w).rev().collect() };
        for &y in &y_range {
            for &x in &x_range {
                if known[y * w + x] {
                    continue;
                }
                let mut cands = Vec::with_capacity(8);
                cands.push(nnf[y * w + x]);
                if fwd {
                    if x > 0 && !known[y * w + (x - 1)] {
                        cands.push(nnf[y * w + (x - 1)]);
                    }
                    if y > 0 && !known[(y - 1) * w + x] {
                        cands.push(nnf[(y - 1) * w + x]);
                    }
                } else {
                    if x + 1 < w && !known[y * w + (x + 1)] {
                        cands.push(nnf[y * w + (x + 1)]);
                    }
                    if y + 1 < h && !known[(y + 1) * w + x] {
                        cands.push(nnf[(y + 1) * w + x]);
                    }
                }
                // Random search around the pre-search best (mirrors Python:
                // the centre is fixed, not updated on improvement).
                let (cy, cx) = cands[0];
                let mut mag = (w.max(h) / 2 + 1) as i32;
                while mag >= 1 {
                    cands.push((
                        cy + rng.random_range(-mag..=mag),
                        cx + rng.random_range(-mag..=mag),
                    ));
                    mag /= 2;
                }
                let mut bb = best[y * w + x];
                for (dy, dx) in cands {
                    let sy = y as i32 + dy;
                    let sx = x as i32 + dx;
                    if sy < r as i32 || sx < r as i32 || sy + r as i32 >= h as i32 || sx + r as i32 >= w as i32 {
                        continue;
                    }
                    let (sy, sx) = (sy as usize, sx as usize);
                    if !valid[sy * w + sx] {
                        continue;
                    }
                    let c = ssd_full(canvas, w, h, y, x, sy, sx, r, bb);
                    if c < bb {
                        bb = c;
                        nnf[y * w + x] = (dy, dx);
                    }
                }
                best[y * w + x] = bb;
            }
        }
    }
    nnf
}

// ---------------------------------------------------------------------------
// Dominant shifts
// ---------------------------------------------------------------------------

/// Dominant offsets pointing at fully-known sources, quantised with floor
/// division (matches Python `dy // 2 * 2` for negatives via `div_euclid`).
fn top_shifts(
    offs: &[(i32, i32)],
    hy: &[usize],
    hx: &[usize],
    valid: &[bool],
    w: usize,
    h: usize,
    r: usize,
    k: usize,
) -> Vec<(i32, i32)> {
    let mut counts: HashMap<(i32, i32), usize> = HashMap::new();
    for (&(dy, dx), (&y, &x)) in offs.iter().zip(hy.iter().zip(hx.iter())) {
        if dy == 0 && dx == 0 {
            continue;
        }
        let sy = y as i32 + dy;
        let sx = x as i32 + dx;
        if sy < r as i32 || sx < r as i32 || sy + r as i32 >= h as i32 || sx + r as i32 >= w as i32 {
            continue;
        }
        if !valid[sy as usize * w + sx as usize] {
            continue;
        }
        let key = (dy.div_euclid(2) * 2, dx.div_euclid(2) * 2);
        *counts.entry(key).or_insert(0) += 1;
    }
    let mut ranked: Vec<((i32, i32), usize)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1));
    let mut shifts: Vec<(i32, i32)> = ranked.into_iter().take(k).map(|(s, _)| s).collect();
    if shifts.is_empty() {
        shifts = vec![(0, 5), (5, 0), (0, -5), (-5, 0), (5, 5), (-5, -5)];
    }
    shifts
}

// ---------------------------------------------------------------------------
// Seam cost + graph edges
// ---------------------------------------------------------------------------

/// He & Sun / Kwatra seam penalty for edge `((y1,x1),(y2,x2))` under shifts
/// `s1` vs `s2`. Symmetric in the labels; truncated at `trunc`.
fn seam_cost(
    canvas: &[[f32; 3]],
    w: usize,
    h: usize,
    y1: usize,
    x1: usize,
    y2: usize,
    x2: usize,
    s1: (i32, i32),
    s2: (i32, i32),
    trunc: f64,
) -> f64 {
    let px = |y: i32, x: i32| -> [f64; 3] {
        let y = y.clamp(0, h as i32 - 1) as usize;
        let x = x.clamp(0, w as i32 - 1) as usize;
        let p = canvas[y * w + x];
        [p[0] as f64, p[1] as f64, p[2] as f64]
    };
    let y1 = y1 as i32;
    let x1 = x1 as i32;
    let y2 = y2 as i32;
    let x2 = x2 as i32;
    let a1 = px(y1 + s1.0, x1 + s1.1);
    let b1 = px(y1 + s2.0, x1 + s2.1);
    let a2 = px(y2 + s1.0, x2 + s1.1);
    let b2 = px(y2 + s2.0, x2 + s2.1);
    let mut e = 0.0;
    for c in 0..3 {
        e += (a1[c] - b1[c]).powi(2) + (a2[c] - b2[c]).powi(2);
    }
    e.min(trunc)
}

/// Right + down neighbour pairs over hole pixels (indices into `hy`/`hx`).
fn neighbour_edges(hy: &[usize], hx: &[usize], w: usize, h: usize) -> Vec<(usize, usize)> {
    let mut idx = vec![-1i64; w * h];
    for (k, (&y, &x)) in hy.iter().zip(hx.iter()).enumerate() {
        idx[y * w + x] = k as i64;
    }
    let mut edges = Vec::new();
    for (k, (&y, &x)) in hy.iter().zip(hx.iter()).enumerate() {
        if x + 1 < w {
            let j = idx[y * w + (x + 1)];
            if j >= 0 {
                edges.push((k, j as usize));
            }
        }
        if y + 1 < h {
            let j = idx[(y + 1) * w + x];
            if j >= 0 {
                edges.push((k, j as usize));
            }
        }
    }
    edges
}

// ---------------------------------------------------------------------------
// s-t cut wrapper over the external max-flow solver
// ---------------------------------------------------------------------------

fn to_cap(v: f64) -> i64 {
    if !v.is_finite() {
        return FLOW_CAP;
    }
    ((v * FLOW_SCALE).round() as i64).clamp(0, FLOW_CAP)
}

/// Binary s-t graph with PyMaxflow-style construction:
/// * `add_tedge(i, cap_source, cap_sink)`: `source -> i` cap `cap_source`
///   (paid when `i` lands in T = TAKE), `i -> sink` cap `cap_sink` (paid
///   when `i` lands in S = KEEP). Capacities accumulate.
/// * `add_undirected(a, b, w)`: symmetric penalty `w` paid on disagreement.
struct StGraph {
    n: usize,
    cap_source: Vec<i64>,
    cap_sink: Vec<i64>,
    pairs: Vec<(usize, usize, i64)>,
}

impl StGraph {
    fn new(n: usize) -> Self {
        Self { n, cap_source: vec![0; n], cap_sink: vec![0; n], pairs: Vec::new() }
    }

    fn add_tedge(&mut self, i: usize, cap_source: f64, cap_sink: f64) {
        self.cap_source[i] = self.cap_source[i].saturating_add(to_cap(cap_source));
        self.cap_sink[i] = self.cap_sink[i].saturating_add(to_cap(cap_sink));
    }

    fn add_undirected(&mut self, a: usize, b: usize, w: f64) {
        let w = to_cap(w);
        if w > 0 {
            self.pairs.push((a, b, w));
        }
    }

    /// Returns per-node source-set membership: `true` = S = KEEP current
    /// label, `false` = T = TAKE alpha.
    fn min_cut(&self) -> Vec<bool> {
        let src = self.n;
        let snk = self.n + 1;
        let mut g = push_relabel::PushRelabel::new(self.n + 2);
        for i in 0..self.n {
            g.add_edge(src, i, self.cap_source[i], 0);
            g.add_edge(i, snk, self.cap_sink[i], 0);
        }
        for &(a, b, w) in &self.pairs {
            g.add_edge(a, b, w, w);
        }
        g.max_flow(src, snk);
        g.min_cut(src)
    }
}

/// Hammer construction for submodular `E(0,0)=A..E(1,1)=D` (`A+D<=B+C`,
/// enforced by the caller via Rother truncation).
/// `x=false` <=> source set (keep), `x=true` <=> sink set (take alpha).
fn add_binary_pairwise(g: &mut StGraph, na: usize, nb: usize, a: f64, b: f64, c: f64, d: f64) {
    let cx = ((c + d) - (a + b)) / 2.0;
    let cy = ((b + d) - (a + c)) / 2.0;
    let w = (b + c - a - d) / 2.0;
    debug_assert!(w >= -1e-9, "non-submodular edge reaching graph: {a},{b},{c},{d}");
    let w = w.max(0.0);
    g.add_tedge(na, cx.max(0.0), (-cx).max(0.0));
    g.add_tedge(nb, cy.max(0.0), (-cy).max(0.0));
    g.add_undirected(na, nb, w);
}

// ---------------------------------------------------------------------------
// Alpha-expansion + ICM + energy
// ---------------------------------------------------------------------------

fn argmin_labels(data: &[f64], n: usize, nl: usize) -> Vec<u32> {
    (0..n)
        .map(|k| {
            let mut best_l = 0u32;
            let mut best_e = f64::INFINITY;
            for li in 0..nl {
                let e = data[k * nl + li];
                if e < best_e {
                    best_e = e;
                    best_l = li as u32;
                }
            }
            best_l
        })
        .collect()
}

fn total_energy_with(
    canvas: &[[f32; 3]],
    w: usize,
    h: usize,
    hy: &[usize],
    hx: &[usize],
    shifts: &[(i32, i32)],
    data: &[f64],
    nl: usize,
    edges: &[(usize, usize)],
    labels: &[u32],
) -> f64 {
    let mut e = 0.0;
    for (k, &l) in labels.iter().enumerate() {
        e += data[k * nl + l as usize];
    }
    for &(a, b) in edges {
        let la = labels[a] as usize;
        let lb = labels[b] as usize;
        if la != lb {
            e += SMOOTH_W
                * seam_cost(canvas, w, h, hy[a], hx[a], hy[b], hx[b], shifts[la], shifts[lb], SMOOTH_TRUNC);
        }
    }
    e
}

fn total_energy(
    canvas: &[[f32; 3]],
    w: usize,
    h: usize,
    hy: &[usize],
    hx: &[usize],
    shifts: &[(i32, i32)],
    data: &[f64],
    nl: usize,
    edges: &[(usize, usize)],
) -> f64 {
    let labels = argmin_labels(data, hy.len(), nl);
    total_energy_with(canvas, w, h, hy, hx, shifts, data, nl, edges, &labels)
}

/// Alpha-expansion with exact proposal-evaluated seam costs.
///
/// Per edge `(a,b)`, current labels `la,lb`, expansion label `α`:
/// `A=w·seam(la,lb)`, `B=w·seam(la,α)`, `C=w·seam(α,lb)`, `D=0`.
/// Rother truncation `A=min(A,B+C)`, then Hammer. Data:
/// `add_tedge(node, D_alpha, D_cur)`. Sink-side nodes take `α` only if
/// `data[k,alpha]` is finite.
fn alpha_expansion(
    canvas: &[[f32; 3]],
    w: usize,
    h: usize,
    hy: &[usize],
    hx: &[usize],
    shifts: &[(i32, i32)],
    data: &[f64],
    edges: &[(usize, usize)],
    iters: usize,
) -> Vec<u32> {
    let n = hy.len();
    let nl = shifts.len();
    let mut labels = argmin_labels(data, n, nl);
    for _ in 0..iters {
        for alpha in 0..nl {
            let mut g = StGraph::new(n);
            for k in 0..n {
                g.add_tedge(k, data[k * nl + alpha], data[k * nl + labels[k] as usize]);
            }
            for &(a, b) in edges {
                let la = labels[a] as usize;
                let lb = labels[b] as usize;
                if la == alpha && lb == alpha {
                    continue;
                }
                let mut a_cost = SMOOTH_W
                    * seam_cost(canvas, w, h, hy[a], hx[a], hy[b], hx[b], shifts[la], shifts[lb], SMOOTH_TRUNC);
                let b_cost = SMOOTH_W
                    * seam_cost(canvas, w, h, hy[a], hx[a], hy[b], hx[b], shifts[la], shifts[alpha], SMOOTH_TRUNC);
                let c_cost = SMOOTH_W
                    * seam_cost(canvas, w, h, hy[a], hx[a], hy[b], hx[b], shifts[alpha], shifts[lb], SMOOTH_TRUNC);
                if a_cost > b_cost + c_cost {
                    a_cost = b_cost + c_cost; // Rother truncation
                }
                add_binary_pairwise(&mut g, a, b, a_cost, b_cost, c_cost, 0.0);
            }
            let seg = g.min_cut();
            for k in 0..n {
                // `seg == false` <=> sink side <=> take alpha (iff feasible).
                if !seg[k] && data[k * nl + alpha].is_finite() {
                    labels[k] = alpha as u32;
                }
            }
        }
    }
    labels
}

/// ICM fallback (same energy; no submodularity requirement).
fn icm(
    canvas: &[[f32; 3]],
    w: usize,
    h: usize,
    hy: &[usize],
    hx: &[usize],
    shifts: &[(i32, i32)],
    data: &[f64],
    edges: &[(usize, usize)],
    iters: usize,
) -> Vec<u32> {
    let n = hy.len();
    let nl = shifts.len();
    let mut labels = argmin_labels(data, n, nl);
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(a, b) in edges {
        adj[a].push(b);
        adj[b].push(a);
    }
    for _ in 0..iters {
        for k in 0..n {
            let mut best_l = labels[k];
            let mut best_e = f64::INFINITY;
            for li in 0..nl {
                if !data[k * nl + li].is_finite() {
                    continue;
                }
                let mut e = data[k * nl + li];
                for &nb in &adj[k] {
                    if li as u32 != labels[nb] {
                        e += SMOOTH_W
                            * seam_cost(
                                canvas, w, h, hy[k], hx[k], hy[nb], hx[nb],
                                shifts[li], shifts[labels[nb] as usize], SMOOTH_TRUNC,
                            );
                    }
                }
                if e < best_e {
                    best_e = e;
                    best_l = li as u32;
                }
            }
            labels[k] = best_l;
        }
    }
    labels
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Luma, Rgb};

    // --- s-t wrapper convention tests (brute-force verified) ---

    /// Energy of an assignment under explicit unary + undirected pairwise.
    fn brute_energy(
        unary_s: &[f64],
        unary_t: &[f64],
        pairs: &[(usize, usize, f64)],
        assign: &[bool], // true = T(take)
    ) -> f64 {
        let mut e = 0.0;
        for (i, &t) in assign.iter().enumerate() {
            e += if t { unary_t[i] } else { unary_s[i] };
        }
        for &(a, b, w) in pairs {
            if assign[a] != assign[b] {
                e += w;
            }
        }
        e
    }

    fn cut_energy(
        unary_s: &[f64],
        unary_t: &[f64],
        pairs: &[(usize, usize, f64)],
        keep: &[bool], // true = S-set = keep
    ) -> f64 {
        let assign: Vec<bool> = keep.iter().map(|&k| !k).collect();
        brute_energy(unary_s, unary_t, pairs, &assign)
    }

    #[test]
    fn tedge_direction_split_is_optimal() {
        // node0 wants T (S:10, T:0), node1 wants S (S:0, T:10), w=3.
        // Optimum: split, energy 3. A swapped data term would return the
        // all-wrong cut (energy 23) instead.
        let mut g = StGraph::new(2);
        g.add_tedge(0, 0.0, 10.0); // D_alpha=0, D_cur=10
        g.add_tedge(1, 10.0, 0.0);
        g.add_undirected(0, 1, 3.0);
        let keep = g.min_cut();
        assert_eq!(keep, vec![false, true], "node0 takes, node1 keeps");
        let e = cut_energy(&[10.0, 0.0], &[0.0, 10.0], &[(0, 1, 3.0)], &keep);
        assert!((e - 3.0).abs() < 0.1, "cut must be optimal, got {e}");
    }

    #[test]
    fn hammer_matches_brute_force() {
        // Asymmetric costs incl. a case needing Rother truncation.
        let cases = [
            (0.0, 5.0, 1.0, 0.0),
            (2.0, 2.0, 2.0, 0.0),
            (10.0, 1.0, 1.0, 0.0), // A > B+C -> truncated to 2
            (0.0, 0.0, 0.0, 0.0),
            (3.0, 7.0, 7.0, 0.0),
        ];
        for (a0, b0, c0, d0) in cases {
            let mut a = a0;
            if a > b0 + c0 {
                a = b0 + c0;
            }
            let mut best = f64::INFINITY;
            for x in [false, true] {
                for y in [false, true] {
                    let e = match (x, y) {
                        (false, false) => a,
                        (false, true) => b0,
                        (true, false) => c0,
                        (true, true) => d0,
                    };
                    best = best.min(e);
                }
            }
            let mut g = StGraph::new(2);
            add_binary_pairwise(&mut g, 0, 1, a, b0, c0, d0);
            let keep = g.min_cut();
            // keep==true <=> S <=> x=false.
            let got = match (keep[0], keep[1]) {
                (true, true) => a,
                (true, false) => b0,
                (false, true) => c0,
                (false, false) => d0,
            };
            assert!(
                (got - best).abs() < 0.1,
                "case ({a0},{b0},{c0},{d0}): cut energy {got} != brute min {best}"
            );
        }
    }

    #[test]
    fn quantisation_floors_negatives_like_python() {
        // Python: -3 // 2 * 2 == -4. Truncation would give -2.
        assert_eq!((-3i32).div_euclid(2) * 2, -4);
        assert_eq!((-4i32).div_euclid(2) * 2, -4);
        assert_eq!(3i32.div_euclid(2) * 2, 2);
    }

    #[test]
    fn valid_source_mask_needs_full_patch() {
        // 7x7, single unknown centre pixel, patch=3: neighbours of the hole
        // are invalid donors, far pixels valid.
        let mut known = vec![true; 49];
        known[3 * 7 + 3] = false;
        let valid = valid_source_mask(&known, 7, 7, 3);
        assert!(!valid[3 * 7 + 3]);
        assert!(!valid[3 * 7 + 2], "patch centred here covers the hole");
        assert!(!valid[2 * 7 + 3]);
        assert!(valid[0], "far corner fully known");
        assert!(valid[6 * 7 + 6]);
    }

    #[test]
    fn shiftmap_completes_on_stripes_and_keeps_background() {
        // Vertical stripes; hole over the middle. Donor material exists on
        // both sides, so the fill must reconstruct stripes, not smear.
        let mut img = RgbImage::new(48, 48);
        for y in 0..48 {
            for x in 0..48 {
                let v = if (x / 6) % 2 == 0 { 30 } else { 220 };
                img.put_pixel(x, y, Rgb([v, v, v]));
            }
        }
        let mut mask = GrayImage::from_pixel(48, 48, Luma([0]));
        for y in 16..32 {
            for x in 16..32 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }
        let out = shiftmap_inpaint_rgb(&img, &mask);
        assert_eq!(out.dimensions(), (48, 48));
        // Background untouched.
        assert_eq!(*out.get_pixel(0, 0), *img.get_pixel(0, 0));
        assert_eq!(*out.get_pixel(47, 47), *img.get_pixel(47, 47));
        // Hole filled with *some* image content (not left white/black flat
        // in a way that ignores stripes): variance across the hole must be
        // non-trivial since stripes continue through it.
        let mut vals = Vec::new();
        for y in 18..30 {
            for x in 18..30 {
                vals.push(out.get_pixel(x, y)[0] as f64);
            }
        }
        let mean = vals.iter().sum::<f64>() / vals.len() as f64;
        let var = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / vals.len() as f64;
        assert!(var.sqrt() > 20.0, "stripes should continue through hole, std={}", var.sqrt());
    }

    #[test]
    fn degenerate_full_mask_does_not_panic() {
        let img = RgbImage::from_pixel(24, 24, Rgb([90, 90, 90]));
        let mask = GrayImage::from_pixel(24, 24, Luma([255]));
        let out = shiftmap_inpaint_rgb(&img, &mask);
        assert_eq!(out.dimensions(), (24, 24));
    }
}
