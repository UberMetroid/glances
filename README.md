# glances-rs

A Linux system monitor in one static binary. CPU, memory, load, network, disk, sensors, processes and alerts — via dashboard, REST, SSE, MCP, CSV, or JSON. A from-scratch port of [Glances](https://github.com/nicolargo/glances) in pure standard-library Rust.

[![ci](https://github.com/UberMetroid/glances-rs/actions/workflows/ci.yml/badge.svg?branch=rust)](https://github.com/UberMetroid/glances-rs/actions/workflows/ci.yml) [![version](https://img.shields.io/badge/version-v0.10.53-ce422b.svg)](https://github.com/UberMetroid/glances-rs/releases) [![dependencies](https://img.shields.io/badge/dependencies-0-success.svg)](Cargo.toml) [![license](https://img.shields.io/badge/license-LGPL--3.0--only-blue.svg)](LICENSE) [![rust](https://img.shields.io/badge/rust-1.98.1%2B-orange.svg)](rust-toolchain.toml) [![platform](https://img.shields.io/badge/platform-linux--only-2f6f5e.svg)](#install)
[![secured by studio2201](https://img.shields.io/badge/secured%20by-studio2201-2f6f5e?logo=shield)](https://studio2201.com/) [![snip](https://img.shields.io/badge/snip-secrets%20audited-2f6f5e?logo=shield)](https://studio2201.com/snip) [![vigil](https://img.shields.io/badge/vigil-dependencies%20scanned-2f6f5e?logo=shield)](https://studio2201.com/vigil) [![aegis](https://img.shields.io/badge/aegis-PQC%20ready-2f6f5e?logo=shield)](https://studio2201.com/aegis) [![proven](https://img.shields.io/badge/proven-attestation%20ready-2f6f5e?logo=shield)](https://studio2201.com/proven) [![boneyard](https://img.shields.io/badge/boneyard-maintained-2f6f5e?logo=shield)](https://studio2201.com/boneyard)

→ [Live site](https://ubermetroid.github.io/glances-rs/) · [About](https://ubermetroid.github.io/glances-rs/about.html) · [Docs](https://github.com/UberMetroid/glances-rs-docs) · [Source](https://github.com/UberMetroid/glances-rs)

## First principles

A system monitor reads numbers the kernel already publishes (`/proc`, `/sys`, sockets) and shows them to a human or ships them to another tool. That is the whole job. Everything else — plugins, servers — is delivery.

Python Glances does this job with an interpreter, a web framework, and a dozen exporter libraries. glances-rs keeps the same architecture, plugin model, and CLI surface, but the dependency graph is empty: one stripped executable, no interpreter, no `site-packages`, nothing to audit upstream.

## Zero trust

The monitor is read-only, but it still listens on ports and parses input. Every row below names something untrusted and what the code does about it.

| Distrusted input | Defense |
|---|---|
| Crate supply chain | No `[dependencies]` at all — enforced by a build-time lint. There is nothing upstream to compromise. |
| Memory safety | `unsafe` only under `src/platform/linux/`, one portable syscall per block, allowlisted by lint. |
| Subprocess injection | `no_shell` lint: `Command` with explicit argv only, never `sh -c`. |
| Paths from flags/config | `PathBuf::join` only; no `format!`-built paths. |
| HTTP exposure | Server binds `0.0.0.0:61208` — put it on a public network only with auth enabled (`--password`, a password file, or `GLANCES_API_KEY`). No built-in TLS; terminate behind nginx/Caddy. |
| Password handling | `--password` and `-u` never take argv values (they leak via `ps`); credentials come from stdin prompts or the `0600` password file. |

Supply chain and secrets are also scanned by studio2201 ([snip](.github/workflows/snip.yml) · [vigil](.github/workflows/vigil.yml) · [aegis](.github/workflows/aegis.yml) · [proven](.github/workflows/proven.yml) · [boneyard](.github/workflows/boneyard.yml)).

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

Lock it down at install time (blank = open):

```bash
GLANCES_API_KEY=$(openssl rand -hex 32) sh install.sh
```

Saves the key `0600` to `~/.config/glances/api-key` (or
`$XDG_CONFIG_HOME`); `glances-rs -w` then requires `X-API-Key` on every
data route. Reinstalls keep an existing file; delete it to reopen.
A `GLANCES_API_KEY` in the launch environment overrides the file.

From source instead:

```bash
git clone https://github.com/UberMetroid/glances-rs.git
cd glances-rs
cargo build --release
./target/release/glances-rs --help
```

Needs Rust 1.98.1 or newer.

Or run the distroless container (same binary, host-PID view):

```bash
podman build --format docker -t glances-rs:0.10.53 -f install/docker/Containerfile .
podman run -d --name glances-rs --pid=host --net=host \
  -v /sys:/sys:ro -v /:/host:ro -e GLANCES_ROOTFS=/host \
  --device nvidia.com/gpu=all \
  -v /usr/bin/nvidia-smi:/usr/bin/nvidia-smi:ro \
  glances-rs:0.10.53 -w
```

- `--pid=host` + `--net=host`: the monitor sees host processes and serves the host's port 61208 directly.
- `-v /:/host:ro` + `GLANCES_ROOTFS=/host`: filesystems report the host mounts, not container overlay.
- `--device nvidia.com/gpu=all` plus the `nvidia-smi` bind: live NVIDIA util/VRAM/temperature (needs the NVIDIA container toolkit CDI spec on the host; without it those fields read Null).
- `-e GLANCES_KWH_RATE=0.08`: your electricity price in dollars per kWh — enables the dollars-per-month figure in the Power section (or set `[power] kwh_rate` in `glances.conf`; the env var wins).
- `-e GLANCES_PUBLIC_API=http://api.ipify.org`: opt-in public IP lookup (refreshed every 5 minutes by a background thread; blank = off, `https` refused).
- Container files live in `install/docker/`; `.containerignore` stays at the repo root because podman only reads it from the build context.
- Releases: push a `vX.Y.Z` tag and the release workflow builds, smokes, and pushes `ghcr.io/ubermetroid/glances-rs:X.Y.Z` plus `:latest` (Unraid installs from `:latest`).

## Run

```bash
glances-rs --stdout-json --stop-after 1   # one JSON snapshot to stdout
glances-rs -w                             # HTTP server: REST + SSE + UI on 61208
glances-rs -c 192.168.1.10 --snmp-force --snmp-community public   # SNMP client
```

`--help` lists every flag. Full CLI reference in [glances-rs-docs](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/cli.md).

## API

The `-w` server speaks upstream-compatible REST on port 61208: `GET /api/4/{plugin}` for any plugin, `/api/4/all` for the full snapshot, per-field history, an SSE event stream, and `POST` mutators for alerts and extended process stats. HTTP Basic when started with `--auth-enabled` (or `X-API-Key` when `GLANCES_API_KEY` is set); no JWT issuer (`POST /api/4/token` answers 501). Full route reference with examples: [docs/api.md](docs/api.md). `GET /api/4/health` rolls every check into one status for dashboards and scripts. When nobody has asked for data in 30 seconds the server drops to one heartbeat tick per 30 seconds instead of ticking every 2 seconds, and resumes full speed on the next request.

## Scope

36 plugins (`cpu`, `mem`, `load`, `network`, `diskio`, `fs`, `sensors`, `gpu`, `processlist`, `alert`, …), 6 output surfaces. Anything not implemented answers an error — never a silent stub. Owned gaps live in [limitations](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/limitations.md).

## Constraints

Enforced by lint on every build. No opt-out.

1. No `[dependencies]` in `Cargo.toml` — `std`/`core`/`alloc` plus direct libc FFI only.
2. No file longer than 256 lines, comments included.
3. No `unsafe` outside `src/platform/linux/`.
4. No shell expansion — explicit argv only.

## Documentation

Full docs live in [glances-rs-docs](https://github.com/UberMetroid/glances-rs-docs): [architecture](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/architecture.md) · [plugins](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/plugins.md) · [outputs](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/outputs.md) · [build & test](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/build.md) · [security](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/security.md) · [limitations](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/limitations.md) · [CLI](https://github.com/UberMetroid/glances-rs-docs/blob/main/docs/cli.md)

## License & Credits

**LGPL-3.0-only**, matching upstream Glances. See [`LICENSE`](LICENSE).

Derivative work of [Glances](https://github.com/nicolargo/glances) by **Nicolas Hennion** (Nicolargo) and contributors — the architecture, plugin model, and CLI surface are theirs. Full attribution on the [about page](https://ubermetroid.github.io/glances-rs/about.html).

