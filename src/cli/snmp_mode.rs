//! SNMP client mode (`-c HOST --snmp-force`, upstream `client.py`
//! `_login_snmp` parity): probe sysName/sysDescr, detect the OS
//! family, then drive the UI (or a one-shot snapshot loop) with
//! per-tick SNMP polling. A dead agent exits non-zero with the same
//! "Connection to SNMP server failed" message as upstream.

use crate::cli::args::{Args, SnmpVersion};
use crate::core::config::Config;
use crate::core::logger;
use crate::core::snmp::{SnmpClient, SnmpCtx};
use crate::core::stats::GlancesStats;

fn version_str(v: SnmpVersion) -> &'static str {
    match v {
        SnmpVersion::V1 => "1",
        SnmpVersion::V2c => "2c",
        SnmpVersion::V3 => "3",
    }
}

/// Run SNMP client mode. Returns false when the agent is unreachable
/// or misconfigured (caller maps to a non-zero exit).
pub fn run_snmp_client(
    host: &str,
    refresh_secs: f32,
    args: &Args,
    config: &Config,
) -> bool {
    let community = args.snmp_community.clone().unwrap_or_else(|| "public".into());
    let client = match SnmpClient::new(host, args.snmp_port, version_str(args.snmp_version), &community) {
        Ok(c) => c,
        Err(e) => {
            logger::error(&format!("SNMP client: {}", e));
            return false;
        }
    };
    let ctx = match SnmpCtx::probe(&client) {
        Ok(c) => c,
        Err(e) => {
            logger::error(&format!("Connection to SNMP server failed ({}:{})", host, args.snmp_port));
            logger::debug(&format!("SNMP probe: {}", e));
            return false;
        }
    };
    logger::info(&format!(
        "SNMP system detected: {}",
        ctx.system_name.as_deref().unwrap_or("unknown")
    ));
    let stats = GlancesStats::new(refresh_secs);
    super::modes::register(&stats, args, config);
    if std::io::IsTerminal::is_terminal(&std::io::stdout()) && !args.quiet {
        match crate::outputs::tui::run_snmp(&stats, args, &ctx) {
            Ok(()) => true,
            Err(e) => {
                logger::error(&format!("snmp tui: {}", e));
                false
            }
        }
    } else {
        snmp_snapshot_loop(&stats, args, &ctx, refresh_secs)
    }
}

/// Non-TTY client loop: one compact line per tick (mirrors the
/// standalone monitor shape for pipes and `--stop-after`).
fn snmp_snapshot_loop(stats: &GlancesStats, args: &Args, ctx: &SnmpCtx, refresh_secs: f32) -> bool {
    use std::io::IsTerminal;
    let one_shot = !std::io::stdout().is_terminal() && args.stop_after.is_none();
    let mut tick: u32 = 0;
    loop {
        if let Err(e) = stats.update_snmp(ctx) {
            logger::warning(&format!("snmp tick failed: {}", e));
        }
        if !args.quiet {
            let snap = stats.snapshot();
            let get = |plugin: &str, key: &str| -> f64 {
                snap.as_object()
                    .and_then(|o| o.get(plugin))
                    .and_then(|p| p.as_object())
                    .and_then(|o| o.get(key))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
            };
            println!(
                "snmp cpu: {:.1}%  mem: {:.1}%  load: {:.2}",
                get("cpu", "total"),
                get("mem", "percent"),
                get("load", "min1"),
            );
        }
        tick = tick.saturating_add(1);
        if one_shot {
            break;
        }
        if let Some(max) = args.stop_after {
            if tick >= max {
                break;
            }
        }
        if refresh_secs > 0.0 {
            std::thread::sleep(std::time::Duration::from_secs_f32(refresh_secs));
        }
    }
    true
}
