#!/bin/sh
# Capture a black-box baseline from a running glances-rs web server.
# Usage: capture.sh <base-url> <out-dir>   (e.g. capture.sh http://localhost:61208 corpus/smoke)
# Captures are shape/behavior references, not exact-match fixtures:
# timestamps, PIDs, and live readings vary run to run.
set -e
BASE="$1"
OUT="$2"
mkdir -p "$OUT"
for ep in version status health alert cpu mem fs network load uptime quicklook; do
    curl -fsSL "$BASE/api/4/$ep" -o "$OUT/$ep.json"
done
curl -fsSL "$BASE/api/4/processlist" -o "$OUT/processlist.json"
echo "captured to $OUT"
