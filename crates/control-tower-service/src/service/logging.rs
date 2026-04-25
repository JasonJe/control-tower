//! Log ring buffer operations.

use std::collections::VecDeque;
use parking_lot::RwLock;

use crate::ServiceState;

/// Max log lines to keep in the ring buffer
const LOG_LINES: usize = 100;

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
            api_port: RwLock::new(9090),
            auto_test: RwLock::new(crate::AutoTestState::default()),
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
