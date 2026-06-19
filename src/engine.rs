//! Engine: load a job → floorplan → thermal solve → report.
//!
//! Mirrors the other engines' shape: files in, a report (text or JSON) out, no
//! subprocess. `vyges-power` supplies the per-block power; this engine turns it
//! into a temperature field. HotSpot is the correlation baseline (see
//! `correlation/`), not a runtime dependency.

use crate::floorplan::Floorplan;
use crate::job::ThermalJob;
use crate::thermal::{self, ThermalReport};

pub fn analyze_job(job: &ThermalJob) -> Result<ThermalReport, String> {
    let fp = Floorplan::load(&job.resolve(&job.floorplan)).map_err(|e| e.to_string())?;
    thermal::analyze(job, &fp).map_err(|e| e.to_string())
}

pub fn passes(rep: &ThermalReport) -> bool {
    rep.passes()
}

/// A tiny built-in design (no files) — `vyges-thermal demo`.
pub fn demo() -> (ThermalJob, ThermalReport) {
    let job = ThermalJob::parse(DEMO_JOB, "").expect("demo job");
    let fp = Floorplan::parse(DEMO_FLP).expect("demo floorplan");
    let rep = thermal::analyze(&job, &fp).expect("demo analyze");
    (job, rep)
}

const DEMO_JOB: &str = "\
design: demo
floorplan: demo.flp
die_um: 200 200
grid: 20 20
thickness_um: 280
theta_ja: 20.0
ambient_c: 25.0
t_limit_c: 105.0
";

// a hot block in one corner + a cooler block elsewhere
const DEMO_FLP: &str = "\
hot   20 20 40 40 0.40 0.10
warm  130 130 50 50 0.10
";

// ---- rendering -----------------------------------------------------------------

fn fmt_power(w: f64) -> String {
    if w.abs() >= 1.0 {
        format!("{w:.3} W")
    } else if w.abs() >= 1e-3 {
        format!("{:.3} mW", w * 1e3)
    } else {
        format!("{:.3} µW", w * 1e6)
    }
}

pub fn render_report(rep: &ThermalReport) -> String {
    let mut s = String::new();
    s.push_str(&format!("vyges-thermal — {}\n", rep.design));
    s.push_str(&format!("  ambient          {:.3} °C\n", rep.ambient_c));
    s.push_str(&format!(
        "  die              {:.1} × {:.1} µm   grid {}×{}\n",
        rep.die_w_um, rep.die_h_um, rep.nx, rep.ny
    ));
    let drop = if rep.dropped_w > 0.0 {
        format!("   ({} off-die, dropped)", fmt_power(rep.dropped_w))
    } else {
        String::new()
    };
    s.push_str(&format!("  total power      {}{}\n", fmt_power(rep.total_power_w), drop));
    let cpl = if rep.coupled {
        format!(" ({} coupling iters)", rep.couple_iters)
    } else {
        String::new()
    };
    s.push_str(&format!("  analysis         {}{}\n", rep.mode(), cpl));
    let verdict = if rep.passes() { "PASS" } else { "FAIL" };
    s.push_str(&format!(
        "  peak temp        {:.2} °C  @ ({:.1}, {:.1}) µm   [{} vs {:.1} °C limit]\n",
        rep.tmax_c, rep.hot_x_um, rep.hot_y_um, verdict, rep.t_limit_c
    ));

    if !rep.blocks.is_empty() {
        let mut blks: Vec<_> = rep.blocks.iter().collect();
        blks.sort_by(|a, b| b.temp_c.partial_cmp(&a.temp_c).unwrap());
        s.push_str("\n  hottest blocks:\n");
        s.push_str(&format!("    {:<16} {:>9}  {:>10}\n", "block", "temp", "power"));
        for b in blks.iter().take(8) {
            s.push_str(&format!(
                "    {:<16} {:>6.2} °C  {:>10}\n",
                b.name,
                b.temp_c,
                fmt_power(b.power_w)
            ));
        }
    }

    if rep.nx <= 48 && rep.ny <= 48 {
        s.push_str("\n  temperature map (cool .  →  hot @):\n");
        s.push_str(&heatmap(rep));
    }
    s
}

fn heatmap(rep: &ThermalReport) -> String {
    const RAMP: &[u8] = b" .:-=+*#%@";
    let tmin = rep.temp_c.iter().cloned().fold(f64::INFINITY, f64::min);
    let span = (rep.tmax_c - tmin).max(1e-9);
    let mut s = String::new();
    for iy in (0..rep.ny).rev() {
        s.push_str("    ");
        for ix in 0..rep.nx {
            let t = rep.temp_c[iy * rep.nx + ix];
            let lvl = (((t - tmin) / span) * (RAMP.len() - 1) as f64).round() as usize;
            s.push(RAMP[lvl.min(RAMP.len() - 1)] as char);
        }
        s.push('\n');
    }
    s
}

pub fn report_json(rep: &ThermalReport) -> String {
    let mut s = String::new();
    s.push('{');
    s.push_str(&format!("\"design\":\"{}\",", rep.design));
    s.push_str(&format!("\"ambient_c\":{},", rep.ambient_c));
    s.push_str(&format!("\"t_limit_c\":{},", rep.t_limit_c));
    s.push_str(&format!("\"tmax_c\":{:.6},", rep.tmax_c));
    s.push_str(&format!("\"hot_x_um\":{:.3},\"hot_y_um\":{:.3},", rep.hot_x_um, rep.hot_y_um));
    s.push_str(&format!("\"pass\":{},", rep.passes()));
    s.push_str(&format!("\"total_power_w\":{:.9},", rep.total_power_w));
    s.push_str(&format!("\"dropped_w\":{:.9},", rep.dropped_w));
    s.push_str(&format!("\"coupled\":{},\"couple_iters\":{},", rep.coupled, rep.couple_iters));
    s.push_str(&format!("\"grid\":[{},{}],", rep.nx, rep.ny));
    s.push_str("\"blocks\":[");
    for (i, b) in rep.blocks.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "{{\"name\":\"{}\",\"temp_c\":{:.4},\"power_w\":{:.9}}}",
            b.name, b.temp_c, b.power_w
        ));
    }
    s.push_str("]}");
    s.push('\n');
    s
}
