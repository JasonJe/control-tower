//! Kernel watchdog: keep Mihomo alive while the user wants it running.
//!
//! A crashed or OOM-killed Mihomo used to stay dead until the user noticed and
//! restarted the service, leaving the machine without a proxy. The watchdog
//! distinguishes "crashed" from "user stopped it" via [`ServiceState`]'s
//! `desired_running` flag and restarts with exponential backoff so a broken
//! config cannot turn into a hot restart loop.

use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ServiceState;

/// First retry delay after a crash.
const BASE_BACKOFF_SECS: u64 = 5;
/// Upper bound for the restart backoff.
const MAX_BACKOFF_SECS: u64 = 300;

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl ServiceState {
    /// Record that the user wants Mihomo running (set after a successful start).
    pub(crate) fn set_desired_running(&self, desired: bool) {
        self.desired_running.store(desired, Ordering::SeqCst);
        if desired {
            self.watchdog_failures.store(0, Ordering::SeqCst);
            self.watchdog_next_at.store(0, Ordering::SeqCst);
        }
    }

    /// Whether the watchdog should keep the kernel alive.
    pub(crate) fn desired_running(&self) -> bool {
        self.desired_running.load(Ordering::SeqCst)
    }

    /// Called periodically from the server loop.
    ///
    /// Restarts Mihomo when it disappeared unexpectedly. Returns without doing
    /// anything when the user stopped the service, when Mihomo is still
    /// running, or while a backoff is pending.
    pub(crate) fn watchdog_tick(&self) {
        if !self.desired_running() {
            return;
        }
        if self.is_running() {
            // A healthy kernel resets the backoff so a later crash can retry
            // immediately instead of inheriting an old delay.
            if self.watchdog_failures.load(Ordering::SeqCst) != 0 {
                self.watchdog_failures.store(0, Ordering::SeqCst);
                self.watchdog_next_at.store(0, Ordering::SeqCst);
            }
            return;
        }

        let now = unix_now();
        if now < self.watchdog_next_at.load(Ordering::SeqCst) {
            return;
        }

        let Some(config_path) = self.last_config_path.read().clone() else {
            return;
        };

        let failures = self.watchdog_failures.load(Ordering::SeqCst);
        tracing::warn!(
            "Mihomo is not running (attempt {}); restarting from {}",
            failures + 1,
            config_path.display()
        );

        match self.start(&config_path) {
            Ok(()) => {
                tracing::info!("Watchdog restarted Mihomo successfully");
                self.watchdog_failures.store(0, Ordering::SeqCst);
                self.watchdog_next_at.store(0, Ordering::SeqCst);
            }
            Err(e) => {
                let failures = failures.saturating_add(1);
                self.watchdog_failures.store(failures, Ordering::SeqCst);
                // 5s, 10s, 20s, ... capped. The shift is bounded so a long
                // outage cannot overflow.
                let backoff = BASE_BACKOFF_SECS
                    .saturating_mul(1u64 << failures.min(6))
                    .min(MAX_BACKOFF_SECS);
                self.watchdog_next_at.store(now + backoff, Ordering::SeqCst);
                tracing::error!(
                    "Watchdog failed to restart Mihomo ({}), retrying in {}s",
                    e,
                    backoff
                );
            }
        }
    }
}
