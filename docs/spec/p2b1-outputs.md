# Spec: P2 batch 1 — outputs (stdout + web)

Behavioral contracts for the 7 output files, derived from ground truth
only: the v0.10.72 oracle's wire behavior (smoke corpus), the suite's
behavioral assertions, and RFC facts (base64, CSV quoting). All wire
shapes, status codes, and bodies stay byte-identical per the frozen
contract; every body, doc, private helper, and test is written fresh.

## csv_stdout — CSV streamer
Role: one `timestamp,plugin,key,value,,` row per stat per tick (unit
and description columns stay empty), header once.
Computation: header is the literal
`timestamp,plugin,key,value,unit,description`. Fields quote when they
contain `,`/`"`/newline (quotes doubled). Cells: Null→empty,
Bool/Int/Uint plain, floats at 2 decimals, NaN→`NaN`, +Inf→`Inf`,
-Inf→`-Inf`, arrays/objects→JSON. Objects flatten one row per key;
arrays emit one row per element (object elements keyed by their `key`
field with the rest JSON-encoded, scalars keyless); anything else is
one keyless row. Timestamps print at 3 decimals. The loop updates,
filters to `stdout_plugins`, writes, flushes, counts ticks
(saturating), stops at `stop_after`, and sleeps the refresh gap.
Update failures warn and continue with stale data.
Oracle: hand-written expected rows from a synthetic snapshot.

## json_stdout — JSON streamer
Role: one `{"timestamp":…,"plugins":{…}}` line per tick, timestamp
first, flushed immediately.
Computation: same loop/filter/sleep/stop semantics as the CSV
streamer. Non-finite floats render null via the shared serializer.
Oracle: exact line match on a synthetic snapshot (key order pinned).

## web/auth — request authentication
Role: Basic header verification plus the API-key gate.
Computation: the header must start with exactly `Basic `; the rest
decodes as strict base64 (length multiple of 4, at most 2 trailing
pad chars, pad only trailing, canonical zero tail bits), parses as
UTF-8, and splits user from password on the FIRST colon. Passwords
verify through the credential file. Routing gates: the favicon is
always open; with nothing configured everything is open; in key-only
mode the dashboard shell paths stay open (they carry no data). With
both gates on, either credential passes. Key comparison trims, rejects
blanks, and runs constant-time (length gate plus xor accumulation).
Oracle: base64 vector matrix including canonical-padding rejections.

## web/meta — aggregate endpoints
Role: dashboard bundle, limits/views/history exports, and service
metadata. Dashboard keys in fixed column order (cpu mem load system
uptime memswap processcount percpu network connections diskio fs
sensors gpu power pressure ip), missing plugins as null, health
rollup under `"health"`. Limits export each model's table (floats
plain, lists as string arrays). Views export the element key (or
null), declared field names, and live decorations; description is the
same payload. History exports `{plugin:{series:[[epoch,value]]}}`
(full or last-`nb`). Status answers `{"version":…}`; pluginslist
answers names in order; serverslist is always `[]`. Per-plugin
history parses `/api/[4/]<name>/history[/<nb>]` (numeric middle
segments are versions; bad shapes and unknown plugins 404).
Oracle: bundle key set and history slicing on synthetic models.

## web/mod — server wiring
Role: bind, key resolution, and the serve loop entry.
Computation: listens on the configured address/port with an empty
credential file when auth is off. The API key resolves from
`GLANCES_API_KEY` first (trimmed, blanks ignored), then the first
line of the `api-key` file beside the user config; the startup log
names the source, never the value. Key-file tests use temp dirs.
Oracle: resolution precedence (covered by rewritten unit tests).

## web/mutate — POST mutators and pid getters
Role: event clears, extended-process pinning, single-process lookup.
Computation: clears run the log clean and answer `{}`. Disabling
unpins and answers `true`. Pinning takes the last path segment: a
numeric pid that exists pins and answers `true`, an absent one 404s,
a non-numeric one 400s with `pid must be numeric`. The extended
getter answers the pinned process JSON or `{}`. The per-pid getter
404s on absent AND non-numeric pids. Pid matching reads the
processlist's numeric pid field.
Oracle: pin round-trip and status matrix (rewritten route tests).

## web/router — dispatch
Role: method+path dispatch with the auth gate first.
Computation: gated paths without valid credentials answer 401 (Basic
flavor when Basic is on, key flavor otherwise). Then a fixed-order
table: static shell/favicon/openapi (pages cache 5 minutes),
aggregate endpoints, versioned and unversioned direct-plugin payloads
(a purely numeric first segment is a version, never a name;
multi-segment leftovers 404), per-plugin values/description (unknown
plugins 404; descriptions carry Debug unit names), the hello SSE
frame, the `ok` liveness probe, event clears, extended pin/disable,
per-pid lookup, history (any path containing `/history`), MCP
dispatch on the configured path plus `/mcp[/]`, the 501 token stub,
and 404 for everything else. Order is load-bearing: specific arms
(history, stream, extended) precede the generic plugin arm.
Oracle: routing matrix on an empty registry (404s, liveness, clears).
