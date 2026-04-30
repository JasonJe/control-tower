//! Control Tower Service Binary
//!
//! IPC server that manages Mihomo subprocess and exposes control interface
//! via Unix socket.

mod scheduler;
mod api;
mod cli;
mod html;
mod http_server;
mod https;
mod ipc_server;
mod ipc_types;
mod service;
mod service_state_ext;
mod session;
mod settings;

use std::collections::VecDeque;

pub use crate::ipc_types::SHUTDOWN;

use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;
use parking_lot::RwLock;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use tracing_subscriber::fmt::time::ChronoLocal;
use scheduler::ProfileCronJob;
use chrono::Utc;

// Re-export IPC types from ipc_types module (duplicates removed)
// Also re-export AutoTestState, LatencyResult (from ipc_types) and SettingsData (from settings)
pub use ipc_types::{AutoTestState, LatencyResult, IpcCommand, IpcResponse, ServiceStatus, parse_message, serialize_response, handle_command};
pub use settings::SettingsData;

/// Default socket path for IPC
pub fn default_socket_path() -> PathBuf {
    PathBuf::from("/tmp/ctsvc.sock")
}

/// Session info
#[derive(Clone)]
pub struct Session {
    pub token: String,
    pub created_at: u64,
}

/// Service state that manages Mihomo subprocess
pub struct ServiceState {
    manager: RwLock<control_tower_service_core::MihomoManager>,
    start_time: RwLock<Option<u64>>,
    log_buffer: RwLock<VecDeque<String>>,
    /// Cron jobs for profile auto-update
    cron_jobs: RwLock<Vec<ProfileCronJob>>,
    /// Last config path used to start Mihomo (used for Restart)
    last_config_path: RwLock<Option<PathBuf>>,
    /// Mihomo API host (default: 127.0.0.1)
    api_host: RwLock<String>,
    /// Mihomo API port (default: 9090)
    api_port: RwLock<u16>,
    /// Auto latency test state
    auto_test: RwLock<AutoTestState>,
    /// Track active connections for history recording
    active_connections: RwLock<std::collections::HashMap<String, crate::service::logging::ConnectionMetadata>>,
    /// Session tokens for API authentication
    sessions: RwLock<std::collections::HashMap<String, Session>>,
}

impl Default for ServiceState {
    fn default() -> Self {
        Self::new()
    }
}


// impl ServiceState methods are in service_state_ext.rs


/// Handle an IPC command using shared service state
pub fn handle_command_with_state(state: &ServiceState, cmd: IpcCommand) -> IpcResponse {
    match cmd {
        IpcCommand::Start { config_path } => {
            tracing::info!("Start command with config: {:?}", config_path);
            state.append_log(format!("Starting Mihomo with config: {:?}", config_path));
            match state.start(&config_path) {
                Ok(()) => {
                    state.append_log("Mihomo started successfully");
                    IpcResponse::success()
                }
                Err(e) => {
                    state.append_log(format!("Failed to start Mihomo: {}", e));
                    IpcResponse::error(e)
                }
            }
        }
        IpcCommand::Stop => {
            tracing::info!("Stop command");
            state.append_log("Stopping Mihomo...");
            match state.stop() {
                Ok(()) => {
                    state.append_log("Mihomo stopped");
                    IpcResponse::success()
                }
                Err(e) => {
                    state.append_log(format!("Failed to stop Mihomo: {}", e));
                    IpcResponse::error(e)
                }
            }
        }
        IpcCommand::Shutdown => {
            tracing::info!("Shutdown command");
            state.append_log("Shutting down...");
            let _ = state.stop();
            SHUTDOWN.store(true, Ordering::SeqCst);
            IpcResponse::success()
        }
        IpcCommand::Restart => {
            tracing::info!("Restart command");
            state.append_log("Restarting Mihomo...");

            // Stop first (ignore errors if not running)
            let _ = state.stop();

            // Use the last config path or error
            let config_path = match state.last_config_path.read().clone() {
                Some(path) => path,
                None => {
                    state.append_log("No previous config path — use 'Start' first");
                    return IpcResponse::error("No previous config path — use 'Start' first");
                }
            };

            match state.start(&config_path) {
                Ok(()) => {
                    state.append_log("Mihomo restarted successfully");
                    IpcResponse::success()
                }
                Err(e) => {
                    state.append_log(format!("Failed to restart Mihomo: {}", e));
                    IpcResponse::error(e)
                }
            }
        }
        IpcCommand::Status => {
            tracing::info!("Status command");
            let status = state.status();
            IpcResponse::success_with_data(&status)
        }
        IpcCommand::Logs { lines } => {
            tracing::info!("Logs command, lines: {:?}", lines);
            // Return actual logs from the buffer (convert u32 to usize)
            let logs = state.get_logs(lines.map(|l| l as usize));
            IpcResponse::success_with_data(&logs)
        }
        IpcCommand::GetProxies => {
            tracing::info!("GetProxies command");
            // Get proxies from Clash API
            match state.get_proxies() {
                Ok(proxies) => IpcResponse::success_with_data(&proxies),
                Err(e) => IpcResponse::error(e),
            }
        }
        IpcCommand::GetConnections => {
            tracing::info!("GetConnections command");
            // Get connections from Clash API
            match state.get_connections() {
                Ok(connections) => IpcResponse::success_with_data(&connections),
                Err(e) => IpcResponse::error(e),
            }
        }
        IpcCommand::CloseConnection { id } => {
            tracing::info!("CloseConnection command, id: {}", id);
            match state.close_connection(&id) {
                Ok(()) => IpcResponse::success(),
                Err(e) => IpcResponse::error(e),
            }
        }
        IpcCommand::ReloadCron => {
            tracing::info!("ReloadCron command");
            state.load_cron_jobs();
            IpcResponse::success()
        }
        IpcCommand::SetMode { mode } => {
            tracing::info!("SetMode command: {}", mode);
            match state.set_mode(&mode) {
                Ok(()) => IpcResponse::success(),
                Err(e) => IpcResponse::error(e),
            }
        }
        IpcCommand::SelectProxy { name } => {
            tracing::info!("SelectProxy command: {}", name);
            match state.select_proxy(&name) {
                Ok(()) => IpcResponse::success(),
                Err(e) => IpcResponse::error(e),
            }
        }
        IpcCommand::TestProxy { name, timeout_ms } => {
            let timeout = timeout_ms.unwrap_or(5000);
            let api_url = state.get_api_url();
            let encoded_name = utf8_percent_encode(&name, NON_ALPHANUMERIC).to_string();
            let url = format!(
                "{}/proxies/{}/delay?url={}&timeout={}",
                api_url,
                encoded_name,
                "http%3A%2F%2Fcp.cloudflare.com%2Fgenerate_204",
                timeout
            );
            match reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_millis(timeout + 1000))
                .build()
            {
                Ok(client) => {
                    match client.get(&url).send() {
                        Ok(response) => {
                            if response.status().is_success() {
                                match response.json::<serde_json::Value>() {
                                    Ok(data) => {
                                        let delay = data.get("delay").and_then(|v| v.as_i64()).unwrap_or(-1);
                                        IpcResponse::success_with_data(serde_json::json!({
                                            "delay": delay,
                                            "error": if delay < 0 { Some("Negative or missing delay".to_string()) } else { None::<String> }
                                        }))
                                    }
                                    Err(e) => IpcResponse::error(format!("Failed to parse response: {}", e)),
                                }
                            } else {
                                IpcResponse::error(format!("Mihomo error: {}", response.status()))
                            }
                        }
                        Err(e) => IpcResponse::error(format!("Request failed: {}", e)),
                    }
                }
                Err(e) => IpcResponse::error(format!("Failed to create HTTP client: {}", e)),
            }
        }
    }
}

// ============ IPC Server Module ============


#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    // ============ Serialization Tests ============

    #[test]
    fn test_ipc_command_start_serialization() {
        let cmd = IpcCommand::Start {
            config_path: PathBuf::from("/tmp/config.yaml"),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"cmd\":\"Start\""));
        assert!(json.contains("/tmp/config.yaml"));
    }

    #[test]
    fn test_ipc_command_stop_serialization() {
        let cmd = IpcCommand::Stop;
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"cmd\":\"Stop\""));
    }

    #[test]
    fn test_ipc_command_status_serialization() {
        let cmd = IpcCommand::Status;
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"cmd\":\"Status\""));
    }

    #[test]
    fn test_ipc_command_logs_serialization() {
        let cmd = IpcCommand::Logs { lines: Some(100) };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"cmd\":\"Logs\""));
        assert!(json.contains("100"));
    }

    #[test]
    fn test_ipc_command_set_mode_serialization() {
        let cmd = IpcCommand::SetMode { mode: "rule".to_string() };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"cmd\":\"SetMode\""));
        assert!(json.contains("rule"));
    }

    #[test]
    fn test_ipc_command_select_proxy_serialization() {
        let cmd = IpcCommand::SelectProxy { name: "香港 101".to_string() };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"cmd\":\"SelectProxy\""));
        assert!(json.contains("香港 101"));
    }

    // ============ Response Tests ============

    #[test]
    fn test_ipc_response_success() {
        let resp = IpcResponse::success();
        assert_eq!(resp.code, 0);
        assert!(resp.message.is_empty());
        assert!(resp.data.is_none());
    }

    #[test]
    fn test_ipc_response_error() {
        let resp = IpcResponse::error("test error");
        assert_eq!(resp.code, -1);
        assert_eq!(resp.message, "test error");
    }

    #[test]
    fn test_ipc_response_success_with_data() {
        #[derive(Serialize)]
        struct StatusData {
            running: bool,
            pid: Option<u32>,
        }
        let data = StatusData { running: true, pid: Some(1234) };
        let resp = IpcResponse::success_with_data(&data);
        assert_eq!(resp.code, 0);
        assert!(resp.data.is_some());
    }

    // ============ Status Tests ============

    #[test]
    fn test_service_status_serialization() {
        let status = ServiceStatus {
            running: true,
            pid: Some(1234),
            uptime_secs: Some(3600),
            state: "Running".to_string(),
            config_path: Some(PathBuf::from("/tmp/config.yaml")),
            circuit_breaker_remaining_secs: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"running\":true"));
        assert!(json.contains("\"pid\":1234"));
        assert!(json.contains("\"uptime_secs\":3600"));
        assert!(json.contains("\"state\":\"Running\""));
    }

    // ============ Deserialization Tests ============

    #[test]
    fn test_ipc_command_deserialization() {
        let json = r#"{"cmd":"Start","data":{"config_path":"/tmp/config.yaml"}}"#;
        let cmd: IpcCommand = serde_json::from_str(json).unwrap();
        match cmd {
            IpcCommand::Start { config_path } => {
                assert_eq!(config_path, PathBuf::from("/tmp/config.yaml"));
            }
            _ => panic!("Expected Start command"),
        }
    }

    #[test]
    fn test_ipc_command_stop_deserialization() {
        let json = r#"{"cmd":"Stop"}"#;
        let cmd: IpcCommand = serde_json::from_str(json).unwrap();
        match cmd {
            IpcCommand::Stop => {}
            _ => panic!("Expected Stop command"),
        }
    }

    // ============ Handler Tests (TDD - these define expected behavior) ============

    #[test]
    fn test_handle_command_start() {
        let state = ServiceState::new();
        let cmd = IpcCommand::Start {
            config_path: PathBuf::from("/nonexistent/config.yaml"),
        };
        let resp = handle_command_with_state(&state, cmd);
        // No config file → must return error
        assert_eq!(resp.code, -1);
        assert!(!resp.message.is_empty());
    }

    #[test]
    fn test_handle_command_stop() {
        let state = ServiceState::new();
        let cmd = IpcCommand::Stop;
        let resp = handle_command_with_state(&state, cmd);
        assert_eq!(resp.code, 0); // Stop on not-running is fine
    }

    #[test]
    fn test_handle_command_status_returns_data() {
        let state = ServiceState::new();
        let cmd = IpcCommand::Status;
        let resp = handle_command_with_state(&state, cmd);
        assert_eq!(resp.code, 0);
        assert!(resp.data.is_some(), "Status should return data");
    }

    #[test]
    fn test_handle_command_logs_returns_data() {
        let state = ServiceState::new();
        let cmd = IpcCommand::Logs { lines: Some(50) };
        let resp = handle_command_with_state(&state, cmd);
        assert_eq!(resp.code, 0);
        assert!(resp.data.is_some(), "Logs should return data");
    }

    // ============ Message Parsing Tests ============

    #[test]
    fn test_parse_message_valid_start() {
        let raw = br#"{"cmd":"Start","data":{"config_path":"/tmp/config.yaml"}}"#;
        let cmd = parse_message(raw).unwrap();
        match cmd {
            IpcCommand::Start { config_path } => {
                assert_eq!(config_path, PathBuf::from("/tmp/config.yaml"));
            }
            _ => panic!("Expected Start"),
        }
    }

    #[test]
    fn test_parse_message_invalid_json() {
        let raw = b"not valid json";
        let result = parse_message(raw);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to parse"));
    }

    #[test]
    fn test_serialize_response_success() {
        let resp = IpcResponse::success();
        let bytes = serialize_response(&resp).unwrap();
        let json_str = String::from_utf8(bytes).unwrap();
        assert!(json_str.contains("\"code\":0"));
    }

    #[test]
    fn test_serialize_response_with_error() {
        let resp = IpcResponse::error("test error");
        let bytes = serialize_response(&resp).unwrap();
        let json_str = String::from_utf8(bytes).unwrap();
        assert!(json_str.contains("\"code\":-1"));
        assert!(json_str.contains("test error"));
    }

    // ============ Socket Path Tests ============

    #[test]
    fn test_default_socket_path_contains_service_name() {
        let path = default_socket_path();
        let path_str = path.to_string_lossy();
        assert!(path_str.contains("ctsvc"));
        assert!(path_str.contains("ctsvc.sock"));
    }

    // ============ IPC Protocol Tests (TDD - defining the protocol) ============

    #[test]
    fn test_full_ipc_roundtrip_start() {
        // Client sends: Start command
        let cmd = IpcCommand::Start {
            config_path: PathBuf::from("/nonexistent/config.yaml"),
        };
        let raw = serde_json::to_vec(&cmd).unwrap();

        // Server parses
        let parsed = parse_message(&raw).unwrap();

        // Server handles (with real state — nonexistent config returns error)
        let state = ServiceState::new();
        let resp = handle_command_with_state(&state, parsed);

        // Server serializes
        let resp_bytes = serialize_response(&resp).unwrap();
        let resp_parsed: IpcResponse = serde_json::from_slice(&resp_bytes).unwrap();

        // Nonexistent config → error
        assert_eq!(resp_parsed.code, -1);
    }

    #[test]
    fn test_full_ipc_roundtrip_status() {
        // Client sends: Status command
        let cmd = IpcCommand::Status;
        let raw = serde_json::to_vec(&cmd).unwrap();

        // Server parses
        let parsed = parse_message(&raw).unwrap();

        // Server handles (with real state)
        let state = ServiceState::new();
        let resp = handle_command_with_state(&state, parsed);

        // Server serializes
        let resp_bytes = serialize_response(&resp).unwrap();
        let resp_parsed: IpcResponse = serde_json::from_slice(&resp_bytes).unwrap();

        assert_eq!(resp_parsed.code, 0);
        assert!(resp_parsed.data.is_some());
    }

    // ============ Handler Tests with MihomoManager Integration (TDD) ============

    /// Test that Start command with nonexistent config returns error
    #[test]
    fn test_handle_command_start_nonexistent_config() {
        let state = ServiceState::new();
        let cmd = IpcCommand::Start {
            config_path: PathBuf::from("/nonexistent/path/config.yaml"),
        };
        let resp = handle_command_with_state(&state, cmd);
        assert_eq!(resp.code, -1);
        assert!(!resp.message.is_empty());
    }

    /// Test that Status returns proper ServiceStatus structure
    #[test]
    fn test_handle_command_status_returns_service_status() {
        let state = ServiceState::new();
        let cmd = IpcCommand::Status;
        let resp = handle_command_with_state(&state, cmd);

        assert_eq!(resp.code, 0);
        assert!(resp.data.is_some());

        // Parse the data as ServiceStatus
        let data = resp.data.unwrap();
        let status: ServiceStatus = serde_json::from_value(data).unwrap();

        // Initially not running
        assert!(!status.running);
        assert!(status.pid.is_none());
    }

    /// Test that Start command twice doesn't panic (idempotent on error)
    #[test]
    fn test_handle_command_start_idempotent() {
        let state = ServiceState::new();
        let cmd = IpcCommand::Start {
            config_path: PathBuf::from("/nonexistent/config.yaml"),
        };

        // First start — config missing → error
        let resp1 = handle_command_with_state(&state, cmd.clone());
        assert_eq!(resp1.code, -1);

        // Second start — same error, no panic
        let resp2 = handle_command_with_state(&state, cmd);
        assert_eq!(resp2.code, -1);
    }

    /// Test Logs command with no lines specified (defaults)
    #[test]
    fn test_handle_command_logs_default_lines() {
        let state = ServiceState::new();
        let cmd = IpcCommand::Logs { lines: None };
        let resp = handle_command_with_state(&state, cmd);

        assert_eq!(resp.code, 0);
        assert!(resp.data.is_some());

        // Data should be a vector
        let data = resp.data.unwrap();
        assert!(data.is_array());
    }

    /// Test Logs command with specific line count
    #[test]
    fn test_handle_command_logs_with_line_count() {
        let state = ServiceState::new();
        let cmd = IpcCommand::Logs { lines: Some(10) };
        let resp = handle_command_with_state(&state, cmd);

        assert_eq!(resp.code, 0);
        assert!(resp.data.is_some());
    }

    // ============ Error Handling Tests ============

    #[test]
    fn test_parse_empty_message() {
        let raw = b"";
        let result = parse_message(raw);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_malformed_json() {
        let raw = b"{ invalid json }";
        let result = parse_message(raw);
        assert!(result.is_err());
    }

    #[test]
    fn test_response_error_contains_message() {
        let resp = IpcResponse::error("mihomo not found");
        let bytes = serialize_response(&resp).unwrap();
        let json_str = String::from_utf8(bytes).unwrap();

        assert!(json_str.contains("mihomo not found"));
        assert!(json_str.contains("\"code\":-1"));
    }

    // ============ ServiceStatus Data Structure Tests ============

    #[test]
    fn test_service_status_not_running() {
        let status = ServiceStatus {
            running: false,
            pid: None,
            uptime_secs: None,
            state: "NotRunning".to_string(),
            config_path: None,
            circuit_breaker_remaining_secs: None,
        };

        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"running\":false"));
        assert!(json.contains("\"pid\":null"));
        assert!(json.contains("\"state\":\"NotRunning\""));
    }

    #[test]
    fn test_service_status_running() {
        let status = ServiceStatus {
            running: true,
            pid: Some(12345),
            uptime_secs: Some(3600),
            state: "Running".to_string(),
            config_path: Some(PathBuf::from("/tmp/config.yaml")),
            circuit_breaker_remaining_secs: None,
        };

        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"running\":true"));
        assert!(json.contains("\"pid\":12345"));
        assert!(json.contains("\"uptime_secs\":3600"));
        assert!(json.contains("\"state\":\"Running\""));
    }

    // ============ IpcResponse Builder Tests ============

    #[test]
    fn test_response_with_string_data() {
        #[derive(Serialize)]
        struct LogLine {
            message: String,
            timestamp: String,
        }

        let log = LogLine {
            message: "test log".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };

        let resp = IpcResponse::success_with_data(&log);
        assert!(resp.data.is_some());
    }

    #[test]
    fn test_multiple_error_codes() {
        // Define error codes
        let resp_err_config = IpcResponse::error("config not found");
        assert_eq!(resp_err_config.code, -1);

        // Different error messages should still be -1
        let resp_err_mihomo = IpcResponse::error("mihomo binary not found");
        assert_eq!(resp_err_mihomo.code, -1);
    }

    // ============ State Management Tests (TDD - define expected state transitions) ============

    /// ServiceState tracks the current state of the service
    #[derive(Debug, Clone, PartialEq)]
    enum ServiceState2 {
        Idle,
        Starting,
        Running { pid: u32, start_time: u64 },
        Stopping,
        Error(String),
    }

    impl Default for ServiceState2 {
        fn default() -> Self {
            ServiceState2::Idle
        }
    }

    #[test]
    fn test_service_state_default_is_idle() {
        let state = ServiceState2::default();
        assert_eq!(state, ServiceState2::Idle);
    }

    #[test]
    fn test_service_state_transition_idle_to_starting() {
        // When Start command is received, state should transition to Starting
        let _state = ServiceState2::Idle;
        // Simulate start
        let new_state = ServiceState2::Starting;
        assert_eq!(new_state, ServiceState2::Starting);
    }

    #[test]
    fn test_service_state_transition_starting_to_running() {
        // When Mihomo successfully starts, state should be Running
        let _state = ServiceState2::Starting;
        let new_state = ServiceState2::Running { pid: 12345, start_time: 1000 };
        assert!(matches!(new_state, ServiceState2::Running { pid: 12345, .. }));
    }

    #[test]
    fn test_service_state_transition_running_to_stopping() {
        let _state = ServiceState2::Running { pid: 12345, start_time: 1000 };
        let new_state = ServiceState2::Stopping;
        assert_eq!(new_state, ServiceState2::Stopping);
    }

    #[test]
    fn test_service_state_transition_to_error() {
        // When something fails, state should be Error with message
        let _state = ServiceState2::Starting;
        let new_state = ServiceState2::Error("mihomo binary not found".to_string());
        assert!(matches!(new_state, ServiceState2::Error(msg) if msg.contains("mihomo")));
    }

    #[test]
    fn test_service_state_running_has_pid() {
        let state = ServiceState2::Running { pid: 999, start_time: 5000 };
        match state {
            ServiceState2::Running { pid, .. } => assert_eq!(pid, 999),
            _ => panic!("Expected Running state"),
        }
    }

    // ============ Response Code Conventions Tests ============

    #[test]
    fn test_response_code_conventions() {
        // 0 = success
        let success = IpcResponse::success();
        assert_eq!(success.code, 0);

        // -1 = general error
        let general_err = IpcResponse::error("general error");
        assert_eq!(general_err.code, -1);

        // Error response should have non-empty message
        let err = IpcResponse::error("config file missing");
        assert!(!err.message.is_empty());
    }

    // ============ Command Serialization Edge Cases Tests ============

    #[test]
    fn test_ipc_command_stop_no_data() {
        // Stop command should serialize without data field
        let cmd = IpcCommand::Stop;
        let json = serde_json::to_string(&cmd).unwrap();
        // Should be just {"cmd":"Stop"} without data
        assert!(json.contains("\"cmd\":\"Stop\""));
        assert!(!json.contains("data"));
    }

    #[test]
    fn test_ipc_command_status_no_data() {
        // Status command should serialize without data field
        let cmd = IpcCommand::Status;
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"cmd\":\"Status\""));
        assert!(!json.contains("data"));
    }

    #[test]
    fn test_ipc_command_logs_with_no_lines() {
        // Logs with lines=None should still serialize properly
        let cmd = IpcCommand::Logs { lines: None };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"cmd\":\"Logs\""));
    }

    // ============ Integration Pattern Tests ============

    #[test]
    fn test_full_start_stop_cycle() {
        // Simulate full start/stop cycle
        // 1. Start command
        let start_cmd = IpcCommand::Start {
            config_path: PathBuf::from("/tmp/test.yaml"),
        };
        let start_resp = handle_command(start_cmd);
        assert_eq!(start_resp.code, 0);

        // 2. Status command - should show running
        let status_cmd = IpcCommand::Status;
        let status_resp = handle_command(status_cmd);
        assert_eq!(status_resp.code, 0);

        // 3. Stop command
        let stop_cmd = IpcCommand::Stop;
        let stop_resp = handle_command(stop_cmd);
        assert_eq!(stop_resp.code, 0);
    }

    #[test]
    fn test_full_start_status_stop_cycle() {
        // More detailed cycle
        let config = PathBuf::from("/tmp/config.yaml");

        // Start
        let resp = handle_command(IpcCommand::Start { config_path: config.clone() });
        assert_eq!(resp.code, 0);

        // Status - should return data
        let resp = handle_command(IpcCommand::Status);
        assert_eq!(resp.code, 0);
        assert!(resp.data.is_some());

        // Logs
        let resp = handle_command(IpcCommand::Logs { lines: Some(50) });
        assert_eq!(resp.code, 0);

        // Stop
        let resp = handle_command(IpcCommand::Stop);
        assert_eq!(resp.code, 0);
    }

    // ============ ServiceState Tests (Real MihomoManager Integration) ============

    #[test]
    fn test_service_state_new() {
        let state = ServiceState::new();
        assert!(!state.is_running());
    }

    #[test]
    fn test_service_state_status_when_not_running() {
        let state = ServiceState::new();
        let status = state.status();

        assert!(!status.running);
        assert!(status.pid.is_none());
        assert!(status.uptime_secs.is_none());
    }

    #[test]
    fn test_service_state_start_with_nonexistent_config() {
        let state = ServiceState::new();
        let config_path = PathBuf::from("/nonexistent/config.yaml");

        let result = state.start(&config_path);
        // Should fail because config doesn't exist
        assert!(result.is_err());
    }

    #[test]
    fn test_service_state_stop_when_not_running() {
        let state = ServiceState::new();
        // Should succeed even if not running (idempotent)
        let result = state.stop();
        assert!(result.is_ok());
    }

    #[test]
    fn test_service_state_twice_is_running() {
        let state = ServiceState::new();

        // First check
        let status1 = state.status();
        assert!(!status1.running);

        // Second check should be same
        let status2 = state.status();
        assert!(!status2.running);
    }

    // ============ handle_command_with_state Tests ============

    #[test]
    fn test_handle_command_with_state_start_error() {
        let state = ServiceState::new();
        let cmd = IpcCommand::Start {
            config_path: PathBuf::from("/nonexistent/config.yaml"),
        };

        let resp = handle_command_with_state(&state, cmd);
        // Should return error because config doesn't exist
        assert_eq!(resp.code, -1);
        assert!(!resp.message.is_empty());
    }

    #[test]
    fn test_handle_command_with_state_stop() {
        let state = ServiceState::new();
        let cmd = IpcCommand::Stop;

        let resp = handle_command_with_state(&state, cmd);
        // Stop on not-running should succeed
        assert_eq!(resp.code, 0);
    }

    #[test]
    fn test_handle_command_with_state_status() {
        let state = ServiceState::new();
        let cmd = IpcCommand::Status;

        let resp = handle_command_with_state(&state, cmd);
        assert_eq!(resp.code, 0);
        assert!(resp.data.is_some());

        // Parse status
        let data = resp.data.unwrap();
        let status: ServiceStatus = serde_json::from_value(data).unwrap();
        assert!(!status.running);
    }

    #[test]
    fn test_handle_command_with_state_logs() {
        let state = ServiceState::new();
        let cmd = IpcCommand::Logs { lines: Some(50) };

        let resp = handle_command_with_state(&state, cmd);
        assert_eq!(resp.code, 0);
        assert!(resp.data.is_some());
    }

    // ============ AutoTestState Tests ============

    #[test]
    fn test_auto_test_state_default() {
        let state = AutoTestState::default();
        assert!(!state.enabled);
        assert_eq!(state.interval_secs, 0);
        assert!(state.fastest.is_none());
        assert!(state.results.is_empty());
    }

    #[test]
    fn test_latency_result_fastest() {
        let results = vec![
            LatencyResult { name: "a".into(), latency: Some(100), error: None },
            LatencyResult { name: "b".into(), latency: Some(50), error: None },
            LatencyResult { name: "c".into(), latency: None, error: Some("N/A".into()) },
        ];
        let fastest = results.iter()
            .filter(|r| r.latency.is_some())
            .min_by_key(|r| r.latency.unwrap())
            .cloned();
        assert!(fastest.is_some());
        assert_eq!(fastest.unwrap().name, "b");
    }
}

// ============ Main Function Implementation ============

/// Main server loop
fn run_server(mut server: ipc_server::IpcServer, state: Arc<ServiceState>) -> Result<(), String> {
    server.start()?;

    tracing::info!(
        "Control Tower Service started on socket: {}",
        server.socket_path().display()
    );

    // Load cron jobs on startup
    state.load_cron_jobs();

    // Run startup profile auto-update if enabled
    {
        let settings = state.get_settings();
        if settings.auto_update_on_startup.unwrap_or(false) {
            tracing::info!("Auto-update on startup enabled, checking all profile subscriptions");
            let state_clone = state.clone();
            thread::spawn(move || {
                let results = state_clone.check_and_update_all_profiles();
                let updated = results.iter().filter(|(_, s)| *s).count();
                tracing::info!("Startup profile update complete: {}/{} profiles updated", updated, results.len());
            });
        }
    }

    let mut last_cron_check = std::time::Instant::now();
    const CRON_CHECK_INTERVAL: Duration = Duration::from_secs(60);

    // Track last provider refresh separately — respects provider's configured interval
    let mut last_provider_refresh = std::time::Instant::now();

    // Main loop - keep accepting connections until shutdown
    // Note: In production, this should use proper async I/O with tokio
    while !cli::SHUTDOWN.load(Ordering::SeqCst) {
        // Check if it's time to check cron jobs
        if last_cron_check.elapsed() >= CRON_CHECK_INTERVAL {
            state.check_and_run_crons();

            // Check if auto latency test is due
            {
                let should_run = {
                    let mut auto_test = state.auto_test.write();
                    if auto_test.running {
                        false
                    } else {
                        let last = auto_test.last_test_at.unwrap_or(0);
                        let interval = auto_test.interval_secs.max(60) as i64;
                        let now = Utc::now().timestamp();
                        if now - last >= interval {
                            auto_test.running = true;
                            true
                        } else {
                            false
                        }
                    }
                };
                if should_run {
                    let test_state = state.clone();
                    std::thread::spawn(move || {
                        let _span = tracing::info_span!("auto_latency_test");
                        test_state.run_auto_latency_test();
                    });
                }
            }

            // Periodic rule provider refresh — respects each provider's configured interval.
            // Only triggers PUT /configs?force=true when the minimum interval across all
            // providers has elapsed since the last refresh.
            {
                let settings = state.get_settings();
                if let Some(ref providers) = settings.rule_providers {
                    if !providers.is_empty() {
                        // Find the shortest interval among all providers
                        let min_interval_secs = providers.iter()
                            .map(|p| p.interval)
                            .filter(|&i| i > 0)
                            .min()
                            .unwrap_or(3600)     // default 1h if no interval set
                            .max(60) as u64;      // at least every 60s
                        let elapsed = last_provider_refresh.elapsed().as_secs();
                        if elapsed >= min_interval_secs {
                            let api_port = *state.api_port.read();
                            let url = format!("http://127.0.0.1:{}/configs?force=true", api_port);
                            match reqwest::blocking::Client::new()
                                .put(&url)
                                .json(&serde_json::json!({}))
                                .timeout(std::time::Duration::from_secs(10))
                                .send()
                            {
                                Ok(res) if res.status().is_success() => {
                                    tracing::info!("Periodic rule provider refresh triggered (min interval {}s)", min_interval_secs);
                                }
                                Ok(res) => {
                                    tracing::warn!("Periodic provider refresh returned: {}", res.status());
                                }
                                Err(e) => {
                                    tracing::warn!("Periodic provider refresh failed: {}", e);
                                }
                            }
                            last_provider_refresh = std::time::Instant::now();
                        }
                    }
                }
            }

            last_cron_check = std::time::Instant::now();
        }

        // Sleep briefly to avoid busy-waiting
        thread::sleep(Duration::from_millis(100));
    }

    tracing::info!("Shutting down IPC server...");
    server.stop();
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let args = cli::Args::parse();

    // Determine working directory from executable location
    let work_dir = control_tower_service_core::exe_dir();

    // Build shared paths model
    let paths = control_tower_service_core::ControlTowerPaths::from_settings(
        work_dir.join("settings.yaml"),
        None,
    );

    // Create logs directory
    let log_dir = paths.log_dir.clone();
    std::fs::create_dir_all(&log_dir)?;

    // Initialize logging with file output
    let log_level = match args.log_level.as_str() {
        "trace" => tracing::Level::TRACE,
        "debug" => tracing::Level::DEBUG,
        "info" => tracing::Level::INFO,
        "warn" => tracing::Level::WARN,
        "error" => tracing::Level::ERROR,
        _ => tracing::Level::INFO,
    };

    // Create rolling file appender (daily rotation, keep for 7 days)
    let file_appender = RollingFileAppender::new(
        Rotation::DAILY,
        &log_dir,
        "ctsvc.log",
    );

    // Initialize subscriber with file output (and stdout only in foreground mode)
    // Use ChronoLocal for local timezone timestamps
    let local_timer = ChronoLocal::new("%Y-%m-%dT%H:%M:%S%.3f%z".to_string());

    let file_layer = fmt::layer()
        .with_target(false)
        .with_ansi(false)
        .with_timer(local_timer.clone())
        .with_writer(file_appender);

    // Only add stdout layer in foreground mode (for debugging)
    // In daemon mode, stdout should not be used
    if args.foreground {
        let stdout_layer = fmt::layer()
            .with_target(false)
            .with_timer(local_timer)
            .with_writer(std::io::stdout);
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::from_default_env().add_directive(log_level.into()))
            .with(file_layer)
            .with(stdout_layer);
        tracing::subscriber::set_global_default(subscriber)?;
    } else {
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::from_default_env().add_directive(log_level.into()))
            .with(file_layer);
        tracing::subscriber::set_global_default(subscriber)?;
    }

    tracing::info!("Control Tower Service v0.1.0");
    tracing::info!("Log directory: {}", log_dir.display());

    // Determine socket path and service port
    let socket_path = args.socket.unwrap_or_else(|| paths.socket_path.clone());
    let service_port = cli::get_service_port();

    tracing::info!("Socket path: {}", socket_path.display());

    // ============ Startup Checks ============

    // Check if socket file already exists (another instance may be running)
    if socket_path.exists() {
        use std::os::unix::net::UnixStream;
        if UnixStream::connect(&socket_path).is_ok() {
            eprintln!("ERROR: ctsvc is already running (socket {} is active)", socket_path.display());
            eprintln!("Hint: Stop the existing service first with: sudo systemctl stop ctsvc");
            std::process::exit(1);
        } else {
            // Socket file exists but can't connect - stale socket, remove it
            tracing::warn!("Removing stale socket file: {}", socket_path.display());
            std::fs::remove_file(&socket_path).ok();
        }
    }

    // Check if service port is available (skip in foreground mode)
    if !args.foreground {
        use std::net::TcpListener;
        let addr = format!("0.0.0.0:{}", service_port);
        match TcpListener::bind(&addr) {
            Ok(listener) => {
                drop(listener);
                tracing::info!("Port {} is available", service_port);
            }
            Err(_) => {
                eprintln!("ERROR: Port {} is already in use", service_port);
                eprintln!("Hint: Stop the application using this port, or change service_port in settings.yaml");
                std::process::exit(1);
            }
        }
    }

    // Create service state
    let state = Arc::new(ServiceState::new());

    // Ensure settings.yaml exists (create with defaults if missing)
    ServiceState::ensure_settings_file();

    // Load settings from settings.yaml (also syncs auto_test.enabled)
    state.load_settings();

    // Auto-start Mihomo if config.yaml exists and no --no-auto-start flag
    let exe_dir = control_tower_service_core::exe_dir();
    let config_path = exe_dir.join("config.yaml");
    if config_path.exists() {
        tracing::info!("Auto-starting Mihomo with config: {}", config_path.display());
        match state.start(&config_path) {
            Ok(_) => tracing::info!("Mihomo started successfully"),
            Err(e) => tracing::warn!("Failed to auto-start Mihomo: {}", e),
        }
    } else {
        tracing::info!("No config.yaml found, Mihomo will not auto-start");
    }

    // Only start HTTP server when not in foreground mode (e2e tests use --foreground)
    if !args.foreground {
        // Get HTTPS settings before moving state into closure
        let settings = state.get_settings();
        let https_config = settings.https;
        let (https_enabled, cert_path, key_path) = match https_config {
            Some(cfg) => (
                cfg.enabled,
                cfg.cert_path.map(PathBuf::from),
                cfg.key_path.map(PathBuf::from),
            ),
            None => (false, None, None),
        };

        let http_state = state.clone();
        std::thread::spawn(move || {
            let _span = tracing::info_span!("http_server", port = service_port);
            let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime for HTTP server");
            rt.block_on(async move {
                if let Err(e) = http_server::start_http_server(service_port, http_state, https_enabled, cert_path, key_path).await {
                    tracing::error!("HTTP server error: {}", e);
                }
            });
        });
        tracing::info!("HTTP API server configured on port {}", service_port);
    } else {
        tracing::info!("Foreground mode: HTTP API server disabled");
    }

    // Create IPC server
    let server = ipc_server::IpcServer::new(socket_path, state.clone());

    // Setup signal handlers
    cli::setup_signal_handlers();

    // Run server
    run_server(server, state)?;

    Ok(())
}
