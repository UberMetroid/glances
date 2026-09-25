# Spec: P1 batch 1 — core primitives

Behavioral contracts for the 11 core files, derived from ground truth only:
the v0.10.72 oracle's observable behavior, the existing suite's assertions
(behavior, not expression), and OS/manual facts. No upstream source consulted.
Public API shapes stay identical per plan decision 9 (interface facts); every
body, doc, private helper, and test is written fresh.

## plugin — the plugin framework
Role: the trait every data source implements plus the shared model struct
(stats, limits, history, views, triggers, rate/MMM state).
Computation: trait defaults — `update_views` routes through the views
rebuild; `update_stats_history` reads the `history_size` limit (default
28800); `set_*` are no-ops; `is_enabled` is true; SNMP default returns an
Unsupported error naming the plugin. Rate tracking: first tick primes, later
ticks add `<key>_gauge`, `<key>_rate_per_sec` (delta/dt), and
`time_since_update`; derived keys are never re-derived. MMM tracking: running
min/max/mean per numeric field, emitted as `<key>_min/_max/_mean`. History
recording: scalar stats record `<field>` series, list stats record
`<elem>_<field>` (element id from the key field as string/int/uint, else the
index); missing fields skipped; cap applied first.
Oracle: synthetic model, two rate ticks with known values (exact rates);
known MMM sequence (exact min/max/mean).

## stats — registry and refresh loop
Role: owns the plugin list, drives ticks, snapshots, fills quicklook,
applies config.
Computation: a tick visits plugins in order, skipping disabled ones. Each
`update` runs under panic capture; on success, history (only when enabled),
alert-command runs, and the views rebuild each run under their own capture.
Update errors: Unsupported logs at debug, anything else warns. A panic logs
an error and keeps previous stats. Quicklook fill (same tick): cpu/mem/swap
copied from sibling plugins, load = min1/cpucore*100 (0 when cores is 0),
cpu_name passed through (Null when absent). Limits config: built-in defaults
per plugin, `history_size` default 28800 from `[global]`, then each
`[<plugin>]` section stored plugin-prefixed (`mem_careful`), values parsed as
float else comma-split lists. Plugin config pushes `[power] kwh_rate`.
Snapshot maps plugin name to cloned stats.
Oracle: stub plugins (one panicking — loop survives; history-disabled —
nothing recorded); quicklook math from synthetic stats (exact).

## threshold — severity vocabulary
Role: four ordered levels plus classification and limit lookup.
Computation: `evaluate` returns the highest band whose threshold the value
meets or exceeds; unset bands are skipped. `get_limit` tries
`<stat>_<sev>` then `<plugin>_<sev>`; Ok always yields 0.0.
Oracle: band matrix including unset bands and exact-boundary equality.

## timer — countdown and counter
Role: elapsed-time gating plus a monotonic counter.
Computation: `finished` is true when duration <= 0 or elapsed >= duration;
`set` replaces the duration and restarts; Counter starts at 0 and increments.
Oracle: real-sleep timing assertions.

## history — bounded sample store
Role: per-key timestamped samples with cap, export, and rate math.
Computation: `add` appends and drains excess past the cap; `get` returns the
last nb samples (0 = all); `reset` clears; `set_max_size` floors at 1;
`snapshot` exports epoch-float pairs; `rate` is the last-two-sample slope,
0.0 with fewer than two samples or non-positive dt. Default cap 28800.
Oracle: truncation counts; slope from crafted timing within tolerance.

## events — severity log
Role: bounded ring of threshold-crossing records.
Computation: cap 0 keeps nothing; at cap the oldest entry is evicted first;
snapshot is oldest-first; `clean(false)` drops Warning and Careful and keeps
Critical and Ok; `clean(true)` drops everything. Default cap 100.
Oracle: wraparound order; clean matrix over all four severities.

## alerts — limit engine
Role: limit lookups, alert classification, built-in default table.
Computation: `get_limit` lowercases and tries `<stat>_<sev>` then
`<plugin>_<sev>` (list values parse their first item). Action lookup tries
stat/plugin × plain/repeat in that order, returning commands plus the repeat
flag, else (None, false). Log-tag lookup tries stat then plugin then the
caller default. Classification: zero-without-highlight, zero-maximum, and
non-finite values yield DEFAULT; stat name is
plugin[_action_key][_header] lowercased; first band hit in
critical→warning→careful order wins, no bands at all yields DEFAULT,
otherwise OK; current below minimum forces CAREFUL. A set log tag appends
`_LOG` and records the event; the trigger word is always recorded. Defaults
table (frozen values): 50/70/90 for cpu/mem/swap/fs/network/processlist
families, load 0.7/1.0/5.0, hdd temp 45/52/60, battery 70/80/90; cpu
ctx_switches scales as 50000×cores×(0.8/0.9/1.0), inserted only when absent.
Oracle: classification matrix with hand-set limits (exact words); trigger
recording; ctx_switches scaling math.

## alert_views — decoration rebuild
Role: recompute every field's decoration string after each tick.
Computation: list stats get one decoration map per element (id from the key
field, index fallback); scalar stats get a single map under the empty id;
anything else clears the views. ALERT-flagged fields classify against 100,
LOG-flagged fields classify with logging, all others stay DEFAULT.
Oracle: synthetic stats plus descriptors yield an exact views map.

## config_dir — Linux path resolution
Role: locate config, user dir, and cache dir. Linux-only (dead platform
branches removed — behavior on Linux is unchanged).
Computation: candidates are the XDG (or `~/.config`) glances.conf, then
`/etc/glances/glances.conf`, then the PREFIX package fallback; empty when
HOME is unset. Resolution prefers the CLI override, then the first existing
candidate, then the first candidate, then `glances.conf`. User dir is the
XDG (or `~/.config`) glances dir (pure computation — the caller creates it).
Cache dir is XDG_CACHE_HOME (or `~/.local/share`) glances, else the temp dir.
Oracle: read-only structure checks (segments present, override wins); no
environment mutation.

## filter/mod — matching engine
Role: process include/exclude matching plus a minimal regex engine.
Computation: an empty/blank filter is inactive and matches everything; an
active filter matches when the name OR the cmdline matches. Patterns support
literals, `^$`, `.` (not newline), `*+?`, classes with negation and ranges,
groups, and alternation; bad patterns are errors. Matching is unanchored
unless `^`/`$` say otherwise; full-match requires consuming the whole input.
The matcher is a position-set backtracker memoized on program identity plus
input position, so empty-matching repeats terminate and nesting stays
polynomial. The parser (`parse_alt`) is an OWNED interface and unchanged.
Oracle: match matrix (anchors, classes, quantified groups, alternation) plus
a termination probe on empty-matching repeats.

## filter/glances — rules and rule lists
Role: one `key:pattern` rule and comma-separated OR lists.
Computation: rules split on the FIRST colon (no colon = no key); patterns
that fail to compile disable the rule. Without a key, a process matches when
its name or its first argv element fullmatches; with a key, when that field
fullmatches (missing/non-string never matches). Cmdline argv lists match the
first element only (empty array matches against ""); plain-string cmdlines
match directly. List setters replace the whole list; emptiness means no
active rule.
Oracle: rule matrix (first-colon split, argv-first-only, replace semantics).

## Test rewrites (tainted → fresh)
`core_filter_list.rs` and `core_stats.rs` are rewritten with fresh names,
structure, and comments; assertions stay behavioral (black-box against the
new code). New independent-oracle coverage lands in `core_oracle.rs`.
