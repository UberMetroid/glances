# glances-rs

A Linux system monitor in one static binary. CPU, memory, swap, load, network, disk, sensors, processes, alerts, and 19 telemetry exporters — served via REST, SSE, XML-RPC, MCP, CSV, or JSON.

Single `glances-rs` binary. Zero runtime dependencies. Drop on any Linux box and run.

→ [Live webpage](https://ubermetroid.github.io/glances-rs/) &middot; [About](https://ubermetroid.github.io/glances-rs/about.html) &middot; [Docs](https://github.com/UberMetroid/glances-rs-docs) &middot; [Source](https://github.com/UberMetroid/glances-rs)

| | |
|---|---|
| ![version](https://img.shields.io/badge/version-v0.10.3-blue.svg) | ![license](https://img.shields.io/badge/license-LGPL--3.0--only-blue.svg) |
| ![rust](https://img.shields.io/badge/rust-1.98.1%2B-orange.svg?logo=rust) | ![platforms](https://img.shields.io/badge/platforms-linux%20only-2f6f5e.svg) |
| ![tests](https://img.shields.io/badge/tests-704%20passing-2f6f5e.svg) | ![size](https://img.shields.io/badge/size-single%20static%20binary-2f6f5e.svg) |

##

| Security Pillar | Verification Badge |
|---|---|
| **Platform Standard** | [![secured by studio2201](https://img.shields.io/badge/secured%20by-studio2201-2f6f5e?logo=shield)](https://studio2201.com) |
| **Credential Defense** | [![snip: 0 secrets](https://img.shields.io/badge/snip-0%20secrets-2f6f5e?logo=shield)](https://studio2201.com/snip) |
| **Supply Chain Surface** | [![vigil: 0 dependencies](https://img.shields.io/badge/vigil-0%20dependencies-2f6f5e?logo=shield)](https://studio2201.com/vigil) |
| **Post-Quantum Cryptography** | [![aegis: PQC compliant](https://img.shields.io/badge/aegis-PQC%20compliant-2f6f5e?logo=shield)](https://studio2201.com/aegis) |
| **Build Provenance & SLSA** | [![proven: ML-DSA-65 verified](https://img.shields.io/badge/proven-ML--DSA--65%20verified-2f6f5e?logo=shield)](https://studio2201.com/proven) |
| **Repository Governance** | [![boneyard: maintained](https://img.shields.io/badge/boneyard-maintained-2f6f5e?logo=shield)](https://studio2201.com/boneyard) |

## What it is

`glances-rs` is a Rust reimplementation of [Glances](https://github.com/nicolargo/glances), the cross-platform system monitor by Nicolas Hennion (Nicolargo) and contributors. Same architecture, same plugin model, same exporter set, same CLI surface — reimplemented from scratch in pure standard-library Rust.

The binary is a single stripped executable. It runs on a fresh Linux box with nothing else installed. It reads `/proc` and `/sys`, computes per-plugin snapshots, and ships them to your tool of choice.

It does not use `serde`, `tokio`, `clap`, `hyper`, or any other crate. The whole dependency graph is empty. `Cargo.toml` has no `[dependencies]` section, enforced by a build-time lint.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/UberMetroid/glances-rs/rust/install.sh -o install.sh
sh install.sh
```

The installer self-verifies its own SHA-256 against the published `install.sh.sha256` before doing anything. Drops the binary into `${XDG_BIN_HOME:-$HOME/.local/bin}`.

After install:

```bash
export PATH="$HOME/.local/bin:$PATH"
glances-rs --version
glances-rs --help
```

For a one-shot build from source:

```bash
git clone https://github.com/UberMetroid/glances-rs.git
cd glances-rs
cargo build --release
./target/release/glances-rs --help
```

You need Rust 1.98.1 or newer.

## Run

```bash
# One-shot JSON snapshot to stdout
glances-rs --stdout-json --stop-after 1

# Selected stats as "name: value" lines
glances-rs --stdout cpu.total,mem.percent --stop-after 1

# HTTP server on port 61208 (REST + SSE + Vue UI + /xmlrpc + /mcp)
glances-rs -w

# XML-RPC server on port 61209 (Python xmlrpc.client compatible)
glances-rs -s

# XML-RPC client (one getAll, prints response, exits)
glances-rs -c 192.168.1.10

# Live tail for the next 10 minutes
glances-rs --stdout-csv -t 5 --stop-after 120

# Just the version banner
glances-rs --version
```

`--help` enumerates every flag. The full CLI surface is documented at [github.com/UberMetroid/glances-rs-docs](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/cli.md).

## What you get

**31 plugins** on Linux, reading live state from `/proc`, `/sys`, and the network stack:

- Core: `cpu`, `percpu`, `irq`, `processcount`, `ip`, `mem`, `memswap`, `load`, `uptime`, `now`, `system`
- I/O: `diskio`, `fs`, `folders`, `raid`, `network`, `connections`, `ports`
- Sensors: `sensors`, `gpu`, `npu`, `wifi`, `mpp`
- Containers / cloud: `containers`, `cloud`, `amps`
- Meta: `alert`, `quicklook`, `help`, `version`, `psutilversion`

**19 exporters**: CSV, JSON, InfluxDB v1 + v2 + v3, StatsD, Prometheus, RESTful, ClickHouse, CouchDB, Elasticsearch, Kafka, MongoDB, MQTT, NATS, OpenTSDB, RabbitMQ, Riemann, Cassandra — all reachable via `--export <name>` (see `--help` for the `--export-<name>-*` option flags).

**5 surfaces**: HTTP REST + SSE (`-w` / `--webserver`), XML-RPC server (`-s`), XML-RPC client (`-c`), MCP-over-JSON-RPC (`POST /mcp`), stdout (`--stdout <spec>`, `--stdout-csv`, `--stdout-json`), plus the standalone monitor (no args).

## Why pure-stdlib Rust

- **No supply-chain attack surface.** The dependency graph is empty. There is nothing to audit, nothing to patch, nothing to compromise upstream.
- **Deterministic linking.** Same source + same `rustc` produces the same binary bytes. Drop the executable on a server and run.
- **Single static binary.** Strip, LTO, `codegen-units = 1`. No `LD_LIBRARY_PATH`, no `pip install`, no `node_modules`.
- **Auditable `unsafe` surface.** Every line of `unsafe` lives under `src/platform/linux/`, enforced by build-time lint.
- **One file = one idea.** Every `.rs` file is ≤ 256 lines. The cap forces the same decomposition that the original Python Glances code uses.

The cost is reinvention: a ~120-line hand-rolled JSON serializer (`src/core/value.rs`), a ~200-line INI parser (`src/core/config.rs`), a ~150-line SHA-256 (`src/core/sha256.rs`). Each is small enough to review in a sitting.

## Constraints (enforced by lint at every `cargo build`)

1. No `[dependencies]` in `Cargo.toml`. Only `std`, `core`, `alloc`, and direct `extern "C"` to libc / Win32 / Mach.
2. No file longer than 256 lines. Comments and blank lines count.
3. No `unsafe` outside `src/platform/linux/`.
4. No shell expansion. `std::process::Command` with explicit argv, never `sh -c`.

If any of these fail, the build fails. There is no opt-out.

## Platforms

| OS | Status | Notes |
|---|---|---|
| Linux x86_64 | ✓ supported | All 31 plugins read live `/proc` and `/sys`. |
| Linux aarch64 | ✓ supported | Same code paths; tested on Raspberry Pi 4 / 5. |
| macOS | ✗ removed | The `macos` platform module was deleted in v0.9.0 — see [Why Linux-only](#why-linux-only) below. |
| Windows | ✗ removed | The `windows` platform module was deleted in v0.9.0. |

## Documentation

Full documentation lives in a separate repository: [github.com/UberMetroid/glances-rs-docs](https://github.com/UberMetroid/glances-rs-docs).

- [Architecture](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/architecture.md)
- [Plugins](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/plugins.md)
- [Exporters](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/exporters.md)
- [Outputs](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/outputs.md)
- [Build & test](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/build.md)
- [Security model](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/security.md)
- [Limitations](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/limitations.md)
- [CLI reference](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/cli.md)

## License

**LGPL-3.0-only**, matching upstream Glances so the Rust port can be linked into GPL-compatible systems without relicensing friction. See [`LICENSE`](LICENSE) in the repo root.

## Credits

`glances-rs` is a derivative work of [Glances](https://github.com/nicolargo/glances) by **Nicolas Hennion** (Nicolargo) and contributors. The original architecture, plugin model, exporter set, and CLI surface are theirs. See [the about page](https://ubermetroid.github.io/glances-rs/about.html) for the full attribution.

## Why Linux-only

The maintainer has only Linux hardware to test on. Shipping macOS or Windows ports as stubs that error at runtime is misleading — better to delete them honestly than pretend they work. The FFI surface (libc sysctl, Mach, IOKit, kernel32, psapi, iphlpapi, pdh) is well-documented and a competent contributor with a Mac or Windows box could restore support in ~2 weeks of focused work. If that sounds like you, open an issue with a hardware-donation offer.
