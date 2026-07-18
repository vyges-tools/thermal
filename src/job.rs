//! Thermal job: the declarative description of what to analyze.
//!
//! A `.thermal` job is a tiny `key: value` file (std-only parser — no deps):
//!
//! ```text
//! design:           counter
//! floorplan:        counter.flp   # blocks: name x y w h power [leak]
//! die_um:           100 100       # die W H (µm)
//! grid:             16 16         # solver grid NX NY
//! thickness_um:     280           # silicon thickness
//! k_si:             148.0         # silicon thermal conductivity (W/m·K)
//! theta_ja:         25.0          # junction-to-ambient thermal resistance (K/W)
//! ambient_c:        25.0          # ambient temperature (°C)
//! t_limit_c:        105.0         # sign-off threshold (fail above this)
//! leak_alpha_per_c: 0.0           # electro-thermal: leakage temp coeff (1/°C); 0 = off
//! leak_t0_c:        25.0          # reference temp for the leakage model
//! ```

use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ThermalJob {
    pub design: String,
    pub floorplan: String,
    pub die_w_um: f64,
    pub die_h_um: f64,
    pub nx: usize,
    pub ny: usize,
    pub thickness_um: f64,
    pub k_si: f64,
    pub theta_ja: f64,
    pub ambient_c: f64,
    pub t_limit_c: f64,
    pub leak_alpha_per_c: f64, // electro-thermal leakage temperature coefficient (1/°C)
    pub leak_t0_c: f64,
    pub base_dir: String,
}

#[derive(Debug)]
pub struct JobError(pub String);
impl std::fmt::Display for JobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "job error: {}", self.0)
    }
}
impl std::error::Error for JobError {}

fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(i) => &line[..i],
        None => line,
    }
}

fn two(s: &str, key: &str) -> Result<(f64, f64), JobError> {
    let t: Vec<&str> = s.split_whitespace().collect();
    match t.as_slice() {
        [a, b] => {
            let a = a
                .parse()
                .map_err(|_| JobError(format!("bad {key}: {s:?}")))?;
            let b = b
                .parse()
                .map_err(|_| JobError(format!("bad {key}: {s:?}")))?;
            Ok((a, b))
        }
        _ => Err(JobError(format!("{key} needs two values, got {s:?}"))),
    }
}

impl ThermalJob {
    pub fn parse(text: &str, base_dir: &str) -> Result<ThermalJob, JobError> {
        let mut kv: BTreeMap<String, String> = BTreeMap::new();
        for raw in text.lines() {
            let line = strip_comment(raw).trim();
            if line.is_empty() {
                continue;
            }
            let (k, v) = line
                .split_once(':')
                .ok_or_else(|| JobError(format!("expected 'key: value', got {line:?}")))?;
            kv.insert(k.trim().to_lowercase(), v.trim().to_string());
        }
        let get = |k: &str| {
            kv.get(k)
                .cloned()
                .ok_or_else(|| JobError(format!("missing key: {k}")))
        };
        let num = |k: &str, d: f64| kv.get(k).and_then(|s| s.parse().ok()).unwrap_or(d);

        let (die_w_um, die_h_um) = two(&get("die_um")?, "die_um")?;
        let (gx, gy) = two(&get("grid")?, "grid")?;
        let nx = gx as usize;
        let ny = gy as usize;
        if nx == 0 || ny == 0 {
            return Err(JobError("grid dimensions must be >= 1".into()));
        }
        let job = ThermalJob {
            design: get("design")?,
            floorplan: get("floorplan")?,
            die_w_um,
            die_h_um,
            nx,
            ny,
            thickness_um: num("thickness_um", 280.0),
            k_si: num("k_si", 148.0),
            theta_ja: num("theta_ja", 25.0),
            ambient_c: num("ambient_c", 25.0),
            t_limit_c: num("t_limit_c", 105.0),
            leak_alpha_per_c: num("leak_alpha_per_c", 0.0),
            leak_t0_c: num("leak_t0_c", num("ambient_c", 25.0)),
            base_dir: base_dir.to_string(),
        };
        if job.die_w_um <= 0.0 || job.die_h_um <= 0.0 {
            return Err(JobError("die_um must be positive".into()));
        }
        if job.theta_ja <= 0.0 || job.k_si <= 0.0 || job.thickness_um <= 0.0 {
            return Err(JobError(
                "k_si, thickness_um, theta_ja must be positive".into(),
            ));
        }
        Ok(job)
    }

    pub fn load(path: &str) -> Result<ThermalJob, JobError> {
        let text = std::fs::read_to_string(path).map_err(|e| JobError(format!("{path}: {e}")))?;
        let base = Path::new(path)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or(".");
        ThermalJob::parse(&text, base)
    }

    pub fn resolve(&self, rel: &str) -> String {
        if Path::new(rel).is_absolute() || self.base_dir.is_empty() {
            rel.to_string()
        } else {
            Path::new(&self.base_dir)
                .join(rel)
                .to_string_lossy()
                .into_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_job_with_defaults() {
        let j = ThermalJob::parse(
            "design: c\nfloorplan: c.flp\ndie_um: 100 80\ngrid: 8 4\ntheta_ja: 30\n",
            "/tmp",
        )
        .unwrap();
        assert_eq!((j.nx, j.ny), (8, 4));
        assert!((j.die_h_um - 80.0).abs() < 1e-9);
        assert!((j.theta_ja - 30.0).abs() < 1e-9);
        assert!((j.k_si - 148.0).abs() < 1e-9); // default
        assert_eq!(j.resolve("c.flp"), "/tmp/c.flp");
    }

    #[test]
    fn missing_die_errors() {
        assert!(ThermalJob::parse("design: c\nfloorplan: c.flp\ngrid: 4 4\n", "").is_err());
    }
}
