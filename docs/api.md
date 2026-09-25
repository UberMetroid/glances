# glances-rs REST API

JSON over HTTP from `glances-rs -w` (default port 61208). The dashboard
at `/` polls these same routes. Versioned paths live under `/api/4/`;
unversioned `/api/` aliases serve identical payloads. Unknown paths 404.

Auth: start with `--auth-enabled` (plus `--username`/`--password` or
`-u`) and every request needs HTTP Basic except `/favicon.ico` (401
otherwise). There is no JWT issuer: `POST /api/4/token` answers 501.

API key: set `GLANCES_API_KEY` to a secret and every data route needs
`X-API-Key: <secret>` instead (401 otherwise, no Basic challenge).
With both on, either credential passes. Without the env var the server
falls back to the first line of `$XDG_CONFIG_HOME/glances/api-key`
(`~/.config/...` when unset) — the `0600` file `install.sh` writes.
Missing or blank file means no gate. The dashboard page itself stays
open so it can ask for the key once, then keeps it in `sessionStorage`
(forgotten when the tab closes). No built-in TLS: the key travels in
cleartext — fine on loopback/Tailscale, not the open internet.

## Routes

- `GET /api/4/status` — liveness: `{"version": "0.10.76"}`.
- `GET /api/4/pluginslist` — registered plugin names (36 by default;
  `irq` needs `--enable-plugin irq`).
- `GET /api/4/serverslist` — always `[]` (single-host server).
- `GET /api/4/all`, `/api/all/values`, `/api/all/stats` — full snapshot,
  every plugin's current object in one dictionary.
- `GET /api/4/all/limits`, `/api/all/limits` — alert thresholds per plugin.
- `GET /api/4/all/views`, `/api/all/views`, `/api/all/description` —
  per-plugin view metadata (sort key, declared fields, alert decorations).
- `GET /api/4/history` — recorded history for every plugin:
  `{"<plugin>": {"<field>": [[timestamp, value], ...]}}`.
- `GET /api/4/health` — worst-of rollup: `{status, summary, checks[]}`,
  non-ok checks first. See thresholds below.
- `GET /api/4/dashboard` — the dashboard's 2s bundle in one round
  trip: 16 fast plugin payloads plus the health rollup under
  `"health"` (processlist and the alert log stay out — the page
  fetches those every 10s).
- `GET /api/4/{plugin}` — one plugin's current object (also
  `/api/4/{plugin}/values` and the `/api/` forms).
- `GET /api/4/{plugin}/description` — field catalog:
  `[{"name": ..., "unit": ...}]`.
- `GET /api/4/{plugin}/history[/{n}]` — per-field `[[ts, value]]`
  arrays, oldest first; `{n}` keeps the tail (`0` = all).
- `GET /api/4/processes/{pid}` — one process by pid (404 when absent).
- `GET /api/4/processes/extended` — pinned extended-stats process,
  `{}` when none.
- `POST /api/4/processes/extended/{pid}` — pin it (one at a time).
- `POST /api/4/processes/extended/disable` — unpin.
- `POST /api/4/events/clear/all` — drop all alerts.
- `POST /api/4/events/clear/warning` — drop warnings (no per-severity
  route for critical; use `/all`).
- `GET /api/4/events/stream` — Server-Sent Events; opens with a
  `hello` event, then alert ticks.
- `GET /healthz` — `ok` (ops check, not JSON).
- `GET /openapi.json` — this API as OpenAPI 3.1 JSON.
- `POST /mcp` — MCP (JSON-RPC), not REST; out of scope here.

Health thresholds (`/api/4/health`): fs/memory ≥90 warn, ≥95
critical; swap ≥50/90; gpu temp ≥80/90 °C; sensor temp ≥85/95 °C; any
CRITICAL alert from the last 60s → critical, WARNING/CAREFUL → warning
(older entries fade without a manual clear); raid failed>0 →
critical, degraded/offline → warning; SMART value≤threshold →
critical. Checks only cover plugins reporting data.

Not served (upstream has them; here they 404 — fetch the whole plugin
object instead): `/api/4/{plugin}/{item}` and everything under it
(`/description`, `/unit`, `/history`, `/value/{v}`),
`/api/4/{plugin}/limits`, `/api/4/{plugin}/top/{n}`.

`glances-rs --api-doc-restful` prints this route list from the binary. Machine-readable spec: `GET /openapi.json`.

## Plugins (36 live, registration order)

`cpu`, `percpu`, `processcount`, `processlist`, `programlist`, `ip`,
`mem`, `memswap`, `load`, `uptime`, `now`, `system`, `fs`, `diskio`,
`folders`, `raid`, `network`, `connections`, `ports`, `containers`,
`cloud`, `amps`, `smart`, `vms`, `sensors`, `gpu`, `npu`, `power`,
`wifi`, `mpp`, `pressure`, `alert`, `quicklook`, `help`, `version`,
`psutilversion`
(`irq` is 37th, disabled by default per upstream).

`power` reports `{cpu_watts, gpu_watts, total_watts, kwh_rate,
usd_per_month, sources[]}` from RAPL (CPU, needs root), `nvidia-smi`
power draw, and amdgpu hwmon; missing sources stay Null. The rate
comes from `GLANCES_KWH_RATE` or `[power] kwh_rate` in `glances.conf`
(dollars per kWh, e.g. `0.08`); the env var wins when both are set.

`network` lists every interface with counters (loopback included;
tunnel `unknown` operstate counts as up), an `ip_addresses` array
matched from the route table, and a `role` tag (`loopback`,
`tailscale`, `local`, or blank). `ip` reports the default route's
address/mask/gateway/mac; `public_ip` fills only when `[ip]
public_api` (or `GLANCES_PUBLIC_API`, env wins) names a plain-`http`
endpoint — `https` is refused, this build ships no TLS stack.

The live list is truth: `GET /api/4/pluginslist`.

## Examples

Values below are from a live host; keys are the stable part.

```bash
$ curl -s localhost:61208/api/4/status
{"version": "0.10.76"}

$ curl -s localhost:61208/api/4/cpu
{"total": 3.97, "user": 2.73, "system": 0.60, "idle": 95.91,
 "iowait": 0.12, "cpucore": 12.0, ...}

$ curl -s localhost:61208/api/4/cpu/description
[{"name": "total", "unit": "Percent"}, {"name": "user", "unit": "Percent"}, ...]

$ curl -s localhost:61208/api/4/processes/1 | head -c 120
{"pid": 1.0, "name": "systemd", ...}
```

## GPU clients & transcoding

Each object in `GET /api/4/gpu` carries `clients` — processes
actively using that card (`pid`, `name`, `service` when the
process, its program path, or one of its parents matches Jellyfin,
Emby, Plex, InvokeAI, or Ollama, NVIDIA-only `mem_mb`, per-client
`transcoding`) — plus card-level `transcoding` (a transcoder
driving a video engine) and `transcoding_by` (the service, or
process name, behind it). The dashboard shows one letter chip per
known service under the card (`transcoding:` and `using:` stay in
separate segments so a bystander is never mislabeled).

Sources: NVIDIA clients come from `nvidia-smi` compute apps mapped
per-GPU by UUID; Intel/AMD clients come from kernel fdinfo engine
counters (video/dec/enc engines advancing ≥1ms per tick). Idle
contexts are omitted; unreadable processes are skipped, never
errors.
