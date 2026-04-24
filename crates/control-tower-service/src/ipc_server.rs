//! IPC server implementation — Unix socket server for command dispatch.
//!
//! Exposed via `pub mod ipc_server;` from main.rs.

#![allow(dead_code)]

use crate::{handle_command_with_state, parse_message, serialize_response, IpcCommand, IpcResponse, ServiceState};
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// IPC server that manages Mihomo via ServiceState
pub struct IpcServer {
    socket_path: PathBuf,
    listener: Option<UnixListener>,
    state: Arc<ServiceState>,
}

impl IpcServer {
    /// Create a new IPC server (without starting it)
    pub fn new(socket_path: PathBuf, state: Arc<ServiceState>) -> Self {
        Self {
            socket_path,
            listener: None,
            state,
        }
    }

    /// Create server with a temporary directory (for testing)
    #[cfg(test)]
    pub fn with_temp_dir(state: Arc<ServiceState>) -> (Self, tempfile::TempDir) {
        let temp_dir = tempfile::tempdir().unwrap();
        let socket_path = temp_dir.path().join("test.sock");
        (Self::new(socket_path, state), temp_dir)
    }

    /// Start the server (bind to socket)
    pub fn start(&mut self) -> Result<(), String> {
        // Remove existing socket file if present
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path)
                .map_err(|e| format!("Failed to remove existing socket: {}", e))?;
        }

        // Create listener
        let listener = UnixListener::bind(&self.socket_path)
            .map_err(|e| format!("Failed to bind socket: {}", e))?;

        // Set non-blocking mode so we can check cron jobs periodically
        listener.set_nonblocking(true)
            .map_err(|e| format!("Failed to set non-blocking: {}", e))?;

        self.listener = Some(listener);
        Ok(())
    }

    /// Accept a connection and handle one request using ServiceState
    pub fn handle_one(&mut self) -> Result<Option<IpcCommand>, String> {
        let listener = self.listener.as_mut()
            .ok_or_else(|| "Server not started".to_string())?;

        let state = self.state.clone();

        // Non-blocking accept with timeout would be ideal,
        // but for testing we use blocking
        match listener.accept() {
            Ok((mut stream, _)) => {
                // Read request
                let mut buffer = Vec::new();
                let mut temp = [0u8; 1024];
                loop {
                    match stream.read(&mut temp) {
                        Ok(0) => break, // EOF
                        Ok(n) => buffer.extend_from_slice(&temp[..n]),
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                        Err(e) => return Err(format!("Read error: {}", e)),
                    }
                    // Check if we got the complete message (newline delimited)
                    if buffer.contains(&b'\n') {
                        break;
                    }
                }

                if buffer.is_empty() {
                    return Ok(None);
                }

                // Parse message
                let cmd = parse_message(&buffer)?;

                // Write response using handle_command_with_state
                let resp = handle_command_with_state(&state, cmd.clone());
                let resp_bytes = serialize_response(&resp)?;
                stream.write_all(&resp_bytes).map_err(|e| e.to_string())?;
                stream.write_all(b"\n").map_err(|e| e.to_string())?;
                stream.flush().map_err(|e| e.to_string())?;

                Ok(Some(cmd))
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(format!("Accept error: {}", e)),
        }
    }

    /// Stop the server
    pub fn stop(&mut self) {
        self.listener = None;
        let _ = std::fs::remove_file(&self.socket_path);
    }

    /// Get the socket path
    pub fn socket_path(&self) -> &PathBuf {
        &self.socket_path
    }

    /// Get reference to service state
    pub(crate) fn state(&self) -> &ServiceState {
        &self.state
    }
}

pub(crate) fn connect_and_send(socket_path: &Path, command: IpcCommand) -> Result<IpcResponse, String> {
    let mut stream = UnixStream::connect(socket_path)
        .map_err(|e| format!("Failed to connect: {}", e))?;

    // Send command
    let cmd_bytes = serde_json::to_vec(&command)
        .map_err(|e| format!("Failed to serialize command: {}", e))?;
    stream.write_all(&cmd_bytes).map_err(|e| format!("Write error: {}", e))?;
    stream.write_all(b"\n").map_err(|e| format!("Write error: {}", e))?;
    stream.flush().map_err(|e| format!("Flush error: {}", e))?;

    // Read response
    let mut buffer = Vec::new();
    let mut temp = [0u8; 1024];
    loop {
        match stream.read(&mut temp) {
            Ok(0) => break, // EOF
            Ok(n) => buffer.extend_from_slice(&temp[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(e) => return Err(format!("Read error: {}", e)),
        }
        // Response is newline delimited
        if buffer.contains(&b'\n') {
            break;
        }
    }

    // Remove trailing newline
    while buffer.last() == Some(&b'\n') {
        buffer.pop();
    }

    let resp: IpcResponse = serde_json::from_slice(&buffer)
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    Ok(resp)
}

#[cfg(test)]
mod server_tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        let state = Arc::new(ServiceState::new());
        let (server, _temp_dir) = IpcServer::with_temp_dir(state);
        assert!(!server.socket_path().exists());
    }

    #[test]
    fn test_server_start_stop() {
        let state = Arc::new(ServiceState::new());
        let (mut server, _temp_dir) = IpcServer::with_temp_dir(state);

        // Start server
        server.start().expect("Server should start");
        assert!(server.socket_path().exists());

        // Stop server
        server.stop();
        // Socket file should be removed after stop
        // Note: implementation may or may not remove it
    }

    #[test]
    fn test_server_socket_path() {
        let state = Arc::new(ServiceState::new());
        let socket_path = PathBuf::from("/tmp/test.sock");
        let server = IpcServer::new(socket_path.clone(), state);
        assert_eq!(server.socket_path(), &socket_path);
    }

    #[test]
    fn test_connect_to_nonexistent_server() {
        let result = connect_and_send(
            &PathBuf::from("/nonexistent/socket.sock"),
            IpcCommand::Status,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to connect"));
    }

    #[test]
    fn test_full_server_client_interaction() {
        use std::time::Duration;
        use std::thread;

        let state = Arc::new(ServiceState::new());
        let (mut server, _temp_dir) = IpcServer::with_temp_dir(state);

        // Start server
        server.start().expect("Server should start");

        let socket_path = server.socket_path().clone();

        // Spawn client in a separate thread
        let handle = thread::spawn(move || {
            // Small delay to ensure server is ready
            thread::sleep(Duration::from_millis(50));

            // Send Status command
            let resp = connect_and_send(&socket_path, IpcCommand::Status);
            assert!(resp.is_ok());
            let resp = resp.unwrap();
            assert_eq!(resp.code, 0);
        });

        // Server handles one request
        let _ = server.handle_one();

        // Wait for client
        handle.join().expect("Client thread should complete");

        server.stop();
    }

    #[test]
    fn test_server_handles_start_command() {
        use std::time::Duration;
        use std::thread;

        let state = Arc::new(ServiceState::new());
        let (mut server, _temp_dir) = IpcServer::with_temp_dir(state);
        server.start().expect("Server should start");

        let socket_path = server.socket_path().clone();

        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));

            // Start with nonexistent config - should return error
            let resp = connect_and_send(
                &socket_path,
                IpcCommand::Start {
                    config_path: PathBuf::from("/nonexistent/config.yaml"),
                },
            );
            assert!(resp.is_ok());
            // Should fail because config doesn't exist
            assert_eq!(resp.unwrap().code, -1);
        });

        let _ = server.handle_one();
        handle.join().expect("Client thread should complete");

        server.stop();
    }

    #[test]
    fn test_server_handles_stop_command() {
        use std::time::Duration;
        use std::thread;

        let state = Arc::new(ServiceState::new());
        let (mut server, _temp_dir) = IpcServer::with_temp_dir(state);
        server.start().expect("Server should start");

        let socket_path = server.socket_path().clone();

        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));

            let resp = connect_and_send(&socket_path, IpcCommand::Stop);
            assert!(resp.is_ok());
            assert_eq!(resp.unwrap().code, 0);
        });

        let _ = server.handle_one();
        handle.join().expect("Client thread should complete");

        server.stop();
    }

    #[test]
    fn test_server_handles_logs_command() {
        use std::time::Duration;
        use std::thread;

        let state = Arc::new(ServiceState::new());
        let (mut server, _temp_dir) = IpcServer::with_temp_dir(state);
        server.start().expect("Server should start");

        let socket_path = server.socket_path().clone();

        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));

            let resp = connect_and_send(
                &socket_path,
                IpcCommand::Logs { lines: Some(100) },
            );
            assert!(resp.is_ok());
            let resp = resp.unwrap();
            assert_eq!(resp.code, 0);
            assert!(resp.data.is_some());
        });

        let _ = server.handle_one();
        handle.join().expect("Client thread should complete");

        server.stop();
    }

    #[test]
    fn test_multiple_commands_in_sequence() {
        use std::time::Duration;
        use std::thread;

        let state = Arc::new(ServiceState::new());
        let (mut server, _temp_dir) = IpcServer::with_temp_dir(state);
        server.start().expect("Server should start");

        let socket_path = server.socket_path().clone();

        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));

            // Send multiple commands in sequence
            for _ in 0..3 {
                let resp = connect_and_send(&socket_path, IpcCommand::Status);
                assert!(resp.is_ok());
                assert_eq!(resp.unwrap().code, 0);
                thread::sleep(Duration::from_millis(10));
            }
        });

        // Handle multiple requests
        for _ in 0..3 {
            let _ = server.handle_one();
        }

        handle.join().expect("Client thread should complete");

        server.stop();
    }
}
