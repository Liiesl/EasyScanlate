//! Shared Laplace assembly/solve helpers for [`crate::harmonic`] and
//! [`crate::poisson`].
//!
//! Both backends assemble the same SPD matrix `A = -Δ_h` in CSR form
//! (`deg` + `offsets`/`cols`) and differ only in the right-hand side.
//! This module owns the bits that are identical:
//!
//! * [`build_idx`] — compact `i32` hole LUT (`-1` = known). Replaces the old
//!   `w*h` `i64` table (~66MB for a 4K crop) with 4 bytes/entry.
//! * [`label_components`] — split disconnected hole components over the CSR
//!   graph. Components are provably decoupled (no `-1` edges between them),
//!   so per-component solves are better conditioned and trivially parallel.
//!   Single-blob masks (the common case) return one group and take a
//!   zero-copy fast path to preserve scipy-parity bit-for-bit.
//! * [`solve_grouped`] — solve all channels over all components. Single
//!   group: 3–4 channel solves in parallel via Rayon with a boundary-mean
//!   warm start. Multi-group: `(group, channel)` jobs in parallel, then
//!   scatter. **Any** channel/component failure returns `None` so callers
//!   fall back at the whole-patch level (never per-channel ghosts).
//!
//! Solver fence: `rtol=1e-4`, `maxiter=2000`, f64 (see [`crate::cg`]), plus
//! the 8-bit adaptive early-exit and boundary-mean warm start. Both change
//! the iteration path versus pure zero-guess CG but converge within `rtol`;
//! u8 output is validated by the harmonic/poisson numeric tests including
//! the multi-blob parity fixture.
//!
//! Deferred (documented, not implemented): Jacobi is a no-op for interior
//! holes (`deg=4` uniform → `D=4I`, CG invariant) and only helps
//! border-touching holes — profile before adding. Mixed-precision (f32
//! state, f64 accumulators) is gated: it changes the iteration path, so it
//! stays off until validated against the numeric tests — f64 ships.
//! Full geometric multigrid needs mask-aware restriction/prolongation with
//! known pixels pinned; a two-level correction is the sane middle path if
//! giant masks complain. Soft masks, biharmonic (`Δ²`), edge-aware /
//! anisotropic diffusion and linear-light/Lab solves are quality backlog —
//! they change the PDE and break scipy parity, so they stay behind separate
//! gates. Boundary stays Neumann (drop out-of-image) for parity; mirror-pad
//! / Dirichlet extrapolation is the known visual alternative.

use rayon::prelude::*;

/// Build the compact hole index table.
///
/// Returns `(idx, ys, xs)` where `idx[y*w+x]` is the hole-row id or `-1`,
/// and `ys[k]/xs[k]` is the image position of row `k`. `None` when
/// `w*h` exceeds `i32::MAX` (caller treats as degenerate and clones).
pub(crate) fn build_idx(
    w: usize,
    h: usize,
    is_hole: impl Fn(usize, usize) -> bool,
) -> Option<(Vec<i32>, Vec<usize>, Vec<usize>)> {
    let area = (w as u64) * (h as u64);
    if area > i32::MAX as u64 {
        return None;
    }
    let mut idx = vec![-1i32; w * h];
    let mut ys: Vec<usize> = Vec::new();
    let mut xs: Vec<usize> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            if is_hole(x, y) {
                idx[y * w + x] = ys.len() as i32;
                ys.push(y);
                xs.push(x);
            }
        }
    }
    Some((idx, ys, xs))
}

/// Flood-fill disconnected components over the CSR hole graph.
///
/// `offsets`/`cols` is the in-hole adjacency (`-1` entries of `A`).
/// Returns one `Vec<u32>` of global row ids per component, in
/// discovery order. Empty system returns empty vec.
pub(crate) fn label_components(n: usize, offsets: &[u32], cols: &[u32]) -> Vec<Vec<u32>> {
    if n == 0 {
        return Vec::new();
    }
    debug_assert_eq!(offsets.len(), n + 1);
    let mut comp_of = vec![u32::MAX; n];
    let mut groups: Vec<Vec<u32>> = Vec::new();
    let mut stack: Vec<u32> = Vec::new();
    for seed in 0..n {
        if comp_of[seed] != u32::MAX {
            continue;
        }
        let cid = groups.len() as u32;
        let mut group = Vec::new();
        stack.clear();
        stack.push(seed as u32);
        comp_of[seed] = cid;
        while let Some(k) = stack.pop() {
            group.push(k);
            let s = offsets[k as usize] as usize;
            let e = offsets[k as usize + 1] as usize;
            for &j in &cols[s..e] {
                let ju = j as usize;
                if comp_of[ju] == u32::MAX {
                    comp_of[ju] = cid;
                    stack.push(j);
                }
            }
        }
        groups.push(group);
    }
    groups
}

/// Boundary-mean warm guess for one channel: `Σrhs / Σknown`, where
/// `known[k] = deg[k] - in_hole_degree[k]`. Falls back to 0 when nothing
/// is known (whole-image hole → zero-rhs fast path handles it anyway).
fn warm_mean(rhs: &[f64], deg: &[f64], offsets: &[u32]) -> f64 {
    let mut sum_rhs = 0.0;
    let mut sum_known = 0.0;
    for k in 0..rhs.len() {
        let hole_deg = (offsets[k + 1] - offsets[k]) as f64;
        let known = deg[k] - hole_deg;
        sum_rhs += rhs[k];
        sum_known += known;
    }
    if sum_known > 0.0 {
        sum_rhs / sum_known
    } else {
        0.0
    }
}

/// Solve all channels over all components.
///
/// * `deg/offsets/cols` — global CSR system, `n` rows.
/// * `rhs` — per-channel right-hand sides, each length `n` (3 for RGB,
///   4 for RGBA, 1 for alpha plane).
///
/// Returns per-channel global solutions on success, `None` if **any**
/// channel/component fails (caller falls back whole-patch).
pub(crate) fn solve_grouped(
    deg: &[f64],
    offsets: &[u32],
    cols: &[u32],
    rhs: &[Vec<f64>],
) -> Option<Vec<Vec<f64>>> {
    let n = deg.len();
    let nch = rhs.len();
    if nch == 0 {
        return Some(Vec::new());
    }
    for b in rhs {
        debug_assert_eq!(b.len(), n);
    }
    if n == 0 {
        return Some(rhs.iter().map(|_| Vec::new()).collect());
    }

    let groups = label_components(n, offsets, cols);

    // Fast path: single component → solve global system directly with a
    // boundary-mean warm start. No remap, no copy of the matrix.
    // Small systems (<1k rows: single glyphs, previews) solve sequentially
    // reusing one scratch (no Rayon spawn overhead, no realloc per channel);
    // larger systems use one Rayon job per channel.
    if groups.len() <= 1 {
        if n < 1024 {
            let mut scratch = crate::cg::CgScratch::default();
            let mut out = Vec::with_capacity(nch);
            for b in rhs {
                let mean = warm_mean(b, deg, offsets);
                let sol = if mean == 0.0 {
                    crate::cg::cg_solve_csr_reuse(deg, offsets, cols, b, None, &mut scratch)
                } else {
                    let x0 = vec![mean; n];
                    crate::cg::cg_solve_csr_reuse(deg, offsets, cols, b, Some(&x0), &mut scratch)
                };
                out.push(sol?);
            }
            return Some(out);
        }
        let sols: Vec<Option<Vec<f64>>> = rhs
            .par_iter()
            .map(|b| {
                let mean = warm_mean(b, deg, offsets);
                if mean == 0.0 {
                    crate::cg::cg_solve_csr(deg, offsets, cols, b)
                } else {
                    let x0 = vec![mean; n];
                    crate::cg::cg_solve_csr_warm(deg, offsets, cols, b, Some(&x0))
                }
            })
            .collect();
        let mut out = Vec::with_capacity(nch);
        for s in sols {
            out.push(s?);
        }
        return Some(out);
    }

    // Multi-component path: build one local CSR per group (partition of the
    // global matrix), then solve (group, channel) jobs in parallel.
    struct Local {
        deg: Vec<f64>,
        offsets: Vec<u32>,
        cols: Vec<u32>,
        rhs: Vec<Vec<f64>>,
        warm: Vec<f64>,
        rows: Vec<u32>,
    }
    // Global→local lookup reused across groups (reset per group).
    let mut g2l = vec![-1i32; n];
    let mut locals: Vec<Local> = Vec::with_capacity(groups.len());
    for group in &groups {
        let m = group.len();
        for (li, &g) in group.iter().enumerate() {
            g2l[g as usize] = li as i32;
        }
        let mut ldeg = vec![0.0f64; m];
        let mut counts = vec![0u32; m];
        for (li, &g) in group.iter().enumerate() {
            let gu = g as usize;
            ldeg[li] = deg[gu];
            counts[li] = offsets[gu + 1] - offsets[gu];
        }
        let mut loff = vec![0u32; m + 1];
        for i in 0..m {
            loff[i + 1] = loff[i] + counts[i];
        }
        let total = loff[m] as usize;
        let mut lcols = vec![0u32; total];
        let mut cursor = loff[..m].to_vec();
        for (li, &g) in group.iter().enumerate() {
            let gu = g as usize;
            let s = offsets[gu] as usize;
            let e = offsets[gu + 1] as usize;
            for &gj in &cols[s..e] {
                let lj = g2l[gj as usize];
                debug_assert!(lj >= 0, "component leak: neighbour outside group");
                let w = cursor[li] as usize;
                lcols[w] = lj as u32;
                cursor[li] += 1;
            }
        }
        let mut lrhs: Vec<Vec<f64>> = Vec::with_capacity(nch);
        let mut lwarm: Vec<f64> = Vec::with_capacity(nch);
        for b in rhs {
            let mut lb = vec![0.0f64; m];
            for (li, &g) in group.iter().enumerate() {
                lb[li] = b[g as usize];
            }
            lwarm.push(warm_mean(&lb, &ldeg, &loff));
            lrhs.push(lb);
        }
        for &g in group {
            g2l[g as usize] = -1;
        }
        locals.push(Local {
            deg: ldeg,
            offsets: loff,
            cols: lcols,
            rhs: lrhs,
            warm: lwarm,
            rows: group.clone(),
        });
    }

    // Flatten to (group, channel) pairs for full Rayon utilisation.
    let mut pairs: Vec<(usize, usize)> = Vec::with_capacity(locals.len() * nch);
    for gi in 0..locals.len() {
        for ci in 0..nch {
            pairs.push((gi, ci));
        }
    }
    let results: Vec<Option<Vec<f64>>> = pairs
        .par_iter()
        .map(|&(gi, ci)| {
            let l = &locals[gi];
            let mean = l.warm[ci];
            if mean == 0.0 {
                crate::cg::cg_solve_csr(&l.deg, &l.offsets, &l.cols, &l.rhs[ci])
            } else {
                let x0 = vec![mean; l.rows.len()];
                crate::cg::cg_solve_csr_warm(&l.deg, &l.offsets, &l.cols, &l.rhs[ci], Some(&x0))
            }
        })
        .collect();

    let mut out: Vec<Vec<f64>> = rhs.iter().map(|_| vec![0.0f64; n]).collect();
    for (pi, &(gi, ci)) in pairs.iter().enumerate() {
        let sol = results[pi].as_ref()?;
        let rows = &locals[gi].rows;
        debug_assert_eq!(sol.len(), rows.len());
        for (li, &g) in rows.iter().enumerate() {
            out[ci][g as usize] = sol[li];
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain_csr(n: usize) -> (Vec<f64>, Vec<u32>, Vec<u32>) {
        // 1-D chain: interior deg 2..3 with Neumann ends.
        let mut offsets = vec![0u32; n + 1];
        let mut cols: Vec<u32> = Vec::new();
        for k in 0..n {
            if k > 0 {
                cols.push((k - 1) as u32);
            }
            if k + 1 < n {
                cols.push((k + 1) as u32);
            }
            offsets[k + 1] = cols.len() as u32;
        }
        let mut deg = vec![0.0; n];
        for k in 0..n {
            let mut d = 0;
            if k > 0 {
                d += 1;
            }
            if k + 1 < n {
                d += 1;
            }
            // +1 known pin so the system is non-singular per row
            d += 1;
            deg[k] = d as f64;
        }
        (deg, offsets, cols)
    }

    #[test]
    fn single_chain_is_one_component() {
        let (deg, offsets, cols) = chain_csr(8);
        let g = label_components(deg.len(), &offsets, &cols);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].len(), 8);
    }

    #[test]
    fn two_chains_are_two_components() {
        // Two disjoint edges: {0-1} and {2-3}.
        let offsets = vec![0, 1, 2, 3, 4];
        let cols = vec![1, 0, 3, 2];
        let g = label_components(4, &offsets, &cols);
        assert_eq!(g.len(), 2);
    }

    #[test]
    fn grouped_solve_matches_global_single_blob() {
        let (deg, offsets, cols) = chain_csr(12);
        let b: Vec<f64> = (0..12).map(|k| (k as f64 + 1.0) * 10.0).collect();
        let global = crate::cg::cg_solve_csr(&deg, &offsets, &cols, &b).expect("global");
        let grouped = solve_grouped(&deg, &offsets, &cols, &[b.clone()]).expect("grouped");
        for (a, g) in global.iter().zip(grouped[0].iter()) {
            assert!((a - g).abs() < 1e-6, "{global:?} vs {:?}", grouped[0]);
        }
    }
}
