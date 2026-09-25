//! SNMP client mode (`-c HOST --snmp-force`): probe the agent,
//! detect its OS family, then print per-tick summaries. Dead agents
//! exit non-zero with the connection message.

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

/// Run the mode. False means the agent was unreachable or
/// misconfigured (the caller exits non-zero).
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
            logger::error(&format!("SNMP client: {e}"));
            return false;
        }
    };
    let ctx = match SnmpCtx::probe(&client) {
        Ok(c) => c,
        Err(e) => {
            logger::error(&format!("Connection to SNMP server failed ({}:{})", host, args.snmp_port));
            logger::debug(&format!("SNMP probe: {e}"));
            return false;
        }
    };
    logger::info(&format!("SNMP system detected: {}", ctx.system_name.as_deref().unwrap_or("unknown")));
    let stats = GlancesStats::new(refresh_secs);
    super::modes::register(&stats, args, config);
    snapshot_loop(&stats, args, &ctx, refresh_secs)
}

/// One compact line per tick. Pipes print once (unless `--stop-after`
/// asks for more); terminals loop until Ctrl-C.
fn snapshot_loop(stats: &GlancesStats, args: &Args, ctx: &SnmpCtx, refresh_secs: f32) -> bool {
    use std::io::IsTerminal;
    let one_shot = !std::io::stdout().is_terminal() && args.stop_after.is_none();
    let mut tick: u32 = 0;
    loop {
        if let Err(e) = stats.update_snmp(ctx) {
            logger::warning(&format!("snmp tick failed: {e}"));
        }
        if !args.quiet {
            let snap = stats.snapshot();
            let num = |plugin: &str, key: &str| -> f64 {
                snap.as_object()
                    .and_then(|o| o.get(plugin))
                    .and_then(|p| p.as_object())
                    .and_then(|o| o.get(key))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
            };
            println!(
                "snmp cpu: {:.1}%  mem: {:.1}%  load: {:.2}",
                num("cpu", "total"),
                num("mem", "percent"),
                num("load", "min1"),
            );
        }
        tick = tick.saturating_add(1);
        if one_shot || args.stop_after.is_some_and(|max| tick >= max) {
            break;
        }
        if refresh_secs > 0.0 {
            std::thread::sleep(std::time::Duration::from_secs_f32(refresh_secs));
        }
    }
    true
}
