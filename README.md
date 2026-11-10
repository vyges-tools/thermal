# vyges-thermal

Steady-state **on-chip thermal analysis**: a floorplan power map in, a die
**temperature field + hotspot** out — and an **electro-thermal coupling** loop
(leakage rises with temperature, re-solved to a fixed point). Part of the Vyges
open EDA sign-off suite.

It is the electrical **dual** of [`vyges-em-ir`](https://github.com/vyges-tools/em-ir):
the steady state is `G_th · ΔT = P`, the same symmetric, diagonally-dominant
system em-ir solves for `G · V = I` — temperature rise ↔ voltage, power ↔ current,
thermal conductance ↔ electrical conductance, ambient ↔ the supply pads.

## Where it sits

```
vyges-char (energy) ─▶ vyges-power (per-instance power) ─▶ vyges-em-ir (IR/EM)
                                     │                              ▲
                                     └────────▶ vyges-thermal ──────┘
                                          (power → temperature; T → leakage → …)
```

`vyges-power` already produces the per-instance power this engine lands on the die
as heat. With temperature-dependent leakage it closes the loop:
`power → thermal → leakage → power`.

## Use

```sh
cargo build --release            # std-only, no external deps
vyges-thermal run   examples/block/block.thermal           # text report + heat map
vyges-thermal run   examples/block/block.thermal --json    # machine-readable
vyges-thermal run   examples/block/block.thermal --fail-on-violation  # exit 3 over t_limit
vyges-thermal demo                                          # built-in design, no files
# common flags: -o FILE · --json · -q/--quiet · -v/--verbose · -h/--help · -V/--version
```

A `.thermal` job + a `.flp` floorplan:

```text
# block.thermal
design:           soc_block
floorplan:        block.flp
die_um:           1000 1000     # die W H (µm)
grid:             32 32         # solver grid NX NY
thickness_um:     280           # silicon thickness
k_si:             148.0         # silicon thermal conductivity (W/m·K)
theta_ja:         40.0          # junction-to-ambient thermal resistance (K/W)
ambient_c:        25.0
t_limit_c:        105.0         # sign-off threshold
leak_alpha_per_c: 0.004         # electro-thermal: leakage +0.4%/°C (0 = off)

# block.flp  —  name  x_um y_um w_um h_um  power_w [leak_w]
cpu      100 100 400 400 0.30 0.10
accel    100 600 400 300 0.20 0.05
sram     600 100 300 300 0.05 0.03
io       600 600 300 300 0.02
```

## What it computes (v0)

- **Grid solve** — the die is an `nx × ny` tile grid; heat conducts laterally
  through silicon (`k_si·A/L`) and vertically to ambient through `theta_ja`
  (spread over the die). Block powers are area-binned onto the tiles; a
  Gauss-Seidel solve gives each tile's temperature.
- **Hotspot + sign-off** — peak temperature, its location, per-block temperatures,
  and a PASS/FAIL against `t_limit_c`. A text **heat map**, plus JSON.
- **Electro-thermal coupling** — when `leak_alpha_per_c > 0`, each block's leakage
  is updated as `P_leak(T) = P_leak0·(1 + α·(T − T0))` and the grid re-solved to a
  fixed point. On the example: peak **48.97 °C** uncoupled → **49.71 °C** coupled
  (8 iterations), total power 570 → 587 mW as leakage grows with temperature.
- **No silent power loss** — power from a block that extends past the die edge is
  reported as `off-die, dropped`, not quietly discarded.

**Honest bounds (depth reserved).** v0 collapses the package into a single
junction-to-ambient resistance with a uniform vertical path — HotSpot's layered
spreader/heat-sink stack is the depth item (see [`correlation/`](correlation/)).
Floorplan placement is a described `.flp`; **DEF-based placement via the
`vyges-loom` foundation** (consuming a real layout + `vyges-power`'s map directly)
is next. Transient response and feeding temperature-dependent wire resistance back
into `vyges-em-ir` are reserved.

## Domain coverage — digital *and* analog / mixed-signal

The solve is **physics and geometry only**: `G_th · ΔT = P` over a tile grid, fed
by a generic `.flp` power map (`name x y w h power [leak]`). Nothing in the path
assumes standard cells, a clock, or a digital netlist — the *only* requirement is
per-block power, from any source. So the same engine runs on **analog and
mixed-signal** blocks exactly as it does on digital ones: see
[`examples/pa/`](examples/pa/), an RF power amplifier whose output stage is the
hotspot (peak ~93 °C, located on the PA block), authored as a plain `.flp` with no
engine change. `vyges-power` is the conventional upstream that supplies that map,
but any power source works.

Scope honestly: this is **thermal (physical) analysis** — temperature field +
hotspot + a sign-off gate. It is *not* analog functional sign-off (no AC/transient
electrical behaviour); for analog the input is just a power map like any other.

## Correlation

**HotSpot** (UVA) — the canonical open on-chip thermal simulator — is the baseline
(there is no thermal tool *in* OpenLane/OpenROAD). `correlation/run.sh` maps the
shared inputs to HotSpot and compares peak temperature + hotspot location. See
[`correlation/README.md`](correlation/README.md).

## Current state (v0)

Steady-state grid solve + hotspot + electro-thermal coupling + sign-off gate; text
(with heat map) + JSON; `--fail-on-violation` CI gate. Pure std, unit + example
tested offline, no subprocess, no deps.

## Open core, certified fab plugins

The engine and its models are open (Apache-2.0). Per-foundry/package thermal
calibration, where applicable, is distributed separately under its own terms.
