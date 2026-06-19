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
    assert!(rep.passes(), "peak {:.1} should be under the 105 °C limit", rep.tmax_c);

    // the CPU (highest power) should be the hottest block
    let hottest = rep.blocks.iter().max_by(|a, b| a.temp_c.partial_cmp(&b.temp_c).unwrap()).unwrap();
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
    assert!(coupled.tmax_c > base.tmax_c, "{} !> {}", coupled.tmax_c, base.tmax_c);
}

#[test]
fn json_has_pass_and_hotspot() {
    let (_j, rep) = engine::demo();
    let js = engine::report_json(&rep);
    assert!(js.contains("\"tmax_c\""));
    assert!(js.contains("\"pass\""));
    assert!(js.contains("\"hot_x_um\""));
}
