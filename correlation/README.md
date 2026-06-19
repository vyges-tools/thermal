# Correlation — vyges-thermal vs HotSpot

The open baseline for on-chip thermal is **HotSpot** (University of Virginia) — the
canonical block/grid thermal simulator. There is no thermal tool *inside* the
OpenLane/OpenROAD flow, so HotSpot is the external "approach" reference (the same
role OpenSTA plays for `vyges-sta-si` and PDNSim for `vyges-em-ir`).

`run.sh` translates the shared inputs (`examples/block/block.flp` + `block.thermal`)
into HotSpot's floorplan + power trace + config, runs both tools, and compares the
peak temperature:

```sh
HOTSPOT=/path/to/hotspot  VYGES_THERMAL=../target/debug/vyges-thermal  ./run.sh
```

## What it validates / what's next

- **Peak temperature + hotspot location** — the headline outputs. Both tools bin
  the same block powers onto the die and solve a steady-state conduction system, so
  the hotspot tile and the peak rise should agree to first order.
- **Modeling difference (the depth item).** v0 collapses the package into a single
  junction-to-ambient resistance (`theta_ja`) with a uniform vertical path. HotSpot
  models a layered stack — silicon, TIM, copper spreader, heat sink — plus lateral
  spreading in each layer. So absolute temperatures differ by the package model, not
  the on-die solve. Closing it = a layered vertical stack (reserved).
- **Electro-thermal coupling** is a vyges-thermal feature (leakage → temperature →
  leakage fixed point); HotSpot takes a fixed power trace, so for an apples-to-apples
  peak comparison run the job with `leak_alpha_per_c: 0` (or feed HotSpot the
  converged power trace vyges-thermal reports).

Honest bound: not a certified sign-off thermal model; this is the open, scriptable
grid solver correlated to HotSpot, with the package model on a clear depth path.
