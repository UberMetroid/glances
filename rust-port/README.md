# glances-rs

A pure-standard-library Rust port of [Glances](https://github.com/nicolargo/glances), the cross-platform system monitor. This is an ongoing rewrite: see the milestones section below for current status.

## What it is

`glances-rs` is the same system-monitor as Python Glances — CPU, memory, swap, load, network, disk, sensors, processes, alerts, exporters, REST API, MCP server — reimplemented in Rust from scratch. **Zero crates.io dependencies**: only `std`, `core`, `alloc`, and direct `extern "C"` calls to libc / Win32 / Mach. This guarantees:

- No supply-chain attack surface.
- No upstream-version drift.
- A single binary that links deterministically.

Every file in `src/` is **≤ 256 lines** (enforced by `qa/lint/line_cap.rs`). Files are split at functional boundaries into subdirectories; the 256-line cap forces the same decomposition that the original Python code does.

## Status

| Milestone | Status |
|---|---|
| **M0** Repo skeleton + Cargo.toml + CLI parser skeleton | ✅ done (v0.1.0) |
| **M1** Core types: `Value`, `Plugin` trait, `Stats`, `History`, `Threshold`, `Timer`, `Logger`, `Error`, `Filter` | ✅ done (v0.1.0) |
| **M2** Config + Args + Password file loading from disk + full CLI flag set | ✅ done (v0.2.0) |
| **M3** Linux platform primitives: `/proc/stat`, `/proc/meminfo`, `/proc/loadavg`, `/proc/uptime`, `/proc/net/dev`, `/proc/diskstats`, `/sys/class/net`, `/sys/class/hwmon` | ✅ done (v0.3.0) |
| **M4** macOS platform primitives | ⏳ stub (FFI in followup) |
| **M5** Windows platform primitives | ⏳ stub (Win32 FFI in followup) |
| **M6** Core plugins: `cpu`, `mem`, `memswap`, `load`, `uptime`, `now`, `system` | ✅ done (v0.4.0) — **reads live /proc on Linux** |
| M7–M11 | 33 more plugins | ⏳ not started |
| M12 | stdout CSV / JSON outputs | ⏳ not started |
| M13 | 24 network exporters (InfluxDB, Kafka, MQTT, Prometheus, etc.) | ⏳ not started |
| M14 | HTTP/1.1 server + REST API + Vue SPA serving + JWT | ⏳ not started |
| M15 | Curses TUI + browser + XML-RPC + MCP | ⏳ not started |

**Test count:** 191 passing, 0 failing (4 lint + 50 unit + 9 integration + 7 plugin smoke + 21 platform-Linux + …)

## What works today

On Linux, `glances-rs` already:

```bash
$ cargo build --release
$ ./target/release/glances-rs --help
# Prints the full Python-Glances-compatible flag list.

$ ./target/release/glances-rs --version
glances-rs 0.4.0

$ ./target/release/glances-rs --issue
glances-rs 0.4.0 debug/system info dump
OS: linux
Arch: x86_64
Family: unix
...
```

And the seven core plugins read live system state (verified by `qa/integration/plugins_smoke.rs`):
- `cpu` — `/proc/stat` parser with per-state percentages
- `mem` — `/proc/meminfo` parser with `used`/`free`/`available`/`percent`
- `memswap` — `/proc/meminfo` swap fields
- `load` — `/proc/loadavg` + `cpucore` from `std::thread::available_parallelism`
- `uptime` — `/proc/uptime`
- `now` — local time + UTC seconds
- `system` — hostname + `std::env::consts` OS/arch info

## What's not yet wired

- Curses TUI (M15) — the stats are there; the renderer is not.
- Web UI + REST API (M14) — FastAPI/uvicorn equivalent; HTTP/1.1 + SSE + JWT must be hand-rolled.
- XML-RPC server/client (M15).
- MCP server (M15).
- Exporters (M13) — 24 of them, mostly just TCP/UDP/HTTP line-protocol writers.
- 33 additional plugins (M7-M11) — all follow the same pattern as M6.

## Repository layout

```
glances-rs/
├── Cargo.toml               (no [dependencies]; only std + core + alloc + libc FFI)
├── README.md
├── LICENSE (LGPL-3.0)
├── src/
│   ├── main.rs              (CLI dispatch; binary entry point)
│   ├── lib.rs               (re-exports for integration tests)
│   ├── cli/                 (handwritten CLI parser — no clap)
│   ├── core/                (Value, Plugin trait, Stats, History, Threshold, etc.)
│   ├── platform/
│   │   ├── linux/           (8 /proc + /sys readers — pure std, no FFI)
│   │   ├── macos/           (cfg-gated stubs)
│   │   └── windows/         (cfg-gated stubs)
│   ├── plugins/             (one file per plugin — 7 done, ~33 to go)
│   ├── exec/, net/, text/   (placeholders)
│   └── qa/                  (test harness — see Testing)
└── qa/
    ├── lint/                (no_crates, line_cap, unsafe_allowlist, no_shell)
    ├── unit/                (per-module tests + edge cases)
    ├── integration/         (end-to-end smoke tests)
    ├── edge/                (malformed inputs, panic isolation, …)
    └── fixtures/            (sample /proc + /sys captures)
```

## Testing

```bash
cargo test            # all 191 tests
cargo test --release  # same, optimized

# Lint enforcement:
cargo test qa::lint::no_crates          # AC-1: no [dependencies] in Cargo.toml
cargo test qa::lint::line_cap          # AC-2: every .rs file ≤ 256 lines
cargo test qa::lint::unsafe_allowlist  # AC-11: no unsafe outside src/platform/* and src/exec/safe_run.rs
cargo test qa::lint::no_shell          # §3.5: no `sh`, `bash`, `cmd.exe /C`

# Plugin smoke (requires Linux):
cargo test plugins_smoke
```

## Hard constraints (enforced by lint)

1. **No `extern crate` and no `[dependencies]`** in `Cargo.toml`.
2. **No file > 256 lines** (comments and blank lines count — encourages terse code).
3. **No `unsafe`** outside `src/platform/{linux,macos,windows}/` and `src/exec/safe_run.rs`.
4. **No shell expansion** — `std::process::Command` with explicit argv, never `sh -c`.
5. **No path concatenation** — every file path is resolved via `std::path::PathBuf::join`.

## License

LGPL-3.0-only, matching upstream Glances.

## Acknowledgements

Based on the Python [Glances](https://github.com/nicolargo/glances) project by Nicolas Hennion and contributors. The Rust port follows the same architecture, plugin model, exporter set, and CLI surface.
