//! Session management for API authentication

use crate::ServiceState;
use crate::Session;

/// Session lifetime (24 hours).
pub const SESSION_TTL_SECS: u64 = 86_400;
/// Maximum number of concurrent sessions (multiple tabs/devices are fine, but
/// logins must not be able to grow the token map without bound).
const MAX_SESSIONS: usize = 16;
/// Cap the public nonce map as well; `/api/auth/nonce` is unauthenticated, so
/// without a limit a client could grow it until the TTL sweep catches up.
const MAX_NONCES: usize = 1024;

/// Failed logins tolerated per client IP inside [`LOGIN_WINDOW_SECS`].
const LOGIN_MAX_FAILURES: usize = 5;
/// Sliding window for failed login attempts.
const LOGIN_WINDOW_SECS: u64 = 60;
/// Cap the IP map. `/api/auth/login` is unauthenticated, so an attacker must
/// not be able to grow it without bound by rotating source addresses.
const LOGIN_MAX_TRACKED_IPS: usize = 1024;

impl ServiceState {
    /// Generate a new session token and store it
    pub fn create_session(&self) -> String {
        let token: String = (0..64)
            .map(|_| {
                let b = rand::random::<u8>();
                format!("{:02x}", b)
            })
            .collect();

        let now = chrono::Utc::now().timestamp() as u64;
        let session = Session {
            token: token.clone(),
            created_at: now,
        };

        let mut sessions = self.sessions.write();
        // Drop expired tokens so the map cannot grow forever.
        sessions.retain(|_, s| now.saturating_sub(s.created_at) < SESSION_TTL_SECS);
        // Evict the oldest session when the cap is reached.
        if sessions.len() >= MAX_SESSIONS {
            let oldest = sessions
                .iter()
                .min_by_key(|(_, s)| s.created_at)
                .map(|(token, _)| token.clone());
            if let Some(oldest) = oldest {
                sessions.remove(&oldest);
            }
        }
        sessions.insert(token.clone(), session);

        tracing::info!("Created new session");
        token
    }

    /// Verify a session token. Expired tokens are dropped in passing.
    pub fn verify_session(&self, token: &str) -> bool {
        let mut sessions = self.sessions.write();
        if let Some(session) = sessions.get(token) {
            // Check if session is not expired (24 hours)
            let now = chrono::Utc::now().timestamp() as u64;
            if now.saturating_sub(session.created_at) < SESSION_TTL_SECS {
                return true;
            } else {
                tracing::debug!("Session expired");
                sessions.remove(token);
            }
        }
        false
    }

    /// Remove a session (logout)
    pub fn remove_session(&self, token: &str) {
        let mut sessions = self.sessions.write();
        sessions.remove(token);
        tracing::info!("Session removed");
    }

    /// Register a freshly issued login challenge nonce.
    ///
    /// Nonces are single-use and expire after [`NONCE_TTL_SECS`]. Without
    /// server-side storage (the previous behaviour) the client picked the nonce
    /// itself, so a captured `hash` + `nonce` pair could be replayed forever to
    /// mint new session tokens.
    pub fn issue_nonce(&self, nonce: String) {
        let now = chrono::Utc::now().timestamp() as u64;
        let mut nonces = self.auth_nonces.write();
        nonces.retain(|_, issued_at| now.saturating_sub(*issued_at) < NONCE_TTL_SECS);
        if nonces.len() >= MAX_NONCES {
            if let Some(oldest) = nonces
                .iter()
                .min_by_key(|(_, issued_at)| **issued_at)
                .map(|(nonce, _)| nonce.clone())
            {
                nonces.remove(&oldest);
            }
        }
        nonces.insert(nonce, now);
    }

    /// Consume a previously issued nonce. Returns `false` when the nonce was
    /// never issued, has expired, or has already been used.
    pub fn consume_nonce(&self, nonce: &str) -> bool {
        let now = chrono::Utc::now().timestamp() as u64;
        let mut nonces = self.auth_nonces.write();
        match nonces.remove(nonce) {
            Some(issued_at) => now.saturating_sub(issued_at) < NONCE_TTL_SECS,
            None => false,
        }
    }

    /// Seconds the client must wait before another login attempt, if any.
    ///
    /// Callers pass `req.peer_addr()`, i.e. the transport-level address. Proxy
    /// headers (`X-Forwarded-For`, ...) are client controlled and must not be
    /// trusted here, otherwise the throttle is trivial to bypass.
    pub fn login_retry_after(&self, ip: &str) -> Option<u64> {
        let now = chrono::Utc::now().timestamp() as u64;
        let mut failures = self.login_failures.write();
        let attempts = failures.get_mut(ip)?;
        attempts.retain(|at| now.saturating_sub(*at) < LOGIN_WINDOW_SECS);
        retry_after(attempts, now)
    }

    /// Record a failed login for `ip`, returning the new retry-after when the
    /// failure limit has just been reached.
    pub fn record_login_failure(&self, ip: &str) -> Option<u64> {
        let now = chrono::Utc::now().timestamp() as u64;
        let mut failures = self.login_failures.write();

        // Drop expired entries first: this both keeps the quota accurate and
        // bounds the map without a separate sweep task.
        failures.retain(|_, attempts| {
            attempts.retain(|at| now.saturating_sub(*at) < LOGIN_WINDOW_SECS);
            !attempts.is_empty()
        });

        if failures.len() >= LOGIN_MAX_TRACKED_IPS && !failures.contains_key(ip) {
            // Evict the IP that has been quiet the longest.
            if let Some(oldest) = failures
                .iter()
                .min_by_key(|(_, attempts)| attempts.last().copied().unwrap_or(0))
                .map(|(ip, _)| ip.clone())
            {
                failures.remove(&oldest);
            }
        }

        let attempts = failures.entry(ip.to_string()).or_default();
        attempts.push(now);
        retry_after(attempts, now)
    }

    /// Forget the failure history of `ip` (successful login).
    pub fn clear_login_failures(&self, ip: &str) {
        self.login_failures.write().remove(ip);
    }
}

/// `Some(seconds)` once `attempts` reaches the failure limit.
fn retry_after(attempts: &[u64], now: u64) -> Option<u64> {
    if attempts.len() < LOGIN_MAX_FAILURES {
        return None;
    }
    let oldest = *attempts.first()?;
    Some(LOGIN_WINDOW_SECS.saturating_sub(now.saturating_sub(oldest)).max(1))
}

/// Login nonces are valid for five minutes.
pub const NONCE_TTL_SECS: u64 = 300;

#[cfg(test)]
mod tests {
    use super::{retry_after, LOGIN_MAX_FAILURES, LOGIN_WINDOW_SECS};

    #[test]
    fn below_the_limit_is_not_throttled() {
        let now = 1_000;
        assert_eq!(retry_after(&[], now), None);
        assert_eq!(retry_after(&[now; LOGIN_MAX_FAILURES - 1], now), None);
    }

    #[test]
    fn failure_limit_locks_until_the_window_slides() {
        let now = 1_000;
        let attempts = vec![now; LOGIN_MAX_FAILURES];
        assert_eq!(retry_after(&attempts, now), Some(LOGIN_WINDOW_SECS));
        assert_eq!(
            retry_after(&attempts, now + LOGIN_WINDOW_SECS - 1),
            Some(1)
        );
    }
}
