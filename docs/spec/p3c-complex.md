# Spec: P3 batch C — complex plugins

Behavioral contract for rewriting the ten remaining TAINTED plugin
files, derived from observed behavior of the v0.10.79 tree (T0),
live OS interfaces, and installed manuals. Code is written to this
spec — never to the old implementation.

## amps — application monitors (stub)

Role: registers the `amps` key so dashboards can list it; emits an
empty array until config-driven AMP wiring lands.

- Inputs: none.
- Computation: none; `update()` resets stats to `[]`.
- Outputs: `Value::Array([])`; item key `name`.
- API: `NAME`, `register`, `AmpsPlugin::{new,default}` + `Plugin` impl.

## cloud — cloud-provider identity

Role: detect AWS/GCP/Azure via the link-local metadata service and
report `{provider, instance_id, region, zone}`; `{provider:"none"}`
off-cloud. Never errors.

- Inputs: TCP 169.254.169.254:80, three GET probes in order —
  AWS `/latest/dynamic/instance-identity/document` (no extra headers),
  GCP `/computeMetadata/v1/instance/?recursive=true`
  (`Metadata-Flavor: Google`), Azure
  `/metadata/instance?api-version=2021-02-01&format=json`
  (`Metadata: true`). First 2xx body that parses wins.
- Computation: `http_get` does connect+write+bounded read, splits
  headers/body on `\r\n\r\n`, requires `HTTP/1.` status 200-299.
  `parse_aws` reads `instanceId`/`region`/`availabilityZone`;
  `parse_gcp` reads `instance.id` (number-tolerant) and splits
  `instance.zone`'s trailing `region-letter` segment on the last `-`;
  `parse_azure` reads `compute.vmId`/`location`/`availabilityZone`.
  Per-probe ceiling 400ms, total budget 1000ms; result cached after
  the first tick (cloud identity is boot-stable).
- Outputs: object with `provider` + (`instance_id`,`region`,`zone`
  when in cloud); item key `provider`.
- API: `NAME`, `register`, `CloudPlugin`, `METADATA_IP/PORT/ADDR`,
  `TOTAL_BUDGET`, `PER_PROBE_TIMEOUT`, `http_get`, `parse_aws`,
  `parse_gcp`, `parse_azure`, `none_object`, `probe`.

## containers — Docker container list

Role: list containers via the Docker Engine API over
`/var/run/docker.sock`. Any failure → empty array, never a stall.

- Inputs: `GET /containers/json` over the unix socket; 2xx body
  parsed as a JSON array.
- Computation: `docker_request` requires `Connection: close`,
  bounds the read at 4 MiB, validates `HTTP/` + 2xx, dechunks
  `Transfer-Encoding: chunked` (chunk extensions stripped, strict
  framing). `project` maps `Id`→`id`, first `Names` entry minus
  leading `/`→`name`, `Image`/`State`/`Status`→same-lower,
  `Created`→`created` (i64, 0 fallback), `Ports` passed through,
  `engine` hardcoded `"docker"`. `parse_json_array` re-exports the
  shared array parser. Read/write timeout 2s.
- Outputs: array of row objects; item key `name` (not the hex id);
  history tracks `cpu_percent`.
- API: `NAME`, `register`, `ContainersPlugin`, `DOCKER_SOCK`,
  `READ_TIMEOUT`, `project`, `docker_request`, `collect`,
  `parse_json_array`.

## gpu_format — GPU id assignment and row rendering

Role: pure presentation helpers for the (OWNED) gpu plugin: stable
per-vendor ids and upstream-aliased row objects.

- Inputs: `GpuInfo` structs from `super::gpu` (OWNED).
- Computation: `assign_gpu_ids` numbers cards per lowercased vendor
  in `(vendor, pci)` order — PCI-ascending, matching NVML index
  order; non-alphanumeric vendors fall back to prefix `gpu`; called
  before sorting so ids are boot-stable. `sort_gpus` orders
  `kind=="internal"` first, then by name. `mem_pct` is
  used/total*100 only when total is known and positive. `gpu_to_value`
  emits `key`/`gpu_id`/`pci`/`vendor`/`name`/`kind`, upstream aliases
  `proc`/`mem`/`temperature`, native `util_pct`/`freq_mhz`/
  `mem_used_mb`/`mem_total_mb`/`temp_c`, `clients` (`pid`/`name`/
  `service`/`mem_mb`/`transcoding`), `transcoding`/`transcoding_by`.
- Outputs: row `Value::Object`s; no plugin struct here.
- API: `mem_pct`, `sort_gpus`, `assign_gpu_ids`, `gpu_to_value`.

## ip — address reporting

Role: report the default-route interface's `address`/`mask`/
`gateway`/`mac` plus cached `public_ip`. Non-Linux → empty strings.

- Inputs: OWNED `route`/`attribute`/`public_ip` submodules;
  `/sys/class/net/<iface>/address` for the MAC. `register` starts
  the process-wide public-IP daemon (never `new()`, so tests spawn
  no threads); `new()` only shares the cell.
- Computation: all fields describe ONE interface — the default-route
  holder — so multi-homed hosts can't mix interfaces. No default
  route → address falls back to first non-loopback FIB address,
  mask/mac empty. `primary_interface` returns the first non-`lo`
  sysfs entry or `""`.
- Outputs: object with the five string keys; no item key.
- API: `NAME`, `register`, `IpPlugin`, `private_ip_from_fib_trie`,
  `mac_address`, `primary_interface`, plus re-exports `attribute_ips`,
  `configure_public`, `extract_ip`, `fetch_public_ip`,
  `resolve_api_url`, `PublicCfg`, `PUBLIC_API_ENV`,
  `address_for_iface`, `default_gateway`, `default_iface`,
  `hex_to_ipv4`, `ipv4_to_u32`, `local_ips_from_fib_trie`,
  `mask_from_route`, `parse_fib_trie`, `parse_route_line`, `routes`,
  `RouteRow`.

## processlist — per-process table

Role: sample every visible pid each tick; publish pid/name/cmdline/
username/threads/cpu%/mem%/`memory_info`/`cpu_times`/`io_counters`/
disk rates/`cpu_num`. Linux-only; unreadable pids skipped.

- Inputs: `/proc` numeric entries: `stat` (comm via first-`(` to
  last-`)`, state, utime/stime fields 14-15, nice 19, threads 20,
  cpu 39, blkio 42 — last two optional), `status` (State/Uid/Gid;
  gids default to uid triple), `statm` (7 page counters × page
  size), `io` (`read_bytes`/`write_bytes`/`syscr`→read_count/
  `syscw`→write_count, zeros when missing), `cmdline` (NUL-split argv
  list, lossy UTF-8), `/proc/stat` aggregate cpu line (saturating
  sum), meminfo total, `/etc/passwd` uid→name (first wins).
- Computation: first sight of a pid reports 0.0 cpu% and I/O rates;
  cpu% = process-tick delta / total-tick delta × 100 (0 when the
  total didn't advance); I/O rates over per-pid wall elapsed.
  `prev`/`seen` pruned to live pids; output sorted by pid.
  `sample_to_value` rounds cpu/mem % to 2dp, maps state chars
  (R/S/D/T-t/Z-X-x/I/else → running/sleeping/disk-sleep/stopped/
  zombie/idle/unknown), clamps `time_since_update` ≥ 0.
  `divide_cpu_percent` divides rendered cpu% by core count (irix).
  `update()` applies the display filter (`set_process_filter`) and
  irix division; `update_views` clears views; key `pid`.
- Outputs: array of row objects per the key list above.
- API: `NAME`, `register`, `page_size`, `ProcessListPlugin`,
  `apply_process_filter`, `sample_all`, re-exports `build_user_map`,
  `parse_io`, `parse_io_text`, `parse_stat_fields`, `parse_statm`,
  `parse_statm_text`, `parse_status_file`, `parse_status_text`,
  `read_cmdline`, `read_total_cpu`, `divide_cpu_percent`,
  `sample_to_value`, `status_name`, `ProcSample`, `StatFields`.

## smart — disk health via smartctl

Role: per-device SMART attributes (ATA table / NVMe log page)
via argv-only `smartctl` spawns. Any failure → empty list.

- Inputs: `smartctl --scan` then `smartctl -a -d <type> <device>`
  per row; parsing lives in OWNED `parse.rs`.
- Computation: sweeps cached 60s (`cache_fresh` boundary is
  strict `< TTL`); `reset()` clears cache. `device_to_value` keys
  rows by `DeviceName` = `"<device> <model>"` (bare device when the
  model is empty) plus `model`/`serial`/`protocol`, `attributes`
  (`num`/`name`/`value`/`worst`/`threshold`/`type`/`raw`), `nvme`
  (`name`/`value` pairs). Non-Linux → empty.
- Outputs: array of device objects; item key `DeviceName`.
- API: `NAME`, `register`, `SmartPlugin`, `cache_fresh`, `collect`,
  `device_to_value`, re-exports `parse_attr_row`,
  `parse_device_output`, `parse_scan`, `SmartAttr`, `SmartDevice`.

## vms — libvirt and Multipass guests

Role: list VMs from `virsh` (`list --all` + `domstats --nowait`,
cpu% as a cpu.time-ns rate) and `multipass` (`list --format csv`,
no cpu fields). Any failure → empty list.

- Inputs: `virsh` in /usr/bin:/bin:/usr/sbin:/sbin; `multipass` in
  /snap/bin:/usr/bin:/bin:/usr/local/bin; parsers in OWNED
  `parse.rs`. Engine versions resolved once via `OnceLock`
  (`virsh --version` trimmed; first `multipass version` line minus
  the `multipass` prefix).
- Computation: cpu% = ns delta / 1e9 / wall-seconds × 100, only when
  a previous sample exists and wall time advanced; `prev` pruned to
  live domains. Memory = `balloon.current/maximum` KiB × 1024;
  vCPUs from `vcpu.current`. Multipass ipv4 `--`/empty → omitted,
  else `"<ip> (<release>)"`. Sweeps share smart's 60s `cache_fresh`
  window; `reset()` clears cache. `row_to_value` renders
  `name`/`status`/`engine`/`engine_version` plus present-only
  `cpu_count`/`cpu_time` (2dp)/`memory_usage`/`memory_total`/`ipv4`.
  Non-Linux → empty.
- Outputs: array of VM objects; item key `name`; history tracks
  `memory_usage`.
- API: `NAME`, `register`, `VmsPlugin`, `collect_virsh`,
  `collect_multipass`, `row_to_value`, re-exports
  `parse_domstats`, `parse_multipass_csv`, `parse_virsh_list`,
  `VmRow`.

## Test rewrites and oracle

`src/qa/unit/plugins_processlist.rs` is rewritten with fresh
fixtures (stat lines with parens-in-comm, status/statm/io vectors,
a rounding/state-map matrix) plus the live two-tick walk
(first-tick zeros, second-tick non-negative, prune). Existing
per-plugin suites for amps/cloud/containers/gpu/ip/smart/vms
(OWNED, fixture-based) are the independent oracle for this batch:
they must pass unmodified against the rewritten modules.
