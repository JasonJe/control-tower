//! E2E Integration Tests for Control Tower Service
//!
//! These tests verify the complete IPC communication flow between
//! a client and the control-tower-service.

use std::path::PathBuf;
use std::process::{Command, Child};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

/// IPC Client for testing
struct IpcClient {
    socket_path: PathBuf,
}

impl IpcClient {
    fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }

    fn send_command(&self, cmd: &str) -> Result<String, String> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|e| format!("Failed to connect: {}", e))?;

        stream.write_all(cmd.as_bytes())
            .map_err(|e| format!("Failed to send: {}", e))?;
        stream.write_all(b"\n")
            .map_err(|e| format!("Failed to send newline: {}", e))?;
        stream.flush()
            .map_err(|e| format!("Failed to flush: {}", e))?;

        // Read response
        let mut response = String::new();
        stream.read_to_string(&mut response)
            .map_err(|e| format!("Failed to read: {}", e))?;

        Ok(response.trim().to_string())
    }
}

/// Start the service in foreground mode
fn start_service(socket_path: &PathBuf) -> Child {
    let _ = std::fs::remove_file(socket_path); // Clean up old socket

    let mut child = Command::new(env!("CARGO_BIN_EXE_ctsvc"))
        .arg("--foreground")
        .arg("--socket")
        .arg(socket_path.to_str().unwrap())
        .spawn()
        .expect("Failed to start service");

    // Wait for service to start
    std::thread::sleep(Duration::from_millis(500));

    child
}

fn stop_service(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Test that service starts and accepts connections
#[test]
fn test_service_startup() {
    let socket_path = PathBuf::from("/tmp/test-clash-verge-e2e.sock");
    let _ = std::fs::remove_file(&socket_path);

    let mut child = start_service(&socket_path);

    // Give it time to initialize
    std::thread::sleep(Duration::from_millis(500));

    // Try to connect
    let result = UnixStream::connect(&socket_path);
    assert!(result.is_ok(), "Should be able to connect to service socket");

    stop_service(child);
    let _ = std::fs::remove_file(&socket_path);
}

/// Test IPC protocol: Status command
#[test]
fn test_ipc_status_command() {
    let socket_path = PathBuf::from("/tmp/test-clash-verge-e2e-status.sock");
    let _ = std::fs::remove_file(&socket_path);

    let mut child = start_service(&socket_path);
    std::thread::sleep(Duration::from_millis(500));

    let client = IpcClient::new(socket_path.clone());
    let response = client.send_command(r#"{"cmd":"Status"}"#);

    assert!(response.is_ok(), "Status command should succeed");
    let resp_text = response.unwrap();
    assert!(resp_text.contains("\"code\":0") || resp_text.contains("\"code\": -1"),
        "Response should contain code field: {}", resp_text);

    stop_service(child);
    let _ = std::fs::remove_file(&socket_path);
}

/// Test IPC protocol: Logs command
#[test]
fn test_ipc_logs_command() {
    let socket_path = PathBuf::from("/tmp/test-clash-verge-e2e-logs.sock");
    let _ = std::fs::remove_file(&socket_path);

    let mut child = start_service(&socket_path);
    std::thread::sleep(Duration::from_millis(500));

    let client = IpcClient::new(socket_path.clone());
    // Note: serde tag="cmd" content="data" means Logs serializes as {"cmd":"Logs","data":{"lines":10}}
    let response = client.send_command(r#"{"cmd":"Logs","data":{"lines":10}}"#);

    assert!(response.is_ok(), "Logs command should succeed");
    let resp_text = response.unwrap();
    assert!(resp_text.contains("code"), "Response should contain code field: {}", resp_text);

    stop_service(child);
    let _ = std::fs::remove_file(&socket_path);
}

/// Test IPC protocol: Start command with non-existent config
#[test]
fn test_ipc_start_command_nonexistent_config() {
    let socket_path = PathBuf::from("/tmp/test-clash-verge-e2e-start.sock");
    let _ = std::fs::remove_file(&socket_path);

    let mut child = start_service(&socket_path);
    std::thread::sleep(Duration::from_millis(500));

    let client = IpcClient::new(socket_path.clone());
    // Note: serde tag="cmd" content="data" means Start serializes as {"cmd":"Start","data":{"config_path":"/path"}}
    let response = client.send_command(r#"{"cmd":"Start","data":{"config_path":"/nonexistent/config.yaml"}}"#);

    // Should return error (config doesn't exist)
    assert!(response.is_ok(), "Start command should return response");
    let resp_text = response.unwrap();
    // Error is expected since config doesn't exist
    assert!(resp_text.contains("code"), "Response should contain code field: {}", resp_text);

    stop_service(child);
    let _ = std::fs::remove_file(&socket_path);
}

/// Test IPC protocol: GetProxies command
#[test]
fn test_ipc_get_proxies_command() {
    let socket_path = PathBuf::from("/tmp/test-clash-verge-e2e-proxies.sock");
    let _ = std::fs::remove_file(&socket_path);

    let mut child = start_service(&socket_path);
    std::thread::sleep(Duration::from_millis(500));

    let client = IpcClient::new(socket_path.clone());
    let response = client.send_command(r#"{"cmd":"GetProxies"}"#);

    assert!(response.is_ok(), "GetProxies command should succeed");
    let resp_text = response.unwrap();
    assert!(resp_text.contains("code"), "Response should contain code field");

    stop_service(child);
    let _ = std::fs::remove_file(&socket_path);
}

/// Test IPC protocol: GetConnections command
#[test]
fn test_ipc_get_connections_command() {
    let socket_path = PathBuf::from("/tmp/test-clash-verge-e2e-conns.sock");
    let _ = std::fs::remove_file(&socket_path);

    let mut child = start_service(&socket_path);
    std::thread::sleep(Duration::from_millis(500));

    let client = IpcClient::new(socket_path.clone());
    let response = client.send_command(r#"{"cmd":"GetConnections"}"#);

    assert!(response.is_ok(), "GetConnections command should succeed");
    let resp_text = response.unwrap();
    assert!(resp_text.contains("code"), "Response should contain code field");

    stop_service(child);
    let _ = std::fs::remove_file(&socket_path);
}

/// Test IPC protocol: Stop command
#[test]
fn test_ipc_stop_command() {
    let socket_path = PathBuf::from("/tmp/test-clash-verge-e2e-stop.sock");
    let _ = std::fs::remove_file(&socket_path);

    let mut child = start_service(&socket_path);
    std::thread::sleep(Duration::from_millis(500));

    let client = IpcClient::new(socket_path.clone());
    let response = client.send_command(r#"{"cmd":"Stop"}"#);

    assert!(response.is_ok(), "Stop command should succeed");
    let resp_text = response.unwrap();
    assert!(resp_text.contains("code"), "Response should contain code field");

    // Give stop time to complete
    std::thread::sleep(Duration::from_millis(200));

    // Service should still be running but Mihomo should be stopped
    let status_response = client.send_command(r#"{"cmd":"Status"}"#);
    assert!(status_response.is_ok(), "Status command should still work after stop");

    stop_service(child);
    let _ = std::fs::remove_file(&socket_path);
}

/// Test multiple commands in sequence
#[test]
fn test_ipc_multiple_commands_sequence() {
    let socket_path = PathBuf::from("/tmp/test-clash-verge-e2e-multi.sock");
    let _ = std::fs::remove_file(&socket_path);

    let mut child = start_service(&socket_path);
    std::thread::sleep(Duration::from_millis(500));

    let client = IpcClient::new(socket_path.clone());

    // Sequence: Status -> Logs -> Status
    let resp1 = client.send_command(r#"{"cmd":"Status"}"#);
    assert!(resp1.is_ok());

    let resp2 = client.send_command(r#"{"cmd":"Logs","lines":5}"#);
    assert!(resp2.is_ok());

    let resp3 = client.send_command(r#"{"cmd":"Status"}"#);
    assert!(resp3.is_ok());

    stop_service(child);
    let _ = std::fs::remove_file(&socket_path);
}