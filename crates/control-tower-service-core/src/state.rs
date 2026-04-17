//! Service state types

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Maximum number of log lines to keep in buffer
const MAX_LOG_LINES: usize = 100;

/// Circuit breaker configuration
const CIRCUIT_BREAKER_MAX_FAILURES: u32 = 3;
const CIRCUIT_BREAKER_COOLDOWN_SECS: u64 = 60;

/// Circuit breaker state for Mihomo auto-restart
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    /// Number of consecutive failures
    failure_count: u32,
    /// Whether the circuit is broken (in cooldown)
    is_broken: bool,
    /// When the circuit was broken (for cooldown calculation)
    broken_at: Option<Instant>,
    /// Cooldown duration
    cooldown: Duration,
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self {
            failure_count: 0,
            is_broken: false,
            broken_at: None,
            cooldown: Duration::from_secs(CIRCUIT_BREAKER_COOLDOWN_SECS),
        }
    }
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if the circuit allows requests
    pub fn is_allowed(&self) -> bool {
        if !self.is_broken {
            return true;
        }

        // Check if cooldown has elapsed
        if let Some(broken_at) = self.broken_at
            && broken_at.elapsed() >= self.cooldown {
                return true;
            }
        false
    }

    /// Record a failure (Mihomo crash or start failure)
    pub fn record_failure(&mut self) {
        if self.is_broken {
            // Already broken, just return
            return;
        }

        self.failure_count += 1;

        if self.failure_count >= CIRCUIT_BREAKER_MAX_FAILURES {
            self.is_broken = true;
            self.broken_at = Some(Instant::now());
            tracing::warn!(
                "Circuit breaker activated after {} failures, cooling down for {} seconds",
                self.failure_count,
                self.cooldown.as_secs()
            );
        }
    }

    /// Record a success (Mihomo started successfully)
    pub fn record_success(&mut self) {
        self.failure_count = 0;
        if self.is_broken {
            tracing::info!("Circuit breaker reset after successful start");
        }
        self.is_broken = false;
        self.broken_at = None;
    }

    /// Get remaining cooldown time in seconds
    pub fn remaining_cooldown_secs(&self) -> Option<u64> {
        if !self.is_broken {
            return None;
        }
        if let Some(broken_at) = self.broken_at {
            let elapsed = broken_at.elapsed();
            if elapsed >= self.cooldown {
                return None; // Cooldown expired
            }
            Some((self.cooldown - elapsed).as_secs())
        } else {
            None
        }
    }

    /// Check if circuit is currently broken
    pub fn is_circuit_broken(&self) -> bool {
        self.is_broken && self.remaining_cooldown_secs().is_some()
    }

    /// Get the number of consecutive failures
    pub fn failure_count(&self) -> u32 {
        self.failure_count
    }
}

/// Service running state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceStatus {
    NotRunning,
    Starting,
    Running,
    Stopping,
    /// Circuit breaker is active with remaining cooldown seconds
    CircuitBroken(u64),
    Error(String),
}

/// Service state
#[derive(Debug, Clone)]
pub struct ServiceState {
    pub status: ServiceStatus,
    pub pid: Option<u32>,
    pub config_dir: std::path::PathBuf,
    log_buffer: VecDeque<String>,
    /// Circuit breaker for Mihomo auto-restart
    pub circuit_breaker: CircuitBreaker,
}

impl Default for ServiceState {
    fn default() -> Self {
        // Use executable's directory as working directory
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));

        Self {
            status: ServiceStatus::NotRunning,
            pid: None,
            config_dir: exe_dir,
            log_buffer: VecDeque::with_capacity(MAX_LOG_LINES),
            circuit_breaker: CircuitBreaker::new(),
        }
    }
}

impl ServiceState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config_dir(mut self, dir: std::path::PathBuf) -> Self {
        self.config_dir = dir;
        self
    }

    pub fn is_running(&self) -> bool {
        matches!(self.status, ServiceStatus::Running)
    }

    pub fn set_running(&mut self, pid: u32) {
        self.status = ServiceStatus::Running;
        self.pid = Some(pid);
    }

    pub fn set_error(&mut self, msg: String) {
        self.status = ServiceStatus::Error(msg);
    }

    pub fn set_starting(&mut self) {
        self.status = ServiceStatus::Starting;
    }

    pub fn set_stopping(&mut self) {
        self.status = ServiceStatus::Stopping;
    }

    pub fn set_not_running(&mut self) {
        self.status = ServiceStatus::NotRunning;
        self.pid = None;
    }

    /// Append a log line to the buffer
    pub fn append_log(&mut self, line: impl Into<String>) {
        if self.log_buffer.len() >= MAX_LOG_LINES {
            self.log_buffer.pop_front();
        }
        self.log_buffer.push_back(line.into());
    }

    /// Get the last N log lines
    pub fn get_logs(&self, lines: Option<usize>) -> Vec<String> {
        let lines = lines.unwrap_or(self.log_buffer.len());
        let lines = lines.min(self.log_buffer.len());

        self.log_buffer.iter().rev().take(lines).rev().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_state_default() {
        let state = ServiceState::default();
        assert_eq!(state.status, ServiceStatus::NotRunning);
        assert!(state.pid.is_none());
        assert!(state.is_running() == false);
    }

    #[test]
    fn test_service_state_with_config_dir() {
        let dir = std::path::PathBuf::from("/tmp/test");
        let state = ServiceState::new().with_config_dir(dir.clone());
        assert_eq!(state.config_dir, dir);
    }

    #[test]
    fn test_service_state_set_running() {
        let mut state = ServiceState::default();
        state.set_running(12345);
        assert!(state.is_running());
        assert_eq!(state.pid, Some(12345));
    }

    #[test]
    fn test_service_state_set_error() {
        let mut state = ServiceState::default();
        state.set_error("test error".to_string());
        assert!(!state.is_running());
        match &state.status {
            ServiceStatus::Error(msg) => assert_eq!(msg, "test error"),
            _ => panic!("expected Error status"),
        }
    }

    #[test]
    fn test_service_state_lifecycle() {
        let mut state = ServiceState::default();

        // NotRunning -> Starting
        state.set_starting();
        assert_eq!(state.status, ServiceStatus::Starting);

        // Starting -> Running
        state.set_running(123);
        assert_eq!(state.status, ServiceStatus::Running);

        // Running -> Stopping
        state.set_stopping();
        assert_eq!(state.status, ServiceStatus::Stopping);

        // Stopping -> NotRunning
        state.set_not_running();
        assert_eq!(state.status, ServiceStatus::NotRunning);
    }

    #[test]
    fn test_logs_buffer_empty() {
        let state = ServiceState::default();
        let logs = state.get_logs(Some(10));
        assert!(logs.is_empty());
    }

    #[test]
    fn test_logs_buffer_append_and_retrieve() {
        let mut state = ServiceState::default();

        state.append_log("line 1");
        state.append_log("line 2");
        state.append_log("line 3");

        let logs = state.get_logs(Some(2));
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0], "line 2");
        assert_eq!(logs[1], "line 3");
    }

    #[test]
    fn test_logs_buffer_limit() {
        let mut state = ServiceState::default();

        for i in 0..150 {
            state.append_log(format!("line {}", i));
        }

        // Should be limited to MAX_LOG_LINES (100)
        let logs = state.get_logs(None);
        assert!(logs.len() <= 100);
    }

    #[test]
    fn test_logs_buffer_all_lines() {
        let mut state = ServiceState::default();

        state.append_log("line 1");
        state.append_log("line 2");
        state.append_log("line 3");

        let logs = state.get_logs(None);
        assert_eq!(logs.len(), 3);
        assert_eq!(logs[0], "line 1");
        assert_eq!(logs[2], "line 3");
    }

    // ============ CircuitBreaker Tests ============

    #[test]
    fn test_circuit_breaker_allows_by_default() {
        let cb = CircuitBreaker::new();
        assert!(cb.is_allowed());
        assert!(!cb.is_circuit_broken());
        assert_eq!(cb.failure_count(), 0);
    }

    #[test]
    fn test_circuit_breaker_records_failures() {
        let mut cb = CircuitBreaker::new();

        cb.record_failure();
        assert_eq!(cb.failure_count(), 1);
        assert!(cb.is_allowed());

        cb.record_failure();
        assert_eq!(cb.failure_count(), 2);
        assert!(cb.is_allowed());
    }

    #[test]
    fn test_circuit_breaker_breaks_after_max_failures() {
        let mut cb = CircuitBreaker::new();
        let max_failures = CIRCUIT_BREAKER_MAX_FAILURES;

        for _ in 0..max_failures {
            cb.record_failure();
        }

        assert!(cb.is_circuit_broken());
        assert!(!cb.is_allowed());
        assert!(cb.remaining_cooldown_secs().is_some());
    }

    #[test]
    fn test_circuit_breaker_resets_on_success() {
        let mut cb = CircuitBreaker::new();

        cb.record_failure();
        cb.record_failure();
        cb.record_success();

        assert_eq!(cb.failure_count(), 0);
        assert!(cb.is_allowed());
    }

    #[test]
    fn test_circuit_breaker_reset_after_broken() {
        let mut cb = CircuitBreaker::new();

        // Break the circuit
        for _ in 0..CIRCUIT_BREAKER_MAX_FAILURES {
            cb.record_failure();
        }
        assert!(cb.is_circuit_broken());

        // Record success should reset
        cb.record_success();
        assert!(!cb.is_circuit_broken());
        assert!(cb.is_allowed());
    }
}