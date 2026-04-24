//! IPC protocol types — extracted from main.rs to reduce file size.
//!
//! Contains: IpcCommand, IpcResponse, ServiceStatus, parse_message,
//! serialize_response, handle_command (stateless stub), SHUTDOWN flag.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

/// Result of a single proxy latency test
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LatencyResult {
    pub name: String,
    pub latency: Option<i64>,
    pub error: Option<String>,
}

/// State for automatic latency testing (Auto Speed Test)
#[derive(Debug, Default)]
pub struct AutoTestState {
    pub enabled: bool,
    pub interval_secs: u64,
    pub last_test_at: Option<i64>,
    pub fastest: Option<LatencyResult>,
    pub results: Vec<LatencyResult>,
    /// Prevents concurrent auto-test runs
    pub running: bool,
}

/// IPC command types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "data")]
#[allow(dead_code)]
pub enum IpcCommand {
    /// Start Mihomo with config
    Start { config_path: std::path::PathBuf },
    /// Stop Mihomo (leaves the daemon running)
    Stop,
    /// Shutdown Mihomo and stop the daemon itself
    Shutdown,
    /// Restart Mihomo with the last config path
    Restart,
    /// Get current status
    Status,
    /// Get Mihomo logs
    Logs { lines: Option<u32> },
    /// Get proxy nodes from Clash
    GetProxies,
    /// Get connections from Clash
    GetConnections,
    /// Close a specific connection
    CloseConnection { id: String },
    /// Reload cron jobs from profiles.yaml
    ReloadCron,
    /// Set proxy mode (rule/global/direct)
    SetMode { mode: String },
    /// Select a proxy in the GLOBAL selector group
    SelectProxy { name: String },
    /// Test a proxy's latency using Mihomo's delay API (no GLOBAL switching)
    TestProxy { name: String, timeout_ms: Option<u64> },
}

/// IPC response types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct IpcResponse {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl IpcResponse {
    pub fn success() -> Self {
        Self {
            code: 0,
            message: String::new(),
            data: None,
        }
    }

    pub fn success_with_data<T: Serialize>(data: T) -> Self {
        Self {
            code: 0,
            message: String::new(),
            data: Some(serde_json::to_value(data).unwrap_or_default()),
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            code: -1,
            message: msg.into(),
            data: None,
        }
    }
}

/// Service status — richer than the original { running, pid, uptime_secs }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ServiceStatus {
    /// Whether Mihomo is currently running
    pub running: bool,
    /// Mihomo process ID
    pub pid: Option<u32>,
    /// Seconds since Mihomo was started
    pub uptime_secs: Option<u64>,
    /// Human-readable lifecycle state name
    pub state: String,
    /// Path to the active config file
    pub config_path: Option<std::path::PathBuf>,
    /// Circuit-breaker cooldown remaining seconds (if tripped), else absent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub circuit_breaker_remaining_secs: Option<u64>,
}

/// Parse a raw IPC message (JSON bytes) into IpcCommand
pub fn parse_message(raw: &[u8]) -> Result<IpcCommand, String> {
    serde_json::from_slice(raw)
        .map_err(|e| format!("Failed to parse IPC message: {}", e))
}

/// Serialize IpcResponse to bytes for sending
pub fn serialize_response(resp: &IpcResponse) -> Result<Vec<u8>, String> {
    serde_json::to_vec(resp)
        .map_err(|e| format!("Failed to serialize response: {}", e))
}

/// Handle an IPC command and return response
/// This function will be implemented to actually manage Mihomo
pub fn handle_command(cmd: IpcCommand) -> IpcResponse {
    match cmd {
        IpcCommand::Start { config_path } => {
            tracing::info!("Start command with config: {:?}", config_path);
            IpcResponse::success()
        }
        IpcCommand::Stop => {
            tracing::info!("Stop command");
            IpcResponse::success()
        }
        IpcCommand::Shutdown => {
            tracing::info!("Shutdown command");
            SHUTDOWN.store(true, Ordering::SeqCst);
            IpcResponse::success()
        }
        IpcCommand::Restart => {
            tracing::info!("Restart command");
            IpcResponse::error("Restart requires stateful handler (use handle_command_with_state)")
        }
        IpcCommand::Status => {
            tracing::info!("Status command");
            let status = ServiceStatus {
                running: false,
                pid: None,
                uptime_secs: None,
                state: "Unknown".to_string(),
                config_path: None,
                circuit_breaker_remaining_secs: None,
            };
            IpcResponse::success_with_data(&status)
        }
        IpcCommand::Logs { lines } => {
            tracing::info!("Logs command, lines: {:?}", lines);
            IpcResponse::success_with_data::<Vec<String>>(vec![])
        }
        IpcCommand::GetProxies => {
            tracing::info!("GetProxies command");
            IpcResponse::error("GetProxies requires stateful handler")
        }
        IpcCommand::GetConnections => {
            tracing::info!("GetConnections command");
            IpcResponse::error("GetConnections requires stateful handler")
        }
        IpcCommand::CloseConnection { id } => {
            tracing::info!("CloseConnection command, id: {}", id);
            IpcResponse::error("CloseConnection requires stateful handler")
        }
        IpcCommand::ReloadCron => {
            tracing::info!("ReloadCron command");
            IpcResponse::success()
        }
        IpcCommand::SetMode { mode } => {
            tracing::info!("SetMode command: {}", mode);
            IpcResponse::error("SetMode requires stateful handler")
        }
        IpcCommand::SelectProxy { name } => {
            tracing::info!("SelectProxy command: {}", name);
            IpcResponse::error("SelectProxy requires stateful handler")
        }
        IpcCommand::TestProxy { .. } => {
            tracing::info!("TestProxy command");
            IpcResponse::error("TestProxy requires stateful handler")
        }
    }
}

/// Shutdown flag — set by Shutdown IPC command
pub static SHUTDOWN: AtomicBool = AtomicBool::new(false);
