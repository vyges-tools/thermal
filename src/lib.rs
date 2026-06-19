//! vyges-thermal — steady-state on-chip thermal analysis.
//!
//! Completes the power-integrity spine: `vyges-char` gives cell energy,
//! `vyges-power` turns activity into per-instance power, `vyges-em-ir` lands
//! that current on the supply mesh — and this engine lands the same power as
//! *heat* on the die and solves for temperature. It is the electrical DUAL of
//! `vyges-em-ir`: the steady state is `G_th · ΔT = P`, the same symmetric,
//! diagonally-dominant system em-ir solves for `G · V = I`, so the Gauss-Seidel
//! kernel is shared in spirit (see [`solver`]).
//!
//! Boundaries (per the Vyges flow architecture): inputs and outputs are files
//! (a floorplan power map in, a temperature report out). The whole v0 is pure
//! std and unit-tested offline — no subprocess, no deps. **HotSpot** (the
//! canonical open on-chip thermal simulator) is the correlation baseline, not a
//! runtime dependency.
//!
//! v0 scope: a uniform grid over the die; lateral silicon conduction + a
//! vertical junction-to-ambient path; steady-state solve; and an **electro-
//! thermal coupling** loop (leakage rises with temperature → re-solve to a fixed
//! point). Depth reserved: DEF-based instance placement via the **vyges-loom**
//! foundation (instead of a described floorplan), a layered/3-D package stack,
//! transient response, and temperature-dependent wire resistance fed back to
//! `vyges-em-ir`.

pub mod engine;
pub mod floorplan;
pub mod job;
pub mod solver;
pub mod thermal;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COPYRIGHT: &str = "© 2026 Vyges. All Rights Reserved.  https://vyges.com";
