# Capture corpus

Black-box baselines captured from the v0.10.72 oracle binary (2026-09-25).
Each rewritten module replays its slice: new-binary output must match modulo
volatile fields only.

Volatile fields (vary run to run, excluded from comparison): timestamps,
PIDs, live readings (percents, temps, byte counters), process tables,
`uptime` strings, unordered JSON object keys.

Regenerate with `sh ../capture.sh <base-url> <dir>` against any running
server. Per-module vectors live in `corpus/<module>/` next to their specs.
