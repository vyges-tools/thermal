//! Steady-state on-chip thermal analysis.
//!
//! The die is discretised into an `nx × ny` grid of tiles. Heat conducts
//! laterally through silicon between adjacent tiles and vertically to ambient
//! through the package (junction-to-ambient resistance `theta_ja`, spread over
//! the die). With block powers binned onto the tiles, the steady state is the
//! linear system `G_th · ΔT = P` — the electrical dual of vyges-em-ir's PDN
//! solve — giving each tile's temperature rise above ambient.
//!
//! Conductances (SI; µm dimensions converted to metres):
//!   - lateral X: `k_si · (dy · t) / dx`,  lateral Y: `k_si · (dx · t) / dy`
//!   - vertical to ambient, per tile: `(1/theta_ja) / (nx·ny)` so the tiles in
//!     parallel sum to the die's `1/theta_ja`.
//!
//! Electro-thermal coupling: leakage rises with temperature. When
//! `leak_alpha_per_c > 0`, each block's leakage is updated as
//! `P_leak(T) = P_leak0 · (1 + α·(T − T0))` and the grid re-solved to a fixed
//! point — the `char → power → em-ir → thermal → power` loop, closed.

use crate::floorplan::Floorplan;
use crate::job::ThermalJob;
use crate::solver::{LinSys, SolveError};

const UM: f64 = 1.0e-6;
const MAX_SOLVER_ITER: usize = 200_000;
// Per-node update at convergence. 1 µK is far below the 0.01 °C we report, and
// keeps Gauss-Seidel iteration counts modest on stiff grids (high lateral vs low
// vertical conductance). Tighten if a layered package model is added.
const SOLVER_TOL_K: f64 = 1.0e-6;
const COUPLE_TOL_W: f64 = 1.0e-12;
const MAX_COUPLE_ITER: usize = 100;

struct Grid {
    nx: usize,
    ny: usize,
    dx_um: f64,
    dy_um: f64,
    gx: f64, // lateral X conductance (W/K)
    gy: f64, // lateral Y conductance (W/K)
    gv: f64, // per-tile vertical conductance to ambient (W/K)
}

impl Grid {
    fn of(job: &ThermalJob) -> Grid {
        let nx = job.nx;
        let ny = job.ny;
        let dx_um = job.die_w_um / nx as f64;
        let dy_um = job.die_h_um / ny as f64;
        let (dx, dy, t) = (dx_um * UM, dy_um * UM, job.thickness_um * UM);
        Grid {
            nx,
            ny,
            dx_um,
            dy_um,
            gx: job.k_si * (dy * t) / dx,
            gy: job.k_si * (dx * t) / dy,
            gv: (1.0 / job.theta_ja) / (nx * ny) as f64,
        }
    }

    fn idx(&self, ix: usize, iy: usize) -> usize {
        iy * self.nx + ix
    }

    /// Grid tile containing a point (µm), clamped to the die.
    fn tile_of(&self, x_um: f64, y_um: f64) -> usize {
        let ix = ((x_um / self.dx_um) as isize).clamp(0, self.nx as isize - 1) as usize;
        let iy = ((y_um / self.dy_um) as isize).clamp(0, self.ny as isize - 1) as usize;
        self.idx(ix, iy)
    }
}

/// Bin block powers onto the grid (area-weighted overlap). Returns the per-tile
/// power and any power dropped because a block extends past the die edge.
fn distribute(grid: &Grid, fp: &Floorplan, block_power: &[f64]) -> (Vec<f64>, f64) {
    let mut tile = vec![0.0f64; grid.nx * grid.ny];
    let mut dropped = 0.0;
    for (b, &p) in fp.blocks.iter().zip(block_power) {
        let area = (b.w_um * b.h_um).max(f64::MIN_POSITIVE);
        let mut placed = 0.0;
        let i0 = ((b.x_um / grid.dx_um) as isize).max(0) as usize;
        let j0 = ((b.y_um / grid.dy_um) as isize).max(0) as usize;
        let i1 = (((b.x_um + b.w_um) / grid.dx_um).ceil() as usize).min(grid.nx);
        let j1 = (((b.y_um + b.h_um) / grid.dy_um).ceil() as usize).min(grid.ny);
        for iy in j0..j1 {
            for ix in i0..i1 {
                let tx0 = ix as f64 * grid.dx_um;
                let ty0 = iy as f64 * grid.dy_um;
                let ox = (b.x_um + b.w_um).min(tx0 + grid.dx_um) - b.x_um.max(tx0);
                let oy = (b.y_um + b.h_um).min(ty0 + grid.dy_um) - b.y_um.max(ty0);
                if ox > 0.0 && oy > 0.0 {
                    let frac = (ox * oy) / area;
                    tile[grid.idx(ix, iy)] += frac * p;
                    placed += frac;
                }
            }
        }
        let miss = (1.0 - placed).max(0.0);
        if miss > 1e-9 {
            dropped += p * miss; // block extends past the die edge
        }
    }
    (tile, dropped)
}

fn build_system(grid: &Grid, tile_power: &[f64]) -> LinSys {
    let mut s = LinSys::new(grid.nx * grid.ny);
    for iy in 0..grid.ny {
        for ix in 0..grid.nx {
            let k = grid.idx(ix, iy);
            let mut diag = grid.gv;
            if ix + 1 < grid.nx {
                diag += grid.gx;
                s.offdiag[k].push((grid.idx(ix + 1, iy), grid.gx));
            }
            if ix > 0 {
                diag += grid.gx;
                s.offdiag[k].push((grid.idx(ix - 1, iy), grid.gx));
            }
            if iy + 1 < grid.ny {
                diag += grid.gy;
                s.offdiag[k].push((grid.idx(ix, iy + 1), grid.gy));
            }
            if iy > 0 {
                diag += grid.gy;
                s.offdiag[k].push((grid.idx(ix, iy - 1), grid.gy));
            }
            s.diag[k] = diag;
            s.rhs[k] = tile_power[k]; // ambient is the reference (ΔT = 0), adds nothing
        }
    }
    s
}

#[derive(Debug)]
pub struct BlockTemp {
    pub name: String,
    pub temp_c: f64,
    pub power_w: f64, // final power (post-coupling)
}

#[derive(Debug)]
pub struct ThermalReport {
    pub design: String,
    pub ambient_c: f64,
    pub t_limit_c: f64,
    pub nx: usize,
    pub ny: usize,
    pub die_w_um: f64,
    pub die_h_um: f64,
    pub temp_c: Vec<f64>, // per-tile absolute temperature (k = iy*nx + ix)
    pub tmax_c: f64,
    pub hot_x_um: f64,
    pub hot_y_um: f64,
    pub blocks: Vec<BlockTemp>,
    pub total_power_w: f64,
    pub dropped_w: f64,
    pub coupled: bool,
    pub couple_iters: usize,
}

impl ThermalReport {
    pub fn passes(&self) -> bool {
        self.tmax_c <= self.t_limit_c
    }
    pub fn mode(&self) -> &'static str {
        if self.coupled {
            "steady-state + electro-thermal"
        } else {
            "steady-state"
        }
    }
}

#[derive(Debug)]
pub enum ThermalError {
    Solve(SolveError),
}
impl std::fmt::Display for ThermalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThermalError::Solve(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for ThermalError {}

/// Solve the grid for the given per-block powers; returns absolute temperatures.
fn solve_grid(job: &ThermalJob, grid: &Grid, fp: &Floorplan, bp: &[f64]) -> Result<(Vec<f64>, f64), ThermalError> {
    let (tile_power, dropped) = distribute(grid, fp, bp);
    let sys = build_system(grid, &tile_power);
    let rise = sys.solve(MAX_SOLVER_ITER, SOLVER_TOL_K).map_err(ThermalError::Solve)?;
    let temp: Vec<f64> = rise.iter().map(|r| job.ambient_c + r).collect();
    Ok((temp, dropped))
}

pub fn analyze(job: &ThermalJob, fp: &Floorplan) -> Result<ThermalReport, ThermalError> {
    let grid = Grid::of(job);
    let dyn_p: Vec<f64> = fp.blocks.iter().map(|b| b.power_w - b.leak_w).collect();
    let leak0: Vec<f64> = fp.blocks.iter().map(|b| b.leak_w).collect();
    let alpha = job.leak_alpha_per_c;
    let coupled = alpha > 0.0 && leak0.iter().any(|&l| l > 0.0);

    let mut bp: Vec<f64> = fp.blocks.iter().map(|b| b.power_w).collect();
    let mut temp;
    let mut dropped;
    let mut iters = 0usize;
    loop {
        let r = solve_grid(job, &grid, fp, &bp)?;
        temp = r.0;
        dropped = r.1;
        if !coupled {
            break;
        }
        // update each block's leakage from its centroid temperature
        let mut max_change = 0.0f64;
        for (i, b) in fp.blocks.iter().enumerate() {
            let tcell = temp[grid.tile_of(b.cx_um(), b.cy_um())];
            let newp = dyn_p[i] + leak0[i] * (1.0 + alpha * (tcell - job.leak_t0_c));
            max_change = max_change.max((newp - bp[i]).abs());
            bp[i] = newp;
        }
        iters += 1;
        if max_change < COUPLE_TOL_W || iters >= MAX_COUPLE_ITER {
            break;
        }
    }

    // hotspot
    let mut hot = 0usize;
    for k in 1..temp.len() {
        if temp[k] > temp[hot] {
            hot = k;
        }
    }
    let (hot_ix, hot_iy) = (hot % grid.nx, hot / grid.nx);
    let blocks: Vec<BlockTemp> = fp
        .blocks
        .iter()
        .enumerate()
        .map(|(i, b)| BlockTemp {
            name: b.name.clone(),
            temp_c: temp[grid.tile_of(b.cx_um(), b.cy_um())],
            power_w: bp[i],
        })
        .collect();

    Ok(ThermalReport {
        design: job.design.clone(),
        ambient_c: job.ambient_c,
        t_limit_c: job.t_limit_c,
        nx: grid.nx,
        ny: grid.ny,
        die_w_um: job.die_w_um,
        die_h_um: job.die_h_um,
        tmax_c: temp[hot],
        hot_x_um: (hot_ix as f64 + 0.5) * grid.dx_um,
        hot_y_um: (hot_iy as f64 + 0.5) * grid.dy_um,
        temp_c: temp,
        blocks,
        total_power_w: bp.iter().sum(),
        dropped_w: dropped,
        coupled,
        couple_iters: iters,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(nx: usize, ny: usize, theta: f64) -> ThermalJob {
        ThermalJob::parse(
            &format!(
                "design: t\nfloorplan: t.flp\ndie_um: 100 100\ngrid: {nx} {ny}\n\
                 theta_ja: {theta}\nambient_c: 25\nt_limit_c: 105\n"
            ),
            "",
        )
        .unwrap()
    }

    #[test]
    fn uniform_power_equals_theta_ja_law() {
        // One tile covering the die, P over the whole die -> ΔT = P·theta_ja.
        let j = job(1, 1, 25.0);
        let fp = Floorplan::parse("blk 0 0 100 100 2.0\n").unwrap();
        let r = analyze(&j, &fp).unwrap();
        assert!((r.tmax_c - (25.0 + 50.0)).abs() < 1e-6, "got {}", r.tmax_c);
        assert!(r.dropped_w.abs() < 1e-12);
    }

    #[test]
    fn power_conserved_across_grid() {
        // A finer grid with the same total power conserves the average rise:
        // Σ(gv·ΔT) over tiles = P_total, and gv is uniform -> mean ΔT = P·theta_ja.
        let j = job(8, 8, 25.0);
        let fp = Floorplan::parse("blk 0 0 100 100 2.0\n").unwrap();
        let r = analyze(&j, &fp).unwrap();
        let mean_rise: f64 =
            r.temp_c.iter().map(|t| t - 25.0).sum::<f64>() / r.temp_c.len() as f64;
        assert!((mean_rise - 50.0).abs() < 1e-3, "mean rise {mean_rise}");
        // a centred point source makes the centre hotter than the mean
        assert!(r.tmax_c - 25.0 > mean_rise);
    }

    #[test]
    fn electrothermal_coupling_raises_temperature() {
        // leakage that grows with T must converge to a HIGHER temp than the
        // uncoupled solve (positive feedback, but bounded).
        let mut j = job(4, 4, 40.0);
        let fp = Floorplan::parse("blk 0 0 100 100 1.0 1.0\n").unwrap(); // all-leakage block
        let base = analyze(&j, &fp).unwrap();
        j.leak_alpha_per_c = 0.005; // 0.5%/°C
        let coupled = analyze(&j, &fp).unwrap();
        assert!(coupled.coupled && coupled.couple_iters > 0);
        assert!(coupled.tmax_c > base.tmax_c, "{} !> {}", coupled.tmax_c, base.tmax_c);
        assert!(coupled.total_power_w > 1.0, "leakage grew with T");
    }

    #[test]
    fn off_die_power_is_reported_not_silently_dropped() {
        let j = job(4, 4, 25.0);
        // block half off the right edge -> ~half its power dropped
        let fp = Floorplan::parse("blk 90 0 20 20 1.0\n").unwrap();
        let r = analyze(&j, &fp).unwrap();
        assert!(r.dropped_w > 0.4 && r.dropped_w < 0.6, "dropped {}", r.dropped_w);
    }
}
