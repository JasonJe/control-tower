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
}

/// Login nonces are valid for five minutes.
pub const NONCE_TTL_SECS: u64 = 300;
