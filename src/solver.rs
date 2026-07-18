//! Gauss-Seidel solver for the thermal conductance system `G_th · T = P`.
//!
//! This is the exact electrical DUAL of vyges-em-ir's PDN solver — temperature
//! rise ↔ node voltage, power ↔ injected current, thermal conductance ↔
//! electrical conductance, the ambient ↔ the supply pads. The reduced system is
//! symmetric positive-definite and diagonally dominant (each diagonal is the sum
//! of incident conductances — lateral neighbours plus the vertical path to
//! ambient — ≥ the off-diagonal sum, strictly so for any tile with a vertical
//! path), so Gauss-Seidel converges. Row `k` is
//! `T[k] ← (rhs[k] + Σ g·T[j]) / diag[k]` over its free neighbours, accelerated
//! by successive over-relaxation (SOR, ω≈1.7) — the system is SPD so 1<ω<2 stays
//! convergent and cuts iterations several-fold on stiff (fine-grid) problems.
//!
//! Pure std — unit-tested on small networks with closed-form answers.

/// Over-relaxation factor. 1.0 = plain Gauss-Seidel; 1<ω<2 accelerates the SPD
/// solve. 1.7 is a robust default for these 2-D conduction grids.
const OMEGA: f64 = 1.7;

#[derive(Debug)]
pub struct LinSys {
    pub n: usize,
    pub diag: Vec<f64>,
    pub offdiag: Vec<Vec<(usize, f64)>>, // (neighbour, conductance)
    pub rhs: Vec<f64>,
}

#[derive(Debug)]
pub enum SolveError {
    Singular(usize),   // tile with zero diagonal (no thermal path to ambient)
    NotConverged(f64), // residual after the iteration cap
}

impl std::fmt::Display for SolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SolveError::Singular(k) => {
                write!(
                    f,
                    "singular thermal grid: tile index {k} has no path to ambient"
                )
            }
            SolveError::NotConverged(r) => write!(f, "solver did not converge (residual {r:.3e})"),
        }
    }
}
impl std::error::Error for SolveError {}

impl LinSys {
    pub fn new(n: usize) -> LinSys {
        LinSys {
            n,
            diag: vec![0.0; n],
            offdiag: vec![Vec::new(); n],
            rhs: vec![0.0; n],
        }
    }

    /// Solve via Gauss-Seidel. `tol` is the max per-node update; `max_iter` caps work.
    pub fn solve(&self, max_iter: usize, tol: f64) -> Result<Vec<f64>, SolveError> {
        for k in 0..self.n {
            if self.diag[k] == 0.0 {
                return Err(SolveError::Singular(k));
            }
        }
        let mut x = vec![0.0f64; self.n];
        let mut last_delta = f64::INFINITY;
        for _ in 0..max_iter {
            let mut delta = 0.0f64;
            for k in 0..self.n {
                let mut acc = self.rhs[k];
                for &(j, g) in &self.offdiag[k] {
                    acc += g * x[j];
                }
                let xgs = acc / self.diag[k];
                let xk = x[k] + OMEGA * (xgs - x[k]); // SOR
                delta = delta.max((xk - x[k]).abs());
                x[k] = xk;
            }
            last_delta = delta;
            if delta < tol {
                return Ok(x);
            }
        }
        Err(SolveError::NotConverged(last_delta))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // One node tied to ambient through a single conductance g: g·T = P -> T = P/g.
    #[test]
    fn single_node_closed_form() {
        let mut s = LinSys::new(1);
        s.diag[0] = 0.04; // g = 1/25 (theta_ja = 25 K/W)
        s.rhs[0] = 2.0; // 2 W
        let t = s.solve(1000, 1e-12).unwrap();
        assert!(
            (t[0] - 50.0).abs() < 1e-6,
            "2 W * 25 K/W = 50 K, got {}",
            t[0]
        );
    }

    // Two tiles, symmetric: each has vertical g_v and they share a lateral g_l.
    // By symmetry both reach the same T = P/g_v (the lateral link carries no heat).
    #[test]
    fn symmetric_pair() {
        let (gv, gl) = (0.04, 1.0);
        let mut s = LinSys::new(2);
        s.diag[0] = gv + gl;
        s.diag[1] = gv + gl;
        s.offdiag[0].push((1, gl));
        s.offdiag[1].push((0, gl));
        s.rhs[0] = 1.0;
        s.rhs[1] = 1.0;
        let t = s.solve(10_000, 1e-12).unwrap();
        assert!((t[0] - 25.0).abs() < 1e-4);
        assert!((t[0] - t[1]).abs() < 1e-9);
    }

    #[test]
    fn floating_tile_is_singular() {
        let s = LinSys::new(1); // diag stays 0
        assert!(matches!(s.solve(10, 1e-9), Err(SolveError::Singular(0))));
    }
}
