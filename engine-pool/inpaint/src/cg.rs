//! Shared conjugate-gradient solver for `-Δ_h` SPD systems.
//!
//! Both [`crate::harmonic`] (Dirichlet Laplace fill) and [`crate::poisson`]
//! (seamless clone) assemble the same positive-definite matrix `A = -Δ_h`:
//! diagonal `deg[k]` = number of in-image neighbours (2..=4), off-diagonal
//! `-1` for in-hole neighbours, out-of-image neighbours dropped (Neumann).
//! Only the right-hand side differs, so one hand-rolled CG suffices — no
//! sparse-matrix dependency needed.
//!
//! Storage is CSR (`offsets`/`cols`) over contiguous memory; see
//! [`crate::laplace`] for assembly. The legacy `Vec<Vec<u32>>` entry point
//! is kept as a thin wrapper for tests.

/// 8-bit-aware early-exit: stop when the largest per-pixel update drops
/// below this (f64 intensity). `5e-4` is ~1/1000 LSB — even hundreds of
/// further iterations could not shift the u8 rounding by 1. This cuts the
/// over-solve tail of `rtol=1e-4` without visible change.
pub(crate) const ADAPTIVE_MAX_DX: f64 = 5e-4;

/// Reusable CG working storage. Lets sequential multi-solve callers
/// (multiple components / channels / boxes) reuse allocations instead of
/// reallocating `x/r/p/ap` per solve. Parallel paths give each Rayon task
/// its own scratch (no sharing across threads).
#[derive(Default)]
pub(crate) struct CgScratch {
    x: Vec<f64>,
    r: Vec<f64>,
    p: Vec<f64>,
    ap: Vec<f64>,
}

impl CgScratch {
    fn ensure(&mut self, n: usize) {
        if self.x.len() != n {
            self.x.resize(n, 0.0);
            self.r.resize(n, 0.0);
            self.p.resize(n, 0.0);
            self.ap.resize(n, 0.0);
        }
    }
}

/// Solves `A·x = rhs` with `A` given implicitly by `deg`/`nbrs`.
///
/// * `deg[k]` — diagonal entry (in-image neighbour count).
/// * `nbrs[k]` — in-hole neighbour indices (each contributes `-1`).
///
/// Zero initial guess, `rtol=1e-4` against `||rhs||`, `maxiter=2000`
/// (mirrors `scipy.sparse.linalg.cg(..., rtol=1e-4, maxiter=2000)` in the
/// Python reference). Returns `None` on breakdown / non-convergence so
/// callers can keep the original pixels instead of emitting garbage.
pub(crate) fn cg_solve(deg: &[f64], nbrs: &[Vec<u32>], rhs: &[f64]) -> Option<Vec<f64>> {
    let n = rhs.len();
    debug_assert_eq!(deg.len(), n);
    debug_assert_eq!(nbrs.len(), n);
    if n == 0 {
        return Some(Vec::new());
    }
    // Convert to CSR once (same neighbour order as `matvec` below, so the
    // iteration path is identical to the CSR fast path for single systems).
    let mut offsets = vec![0u32; n + 1];
    for (k, nb) in nbrs.iter().enumerate() {
        offsets[k + 1] = offsets[k] + nb.len() as u32;
    }
    let total = offsets[n] as usize;
    let mut cols = vec![0u32; total];
    let mut cursor = offsets[..n].to_vec();
    for (k, nb) in nbrs.iter().enumerate() {
        for &j in nb {
            let w = cursor[k] as usize;
            cols[w] = j;
            cursor[k] += 1;
        }
    }
    cg_solve_csr(deg, &offsets, &cols, rhs)
}

/// CSR solve with zero initial guess. Same `rtol`/`maxiter` fence as
/// [`cg_solve`], plus the [`ADAPTIVE_MAX_DX`] 8-bit early-exit.
pub(crate) fn cg_solve_csr(
    deg: &[f64],
    offsets: &[u32],
    cols: &[u32],
    rhs: &[f64],
) -> Option<Vec<f64>> {
    cg_solve_csr_warm(deg, offsets, cols, rhs, None)
}

/// CSR solve with an optional warm-start guess `x0` (e.g. boundary mean).
/// `None` means zero guess (scipy parity path). A constant-`x0` changes the
/// iteration path but converges to the same solution within `rtol`; u8
/// output is unaffected (validated by harmonic/poisson tests).
pub(crate) fn cg_solve_csr_warm(
    deg: &[f64],
    offsets: &[u32],
    cols: &[u32],
    rhs: &[f64],
    x0: Option<&[f64]>,
) -> Option<Vec<f64>> {
    let mut scratch = CgScratch::default();
    cg_solve_csr_inner(deg, offsets, cols, rhs, x0, &mut scratch).map(|_| {
        std::mem::take(&mut scratch.x)
    })
}

/// CSR solve reusing caller-provided scratch (sequential multi-solve fast
/// path). Returns the solution by cloning out of scratch; scratch keeps its
/// allocations for the next call. Returns `None` on breakdown.
pub(crate) fn cg_solve_csr_reuse(
    deg: &[f64],
    offsets: &[u32],
    cols: &[u32],
    rhs: &[f64],
    x0: Option<&[f64]>,
    scratch: &mut CgScratch,
) -> Option<Vec<f64>> {
    cg_solve_csr_inner(deg, offsets, cols, rhs, x0, scratch)?;
    Some(scratch.x.clone())
}

fn cg_solve_csr_inner(
    deg: &[f64],
    offsets: &[u32],
    cols: &[u32],
    rhs: &[f64],
    x0: Option<&[f64]>,
    scratch: &mut CgScratch,
) -> Option<()> {
    let n = rhs.len();
    debug_assert_eq!(deg.len(), n);
    debug_assert_eq!(offsets.len(), n + 1);
    if n == 0 {
        scratch.ensure(0);
        return Some(());
    }
    let b_norm = rhs.iter().map(|v| v * v).sum::<f64>().sqrt();
    if b_norm == 0.0 {
        scratch.ensure(n);
        scratch.x.fill(0.0);
        return Some(());
    }
    let tol = 1e-4 * b_norm;
    let max_iter = 2000usize;

    scratch.ensure(n);
    let CgScratch { x, r, p, ap } = scratch;

    if let Some(x0) = x0 {
        debug_assert_eq!(x0.len(), n);
        x.copy_from_slice(x0);
        // r = b - A·x0
        matvec_csr(deg, offsets, cols, x0, ap);
        for k in 0..n {
            r[k] = rhs[k] - ap[k];
        }
    } else {
        x.fill(0.0);
        r.copy_from_slice(rhs);
    }
    p.copy_from_slice(r);
    let mut rs_old = dot(r, r);
    if rs_old.sqrt() <= tol {
        if x0.is_none() {
            // zero-guess fast path already has x == 0
        }
        return Some(());
    }
    // If the warm guess is already within tol, `x` holds it.
    for _ in 0..max_iter {
        matvec_csr(deg, offsets, cols, p, ap);
        let p_ap = dot(p, ap);
        if !p_ap.is_finite() || p_ap <= 0.0 {
            return None; // breakdown: A lost definiteness numerically
        }
        let alpha = rs_old / p_ap;
        if !alpha.is_finite() {
            return None;
        }
        let mut max_dx: f64 = 0.0;
        for k in 0..n {
            let dx = alpha * p[k];
            let adx = dx.abs();
            if adx > max_dx {
                max_dx = adx;
            }
            x[k] += dx;
            r[k] -= alpha * ap[k];
        }
        let rs_new = dot(r, r);
        if rs_new.sqrt() <= tol {
            return Some(());
        }
        // 8-bit-aware early exit: further updates cannot move u8 rounding.
        if max_dx <= ADAPTIVE_MAX_DX {
            return Some(());
        }
        let beta = rs_new / rs_old;
        if !beta.is_finite() {
            return None;
        }
        for k in 0..n {
            p[k] = r[k] + beta * p[k];
        }
        rs_old = rs_new;
    }
    None
}

/// `out = A·x` for the `-Δ_h` matrix in CSR form:
/// `out[k] = deg[k]*x[k] - Σ x[cols[j]]`.
fn matvec_csr(deg: &[f64], offsets: &[u32], cols: &[u32], x: &[f64], out: &mut [f64]) {
    for (k, o) in out.iter_mut().enumerate() {
        let mut acc = deg[k] * x[k];
        let s = offsets[k] as usize;
        let e = offsets[k + 1] as usize;
        for &j in &cols[s..e] {
            acc -= x[j as usize];
        }
        *o = acc;
    }
}

/// `out = A·x` for the legacy `Vec<Vec<u32>>` form (tests only).
#[allow(dead_code)]
fn matvec(deg: &[f64], nbrs: &[Vec<u32>], x: &[f64], out: &mut [f64]) {
    for (k, o) in out.iter_mut().enumerate() {
        let mut acc = deg[k] * x[k];
        for &j in &nbrs[k] {
            acc -= x[j as usize];
        }
        *o = acc;
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b.iter()).map(|(u, v)| u * v).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_csr(nbrs: &[Vec<u32>]) -> (Vec<u32>, Vec<u32>) {
        let n = nbrs.len();
        let mut offsets = vec![0u32; n + 1];
        for (k, nb) in nbrs.iter().enumerate() {
            offsets[k + 1] = offsets[k] + nb.len() as u32;
        }
        let total = offsets[n] as usize;
        let mut cols = vec![0u32; total];
        let mut cursor = offsets[..n].to_vec();
        for (k, nb) in nbrs.iter().enumerate() {
            for &j in nb {
                let w = cursor[k] as usize;
                cols[w] = j;
                cursor[k] += 1;
            }
        }
        (offsets, cols)
    }

    #[test]
    fn cg_solves_small_spd_system() {
        // A = [[2,-1],[-1,2]], b = [1,1] -> x = [1,1].
        let deg = vec![2.0, 2.0];
        let nbrs = vec![vec![1], vec![0]];
        let x = cg_solve(&deg, &nbrs, &[1.0, 1.0]).expect("must converge");
        assert!((x[0] - 1.0).abs() < 1e-6);
        assert!((x[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cg_zero_rhs_returns_zeros() {
        let deg = vec![4.0, 4.0];
        let nbrs = vec![vec![1], vec![0]];
        let x = cg_solve(&deg, &nbrs, &[0.0, 0.0]).expect("zero rhs");
        assert_eq!(x, vec![0.0, 0.0]);
    }

    #[test]
    fn cg_empty_system_returns_empty() {
        let x = cg_solve(&[], &[], &[]).expect("empty");
        assert!(x.is_empty());
    }

    #[test]
    fn csr_matches_legacy_path() {
        let deg = vec![4.0, 4.0, 4.0, 4.0];
        let nbrs = vec![vec![1, 2], vec![0, 3], vec![0, 3], vec![1, 2]];
        let b = vec![10.0, 20.0, 30.0, 40.0];
        let legacy = cg_solve(&deg, &nbrs, &b).expect("legacy");
        let (offsets, cols) = to_csr(&nbrs);
        let csr = cg_solve_csr(&deg, &offsets, &cols, &b).expect("csr");
        for (a, c) in legacy.iter().zip(csr.iter()) {
            assert!((a - c).abs() < 1e-9, "{legacy:?} vs {csr:?}");
        }
    }

    #[test]
    fn warm_start_converges_to_same_solution() {
        let deg = vec![4.0, 4.0, 4.0, 4.0];
        let nbrs = vec![vec![1, 2], vec![0, 3], vec![0, 3], vec![1, 2]];
        let b = vec![100.0, 100.0, 200.0, 200.0];
        let (offsets, cols) = to_csr(&nbrs);
        let cold = cg_solve_csr(&deg, &offsets, &cols, &b).expect("cold");
        let mean = b.iter().sum::<f64>() / b.len() as f64 / 2.0;
        let x0 = vec![mean; b.len()];
        let warm = cg_solve_csr_warm(&deg, &offsets, &cols, &b, Some(&x0)).expect("warm");
        for (a, w) in cold.iter().zip(warm.iter()) {
            assert!((a - w).abs() < 1e-3, "{cold:?} vs {warm:?}");
        }
    }
}
