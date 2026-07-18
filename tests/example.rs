//! End-to-end: load the example job, analyze, and check the report + coupling.

use vyges_thermal::engine;
use vyges_thermal::job::ThermalJob;

#[test]
fn example_block_runs_end_to_end() {
    let job = ThermalJob::load("examples/block/block.thermal").expect("load job");
    let rep = engine::analyze_job(&job).expect("analyze");

    assert_eq!(rep.blocks.len(), 4);
    assert!(rep.tmax_c > rep.ambient_c, "die heats above ambient");
    assert!(rep.dropped_w.abs() < 1e-9, "all blocks fit on-die");
    assert!(
        rep.passes(),
        "peak {:.1} should be under the 105 °C limit",
        rep.tmax_c
    );

    // the CPU (highest power) should be the hottest block
    let hottest = rep
        .blocks
        .iter()
        .max_by(|a, b| a.temp_c.partial_cmp(&b.temp_c).unwrap())
        .unwrap();
    assert_eq!(hottest.name, "cpu");

    // coupling is on in the example -> it iterated and grew the leakage power
    assert!(rep.coupled && rep.couple_iters > 0);
    let nominal: f64 = 0.30 + 0.20 + 0.05 + 0.02;
    assert!(rep.total_power_w > nominal, "leakage rose with temperature");
}

#[test]
fn coupling_increases_peak_vs_uncoupled() {
    let mut job = ThermalJob::load("examples/block/block.thermal").unwrap();
    let coupled = engine::analyze_job(&job).unwrap();
    job.leak_alpha_per_c = 0.0; // disable electro-thermal feedback
    let base = engine::analyze_job(&job).unwrap();
    assert!(!base.coupled);
    assert!(
        coupled.tmax_c > base.tmax_c,
        "{} !> {}",
        coupled.tmax_c,
        base.tmax_c
    );
}

// --- Analog (mixed-signal) coverage: RF power amplifier ----------------------
// The solve is physics/geometry only — it needs per-block power, not a digital
// netlist — so an analog power map is just another `.flp`. The PA output stage
// dissipates ~10× its neighbours and must come out as the hotspot.

#[test]
fn analog_pa_output_stage_is_the_hotspot() {
    let job = ThermalJob::load("examples/pa/pa.thermal").expect("load pa job");
    let rep = engine::analyze_job(&job).expect("analyze");

    assert_eq!(rep.blocks.len(), 5);
    assert!(rep.tmax_c > rep.ambient_c, "die heats above ambient");
    assert!(rep.dropped_w.abs() < 1e-9, "all blocks fit on-die");

    // the PA output stage (highest power) is the hottest block ...
    let hottest = rep
        .blocks
        .iter()
        .max_by(|a, b| a.temp_c.partial_cmp(&b.temp_c).unwrap())
        .unwrap();
    assert_eq!(hottest.name, "pa_output");

    // ... and the grid hotspot sits at/near it (block is centred on the die).
    assert!(
        (rep.hot_x_um - 300.0).abs() < 60.0,
        "hotspot x {}",
        rep.hot_x_um
    );
    assert!(
        (rep.hot_y_um - 300.0).abs() < 60.0,
        "hotspot y {}",
        rep.hot_y_um
    );

    // the high-power PA runs hotter than the low-power balun next to it
    // (silicon spreads heat, so the gradient is modest but the ordering holds).
    let balun = rep.blocks.iter().find(|b| b.name == "balun").unwrap();
    assert!(
        hottest.temp_c > balun.temp_c,
        "PA {} should be hotter than balun {}",
        hottest.temp_c,
        balun.temp_c
    );
}

#[test]
fn analog_pa_delta_t_scales_with_power() {
    // Doubling every block's power ~doubles the temperature rise (linear solve;
    // coupling off so leakage feedback doesn't perturb the linearity).
    let mut job = ThermalJob::load("examples/pa/pa.thermal").unwrap();
    job.leak_alpha_per_c = 0.0;
    let base = engine::analyze_job(&job).unwrap();

    let fp = vyges_thermal::floorplan::Floorplan::load(&job.resolve(&job.floorplan)).unwrap();
    let doubled = vyges_thermal::floorplan::Floorplan {
        blocks: fp
            .blocks
            .iter()
            .map(|b| {
                let mut b = b.clone();
                b.power_w *= 2.0;
                b.leak_w *= 2.0;
                b
            })
            .collect(),
    };
    let rep2 = vyges_thermal::thermal::analyze(&job, &doubled).unwrap();

    let rise1 = base.tmax_c - base.ambient_c;
    let rise2 = rep2.tmax_c - rep2.ambient_c;
    assert!(
        (rise2 / rise1 - 2.0).abs() < 1e-3,
        "rise {rise1} -> {rise2} not ~2×"
    );
}

#[test]
fn json_has_pass_and_hotspot() {
    let (_j, rep) = engine::demo();
    let js = engine::report_json(&rep);
    assert!(js.contains("\"tmax_c\""));
    assert!(js.contains("\"pass\""));
    assert!(js.contains("\"hot_x_um\""));
}
