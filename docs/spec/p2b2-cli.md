# Spec: P2 batch 2 — CLI and entry point

Behavioral contracts for the 5 CLI files, derived from ground truth
only: the v0.10.72 oracle's `--help`/mode outputs, the suite's flag
assertions, and the frozen flag table. Every flag string, default,
mode mapping, and printed line stays byte-identical (the CLI is the
most user-visible contract here); every body, doc, and test is fresh.

## cli/args — argument model
Role: the Mode/SnmpVersion enums, the Args struct with all defaults,
and the parse entry points.
Computation: defaults are load-bearing (Standalone mode, 2.0s
refresh, port 61208, bind 0.0.0.0, `/mcp`, SNMP 161/V2c, separator
and short process names ON, everything else off/empty). Parsing
tokenizes argv (owned tokenizer) and applies flags left to right, so
later flags override earlier ones. Env argv reads lossy (never panic
on non-UTF-8); the slice variant parses without touching the
environment.
Oracle: default pinning plus last-flag-wins precedence.

## cli/flags — flag dispatch
Role: apply one token to Args. The full table is frozen: mode flags
(-w/-h/-V/--stdout-*/--fetch/--modules-list/--module-list/--api-doc*/
--issue), toggles (debug/quiet/history/webui/config-exec/auth/
access-log/fs/process/irq/mcp/browser and the display set), valued
flags (-t finite-positive only, --stdout mode+spec, ports, plugin
lists comma-split/trimmed/non-empty, config/paths, -c host+Client,
-u name, sort/focus/strftime/fetch-template, snmp community/port/
version/user/auth with unknown versions falling back to V2c,
mcp-path, secure-config, --ping target+Ping mode), valued
--stdout-csv/json recording plugin lists, no-op accepted flags, and
silent ignore for unknown flags. Bare positionals warn and change
nothing.
Oracle: full matrix (rewritten flag tests) plus precedence chains.

## cli/modes — registration and one-shot printers
Role: plugin set selection plus `--fetch`/`--modules-list` output.
Computation: registration honors enable/disable lists and the display
subsets (-2 sidebar's 12 plugins, -3 quicklook, -4 keeps
quicklook+load while dropping cpu/mem/memswap, -5 the top set,
--light the sidebar plus the heavy set, --disable-process the
process family), then sets history posture, action posture, the
process filter/irix push, and both config applications. Fetch prints
the template verbatim when set, else the fixed 9-line summary
(version banner, Host, OS, Kernel, Uptime seconds, CPU percent+cores,
Memory percent, 3 load values, process count; missing readings show
`-`/0). Modules-list prints `Plugins:` plus one name per line.
Oracle: subset matrices (rewritten) plus fetch shape (A/B).

## cli/snmp_mode — SNMP client loop
Role: probe an agent and print per-tick summaries. Community
defaults to `public`; construction failures log and exit false;
unreachable agents log the connection message plus a debug probe
line and exit false; reachable ones log the detected OS family
(`unknown` when undetected). Ticks poll the agent (failures warn
and print zeros), print the cpu/mem/load line unless quiet, stop
after one shot on pipes without `--stop-after`, honor the tick cap,
and sleep the refresh gap. All log/print strings frozen.
Oracle: unreachable-agent exit path (offline test, fast timeout).

## main — startup and dispatch
Role: wire everything in order and map modes to exit codes.
Computation: parse, assert Linux, init logging, load config
(warn+defaults on failure) and the password file (secure path or
default; warn+empty on failure), resolve auth (stdin only with
prompt flags), configure the IP opt-in, log the startup banner for
long-running modes only (exact mode list and format), resolve the
effective refresh (config `[global]/refresh` wins only when the CLI
still holds its 2.0 default), then dispatch: Help/Version/Issue/
ApiDoc/Fetch/ModulesList print and succeed; StdoutCsv/Json/Path
run their streamers; WebServer registers, spawns the refresh loop,
logs the listen line, optionally opens a browser after 1s, and
maps serve errors to failure; Client requires `--snmp-force` plus
a host (exact stderr otherwise); Standalone fails with the
removed-UI message; Ping maps probe success/failure/absence to the
three exact outcomes.
Oracle: dispatch matrix via subprocess probes is out of scope for
unit tests; covered by A/B (`--modules-list` exact, `--fetch`
shape, `--version`/`--issue` shapes).

## Test rewrites and oracle
`cli_flags.rs` and `cli_flags_modes.rs` are rewritten with fresh
names and structure (same flag assertions). New `cli_oracle.rs`
pins defaults and precedence.
