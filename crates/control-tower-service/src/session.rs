//! Session management for API authentication

use crate::ServiceState;
use crate::Session;

impl ServiceState {
    /// Generate a new session token and store it
    pub fn create_session(&self) -> String {
        let token: String = (0..64)
            .map(|_| {
                let b = rand::random::<u8>();
                format!("{:02x}", b)
            })
            .collect();

        let session = Session {
            token: token.clone(),
            created_at: chrono::Utc::now().timestamp() as u64,
        };

        let mut sessions = self.sessions.write();
        sessions.insert(token.clone(), session);

        tracing::info!("Created new session");
        token
    }

    /// Verify a session token and remove it if valid
    pub fn verify_session(&self, token: &str) -> bool {
        let mut sessions = self.sessions.write();
        if let Some(session) = sessions.get(token) {
            // Check if session is not expired (24 hours)
            let now = chrono::Utc::now().timestamp() as u64;
            if now - session.created_at < 86400 {
                return true;
            } else {
                tracing::debug!("Session expired: {}", token);
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
}
