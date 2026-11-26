//! Service management module

use anyhow::Result;
use serde_json::Value;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::get_config_dir;
use crate::ServiceAction;

const CLASH_API_HOST: &str = "127.0.0.1";

/// Default socket path for IPC
fn default_socket_path() -> PathBuf {
    PathBuf::from("/tmp/ctsvc.sock")
}

/// Check if Clash API is running
pub fn is_clash_api_running() -> bool {
    TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], crate::settings::get_api_port())),
        Duration::from_secs(1),
    ).is_ok()
}

/// IPC command types (must match control-tower-service)
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "cmd", content = "data")]
pub enum IpcCommand {
    Start { config_path: PathBuf },
    Stop,
    Shutdown,
    Restart,
    Status,
    ReloadCron,
    GetConnections,
    CloseConnection { id: String },
    SetMode { mode: String },
    SelectProxy { name: String },
    TestProxy { name: String, timeout_ms: Option<u64> },
    ApplyTun { tun_enabled: bool },
}

/// Enable or disable TUN mode through the service IPC socket.
///
/// This is used instead of the HTTP `/api/settings/apply-tun` endpoint so the
/// CLI keeps working when API authentication is enabled (the CLI has no
/// session token).
pub fn apply_tun(tun_enabled: bool) -> Result<()> {
    let resp = ipc_connect_and_send(&IpcCommand::ApplyTun { tun_enabled })
        .map_err(|e| anyhow::anyhow!(e))?;
    if !resp.is_success() {
        anyhow::bail!(if resp.message.is_empty() {
            "Failed to apply TUN settings".to_string()
        } else {
            resp.message
        });
    }
    Ok(())
}

/// IPC response types
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct IpcResponse {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl IpcResponse {
    pub fn is_success(&self) -> bool {
        self.code == 0
    }
}

/// Connect to IPC server and send command
fn ipc_connect_and_send(command: &IpcCommand) -> Result<IpcResponse, String> {
    use std::io::{Read, Write};

    let socket_path = default_socket_path();

    if !socket_path.exists() {
        return Err(format!("Service socket not found at {:?}. Is the service running?", socket_path));
    }

    let mut stream = UnixStream::connect(&socket_path)
        .map_err(|e| format!("Failed to connect to service: {}", e))?;

    // Send command
    let cmd_bytes = serde_json::to_vec(command)
        .map_err(|e| format!("Failed to serialize command: {}", e))?;
    stream.write_all(&cmd_bytes).map_err(|e| format!("Write error: {}", e))?;
    stream.write_all(b"\n").map_err(|e| format!("Flush error: {}", e))?;
    stream.flush().map_err(|e| format!("Flush error: {}", e))?;

    // Read response
    let mut buffer = Vec::new();
    let mut temp = [0u8; 4096];
    loop {
        match stream.read(&mut temp) {
            Ok(0) => break,
            Ok(n) => buffer.extend_from_slice(&temp[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(e) => return Err(format!("Read error: {}", e)),
        }
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

use std::os::unix::net::UnixStream;

/// Get proxy list from Clash API
pub async fn get_clash_proxies() -> Result<Value> {
    let url = format!("http://{}:{}/proxies", CLASH_API_HOST, crate::settings::get_api_port());

    let client = reqwest::Client::new();
    let response = match client
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(_) => {
            // Service not running, return empty proxy list gracefully
            return Ok(serde_json::json!({
                "proxies": {},
                "mode": "rule"
            }));
        }
    };

    if !response.status().is_success() {
        anyhow::bail!("Clash API error: {}", response.status());
    }

    let json: Value = response.json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse response: {}", e))?;

    Ok(json)
}

/// Select a proxy via IPC, falling back to direct API.
pub async fn select_proxy(name: &str) -> Result<()> {
    // Try IPC first if daemon is running
    if default_socket_path().exists() {
        if let Ok(resp) = ipc_connect_and_send(&IpcCommand::SelectProxy {
            name: name.to_string(),
        }) {
            if resp.is_success() {
                crate::settings::set_selected_proxy(name.to_string());
                return Ok(());
            }
            tracing::warn!("IPC SelectProxy failed: {}", resp.message);
        }
    }

    // Fallback to direct Mihomo API
    let url = format!("http://{}:{}/proxies/GLOBAL", CLASH_API_HOST, crate::settings::get_api_port());

    let client = reqwest::Client::new();
    let response = client
        .put(&url)
        .json(&serde_json::json!({ "name": name }))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to select proxy: {}", e))?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to select proxy: {}", response.status());
    }

    crate::settings::set_selected_proxy(name.to_string());
    Ok(())
}

/// Test a proxy's latency via IPC using Mihomo's delay API (no GLOBAL switching)
pub async fn test_proxy_ipc(name: &str, timeout_ms: Option<u64>) -> Result<u64> {
    if default_socket_path().exists() {
        let resp = ipc_connect_and_send(&IpcCommand::TestProxy {
            name: name.to_string(),
            timeout_ms,
        }).map_err(anyhow::Error::msg)?;
        if resp.is_success() {
            if let Some(data) = resp.data {
                if let Some(delay) = data.get("delay").and_then(|v| v.as_i64()).map(|d| d as u64) {
                    return Ok(delay);
                }
            }
            anyhow::bail!("Invalid response from TestProxy IPC");
        } else {
            anyhow::bail!("TestProxy IPC failed: {}", resp.message);
        }
    }
    anyhow::bail!("Service not running (socket not found)");
}

/// Get current mode from Clash
pub async fn get_mode() -> Result<String> {
    // First check settings.yaml for saved mode
    if let Some(mode) = crate::settings::get_mode() {
        return Ok(mode);
    }

    let port = crate::settings::get_api_port();
    let url = format!("http://{}:{}/proxies", CLASH_API_HOST, port);

    let client = reqwest::Client::new();

    let response = match client
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(_e) => {
            // Service not running or not reachable, return default
            return Ok("rule".to_string());
        }
    };

    if !response.status().is_success() {
        return Ok("rule".to_string());
    }

    let json: Value = response.json().await
        .map_err(|e| anyhow::anyhow!("Failed to parse response: {}", e))?;

    // Get the "mode" field from proxies response
    if let Some(mode) = json.get("mode").and_then(|v| v.as_str()) {
        return Ok(mode.to_string());
    }

    Ok("rule".to_string())
}

/// Set Clash mode via IPC, falling back to direct API.
/// Updates settings.yaml and config.yaml for persistence.
pub async fn set_mode(mode: &str) -> Result<()> {
    // Try IPC first if daemon is running
    if default_socket_path().exists() {
        if let Ok(resp) = ipc_connect_and_send(&IpcCommand::SetMode {
            mode: mode.to_string(),
        }) {
            if resp.is_success() {
                crate::settings::set_mode(mode.to_string());
                if let Err(e) = update_config_mode(mode) {
                    tracing::warn!("Failed to update config.yaml with mode: {}", e);
                }
                return Ok(());
            }
            tracing::warn!("IPC SetMode failed: {}", resp.message);
        }
    }

    // Fallback to direct Mihomo API
    let port = crate::settings::get_api_port();
    let url = format!("http://{}:{}/configs", CLASH_API_HOST, port);

    let client = reqwest::Client::new();
    let response = client
        .patch(&url)
        .json(&serde_json::json!({ "mode": mode }))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to set mode: {}", e))?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to set mode: {}", response.status());
    }

    // Persist mode to settings.yaml
    crate::settings::set_mode(mode.to_string());

    // Also update config.yaml so it persists across Mihomo restarts
    if let Err(e) = update_config_mode(mode) {
        tracing::warn!("Failed to update config.yaml with mode: {}", e);
    }

    Ok(())
}

/// Update the mode field in config.yaml
fn update_config_mode(mode: &str) -> Result<()> {
    let store = control_tower_service_core::ActiveConfigStore::new(
        crate::settings::shared_paths()?,
    );
    store.set_mode(mode)?;
    Ok(())
}

/// Get connections from Clash
pub async fn get_connections() -> Result<Value> {
    let url = format!("http://{}:{}/connections", CLASH_API_HOST, crate::settings::get_api_port());

    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get connections: {}", e))?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to get connections: {}", response.status());
    }

    let json: Value = response.json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse response: {}", e))?;

    Ok(json)
}

/// Close a connection by ID
pub async fn close_connection_by_id(id: &str) -> Result<()> {
    let url = format!("http://{}:{}/connections/{}", CLASH_API_HOST, crate::settings::get_api_port(), id);

    let client = reqwest::Client::new();
    let response = client
        .delete(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to close connection: {}", e))?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to close connection: {}", response.status());
    }

    Ok(())
}

/// Get connections via IPC (falls back to direct API)
pub async fn get_connections_via_ipc() -> Result<Value> {
    if default_socket_path().exists() {
        if let Ok(resp) = ipc_connect_and_send(&IpcCommand::GetConnections) {
            if resp.is_success() {
                return Ok(resp.data.unwrap_or_default());
            }
            tracing::warn!("IPC GetConnections failed: {}", resp.message);
        }
    }
    get_connections().await
}

/// Close a connection via IPC (falls back to direct API)
pub async fn close_connection_via_ipc(id: &str) -> Result<()> {
    if default_socket_path().exists() {
        if let Ok(resp) = ipc_connect_and_send(&IpcCommand::CloseConnection {
            id: id.to_string(),
        }) {
            if resp.is_success() {
                return Ok(());
            }
            tracing::warn!("IPC CloseConnection failed: {}", resp.message);
        }
    }
    close_connection_by_id(id).await
}

/// Start Mihomo service via IPC
pub async fn start_service() -> Result<()> {
    let socket_path = default_socket_path();

    // First check if service daemon is running
    if socket_path.exists() {
        // Service daemon is running, send Start command via IPC
        let config_dir = get_config_dir()?;
        let config_path = config_dir.join("config.yaml");

        if !config_path.exists() {
            anyhow::bail!("Config file not found: {:?}", config_path);
        }

        println!("Sending start command to service daemon...");
        let cmd = IpcCommand::Start { config_path };
        match ipc_connect_and_send(&cmd) {
            Ok(resp) => {
                if resp.is_success() {
                    println!("Control Tower service started successfully");
                } else {
                    anyhow::bail!("Failed to start service: {}", resp.message);
                }
            }
            Err(e) => {
                // Socket exists but daemon not responding - might be stale
                // Try to remove stale socket and spawn fresh daemon
                tracing::warn!("Service socket exists but daemon not responding: {}", e);
                std::fs::remove_file(&socket_path).ok();
                println!("Stale socket removed. Attempting to start fresh...");
            }
        }
    }

    // At this point, either socket didn't exist, or was stale and removed
    // We need to spawn the daemon
    if !socket_path.exists() {
        // Service daemon not running - try to start it
        println!("Service daemon not running. Attempting to start...");

        // Try to spawn ctsvc in background
        // Look for the service binary
        let exe_path = std::env::current_exe()
            .map_err(|e| anyhow::anyhow!("Failed to get current exe: {}", e))?;
        let service_exe = exe_path.parent()
            .ok_or_else(|| anyhow::anyhow!("Failed to get parent dir"))?
            .join("ctsvc");

        if !service_exe.exists() {
            // Try direct binary name
            let alt_exe = PathBuf::from("ctsvc");
            if alt_exe.exists() {
                spawn_service_daemon(&alt_exe)?;
            } else {
                anyhow::bail!(
                    "ctsvc binary not found. Please ensure it is installed and in PATH.\n\
                     Looked in: {:?}\n\
                     Current exe: {:?}",
                    service_exe, exe_path
                );
            }
        } else {
            spawn_service_daemon(&service_exe)?;
        }

        // Wait a moment for daemon to start
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Check if socket now exists and send start command
        let config_dir = get_config_dir()?;
        let config_path = config_dir.join("config.yaml");

        if !config_path.exists() {
            anyhow::bail!("Config file not found: {:?}", config_path);
        }

        // Retry IPC connection a few times
        for i in 0..5 {
            if socket_path.exists() {
                let cmd = IpcCommand::Start { config_path: config_path.clone() };
                match ipc_connect_and_send(&cmd) {
                    Ok(resp) => {
                        if resp.is_success() {
                            println!("Control Tower service started successfully");
                            return Ok(());
                        } else {
                            anyhow::bail!("Failed to start service: {}", resp.message);
                        }
                    }
                    Err(e) => {
                        if i < 4 {
                            tokio::time::sleep(Duration::from_millis(200)).await;
                            continue;
                        }
                        anyhow::bail!("Failed to communicate with service: {}", e);
                    }
                }
            }
            if i < 4 {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }

        anyhow::bail!("Service daemon started but IPC socket not available");
    }

    Ok(())
}

/// Spawn the service daemon in background
fn spawn_service_daemon(exe_path: &PathBuf) -> Result<()> {
    use std::process::Command;

    // Start the service in background (detached)
    // Redirect stdout/stderr to /dev/null to prevent any terminal output
    // The service will only write logs to file
    let child = Command::new(exe_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to spawn service daemon: {}", e))?;

    println!("Spawned service daemon with PID {}", child.id());
    Ok(())
}

/// Stop Mihomo service via IPC
pub async fn stop_service() -> Result<()> {
    let socket_path = default_socket_path();

    if !socket_path.exists() {
        // Check if Mihomo is running directly
        if is_clash_api_running() {
            // Mihomo running but no IPC daemon - try direct shutdown
            println!("Mihomo running but service daemon not found. Shutting down directly...");
            let url = format!("http://{}:{}/shutdown", CLASH_API_HOST, crate::settings::get_api_port());
            let client = reqwest::Client::new();
            match client.post(&url).timeout(Duration::from_secs(5)).send().await {
                Ok(_) => println!("Control Tower service stopped"),
                Err(e) => anyhow::bail!("Failed to stop service: {}", e),
            }
        } else {
            println!("Control Tower service is not running");
        }
        return Ok(());
    }

    // Send Stop command via IPC
    println!("Sending stop command to service daemon...");
    let cmd = IpcCommand::Stop;
    match ipc_connect_and_send(&cmd) {
        Ok(resp) => {
            if resp.is_success() {
                println!("Control Tower service stopped");
            } else {
                anyhow::bail!("Failed to stop service: {}", resp.message);
            }
        }
        Err(e) => anyhow::bail!("Failed to communicate with service: {}", e),
    }

    Ok(())
}

/// Restart Mihomo service via IPC
pub async fn restart_service() -> Result<()> {
    let socket_path = default_socket_path();

    if !socket_path.exists() {
        println!("Service daemon not running. Starting service...");
        return start_service().await;
    }

    // Send Restart command via IPC
    println!("Sending restart command to service daemon...");
    let cmd = IpcCommand::Restart;
    match ipc_connect_and_send(&cmd) {
        Ok(resp) => {
            if resp.is_success() {
                println!("Control Tower service restarted");
            } else {
                anyhow::bail!("Failed to restart service: {}", resp.message);
            }
        }
        Err(e) => anyhow::bail!("Failed to communicate with service: {}", e),
    }

    Ok(())
}
pub async fn status() -> Result<()> {
    let socket_path = default_socket_path();

    if !socket_path.exists() {
        // No IPC daemon - check if Mihomo running directly
        if is_clash_api_running() {
            println!("Status: Running (direct mode, no service daemon)");
            println!("API: http://127.0.0.1:9090");
            if let Ok(mode) = get_mode().await {
                println!("Mode: {}", mode);
            }
        } else {
            println!("Status: Stopped");
        }
        return Ok(());
    }

    // Send Status command via IPC
    let cmd = IpcCommand::Status;
    match ipc_connect_and_send(&cmd) {
        Ok(resp) => {
            if resp.is_success() {
                if let Some(data) = resp.data {
                    let status = data.as_object()
                        .ok_or_else(|| anyhow::anyhow!("Invalid status response"))?;
                    println!("Status: {}", if status.get("running").and_then(|v| v.as_bool()).unwrap_or(false) { "Running" } else { "Stopped" });
                    if let Some(pid) = status.get("pid").and_then(|v| v.as_u64()) {
                        println!("PID: {}", pid);
                    }
                    if let Some(uptime) = status.get("uptime_secs").and_then(|v| v.as_u64()) {
                        println!("Uptime: {} seconds", uptime);
                    }
                    println!("API: http://127.0.0.1:9090");
                    // Also get Clash mode
                    if let Ok(mode) = get_mode().await {
                        println!("Mode: {}", mode);
                    }
                } else {
                    println!("Status: Running (no detailed info)");
                }
            } else {
                println!("Failed to get status: {}", resp.message);
            }
        }
        Err(e) => {
            println!("Status: Error (IPC communication failed: {})", e);
            println!("Note: Mihomo API might still be accessible directly");
            if is_clash_api_running() {
                println!("API: http://127.0.0.1:9090");
            }
        }
    }

    Ok(())
}

pub async fn handle(action: ServiceAction) -> Result<()> {
    match action {
        ServiceAction::Start => start_service().await?,
        ServiceAction::Stop => stop_service().await?,
        ServiceAction::Status => status().await?,
        ServiceAction::Restart => {
            stop_service().await?;
            start_service().await?;
        }
        ServiceAction::Install => install_service()?,
    }
    Ok(())
}

// ============ systemd installation ============

const INSTALL_DIR: &str = "/opt/ctsvc";
const SERVICE_NAME: &str = "ctsvc";
const SERVICE_FILE: &str = "/etc/systemd/system/ctsvc.service";

fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// Install the service as a systemd unit.
///
/// Prefers running the daemon as the invoking desktop user (`SUDO_USER`) so
/// the desktop system-proxy integration writes to the right session and the
/// config directory stays writable without privileges. `CT_USER` overrides the
/// choice; otherwise root is used.
pub fn install_service() -> Result<()> {
    if !is_root() {
        anyhow::bail!("`ctctl service install` must run as root. Try: sudo ctctl service install");
    }

    let current_exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("Failed to locate ctctl: {}", e))?;
    let source_dir = current_exe
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Failed to locate ctctl directory"))?;
    let service_bin = source_dir.join("ctsvc");
    if !service_bin.exists() {
        anyhow::bail!(
            "ctsvc not found next to ctctl ({}). Run this from the extracted release tarball.",
            service_bin.display()
        );
    }

    let run_user = std::env::var("CT_USER")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("SUDO_USER")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && s != "root")
        })
        .unwrap_or_else(|| "root".to_string());

    println!("Installing ctsvc to {} (service user: {})", INSTALL_DIR, run_user);

    // Stop an existing unit before replacing files, ignore "not loaded".
    let _ = std::process::Command::new("systemctl")
        .args(["stop", SERVICE_NAME])
        .status();

    let install_dir = PathBuf::from(INSTALL_DIR);
    std::fs::create_dir_all(&install_dir)
        .map_err(|e| anyhow::anyhow!("Failed to create {}: {}", install_dir.display(), e))?;

    // Binaries are always replaced.
    copy_file(&service_bin, &install_dir.join("ctsvc"))?;
    let cli_bin = source_dir.join("ctctl");
    if cli_bin.exists() {
        copy_file(&cli_bin, &install_dir.join("ctctl"))?;
    }

    // User data is only seeded on first install, so an upgrade never wipes
    // credentials, profiles or the generated config.
    for name in ["settings.yaml", "profiles.yaml"] {
        let src = source_dir.join(name);
        let dst = install_dir.join(name);
        if src.exists() && !dst.exists() {
            copy_file(&src, &dst)?;
        }
    }
    let src_profiles = source_dir.join("profiles");
    let dst_profiles = install_dir.join("profiles");
    if src_profiles.is_dir() && !dst_profiles.exists() {
        copy_dir_all(&src_profiles, &dst_profiles)?;
    }

    // Mihomo + geo databases always follow the package version.
    let src_mihomo = source_dir.join("mihomo");
    if src_mihomo.is_dir() {
        copy_dir_all(&src_mihomo, &install_dir.join("mihomo"))?;
    }

    let settings = install_dir.join("settings.yaml");
    if settings.exists() {
        // The file can hold the auth verifier.
        set_file_mode(&settings, 0o600);
    }
    set_file_mode(&install_dir.join("ctsvc"), 0o755);

    // Hand the install directory to the service user so it can write
    // settings/profiles/config/logs without root.
    if run_user != "root" {
        let owner = format!("{}:{}", run_user, run_user);
        run_checked("chown", &["-R", owner.as_str(), INSTALL_DIR])?;
    }

    std::fs::write(SERVICE_FILE, systemd_unit(&run_user))
        .map_err(|e| anyhow::anyhow!("Failed to write {}: {}", SERVICE_FILE, e))?;

    run_checked("systemctl", &["daemon-reload"])?;
    run_checked("systemctl", &["enable", SERVICE_NAME])?;
    run_checked("systemctl", &["restart", SERVICE_NAME])?;

    println!();
    println!("========================================");
    println!("  ctsvc installed successfully!");
    println!("========================================");
    println!();
    println!("Service user: {}", run_user);
    println!("Commands:");
    println!("  Status:  sudo systemctl status {}", SERVICE_NAME);
    println!("  Logs:    sudo journalctl -u {} -f", SERVICE_NAME);
    println!("  Stop:    sudo systemctl stop {}", SERVICE_NAME);
    println!();
    println!("Web UI:  http://127.0.0.1:8080");
    Ok(())
}

fn systemd_unit(run_user: &str) -> String {
    format!(
        "[Unit]\n\
         Description=ctsvc - Control Tower Service\n\
         After=network.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart=\"{INSTALL_DIR}/ctsvc\"\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         User={run_user}\n\
         Environment=\"RUST_LOG=info\"\n\
         # The IPC socket lives in /tmp; PrivateTmp would hide it from ctctl.\n\
         NoNewPrivileges=true\n\
         AmbientCapabilities=CAP_NET_ADMIN\n\
         ProtectSystem=strict\n\
         ProtectHome=false\n\
         UMask=0077\n\
         ReadWritePaths={INSTALL_DIR}\n\
         StandardOutput=journal\n\
         StandardError=journal\n\
         SyslogIdentifier=ctsvc\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n"
    )
}

fn copy_file(src: &Path, dst: &Path) -> Result<()> {
    std::fs::copy(src, dst)
        .map_err(|e| anyhow::anyhow!("Failed to copy {} to {}: {}", src.display(), dst.display(), e))?;
    Ok(())
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)
        .map_err(|e| anyhow::anyhow!("Failed to create {}: {}", dst.display(), e))?;
    for entry in std::fs::read_dir(src)
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", src.display(), e))?
    {
        let entry = entry.map_err(|e| anyhow::anyhow!("Failed to read {}: {}", src.display(), e))?;
        let target = dst.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|e| anyhow::anyhow!("Failed to stat {}: {}", entry.path().display(), e))?;
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            copy_file(&entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_file_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn set_file_mode(_path: &Path, _mode: u32) {}

fn run_checked(program: &str, args: &[&str]) -> Result<()> {
    let status = std::process::Command::new(program)
        .args(args)
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run {}: {}", program, e))?;
    if !status.success() {
        anyhow::bail!("{} {:?} failed ({})", program, args, status);
    }
    Ok(())
}

/// Reload cron jobs from profiles.yaml via IPC
pub async fn reload_cron() -> Result<()> {
    let socket_path = default_socket_path();

    if !socket_path.exists() {
        return Ok(());
    }

    let cmd = IpcCommand::ReloadCron;
    match ipc_connect_and_send(&cmd) {
        Ok(resp) => {
            if resp.is_success() {
                tracing::info!("Cron jobs reloaded successfully");
            } else {
                tracing::warn!("Failed to reload cron jobs: {}", resp.message);
            }
        }
        Err(e) => {
            tracing::warn!("Failed to communicate with service: {}", e);
        }
    }

    Ok(())
}
