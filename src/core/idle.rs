//! Idle-aware refresh cadence for the web server.
//!
//! The dashboard polls every couple of seconds while it is open, but most
//! of the time on a home server nobody is looking at all. Ticking every
//! plugin on the fast cadence while unwatched is pure waste heat, so the
//! background loop asks [`should_tick`] before each tick:
//!
//! - A request arrived in the last [`IDLE_AFTER_SECS`]? Tick fast.
//! - Nobody around? One heartbeat tick per [`IDLE_TICK_SECS`], just enough
//!   that history and alerts stay alive.
//!
//! The first request after a quiet spell gets data up to 30 seconds old;
//! the loop notices the traffic and is back to full speed within one
//! refresh interval, so the dashboard self-heals on its next poll.

use std::sync::Arc;

use super::stats::GlancesStats;

/// No requests for this long means nobody is watching.
pub const IDLE_AFTER_SECS: u64 = 30;
/// Heartbeat cadence while idle.
pub const IDLE_TICK_SECS: u64 = 30;

/// Wall-clock seconds. A broken (pre-epoch) clock reads as 0 rather
/// than panicking; callers use saturating math so time jumps are safe.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Pure tick decision: `now`, `last_served` and `last_tick` are unix
/// seconds. Second granularity (sub-second refreshes clamp to 1s).
pub fn should_tick(now: u64, last_served: u64, last_tick: u64, refresh_secs: f32) -> bool {
    let since_tick = now.saturating_sub(last_tick);
    if now.saturating_sub(last_served) <= IDLE_AFTER_SECS {
        since_tick >= refresh_secs.max(1.0) as u64
    } else {
        since_tick >= IDLE_TICK_SECS
    }
}

/// Background refresh loop for driver-less modes (web server): update
/// plugins on the refresh cadence while watched, heartbeat while idle.
pub fn spawn_refresh_loop(stats: Arc<GlancesStats>, refresh_secs: f32) {
    if !(refresh_secs.is_finite() && refresh_secs > 0.0) {
        return;
    }
    std::thread::spawn(move || {
        // Pretend the last tick was a full heartbeat ago so the
        // first iteration ticks immediately (fresh startup data).
        let mut last_tick = unix_now().saturating_sub(IDLE_TICK_SECS);
        loop {
            let now = unix_now();
            if should_tick(now, stats.last_served_secs(), last_tick, refresh_secs) {
                if let Err(e) = stats.update() {
                    super::logger::warning(&format!("refresh: stats.update() failed: {}", e));
                }
                last_tick = unix_now();
            }
            std::thread::sleep(std::time::Duration::from_secs_f32(refresh_secs));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_ticks_when_refresh_due() {
        assert!(should_tick(100, 99, 98, 2.0));
    }

    #[test]
    fn active_skips_early_wakeup() {
        assert!(!should_tick(100, 100, 100, 2.0));
    }

    #[test]
    fn boundary_still_counts_as_watched() {
        assert!(should_tick(100, 100 - IDLE_AFTER_SECS, 90, 2.0));
        assert!(!should_tick(100, 100 - IDLE_AFTER_SECS - 1, 90, 2.0));
    }

    #[test]
    fn idle_heartbeat_fires_every_thirty_seconds() {
        assert!(!should_tick(100, 0, 90, 2.0));
        assert!(should_tick(100, 0, 70, 2.0));
    }

    #[test]
    fn backward_clock_is_safe() {
        assert!(!should_tick(50, 100, 100, 2.0));
    }

    #[test]
    fn sub_second_refresh_clamps_to_one_second() {
        assert!(should_tick(100, 100, 99, 0.5));
        assert!(!should_tick(100, 100, 100, 0.5));
    }

    #[test]
    fn mark_served_roundtrip() {
        let stats = GlancesStats::new(2.0);
        stats.mark_served();
        let stamped = stats.last_served_secs();
        assert!(stamped <= unix_now());
        assert!(unix_now() - stamped < 5);
    }
}
