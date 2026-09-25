# Spec: P3 batch A — simple plugins

Behavioral contracts for the 10 simple-plugin files, derived from
ground truth only: the v0.10.72 oracle's plugin payloads (smoke
corpus + A/B), the suite's assertions, and OS/manual facts
(/proc layouts, SNMP OIDs). All plugin names, payload keys, value
types, registration order, alert wiring, and SNMP OIDs stay
identical; every body, doc, and test is fresh.

## plugins/mod — registry
Role: module tree plus ordered registration with enable/disable
filtering. The ALL table (names, register fns, ORDER) is frozen —
`--modules-list` and `/api/4/pluginslist` expose the order, and
helper modules (json, gpu_*, fs_rootfs) stay unregistered. Filtering:
narrowing happens ONLY with `all` in disabled plus explicit enables;
a bare enable list never narrows (it only switches on irq, the one
default-disabled plugin); named disables always win; unknown names
are ignored.
Oracle: subset matrices (covered by rewritten CLI tests).

## version — build metadata
Role: static payload — glances_version (crate version), api_version
+ plugin_version ("4"), crustacean ("rust"), std ("true"). Update is
a no-op; all values are strings. Payload builder stays public for
tests.
Oracle: key set and value shapes (rewritten inline tests).

## uptime — seconds since boot
Role: one `seconds` float from the boot clock; SNMP divides
sysUpTime.0 ticks by 100.
Oracle: post-update value is positive and finite (new inline test).

## help — key-binding map
Role: static table of UI key bindings. All 33 entries are frozen
user-visible data (key → description). Update is a no-op.
Oracle: count plus well-known keys (rewritten inline tests).

## load — load averages
Role: min1/min5/min15 from /proc/loadavg plus machine-wide logical
core count (never the affinity-limited parallelism count). SNMP
reads the UCD load strings with a local core count. Views: min15
classifies with logging, min5 without, both against 100×cores
(cores floored at 1); missing keys skip silently.
Oracle: view math on synthetic stats (new inline test).

## mem — RAM accounting
Role: the 10-field memory payload from meminfo with two documented
adjustments: ZFS ARC counts toward cached with its shrinkable part
moved from used to available (percent recomputed), and used/percent
clamp so containerized readings never go negative or past 100.
SNMP reads UCD kilobytes (total≤0 resets; available mirrors free).
Views classify percent with logging when used and total are both
positive. History records percent.
Oracle: clamp behavior on synthetic extremes (new inline test).

## cpu — aggregate processor stats
Role: tick-over-tick state percentages, cumulative counters with
per-second siblings, and static identity. First tick emits zeros
for percentages (no previous sample); syscalls/dpc pin at 0.0 on
Linux; time_since_update measures the tick gap. State shares are
field-delta over total-delta ×100; total is busy share clamped.
SNMP: UCD percentages by default, hrProcessorLoad mean on
windows/esxi (empty table resets). Alert wiring logs
total/user/system/iowait/dpc and alerts steal; ctx_switches
classifies against 100×cores once a tick gap exists.
Oracle: state_pcts exact math on crafted deltas (new inline test).

## percpu — per-CPU rows
Role: one row per logical CPU with the aggregate field set plus
cpu_number identity (`cpu<N>`) and softirq/busy extras. Rows key
off `cpu_number` for views and history. Missing previous samples
(first tick, hotplug) report zeros with a stable schema. Total is
100−idle (NOT busy share — iowait and steal count as used).
Oracle: row schema on a live read (covered by suite + A/B).

## quicklook — summary holder
Role: cpu/mem/swap/load/cpu_name filled by the stats post-pass
(the plugin itself has no data source). Views classify cpu/mem/
swap against 100. History records cpu/percpu/mem/swap/load.
Oracle: key set and standalone no-op (rewritten inline tests).

## processcount — process census
Role: total/running/sleeping/thread/pid_max as unsigned ints.
Total counts numeric /proc entries; states come from each stat
line's state char (R runs, S/I sleep, D/W and the rest count
neither); threads sum field 20. Stat parsing splits on the FIRST
space then the LAST `)` (comm may hold spaces and parens), takes
the next token as state, skips 16 fields, and parses threads.
Read failures fall back per-process (skip) and whole-scan
(/proc/stat counts). pid_max reads the sysctl, 0 on error. SNMP
resets (no standard MIB). The non-Linux zero branch is dead code
in this Linux-only tree and goes away.
Oracle: stat-line and /proc/stat vectors (new oracle tests).
