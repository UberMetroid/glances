# Ownership ledger

Zero-trust rewrite tracker. Every `.rs` file under `src/` appears exactly once.
- TAINTED: ported from upstream; must pass the 7-step air-gap (observe, specify, quarantine, implement, prove, review, flip) before it counts as ours.
- OWNED: original in-project expression, or a TAINTED file that completed the air-gap.

The `qa::lint::ownership` test enforces accuracy: full coverage, no duplicates, valid statuses. Completion is 65/65 flipped, read by a human — the lint never goes red.

## Tainted (20)

| File | Status | Spec | Commit |
| ---- | ------ | ---- | ------ |
 | src/cli/args.rs | OWNED | docs/spec/p2b2-cli.md | |
 | src/cli/flags.rs | OWNED | docs/spec/p2b2-cli.md | |
 | src/cli/modes.rs | OWNED | docs/spec/p2b2-cli.md | |
 | src/cli/snmp_mode.rs | OWNED | docs/spec/p2b2-cli.md | |
 | src/core/actions/mod.rs | OWNED | docs/spec/p1b2-services.md | |
 | src/core/actions/run.rs | OWNED | docs/spec/p1b2-services.md | |
 | src/core/alerts.rs | OWNED | docs/spec/p1b1-core.md | |
 | src/core/alert_views.rs | OWNED | docs/spec/p1b1-core.md | |
 | src/core/config_dir.rs | OWNED | docs/spec/p1b1-core.md | |
 | src/core/events.rs | OWNED | docs/spec/p1b1-core.md | |
 | src/core/filter/glances.rs | OWNED | docs/spec/p1b1-core.md | |
 | src/core/filter/mod.rs | OWNED | docs/spec/p1b1-core.md | |
 | src/core/history.rs | OWNED | docs/spec/p1b1-core.md | |
 | src/core/password/mod.rs | OWNED | docs/spec/p1b2-services.md | |
 | src/core/password/prompt.rs | OWNED | docs/spec/p1b2-services.md | |
 | src/core/pbkdf2.rs | OWNED | docs/spec/p1b2-services.md | |
 | src/core/plugin.rs | OWNED | docs/spec/p1b1-core.md | |
 | src/core/snmp/client.rs | OWNED | docs/spec/p1b2-services.md | |
 | src/core/snmp/mod.rs | OWNED | docs/spec/p1b2-services.md | |
 | src/core/stats.rs | OWNED | docs/spec/p1b1-core.md | |
 | src/core/threshold.rs | OWNED | docs/spec/p1b1-core.md | |
 | src/core/timer.rs | OWNED | docs/spec/p1b1-core.md | |
 | src/main.rs | OWNED | docs/spec/p2b2-cli.md | |
 | src/outputs/csv_stdout.rs | OWNED | docs/spec/p2b1-outputs.md | |
 | src/outputs/json_stdout.rs | OWNED | docs/spec/p2b1-outputs.md | |
 | src/outputs/web/auth.rs | OWNED | docs/spec/p2b1-outputs.md | |
 | src/outputs/web/meta.rs | OWNED | docs/spec/p2b1-outputs.md | |
 | src/outputs/web/mod.rs | OWNED | docs/spec/p2b1-outputs.md | |
 | src/outputs/web/mutate.rs | OWNED | docs/spec/p2b1-outputs.md | |
 | src/outputs/web/router.rs | OWNED | docs/spec/p2b1-outputs.md | |
 | src/plugins/alert.rs | TAINTED | | |
 | src/plugins/amps.rs | TAINTED | | |
 | src/plugins/cloud.rs | TAINTED | | |
 | src/plugins/connections.rs | TAINTED | | |
 | src/plugins/containers.rs | TAINTED | | |
 | src/plugins/cpu.rs | OWNED | docs/spec/p3a-plugins.md | |
 | src/plugins/diskio.rs | TAINTED | | |
 | src/plugins/fs.rs | TAINTED | | |
 | src/plugins/gpu_format.rs | TAINTED | | |
 | src/plugins/help.rs | OWNED | docs/spec/p3a-plugins.md | |
 | src/plugins/ip/mod.rs | TAINTED | | |
 | src/plugins/irq.rs | TAINTED | | |
 | src/plugins/load.rs | OWNED | docs/spec/p3a-plugins.md | |
 | src/plugins/mem.rs | OWNED | docs/spec/p3a-plugins.md | |
 | src/plugins/mod.rs | OWNED | docs/spec/p3a-plugins.md | |
 | src/plugins/network.rs | TAINTED | | |
 | src/plugins/percpu.rs | OWNED | docs/spec/p3a-plugins.md | |
 | src/plugins/ports.rs | TAINTED | | |
 | src/plugins/processcount.rs | OWNED | docs/spec/p3a-plugins.md | |
 | src/plugins/processlist/mod.rs | TAINTED | | |
 | src/plugins/processlist/read.rs | TAINTED | | |
 | src/plugins/processlist/sample.rs | TAINTED | | |
 | src/plugins/programlist.rs | TAINTED | | |
 | src/plugins/quicklook.rs | OWNED | docs/spec/p3a-plugins.md | |
 | src/plugins/smart/mod.rs | TAINTED | | |
 | src/plugins/uptime.rs | OWNED | docs/spec/p3a-plugins.md | |
 | src/plugins/version.rs | OWNED | docs/spec/p3a-plugins.md | |
 | src/plugins/vms/mod.rs | TAINTED | | |
 | src/qa/unit/cli_flags_modes.rs | OWNED | docs/spec/p2b2-cli.md | |
 | src/qa/unit/cli_flags.rs | OWNED | docs/spec/p2b2-cli.md | |
 | src/qa/unit/core_actions_run.rs | OWNED | docs/spec/p1b2-services.md | |
 | src/qa/unit/core_filter_list.rs | OWNED | docs/spec/p1b1-core.md | |
 | src/qa/unit/core_stats.rs | OWNED | docs/spec/p1b1-core.md | |
 | src/qa/unit/plugins_fs.rs | TAINTED | | |
 | src/qa/unit/plugins_processlist.rs | TAINTED | | |

## Owned (163)

| File | Status | Spec | Commit |
| ---- | ------ | ---- | ------ |
 | src/cli/help.rs | OWNED | | |
 | src/cli/mod.rs | OWNED | | |
 | src/cli/parse.rs | OWNED | | |
 | src/cli/ping.rs | OWNED | | |
 | src/core/config.rs | OWNED | | |
 | src/core/error.rs | OWNED | | |
 | src/core/filter/parse.rs | OWNED | | |
 | src/core/hex.rs | OWNED | | |
 | src/core/idle.rs | OWNED | | |
 | src/core/logger.rs | OWNED | | |
 | src/core/mod.rs | OWNED | | |
 | src/core/password/hash.rs | OWNED | | |
 | src/core/sha256.rs | OWNED | | |
 | src/core/snmp/proto.rs | OWNED | | |
 | src/core/stats_actions.rs | OWNED | | |
 | src/core/value.rs | OWNED | | |
 | src/lib.rs | OWNED | | |
 | src/outputs/api_doc.rs | OWNED | | |
 | src/outputs/filter.rs | OWNED | | |
 | src/outputs/mcp/mod.rs | OWNED | | |
 | src/outputs/mod.rs | OWNED | | |
 | src/outputs/stdout_path.rs | OWNED | | |
 | src/outputs/web/health.rs | OWNED | | |
 | src/outputs/web/request.rs | OWNED | | |
 | src/outputs/web/response.rs | OWNED | | |
 | src/outputs/web/server.rs | OWNED | | |
 | src/outputs/web/sse.rs | OWNED | | |
 | src/outputs/web/static_fs.rs | OWNED | | |
 | src/platform/linux/mod.rs | OWNED | | |
 | src/platform/linux/proc_cpuinfo.rs | OWNED | | |
 | src/platform/linux/proc_diskstats.rs | OWNED | | |
 | src/platform/linux/proc_loadavg.rs | OWNED | | |
 | src/platform/linux/proc_meminfo.rs | OWNED | | |
 | src/platform/linux/proc_net_dev.rs | OWNED | | |
 | src/platform/linux/proc_stat.rs | OWNED | | |
 | src/platform/linux/proc_uptime.rs | OWNED | | |
 | src/platform/linux/statvfs.rs | OWNED | | |
 | src/platform/linux/sys_class_hwmon.rs | OWNED | | |
 | src/platform/linux/sys_class_net.rs | OWNED | | |
 | src/platform/linux/sysconf.rs | OWNED | | |
 | src/platform/linux/uname.rs | OWNED | | |
 | src/platform/mod.rs | OWNED | | |
 | src/plugins/folders.rs | OWNED | | |
 | src/plugins/fs_rootfs.rs | OWNED | | |
 | src/plugins/gpu_drm.rs | OWNED | | |
 | src/plugins/gpu_nvidia.rs | OWNED | | |
 | src/plugins/gpu_proc.rs | OWNED | | |
 | src/plugins/gpu.rs | OWNED | | |
 | src/plugins/gpu_sysfs.rs | OWNED | | |
 | src/plugins/ip/attribute.rs | OWNED | | |
 | src/plugins/ip/public_ip.rs | OWNED | | |
 | src/plugins/ip/route.rs | OWNED | | |
 | src/plugins/json/mod.rs | OWNED | | |
 | src/plugins/json/strparse.rs | OWNED | | |
 | src/plugins/memswap.rs | OWNED | | |
 | src/plugins/mpp.rs | OWNED | | |
 | src/plugins/now.rs | OWNED | | |
 | src/plugins/npu.rs | OWNED | | |
 | src/plugins/power.rs | OWNED | | |
 | src/plugins/pressure.rs | OWNED | | |
 | src/plugins/psutilversion.rs | OWNED | | |
 | src/plugins/raid.rs | OWNED | | |
 | src/plugins/sensors.rs | OWNED | | |
 | src/plugins/smart/parse.rs | OWNED | | |
 | src/plugins/system.rs | OWNED | | |
 | src/plugins/vms/parse.rs | OWNED | | |
 | src/plugins/wifi.rs | OWNED | | |
 | src/qa/edge/empty_inputs.rs | OWNED | | |
 | src/qa/edge/malformed_configs.rs | OWNED | | |
 | src/qa/edge/mod.rs | OWNED | | |
 | src/qa/edge/plugin_panic_isolation.rs | OWNED | | |
 | src/qa/harness/mod.rs | OWNED | | |
 | src/qa/integration/binary_runs.rs | OWNED | | |
 | src/qa/integration/cli_help.rs | OWNED | | |
 | src/qa/integration/cli_parse.rs | OWNED | | |
 | src/qa/integration/dashboard_fold_flow.rs | OWNED | | |
 | src/qa/integration/dashboard_harness.rs | OWNED | | |
 | src/qa/integration/dashboard_key_flow.rs | OWNED | | |
 | src/qa/integration/dashboard_pause_flow.rs | OWNED | | |
 | src/qa/integration/dashboard_theme_flow.rs | OWNED | | |
 | src/qa/integration/dashboard_warn_flow.rs | OWNED | | |
 | src/qa/integration/installer.rs | OWNED | | |
 | src/qa/integration/mod.rs | OWNED | | |
 | src/qa/integration/plugins_smoke.rs | OWNED | | |
 | src/qa/integration/web_api_key.rs | OWNED | | |
 | src/qa/integration/web_api_smoke.rs | OWNED | | |
 | src/qa/lint/line_cap.rs | OWNED | | |
 | src/qa/lint/mod.rs | OWNED | | |
 | src/qa/lint/no_crates.rs | OWNED | | |
 | src/qa/lint/no_shell.rs | OWNED | | |
 | src/qa/lint/ownership.rs | OWNED | | |
 | src/qa/lint/unsafe_allowlist.rs | OWNED | | |
 | src/qa/mod.rs | OWNED | | |
 | src/qa/unit/cli_parse.rs | OWNED | | |
 | src/qa/unit/core_alert.rs | OWNED | | |
 | src/qa/unit/core_config_dir.rs | OWNED | | |
 | src/qa/unit/core_config.rs | OWNED | | |
 | src/qa/unit/core_filter.rs | OWNED | | |
 | src/qa/unit/core_hex.rs | OWNED | | |
 | src/qa/unit/core_history.rs | OWNED | | |
 | src/qa/unit/core_logger.rs | OWNED | | |
 | src/qa/unit/core_password.rs | OWNED | | |
 | src/qa/unit/core_sha256.rs | OWNED | | |
 | src/qa/unit/core_threshold.rs | OWNED | | |
 | src/qa/unit/core_timer.rs | OWNED | | |
 | src/qa/unit/core_value.rs | OWNED | | |
 | src/qa/unit/mod.rs | OWNED | | |
 | src/qa/unit/outputs_api_doc.rs | OWNED | | |
 | src/qa/unit/outputs_csv.rs | OWNED | | |
 | src/qa/unit/outputs_json.rs | OWNED | | |
 | src/qa/unit/outputs_web_dashboard.rs | OWNED | | |
 | src/qa/unit/outputs_web_health.rs | OWNED | | |
 | src/qa/unit/outputs_web_meta.rs | OWNED | | |
 | src/qa/unit/outputs_web_router.rs | OWNED | | |
 | src/qa/unit/platform_linux_proc_diskstats.rs | OWNED | | |
 | src/qa/unit/platform_linux_proc_loadavg.rs | OWNED | | |
 | src/qa/unit/platform_linux_proc_meminfo.rs | OWNED | | |
 | src/qa/unit/platform_linux_proc_net_dev.rs | OWNED | | |
 | src/qa/unit/platform_linux_proc_stat.rs | OWNED | | |
 | src/qa/unit/platform_linux_proc_uptime.rs | OWNED | | |
 | src/qa/unit/platform_linux_sys_class_hwmon.rs | OWNED | | |
 | src/qa/unit/platform_linux_sys_class_net.rs | OWNED | | |
 | src/qa/unit/plugins_amps.rs | OWNED | | |
 | src/qa/unit/plugins_cloud.rs | OWNED | | |
 | src/qa/unit/plugins_connections.rs | OWNED | | |
 | src/qa/unit/plugins_containers.rs | OWNED | | |
 | src/qa/unit/plugins_cpu.rs | OWNED | | |
 | src/qa/unit/plugins_diskio.rs | OWNED | | |
 | src/qa/unit/plugins_folders.rs | OWNED | | |
 | src/qa/unit/plugins_gpu_drm.rs | OWNED | | |
 | src/qa/unit/plugins_gpu_nvidia.rs | OWNED | | |
 | src/qa/unit/plugins_gpu.rs | OWNED | | |
 | src/qa/unit/plugins_ip.rs | OWNED | | |
 | src/qa/unit/plugins_irq.rs | OWNED | | |
 | src/qa/unit/plugins_json.rs | OWNED | | |
 | src/qa/unit/plugins_mpp.rs | OWNED | | |
 | src/qa/unit/plugins_network.rs | OWNED | | |
 | src/qa/unit/plugins_npu.rs | OWNED | | |
 | src/qa/unit/plugins_percpu.rs | OWNED | | |
 | src/qa/unit/plugins_ports.rs | OWNED | | |
 | src/qa/unit/plugins_pressure.rs | OWNED | | |
 | src/qa/unit/plugins_processcount.rs | OWNED | | |
 | src/qa/unit/plugins_programlist.rs | OWNED | | |
 | src/qa/unit/plugins_raid.rs | OWNED | | |
 | src/qa/unit/plugins_sensors.rs | OWNED | | |
 | src/qa/unit/plugins_smart.rs | OWNED | | |
 | src/qa/unit/plugins_vms.rs | OWNED | | |
 | src/qa/unit/plugins_wifi.rs | OWNED | | |
 | src/qa/unit/snmp_client.rs | OWNED | | |

 | src/qa/unit/core_oracle.rs | OWNED | | |

 | src/qa/unit/outputs_oracle.rs | OWNED | | |

 | src/qa/unit/cli_oracle.rs | OWNED | | |

 | src/qa/unit/plugins_oracle.rs | OWNED | | |

## Flip log

(empty — flips append here as bullets: date, file, archived hash, spec, commit)
- 2026-09-25 P1B1 (11 core + 2 tests): KNOWN TRANSIENT stats.rs/plugin.rs reference P1B2 snmp/actions types (signature-level; logic dependency resolves in P1B2). Quarantine sha256 81d9e6424e7a108f4b8abc02816c79884e643ed4a64012c1da62dbeb05f3a2bd, spec docs/spec/p1b1-core.md
- 2026-09-25 P1B2 (7 services + 1 test): quarantine sha256 aed3ec982ead238c84114bc798aa41287e6ab50af5558c22451abef2f16fc196, spec docs/spec/p1b2-services.md. Notes: mustache renders single-pass (old recursive version could loop forever on self-referential values); prompt.rs references P2 cli/args types (transient, resolves in P2).
- 2026-09-25 P2B1 (7 outputs): quarantine sha256 5b9870a9b289ac3002919a20d3796c9dd76d1eeeabd9d3ba9a3781d41e794766, spec docs/spec/p2b1-outputs.md. Notes: base64 canonical-bit masks corrected per RFC 4648 (old code had them swapped, rejecting valid "TWE="-style inputs); outputs/* reference P2B2 cli/args fields (transient, resolves in P2B2).
- 2026-09-25 P2B2 (5 CLI + 2 tests): quarantine sha256 431f9079ced58c68679b045e88e408d2da578d23182db1ab04f09caf65e13979, spec docs/spec/p2b2-cli.md. Notes: modes.rs references P3A plugins::register_filtered/plugin_names and main.rs references P3C ip::configure_public (transients, frozen signatures, resolve in P3).
- 2026-09-25 P3A (10 simple plugins): quarantine sha256 848faf2a6cc06c8f8569aa3edcba587afeb3f24c7acfee26324f7ef0b0a91950, spec docs/spec/p3a-plugins.md. Note: processcount non-Linux zero branch removed (dead in this Linux-only tree).
