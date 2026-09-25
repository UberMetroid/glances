# Spec: P3 batch B — IO plugins

Behavioral contracts for the 8 IO-plugin files, derived from ground
truth only: the v0.10.72 oracle's plugin payloads (smoke corpus +
A/B), the suite's assertions, and OS facts (/proc layouts, SNMP
OIDs). All names, payload keys, value types, filter rules, alert
wiring, and OIDs stay identical; every body, doc, and test is fresh.

## diskio — per-device IO counters
Role: whole-disk rows (counts, bytes, mean latencies, tick rates)
from diskstats. Latencies are time/op with 0.0 on zero ops. Devices
filter to whole disks: partitions (sysfs authoritative, name
heuristic fallback with the denylist + dasd/mmcblk + p<N> +
letter-digit-length rules), dm-*, md<digits>, and zd* go. Rates are
saturating deltas over wall time (first tick zeros). Views alert
rx/tx rates per disk (highlight on, max 100) onto counter and rate
fields alike. History records both rate series, keyed by disk_name.
Oracle: filter matrix on synthetic names.

## network — per-NIC throughput
Role: per-interface rows with tick deltas, cumulative gauges, and
rates, capped at 64 NICs. Up means operstate up/unknown (tunnels
report unknown while working). Speed converts Mbps→bps. Roles:
loopback (lo name or 127.*), tailscale (name substring or 100.64/10),
local (RFC 1918), else untagged. SNMP walks ifEntry grouped by
instance (loopback type skipped; name/rx/tx/up/speed columns).
Views alert bit-rates per interface (name before any colon), falling
back to link speed when unconfigured. History records both rate
series, keyed by interface_name. Cross-batch ip:: helpers are
frozen-signature transients.
Oracle: up/role matrices (pure functions).

## fs — mount usage
Role: per-mount rows (device, type, point, options, size/used/free/
percent) via the rootfs reader (GLANCES_ROOTFS-aware). Mount lines
split device/point/type/options with octal unescaping (space/tab/
newline/backslash); pseudo types and path-boundary prefixes filter
out. SNMP reads UCD dskTable (KB) or hrStorage (alloc-unit math),
skipping empty/zero rows. Views alert used-per-mount except
read-only mounts (options containing a bare `ro` field). History
records percent, keyed by mnt_point.
Oracle: covered by the rewritten fs tests (parse/filter/contract).

## connections — socket-state census
Role: TCP state counts (all 12 codes plus UNKNOWN, always present
for a stable shape) plus best-effort conntrack gauges (Null until
readable). Both tcp tables tally together; UDP rows never count.
Missing tables and unreadable conntrack degrade silently.
Oracle: tally vectors on crafted rows.

## ports — socket table
Role: LISTEN/ESTABLISHED TCP rows plus all UDP rows (capped at
1000, tcp→tcp6→udp order), keyed by inode. Parsing accepts the
11-token modern and 12-token legacy layouts (queue-combining
detected by the colon), skipping short lines. Addresses decode
(IPv4 byte-reversed, IPv6 per-word-reversed, ports big-endian)
with raw fallback; rows carry raw and decoded forms plus the
state word.
Oracle: parse/decode vectors on hand-written table text.

## irq — interrupt rates
Role: top-5 IRQ lines by per-second rate (cumulative counts from
/proc/interrupts over wall time; first tick zeros). Names follow
`<num>_<last word>` (bare number without a name word; bare label
for named lines); CPU headers and blanks skip. Missing files yield
empty stats. Sort is rate-descending, stable. Keyed by irq_line.
Oracle: parse vectors on fixture text.

## programlist — processes by program
Role: processlist samples grouped by name (first-seen order,
cpu-descending): counters summed, username/status/nice collapsing
to `_` on disagreement, member pids listed. Percents round to 2
decimals; memory/times/io nest as sub-objects. The non-Linux
empty branch is dead code in this Linux-only tree and goes away.
Cross-batch processlist sampler/types are frozen transients.
Oracle: covered by live suite + A/B (sampler is P3C).

## alert — event-log surface
Role: the shared event log as newest-first alert records (capped
at 100): type word, stat, value, epoch timestamp. Update is a
no-op (views rebuild from the log after every tick); count()
reports the held records.
Oracle: record shape on a seeded log (rewritten inline tests).

## Test rewrites and oracle
`plugins_fs.rs` is rewritten with fresh names and structure (same
parse/filter/contract assertions). New P3B oracle assertions append
to `plugins_oracle.rs` (diskio/ports/connections/irq/network).
