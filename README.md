# glances-rs

A Linux system monitor in one static binary. CPU, memory, load, network, disk, sensors, processes, alerts, and 24 telemetry exporters — via TUI, REST, SSE, XML-RPC, MCP, CSV, or JSON. A from-scratch port of [Glances](https://github.com/nicolargo/glances) in pure standard-library Rust.

[![ci](https://github.com/UberMetroid/glances-rs/actions/workflows/ci.yml/badge.svg?branch=rust)](https://github.com/UberMetroid/glances-rs/actions/workflows/ci.yml) [![version](https://img.shields.io/badge/version-v0.10.11-ce422b.svg)](https://github.com/UberMetroid/glances-rs/releases) [![dependencies](https://img.shields.io/badge/dependencies-0-success.svg)](Cargo.toml) [![license](https://img.shields.io/badge/license-LGPL--3.0--only-blue.svg)](LICENSE) [![rust](https://img.shields.io/badge/rust-1.98.1%2B-orange.svg)](rust-toolchain.toml) [![platform](https://img.shields.io/badge/platform-linux--only-2f6f5e.svg)](#install)

→ [Live site](https://ubermetroid.github.io/glances-rs/) · [About](https://ubermetroid.github.io/glances-rs/about.html) · [Docs](https://github.com/UberMetroid/glances-rs-docs) · [Source](https://github.com/UberMetroid/glances-rs)

## First principles

A system monitor reads numbers the kernel already publishes (`/proc`, `/sys`, sockets) and shows them to a human or ships them to another tool. That is the whole job. Everything else — plugins, exporters, servers — is delivery.

Python Glances does this job with an interpreter, a web framework, and a dozen exporter libraries. glances-rs keeps the same architecture, plugin model, and CLI surface, but the dependency graph is empty: one stripped executable, no interpreter, no `site-packages`, nothing to audit upstream.

## Zero trust

The monitor is read-only, but it still listens on ports and parses input. Every row below names something untrusted and what the code does about it.

| Distrusted input | Defense |
|---|---|
| Crate supply chain | No `[dependencies]` at all — enforced by a build-time lint. There is nothing upstream to compromise. |
| Memory safety | `unsafe` only under `src/platform/linux/`, one portable syscall per block, allowlisted by lint. |
| Subprocess injection | `no_shell` lint: `Command` with explicit argv only, never `sh -c`. |
| Paths from flags/config | `PathBuf::join` only; no `format!`-built paths. |
| HTTP exposure | Server binds `0.0.0.0:61208` — put it on a public network only with auth enabled (`--password` or a password file). No built-in TLS; terminate behind nginx/Caddy. |
| Password handling | `--password` and `-u` never take argv values (they leak via `ps`); credentials come from stdin prompts or the `0600` password file. |

Supply chain and secrets are also scanned by studio2201 ([snip · vigil · aegis · proven · boneyard](https://github.com/UberMetroid/glances-rs/tree/rust/.github/workflows)).

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/UberMetroid/glances-rs/rust/install.sh -o install.sh
sh install.sh
```

The installer verifies its own SHA-256 against the published `install.sh.sha256`, then drops the binary into `${XDG_BIN_HOME:-$HOME/.local/bin}`. Afterwards:

```bash
export PATH="$HOME/.local/bin:$PATH"
glances-rs --version
```

From source instead:

```bash
git clone https://github.com/UberMetroid/glances-rs.git
cd glances-rs
cargo build --release
./target/release/glances-rs --help
```

Needs Rust 1.98.1 or newer.

## Run

```bash
glances-rs                                # TUI (q quit, arrows select, h help)
glances-rs --stdout-json --stop-after 1   # one JSON snapshot to stdout
glances-rs -w                             # HTTP server: REST + SSE + UI on 61208
glances-rs -s                             # XML-RPC server on 61209
glances-rs -c 192.168.1.10                # XML-RPC client, one getAll
```

`--help` lists every flag. Full CLI reference in [glances-rs-docs](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/cli.md).

## Scope

35 plugins (`cpu`, `mem`, `load`, `network`, `diskio`, `fs`, `sensors`, `gpu`, `processlist`, `alert`, …), 24 exporters (`--export prometheus`, `--export influxdb`, …), 6 output surfaces. Anything not implemented answers an error — never a silent stub. Owned gaps live in [limitations](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/limitations.md).

## Constraints

Enforced by lint on every build. No opt-out.

1. No `[dependencies]` in `Cargo.toml` — `std`/`core`/`alloc` plus direct libc FFI only.
2. No file longer than 256 lines, comments included.
3. No `unsafe` outside `src/platform/linux/`.
4. No shell expansion — explicit argv only.

## Documentation

Full docs live in [glances-rs-docs](https://github.com/UberMetroid/glances-rs-docs): [architecture](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/architecture.md) · [plugins](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/plugins.md) · [exporters](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/exporters.md) · [outputs](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/outputs.md) · [build & test](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/build.md) · [security](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/security.md) · [limitations](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/limitations.md) · [CLI](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/cli.md)

## License & Credits

**LGPL-3.0-only**, matching upstream Glances. See [`LICENSE`](LICENSE).

Derivative work of [Glances](https://github.com/nicolargo/glances) by **Nicolas Hennion** (Nicolargo) and contributors — the architecture, plugin model, exporter set, and CLI surface are theirs. Full attribution on the [about page](https://ubermetroid.github.io/glances-rs/about.html).

