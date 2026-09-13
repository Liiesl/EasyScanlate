//! Shared conjugate-gradient solver for `-Δ_h` SPD systems.
//!
//! Both [`crate::harmonic`] (Dirichlet Laplace fill) and [`crate::poisson`]
//! (seamless clone) assemble the same positive-definite matrix `A = -Δ_h`:
//! diagonal `deg[k]` = number of in-image neighbours (2..=4), off-diagonal
//! `-1` for in-hole neighbours, out-of-image neighbours dropped (Neumann).
//! Only the right-hand side differs, so one hand-rolled CG suffices — no
//! sparse-matrix dependency needed.

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
    let b_norm = rhs.iter().map(|v| v * v).sum::<f64>().sqrt();
    if b_norm == 0.0 {
        return Some(vec![0.0; n]);
    }
    let tol = 1e-4 * b_norm;
    let max_iter = 2000usize;

    let mut x = vec![0.0f64; n];
    let mut r = rhs.to_vec();
    let mut p = rhs.to_vec();
    let mut rs_old = dot(&r, &r);
    if rs_old.sqrt() <= tol {
        return Some(x);
    }
    let mut ap = vec![0.0f64; n];
    for _ in 0..max_iter {
        matvec(deg, nbrs, &p, &mut ap);
        let p_ap = dot(&p, &ap);
        if !p_ap.is_finite() || p_ap <= 0.0 {
            return None; // breakdown: A lost definiteness numerically
        }
        let alpha = rs_old / p_ap;
        if !alpha.is_finite() {
            return None;
        }
        for k in 0..n {
            x[k] += alpha * p[k];
            r[k] -= alpha * ap[k];
        }
        let rs_new = dot(&r, &r);
        if rs_new.sqrt() <= tol {
            return Some(x);
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

/// `out = A·x` for the `-Δ_h` matrix: `out[k] = deg[k]*x[k] - Σ x[nbr]`.
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
}
