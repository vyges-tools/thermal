//! vyges-thermal CLI.
//!
//!   vyges-thermal run   JOB [-o OUT] [--json] [--fail-on-violation]  analyze -> report
//!   vyges-thermal check JOB                                          validate the job
//!   vyges-thermal demo  [-o OUT] [--json]                            built-in design
//!
//! Common flags: -h/--help, -V/--version, -q/--quiet, -v/--verbose.
//! Exit codes: 0 ok · 1 runtime/solver error · 2 usage/validation · 3 over the
//! temperature limit (only with --fail-on-violation).

use std::process::exit;

use vyges_thermal::engine;
use vyges_thermal::job::ThermalJob;
use vyges_thermal::thermal::ThermalReport;

const USAGE: &str = "\
vyges-thermal — steady-state on-chip thermal analysis (floorplan -> temperature)

usage:
  vyges-thermal run   JOB [-o OUT] [--json] [--fail-on-violation]
  vyges-thermal check JOB
  vyges-thermal demo  [-o OUT] [--json]

A JOB is a small declarative `.thermal` file (die + grid + material params +
a `floorplan:` of blocks with placement and power). With per-block leakage and
`leak_alpha_per_c` it runs the electro-thermal coupling loop. The report gives
the peak temperature, the hotspot location, per-block temperatures, and a
PASS/FAIL against `t_limit_c`.

flags:
  -o FILE               write output to FILE (default: stdout)
  --json                machine-readable JSON instead of the text report
  --fail-on-violation   exit 3 if the peak temperature exceeds t_limit_c
  -q, --quiet           suppress non-essential output
  -v, --verbose         extra detail on stderr
  --describe            print a machine-readable JSON description of the command
  -h, --help            show this help
  -V, --version         show version
  --bug-report      file a bug (central: vyges/community)
  --feature-request request a feature (central)
  --sponsor         sponsor Vyges (github.com/sponsors/vyges-ip)
  --star            star this tool on GitHub ⭐
";

const BUG_URL: &str =
    "https://github.com/vyges/community/issues/new?template=bug_report_template.yaml";
const FEATURE_URL: &str = "https://github.com/vyges/community/issues/new?labels=enhancement";
const SPONSOR_URL: &str = "https://github.com/sponsors/vyges-ip";
const STAR_URL: &str = "https://github.com/vyges-tools/thermal";

fn link(label: &str, url: &str) {
    use std::io::IsTerminal;
    println!("{label}:\n  {url}");
    if std::io::stdout().is_terminal() {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let _ = std::process::Command::new(opener).arg(url).status();
    }
}

#[derive(Default)]
struct Cli {
    positionals: Vec<String>,
    out: Option<String>,
    json: bool,
    quiet: bool,
    verbose: bool,
    fail_on_violation: bool,
    help: bool,
    version: bool,
    bug_report: bool,
    feature_request: bool,
    sponsor: bool,
    star: bool,
}

fn parse_cli(args: &[String]) -> Cli {
    let mut c = Cli::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                c.out = args.get(i + 1).cloned();
                i += 1;
            }
            "--json" => c.json = true,
            "--fail-on-violation" => c.fail_on_violation = true,
            "-q" | "--quiet" => c.quiet = true,
            "-v" | "--verbose" => c.verbose = true,
            "-h" | "--help" => c.help = true,
            "-V" | "--version" => c.version = true,
            "--bug-report" => c.bug_report = true,
            "--feature-request" => c.feature_request = true,
            "--sponsor" => c.sponsor = true,
            "--star" => c.star = true,
            other => c.positionals.push(other.to_string()),
        }
        i += 1;
    }
    c
}

/// Add `"report_path"` to a `--json` payload so the result says where its report landed.
///
/// String surgery rather than a JSON round-trip because this crate is std-only. Inserting
/// after the opening brace keeps every existing field untouched; an empty object gets no
/// trailing comma.
fn with_report_path(json: &str, path: Option<&str>) -> String {
    let (Some(p), Some(rest)) = (path, json.trim_start().strip_prefix('{')) else {
        return json.to_string();
    };
    let esc = p.replace('\\', "\\\\").replace('"', "\\\"");
    let sep = if rest.trim_start().starts_with('}') {
        ""
    } else {
        ","
    };
    format!("{{\"report_path\": \"{esc}\"{sep}{rest}")
}

/// Write the report, and — when `--json` — always put the machine payload on stdout.
///
/// `-o` used to redirect the JSON itself, so asking for the report file cost you the parsed
/// result: stdout carried only `wrote <path>`. Now the file still receives exactly what it
/// did before, the notice moves to stderr where status messages belong, and stdout keeps the
/// payload so a caller gets the verdict *and* the artifact.
fn write_out(text: &str, cli: &Cli) {
    match &cli.out {
        Some(path) => match std::fs::write(path, text) {
            Ok(_) => {
                if !cli.quiet {
                    eprintln!("wrote {path}");
                }
                if cli.json {
                    print!("{text}");
                }
            }
            Err(e) => {
                eprintln!("error: {path}: {e}");
                exit(1);
            }
        },
        None => print!("{text}"),
    }
}

/// Emit the vyges-events causal trail for the thermal verdict — to stderr (the
/// report goes to stdout / -o). code=THERMAL-* is the clustering key; objects are
/// the block/coordinate refs used for cross-stage co-reference. One warn per block
/// over `t_limit_c`, then always a completion summary (peak temp + hotspot count).
fn emit_thermal_events(rep: &ThermalReport) {
    use vyges_events::{Event, Severity};
    let mut hotspots = 0usize;
    for b in &rep.blocks {
        if b.temp_c > rep.t_limit_c {
            hotspots += 1;
            vyges_events::emit(
                &Event::new(
                    "vyges-thermal",
                    Severity::Warn,
                    format!(
                        "hotspot '{}' at {:.2} °C exceeds limit {:.2} °C ({:.4} W)",
                        b.name, b.temp_c, rep.t_limit_c, b.power_w
                    ),
                )
                .with_code("THERMAL-HOTSPOT")
                .with_objects(vec![format!("block:{}", b.name)]),
            );
        }
    }
    vyges_events::emit(
        &Event::new(
            "vyges-thermal",
            Severity::Info,
            format!(
                "thermal {} — peak {:.2} °C at ({:.1}, {:.1}) µm, {} hotspot(s) over {:.2} °C limit",
                if rep.passes() { "PASS" } else { "FAIL" },
                rep.tmax_c,
                rep.hot_x_um,
                rep.hot_y_um,
                hotspots,
                rep.t_limit_c
            ),
        )
        .with_code("THERMAL-DONE")
        .with_objects(vec![format!("hotspot:{:.1},{:.1}", rep.hot_x_um, rep.hot_y_um)]),
    );
}

fn emit(rep: &ThermalReport, cli: &Cli) -> ! {
    emit_thermal_events(rep); // vyges-events causal trail on stderr; the report goes to stdout / -o
    let text = if cli.json {
        with_report_path(&engine::report_json(rep), cli.out.as_deref())
    } else {
        engine::render_report(rep)
    };
    write_out(&text, cli);
    if cli.fail_on_violation && !engine::passes(rep) {
        if !cli.quiet {
            eprintln!(
                "thermal VIOLATED: peak {:.2} °C > {:.2} °C limit",
                rep.tmax_c, rep.t_limit_c
            );
        }
        exit(3);
    }
    exit(0);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--describe") {
        // Machine-readable description of `run` for tooling that drives it.
        const DESCRIBE: &str = r#"{
  "name": "thermal",
  "summary": "steady-state on-chip thermal analysis (floorplan -> temperature)",
  "invocation": {
    "args_template": ["run", "{job}"],
    "optional": [ { "arg": "out", "flag": "-o" } ],
    "emits_json": true
  },
  "inputs": {
    "type": "object",
    "required": ["job"],
    "properties": {
      "job": { "type": "string", "description": "path to a .thermal job file (die + grid + material params + a floorplan of blocks with placement and power)" },
      "out": { "type": "string", "description": "write the report to FILE instead of stdout" }
    }
  },
  "artifacts": [ { "role": "thermal_report", "field": "report_path" } ],
  "assertion": {
    "id": "thermal-within-limit",
    "field": "pass",
    "pass_when": { "is_true": true }
  },
  "consumes": ["floorplan", "power_report"]
}
"#;
        print!("{DESCRIBE}");
        return;
    }

    let cli = parse_cli(&args);

    if cli.bug_report {
        return link("Report a bug (central — vyges/community)", BUG_URL);
    }
    if cli.feature_request {
        return link("Request a feature (central — vyges/community)", FEATURE_URL);
    }
    if cli.sponsor {
        return link("Sponsor Vyges", SPONSOR_URL);
    }
    if cli.star {
        return link("Star vyges-thermal on GitHub ⭐", STAR_URL);
    }
    if cli.version {
        println!(
            "vyges-thermal {} ({})",
            vyges_thermal::VERSION,
            env!("VYGES_GIT_SHA")
        );
        println!("{}", vyges_thermal::COPYRIGHT);
        return;
    }
    let cmd = cli.positionals.first().cloned().unwrap_or_default();
    if cli.help || cmd.is_empty() {
        print!("{USAGE}");
        exit(if cmd.is_empty() && !cli.help { 2 } else { 0 });
    }

    match cmd.as_str() {
        "demo" => {
            let (_job, rep) = engine::demo();
            emit(&rep, &cli);
        }
        "check" => {
            let Some(path) = cli.positionals.get(1) else {
                eprintln!("usage: vyges-thermal check JOB");
                exit(2);
            };
            match ThermalJob::load(path) {
                Ok(j) => println!(
                    "OK  design={} floorplan={} die={}×{}µm grid={}×{} theta_ja={} t_limit={}°C",
                    j.design,
                    j.floorplan,
                    j.die_w_um,
                    j.die_h_um,
                    j.nx,
                    j.ny,
                    j.theta_ja,
                    j.t_limit_c
                ),
                Err(e) => {
                    eprintln!("error: {e}");
                    exit(2);
                }
            }
        }
        "run" => {
            let Some(path) = cli.positionals.get(1) else {
                eprintln!("usage: vyges-thermal run JOB [-o OUT]");
                exit(2);
            };
            let job = match ThermalJob::load(path) {
                Ok(j) => j,
                Err(e) => {
                    eprintln!("error: {e}");
                    exit(2);
                }
            };
            if cli.verbose {
                eprintln!("solving {} ({}×{} grid)", job.floorplan, job.nx, job.ny);
            }
            match engine::analyze_job(&job) {
                Ok(rep) => emit(&rep, &cli),
                Err(e) => {
                    eprintln!("error: {e}");
                    exit(1);
                }
            }
        }
        other => {
            eprintln!("vyges-thermal: unknown command {other:?}\n");
            print!("{USAGE}");
            exit(2);
        }
    }
}
