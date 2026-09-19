# glances-rs

**glances-rs** is a pure-standard-library Rust port of [Glances](https://github.com/nicolargo/glances), the cross-platform system monitor &mdash; reimplemented from scratch with **zero crates.io dependencies** and shipped as a single static binary. &rarr; [Live webpage](site/index.html) &middot; [About](site/about.html) &middot; [Source on GitHub](https://github.com/UberMetroid/glances-rs/tree/Rust)

Current version: **0.8.0** &middot; **596 tests passing**

---

## Status

| Milestone | Version | Scope |
|---|---|---|
| **M0** | v0.1.0 | Repo skeleton, `Cargo.toml`, CLI parser skeleton. |
| **M1** | v0.1.0 | Core types: `Value`, `Plugin` trait, `Stats`, `History`, `Threshold`, `Timer`, `Logger`, `Error`, `Filter`. |
| **M2** | v0.2.0 | Config + Args + password file loading from disk and the full CLI flag set. |
| **M3** | v0.3.0 | Linux platform primitives: `/proc/stat`, `/proc/meminfo`, `/proc/loadavg`, `/proc/uptime`, `/proc/net/dev`, `/proc/diskstats`, `/sys/class/net`, `/sys/class/hwmon`. |
| **M4** | v0.4.0 | macOS platform primitives &mdash; scaffolded; FFI pending. |
| **M5** | v0.4.0 | Windows platform primitives &mdash; scaffolded; Win32 FFI pending. |
| **M6** | v0.4.0 | 7 core plugins: `cpu`, `mem`, `memswap`, `load`, `uptime`, `now`, `system`. Reads live `/proc` on Linux. |
| **M7&ndash;M11** | v0.6.0 | 24 plugins total: `percpu`, `irq`, `processcount`, `ip`, `fs`, `diskio`, `folders`, `raid`, `network`, `connections`, `ports`, `containers`, `cloud`, `amps`, `sensors`, `gpu`, `npu`, `wifi`, `mpp`, `alert`, `quicklook`, `help`, `version`, `psutilversion`. |
| **M12** | v0.5.0 | stdout CSV / JSON / `api_doc` outputs. |
| **M13** | v0.6.0 | 18 exporters: `csv`, `json`, `influxdb`, `influxdb2`, `statsd`, `prometheus`, `restful`, `cassandra`, `clickhouse`, `couchdb`, `elasticsearch`, `kafka`, `mongodb`, `mqtt`, `nats`, `opentsdb`, `rabbitmq`, `riemann`. |
| **M14** | v0.5.0 | HTTP server scaffolding: REST API, auth, SSE. |
| **M15** | v0.7.0 | XML-RPC server + MCP-over-JSON-RPC + cleanup. TUI, browser UI deferred to followup. |

**Tests:** `596` passing.

---

## Why pure stdlib?

- **No supply-chain attack surface.** No transitive dependency graph to audit, patch, or compromise.
- **Deterministic linking.** The same source tree always produces the same binary.
- **Single static binary.** Strip, LTO, `codegen-units = 1` &mdash; drop the executable on a server and run it.
- **Auditable `unsafe` surface.** Every line of `unsafe` lives under `src/platform/{linux,macos,windows}/`, enforced by lint.

---

## Building

```bash
cargo build --release
```

The release profile in `Cargo.toml` enables `opt-level = 3`, `lto = "thin"`, `codegen-units = 1`, and `strip = true`. The resulting binary at `./target/release/glances-rs` is fully static on Linux x86_64 (no `libpython`, no `site-packages`, no shared crates).

---

## Running

Show the full CLI surface (the same flag set as upstream Python Glances):

```bash
./target/release/glances-rs --help
```

Print the version banner:

```bash
./target/release/glances-rs --version
```

Dump OS / arch / library info for bug reports:

```bash
./target/release/glances-rs --issue
```

Stream live stats to stdout as CSV:

```bash
./target/release/glances-rs --stdout csv
```

Stream live stats to stdout as JSON:

```bash
./target/release/glances-rs --stdout json
```

Launch the embedded HTTP server on port `61208` (REST API + SSE + Vue UI):

```bash
./target/release/glances-rs --web
```

---

## Constraints

The crate is **std-only**. Hard rules, enforced by lint at every `cargo build`:

1. **No `[dependencies]`** and no `extern crate` in `Cargo.toml`. Only `std`, `core`, `alloc`, and direct `extern "C"` to libc / Win32 / Mach.
2. **No file &gt; 256 lines.** Comments and blank lines count &mdash; this forces the same decomposition the original Python code does.
3. **No `unsafe`** outside `src/platform/{linux,macos,windows}/` and `src/exec/safe_run.rs`.
4. **No shell expansion.** `std::process::Command` with explicit argv, never `sh -c`.
5. **No path concatenation.** Every path is resolved via `std::path::PathBuf::join`.

---

## License

**LGPL-3.0-only**, matching upstream Glances so the Rust port can be linked into GPL-compatible systems without relicensing friction. See `LICENSE` in the repo root.

---

## Acknowledgements

Based on the Python [Glances](https://github.com/nicolargo/glances) project by Nicolas Hennion (<em>Nicolargo</em>) and contributors. The Rust port follows the same architecture, plugin model, exporter set, and CLI surface.
