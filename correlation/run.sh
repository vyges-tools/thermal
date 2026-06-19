#!/usr/bin/env bash
# Correlate vyges-thermal against HotSpot (the canonical open on-chip thermal
# simulator) on the example block. Runs on a host with a built HotSpot binary.
#
#   HOTSPOT=/path/to/hotspot  VYGES_THERMAL=/path/to/vyges-thermal  ./run.sh
#
# It translates the shared inputs (examples/block/block.flp + block.thermal) into
# HotSpot's floorplan (.flp, metres) + power trace (.ptrace) + config, runs both
# tools, and prints the peak-temperature comparison. Exact agreement is not
# expected — HotSpot models a layered spreader/heat-sink package while v0 uses a
# single junction-to-ambient resistance (the documented depth item); the peak and
# the hotspot location are what we correlate.
set -euo pipefail
cd "$(dirname "$0")"
HS="${HOTSPOT:?set HOTSPOT to a built hotspot binary}"
VT="${VYGES_THERMAL:-vyges-thermal}"
FLP=../examples/block/block.flp
JOB=../examples/block/block.thermal

# our .flp (µm: name x y w h power leak)  ->  HotSpot .flp (m: name w h x y)
awk '!/^#/ && NF>=6 { printf "%s\t%.9f\t%.9f\t%.9f\t%.9f\n", $1, $4*1e-6, $5*1e-6, $2*1e-6, $3*1e-6 }' "$FLP" > hs.flp
# HotSpot ptrace: a header row of unit names, then one row of powers (W)
awk '!/^#/ && NF>=6 { n=n $1"\t"; p=p $6"\t" } END { print n; print p }' "$FLP" > hs.ptrace

theta=$(awk -F'[: ]+' '/^theta_ja/{print $2}' "$JOB")
amb=$(awk   -F'[: ]+' '/^ambient_c/{print $2}' "$JOB")
cat > hs.config <<C
-ambient $(awk "BEGIN{print $amb+273.15}")
-r_convec $theta
C

echo "HotSpot vs vyges-thermal — example block:"
"$HS" -f hs.flp -p hs.ptrace -steady_file hs.steady -config_file hs.config >/dev/null 2>&1 || \
  "$HS" -f hs.flp -p hs.ptrace -steady_file hs.steady >/dev/null 2>&1
hs_peak=$(awk 'NR>0{t=$2-273.15; if(t>m)m=t} END{printf "%.2f", m}' hs.steady)
vt_peak=$("$VT" run "$JOB" --json | awk -F'[:,]' '/tmax_c/{for(i=1;i<=NF;i++) if($i ~ /tmax_c/){print $(i+1); exit}}')
printf "  HotSpot peak       %s °C\n" "$hs_peak"
printf "  vyges-thermal peak %.2f °C\n" "$vt_peak"
echo
echo "Note: single-theta_ja (v0) vs HotSpot's layered package — expect same"
echo "ballpark + hotspot location, not an exact match. That's the depth item."
