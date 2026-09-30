//! Pausing a sandbox nobody is using: boxd's auto-suspend, done the way this
//! node already pauses -- to its snapshot store, resumable on any node, and
//! woken by the next request through the proxy when `autoResume` is on.
//!
//! Set per sandbox with `idleTimeout` (seconds) on create, or for every
//! sandbox that does not set it with `--idle-pause-after`. Not in E2B's API,
//! whose `autoPause` only acts at the timeout; an E2B SDK passes the field
//! through untouched.
//!
//! # What "idle" means
//!
//! Two things, both for the whole window:
//!
//! - **No use.** No request through the proxy -- envd, and the sandbox's own
//!   ports -- in flight or begun, and no `/exec` running.
//! - **A quiet guest.** Every CPU sample the node took of it in the window
//!   reported under [`IDLE_CPU_PCT`].
//!
//! The second is what traffic alone would get wrong. A long build, a
//! training run, a crawler: work that nobody is watching makes no requests,
//! and freezing it halfway because no one looked at it for a minute is the
//! wrong answer. A guest with no sample in the window is not idle either:
//! not knowing is not quiet.

use std::sync::Arc;

use crate::{pause_sandbox, telemetry, AppState};

/// A guest using less CPU than this, in every sample of the window, is quiet.
pub(crate) const IDLE_CPU_PCT: f64 = 5.0;
/// The shortest window: long enough for several of the node's CPU samples,
/// so one lucky quiet sample does not decide it.
pub(crate) const MIN_WINDOW_SECS: u64 = 30;

/// An idle window is 0 (off) or at least [`MIN_WINDOW_SECS`].
pub(crate) fn check_window(secs: u64) -> Result<(), String> {
    if secs != 0 && secs < MIN_WINDOW_SECS {
        return Err(format!(
            "{secs}s is too short to tell idle from between requests; 0 or at least \
             {MIN_WINDOW_SECS}"
        ));
    }
    Ok(())
}

/// Whether a guest whose busiest sample in the window was `busiest` is quiet.
pub(crate) fn quiet(busiest: Option<f64>) -> bool {
    busiest.is_some_and(|pct| pct < IDLE_CPU_PCT)
}

/// Pause every running sandbox that has been idle for its window, as of `now`.
pub(crate) async fn pause_idle(state: &Arc<AppState>, now: u64) {
    let candidates: Vec<(String, u64)> = state
        .sandboxes
        .lock()
        .iter()
        .filter(|(_, live)| live.lifecycle.idle_pause_secs > 0 && !live.activity.busy())
        .filter_map(|(id, live)| {
            let since = now.saturating_sub(live.lifecycle.idle_pause_secs * 1000);
            (live.activity.last_active_ms() <= since).then(|| (id.clone(), since))
        })
        .collect();
    for (id, since) in candidates {
        if !quiet(telemetry::busiest_since(state, &id, since)) {
            continue;
        }
        // As an eviction does: re-checked under the sandbox's lock, so a
        // request that arrived since it was chosen keeps it running.
        match pause_sandbox(state, &id, true).await {
            Ok(()) => tracing::info!("sandbox {id} was idle and paused"),
            Err((_, e)) => tracing::warn!("pausing idle sandbox {id}: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_guest_is_quiet_only_when_every_sample_was() {
        assert!(quiet(Some(0.0)));
        assert!(quiet(Some(4.9)));
        assert!(!quiet(Some(5.0)));
        assert!(!quiet(Some(97.0)));
        // No sample in the window: not knowing is not quiet.
        assert!(!quiet(None));
    }

    #[test]
    fn windows_are_off_or_long_enough_to_mean_something() {
        assert!(check_window(0).is_ok());
        assert!(check_window(MIN_WINDOW_SECS).is_ok());
        assert!(check_window(3600).is_ok());
        assert!(check_window(1).is_err());
        assert!(check_window(MIN_WINDOW_SECS - 1).is_err());
    }
}
