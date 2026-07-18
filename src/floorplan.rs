//! Floorplan reader: blocks with placement + power, the on-chip power map a
//! thermal solve needs. A `.flp` is a tiny whitespace-columns file (std-only):
//!
//! ```text
//! # name    x_um  y_um  w_um  h_um  power_w  [leak_w]
//! clkbuf    10    10    5     5     4.0e-3   2.0e-3
//! u0        20    10    5     5     0.8e-3
//! ```
//!
//! `power_w` is the block's total power at the reference temperature; the
//! optional `leak_w` is the *leakage* portion of it (≤ power_w) — the part that
//! rises with temperature in the electro-thermal coupling. Omit it (→ 0) and the
//! block's power is temperature-independent. The format mirrors HotSpot's
//! floorplan so a `.flp` + power trace correlates directly (see `correlation/`).

#[derive(Debug, Clone)]
pub struct Block {
    pub name: String,
    pub x_um: f64,
    pub y_um: f64,
    pub w_um: f64,
    pub h_um: f64,
    pub power_w: f64, // total power at the reference temperature
    pub leak_w: f64,  // leakage portion of power_w (≤ power_w); 0 = temp-independent
}

impl Block {
    pub fn cx_um(&self) -> f64 {
        self.x_um + self.w_um / 2.0
    }
    pub fn cy_um(&self) -> f64 {
        self.y_um + self.h_um / 2.0
    }
}

#[derive(Debug, Clone, Default)]
pub struct Floorplan {
    pub blocks: Vec<Block>,
}

#[derive(Debug)]
pub struct FloorplanError(pub String);
impl std::fmt::Display for FloorplanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "floorplan error: {}", self.0)
    }
}
impl std::error::Error for FloorplanError {}

impl Floorplan {
    pub fn load(path: &str) -> Result<Floorplan, FloorplanError> {
        let text =
            std::fs::read_to_string(path).map_err(|e| FloorplanError(format!("{path}: {e}")))?;
        Floorplan::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Floorplan, FloorplanError> {
        let mut blocks = Vec::new();
        for (lineno, raw) in text.lines().enumerate() {
            let line = match raw.find('#') {
                Some(i) => &raw[..i],
                None => raw,
            }
            .trim();
            if line.is_empty() {
                continue;
            }
            let t: Vec<&str> = line.split_whitespace().collect();
            if t.len() < 6 {
                return Err(FloorplanError(format!(
                    "line {}: expected `name x y w h power [leak]`, got {line:?}",
                    lineno + 1
                )));
            }
            let num = |s: &str, what: &str| -> Result<f64, FloorplanError> {
                s.parse::<f64>()
                    .map_err(|_| FloorplanError(format!("line {}: bad {what}: {s:?}", lineno + 1)))
            };
            let power_w = num(t[5], "power")?;
            let leak_w = if t.len() >= 7 {
                num(t[6], "leak")?
            } else {
                0.0
            };
            blocks.push(Block {
                name: t[0].to_string(),
                x_um: num(t[1], "x")?,
                y_um: num(t[2], "y")?,
                w_um: num(t[3], "w")?,
                h_um: num(t[4], "h")?,
                power_w,
                leak_w: leak_w.min(power_w),
            });
        }
        if blocks.is_empty() {
            return Err(FloorplanError("no blocks".into()));
        }
        Ok(Floorplan { blocks })
    }

    pub fn total_power_w(&self) -> f64 {
        self.blocks.iter().map(|b| b.power_w).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_blocks_and_optional_leak() {
        let f =
            Floorplan::parse("# c\nclkbuf 10 10 5 5 4.0e-3 2.0e-3\nu0 20 10 5 5 0.8e-3\n").unwrap();
        assert_eq!(f.blocks.len(), 2);
        assert_eq!(f.blocks[0].name, "clkbuf");
        assert!((f.blocks[0].leak_w - 2.0e-3).abs() < 1e-12);
        assert_eq!(f.blocks[1].leak_w, 0.0); // omitted -> temp-independent
        assert!((f.blocks[0].cx_um() - 12.5).abs() < 1e-9);
    }

    #[test]
    fn leak_clamped_to_total() {
        let f = Floorplan::parse("a 0 0 1 1 1.0 5.0\n").unwrap();
        assert_eq!(f.blocks[0].leak_w, 1.0); // clamped to power_w
    }

    #[test]
    fn too_few_columns_errors() {
        assert!(Floorplan::parse("a 0 0 1 1\n").is_err());
    }
}
