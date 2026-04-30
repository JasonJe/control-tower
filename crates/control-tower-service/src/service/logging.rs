//! Log ring buffer operations.

use std::collections::{HashMap, VecDeque};
use parking_lot::RwLock;

use crate::ServiceState;
use crate::settings::consts::DEFAULT_MIHOMO_API_PORT;

/// Max log lines to keep in the ring buffer
const LOG_LINES: usize = 100;

/// Connection metadata for tracking closed connections
#[derive(Debug, Clone)]
pub struct ConnectionMetadata {
    pub id: String,
    pub source_ip: String,
    pub destination: String,
    pub chains: Vec<String>,
    pub upload: u64,
    pub download: u64,
}

impl ServiceState {
    /// Create a new ServiceState instance
    pub fn new() -> Self {
        Self {
            manager: RwLock::new(control_tower_service_core::MihomoManager::new()),
            start_time: RwLock::new(None),
            log_buffer: RwLock::new(VecDeque::with_capacity(LOG_LINES)),
            cron_jobs: RwLock::new(Vec::new()),
            last_config_path: RwLock::new(None),
            api_host: RwLock::new("127.0.0.1".to_string()),
            api_port: RwLock::new(DEFAULT_MIHOMO_API_PORT),
            auto_test: RwLock::new(crate::AutoTestState::default()),
            // Track active connections for history recording
            active_connections: RwLock::new(HashMap::new()),
        }
    }

    /// Append a log line
    pub fn append_log(&self, line: impl Into<String>) {
        let mut buffer = self.log_buffer.write();
        if buffer.len() >= LOG_LINES {
            buffer.pop_front();
        }
        buffer.push_back(line.into());
    }

    /// Get log lines
    pub fn get_logs(&self, lines: Option<usize>) -> Vec<String> {
        let buffer = self.log_buffer.read();
        let lines = lines.unwrap_or(buffer.len()).min(buffer.len());
        buffer.iter().rev().take(lines).rev().cloned().collect()
    }
}
