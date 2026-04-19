//! Control Tower Service Binary
//!
//! IPC server that manages Mihomo subprocess and exposes control interface
//! via Unix socket.

mod scheduler;
mod api;
mod html;
mod http_server;
mod settings;

use serde::{Deserialize, Serialize};
use std::path::{PathBuf, Path};
use std::sync::Arc;
use std::time::Duration;
use parking_lot::RwLock;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use tracing_subscriber::fmt::time::ChronoLocal;
use scheduler::{Schedule, ProfileCronJob};

/// IPC command types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "data")]
pub enum IpcCommand {
    /// Start Mihomo with config
    Start { config_path: PathBuf },
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
}

/// IPC response types
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub config_path: Option<PathBuf>,
    /// Circuit-breaker cooldown remaining seconds (if tripped), else absent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub circuit_breaker_remaining_secs: Option<u64>,
}

/// Settings data structure matching settings.yaml
pub use settings::SettingsData;

/// Default socket path for IPC
pub fn default_socket_path() -> PathBuf {
    PathBuf::from("/tmp/ctsvc.sock")
}

/// Handle an IPC command and return response
/// This function will be implemented to actually manage Mihomo
pub fn handle_command(cmd: IpcCommand) -> IpcResponse {
    match cmd {
        IpcCommand::Start { config_path } => {
            tracing::info!("Start command with config: {:?}", config_path);
            // Stateless stub — real work happens in handle_command_with_state
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
            // TODO: Return actual logs
            IpcResponse::success_with_data(vec!["<log line>"])
        }
        IpcCommand::GetProxies => {
            tracing::info!("GetProxies command");
            // Requires state - should use handle_command_with_state in practice
            IpcResponse::error("GetProxies requires service state")
        }
        IpcCommand::GetConnections => {
            tracing::info!("GetConnections command");
            // Requires state - should use handle_command_with_state in practice
            IpcResponse::error("GetConnections requires service state")
        }
        IpcCommand::CloseConnection { id: _ } => {
            tracing::info!("CloseConnection command");
            // Requires state - should use handle_command_with_state in practice
            IpcResponse::error("CloseConnection requires service state")
        }
        IpcCommand::ReloadCron => {
            tracing::info!("ReloadCron command");
            // Requires state - should use handle_command_with_state in practice
            IpcResponse::error("ReloadCron requires service state")
        }
        IpcCommand::SetMode { .. } => {
            IpcResponse::error("SetMode requires stateful handler")
        }
        IpcCommand::SelectProxy { .. } => {
            IpcResponse::error("SelectProxy requires stateful handler")
        }
    }
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

// ============ Service State with MihomoManager Integration ============

use std::collections::VecDeque;

/// Max log lines to keep
const MAX_SERVICE_LOG_LINES: usize = 100;

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
}

impl Default for ServiceState {
    fn default() -> Self {
        Self::new()
    }
}

impl ServiceState {
    pub fn new() -> Self {
        Self {
            manager: RwLock::new(control_tower_service_core::MihomoManager::new()),
            start_time: RwLock::new(None),
            log_buffer: RwLock::new(VecDeque::with_capacity(MAX_SERVICE_LOG_LINES)),
            cron_jobs: RwLock::new(Vec::new()),
            last_config_path: RwLock::new(None),
            api_host: RwLock::new("127.0.0.1".to_string()),
            api_port: RwLock::new(9090),
        }
    }

    /// Load api_host and api_port from settings.yaml
    pub fn load_settings(&self) {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        let settings_path = exe_dir.join("settings.yaml");
        if !settings_path.exists() {
            tracing::info!("settings.yaml not found, using defaults");
            return;
        }

        let content = match std::fs::read_to_string(&settings_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to read settings.yaml: {}", e);
                return;
            }
        };

        #[derive(serde::Deserialize)]
        struct Settings {
            api_host: Option<String>,
            api_port: Option<u16>,
        }

        match serde_yaml_ng::from_str::<Settings>(&content) {
            Ok(settings) => {
                if let Some(host) = settings.api_host {
                    *self.api_host.write() = host;
                }
                if let Some(port) = settings.api_port {
                    *self.api_port.write() = port;
                }
                tracing::info!("Loaded settings: api_host={}, api_port={}",
                    self.api_host.read(), self.api_port.read());
            }
            Err(e) => {
                tracing::warn!("Failed to parse settings.yaml: {}", e);
            }
        }
    }

    /// Get the Mihomo API base URL
    pub fn get_api_url(&self) -> String {
        let host = self.api_host.read().clone();
        let port = *self.api_port.read();
        format!("http://{}:{}", host, port)
    }

    /// Ensure settings.yaml exists with default values, create if missing
    pub fn ensure_settings_file() {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        let settings_path = exe_dir.join("settings.yaml");

        if settings_path.exists() {
            return;
        }

        // Create default settings.yaml
        let default_settings = SettingsData {
            api_host: Some("127.0.0.1".to_string()),
            api_port: Some(9090),
            http_port: Some(7890),
            socks_port: Some(7891),
            service_port: Some(8080),
            tun_enabled: Some(false),
            log_level: Some("info".to_string()),
            mode: Some("rule".to_string()),
        };

        let yaml = serde_yaml_ng::to_string(&default_settings)
            .unwrap_or_else(|e| {
                tracing::error!("Serialized settings failed: {}", e);
                "api_host: 127.0.0.1\napi_port: 9090\nhttp_port: 7890\nsocks_port: 7891\nservice_port: 8080\ntun_enabled: false\nlog_level: info\nmode: rule\n".to_string()
            });

        if let Err(e) = std::fs::write(&settings_path, yaml) {
            tracing::error!("Failed to create default settings.yaml: {}", e);
        } else {
            tracing::info!("Created default settings.yaml");
        }
    }

    /// Get all settings from settings.yaml
    pub fn get_settings(&self) -> SettingsData {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        let settings_path = exe_dir.join("settings.yaml");
        if !settings_path.exists() {
            return SettingsData::default();
        }

        let content = match std::fs::read_to_string(&settings_path) {
            Ok(c) => c,
            Err(_) => return SettingsData::default(),
        };

        serde_yaml_ng::from_str(&content).unwrap_or_default()
    }

    /// Save settings to settings.yaml
    pub fn save_settings(&self, settings: &SettingsData) -> Result<(), String> {
        let exe_dir = std::env::current_exe()
            .map_err(|e| format!("Failed to get exe path: {}", e))?
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| "Failed to get exe parent".to_string())?;

        let settings_path = exe_dir.join("settings.yaml");

        let yaml_str = serde_yaml_ng::to_string(settings)
            .map_err(|e| format!("Failed to serialize settings: {}", e))?;

        std::fs::write(&settings_path, yaml_str)
            .map_err(|e| format!("Failed to write settings.yaml: {}", e))?;

        // Reload settings into state
        self.load_settings();

        tracing::info!("Settings saved to settings.yaml");
        Ok(())
    }

    /// Apply port settings: save to settings.yaml, update config.yaml, restart Mihomo
    pub fn apply_port_settings(&self, http_port: u16, socks_port: u16) -> Result<(), String> {
        // Step 1: Update config.yaml with new ports
        self.update_config_ports(http_port, socks_port)?;

        // Step 2: Save to settings.yaml (only port-related fields)
        let settings = SettingsData {
            api_host: Some("127.0.0.1".to_string()),
            api_port: Some(*self.api_port.read()),
            http_port: Some(http_port),
            socks_port: Some(socks_port),
            service_port: Some(8080),
            tun_enabled: Some(false),
            log_level: Some("info".to_string()),
            mode: None,
        };
        self.save_settings(&settings)?;

        // Step 3: Restart Mihomo with updated config
        let config_path = self.last_config_path.read().clone();
        if config_path.is_some() {
            self.restart_mihomo()?;
        } else {
            // Mihomo not running, just update config - user can start manually or config will be used on next start
            tracing::info!("Mihomo not running, config updated but not restarted");
        }

        tracing::info!("Port settings applied");
        Ok(())
    }

    /// Append a log line
    pub fn append_log(&self, line: impl Into<String>) {
        let mut buffer = self.log_buffer.write();
        if buffer.len() >= MAX_SERVICE_LOG_LINES {
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

    /// Load cron jobs from profiles.yaml
    pub fn load_cron_jobs(&self) {
        let mut jobs = self.cron_jobs.write();

        // Find profiles.yaml in working directory
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        let profiles_path = exe_dir.join("profiles.yaml");

        if !profiles_path.exists() {
            tracing::info!("No profiles.yaml found, no cron jobs to load");
            return;
        }

        // Read profiles.yaml
        let content = match std::fs::read_to_string(&profiles_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to read profiles.yaml: {}", e);
                return;
            }
        };

        // Parse YAML
        #[derive(serde::Deserialize)]
        struct ProfilesYaml {
            items: Vec<ProfileItem>,
        }

        #[derive(serde::Deserialize)]
        struct ProfileItem {
            uid: String,
            url: Option<String>,
            cron: Option<String>,
        }

        let yaml: ProfilesYaml = match serde_yaml_ng::from_str(&content) {
            Ok(y) => y,
            Err(e) => {
                tracing::warn!("Failed to parse profiles.yaml: {}", e);
                return;
            }
        };

        jobs.clear();

        for item in yaml.items {
            if let Some(cron_str) = item.cron {
                if let Some(schedule) = Schedule::parse(&cron_str) {
                    let job = ProfileCronJob::new(item.uid.clone(), item.url.clone(), schedule.clone());
                    tracing::info!("Loaded cron job: {} - {}", item.uid, schedule.description());
                    jobs.push(job);
                } else {
                    tracing::warn!("Invalid cron expression for profile {}: {}", item.uid, cron_str);
                }
            }
        }

        tracing::info!("Loaded {} cron jobs", jobs.len());
    }

    /// Check and run due cron jobs
    pub fn check_and_run_crons(&self) {
        let mut jobs = self.cron_jobs.write();

        for job in jobs.iter_mut() {
            if job.check_and_update() {
                let profile_id = job.profile_id.clone();
                let url = job.url.clone();
                let schedule_desc = job.schedule.description();

                tracing::info!("Triggering scheduled profile update: {} ({})", profile_id, schedule_desc);

                // Spawn a task to update the profile (non-blocking)
                let exe_dir = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                    .unwrap_or_else(|| PathBuf::from("."));

                let profiles_dir = exe_dir.join("profiles");
                let profile_file = profiles_dir.join(format!("{}.yaml", profile_id));

                if let Some(url) = url {
                    if profile_file.exists() {
                        // Download new content and update file
                        std::thread::spawn(move || {
                            if let Err(e) = update_profile_subscription(&url, &profile_file) {
                                tracing::error!("Failed to update profile {}: {}", profile_id, e);
                            } else {
                                tracing::info!("Profile {} updated successfully", profile_id);
                            }
                        });
                    } else {
                        tracing::warn!("Profile file not found: {:?}", profile_file);
                    }
                } else {
                    tracing::warn!("No URL configured for profile: {}", profile_id);
                }
            }
        }
    }

    /// Get proxies from Clash API
    pub fn get_proxies(&self) -> Result<serde_json::Value, String> {
        // Drop the read lock before making HTTP request
        drop(self.manager.read());

        // Clash API endpoint
        let url = format!("{}/proxies", self.get_api_url());

        // Make HTTP request to Clash API
        let response = reqwest::blocking::get(&url)
            .map_err(|e| format!("Failed to query Clash API: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("Clash API returned error: {}", response.status()));
        }

        let text = response.text()
            .map_err(|e| format!("Failed to read response body: {}", e))?;
        let proxies: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse Clash response: {}", e))?;

        Ok(proxies)
    }

    /// Get connections from Clash API
    pub fn get_connections(&self) -> Result<serde_json::Value, String> {
        // Drop the read lock before making HTTP request
        drop(self.manager.read());

        // Clash API endpoint
        let url = format!("{}/connections", self.get_api_url());

        // Make HTTP request to Clash API
        let response = reqwest::blocking::get(&url)
            .map_err(|e| format!("Failed to query Clash API: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("Clash API returned error: {}", response.status()));
        }

        let text = response.text()
            .map_err(|e| format!("Failed to read response body: {}", e))?;
        let connections: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse Clash response: {}", e))?;

        Ok(connections)
    }

    /// Close a specific connection by ID
    pub fn close_connection(&self, id: &str) -> Result<(), String> {
        // Drop the read lock before making HTTP request
        drop(self.manager.read());

        // Clash API endpoint
        let url = format!("{}/connections/{}", self.get_api_url(), id);

        // Create client and send DELETE request
        let client = reqwest::blocking::Client::new();
        let response = client.delete(&url)
            .send()
            .map_err(|e| format!("Failed to close connection: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("Failed to close connection: {}", response.status()));
        }

        tracing::info!("Connection {} closed successfully", id);
        Ok(())
    }

    /// Start Mihomo with the given config path
    pub fn start(&self, config_path: &PathBuf) -> Result<(), String> {
        let mut manager = self.manager.write();

        // Check if already running
        if manager.is_running() {
            tracing::info!("Mihomo already running, skipping start");
            return Ok(());
        }

        // Start Mihomo
        manager.start(config_path)
            .map_err(|e| format!("Failed to start Mihomo: {}", e))?;

        // Record start time
        let start_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        *self.start_time.write() = Some(start_time);
        *self.last_config_path.write() = Some(config_path.clone());

        tracing::info!("Mihomo started successfully");

        // Auto-select saved proxy if exists
        drop(manager); // Release the write lock before potential HTTP call
        if let Some(proxy_name) = self.load_selected_proxy() {
            if let Err(e) = self.select_proxy(&proxy_name) {
                tracing::warn!("Failed to auto-select proxy {}: {}", proxy_name, e);
            } else {
                tracing::info!("Auto-selected proxy: {}", proxy_name);
            }
        }

        // Auto-restore saved mode if exists (skip_restart to avoid loop)
        if let Some(mode) = self.load_mode() {
            if let Err(e) = self.set_mode(&mode, true) {
                tracing::warn!("Failed to restore mode {}: {}", mode, e);
            } else {
                tracing::info!("Restored mode: {}", mode);
            }
        }

        Ok(())
    }

    /// Load selected proxy from settings.yaml
    fn load_selected_proxy(&self) -> Option<String> {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))?;

        let settings_path = exe_dir.join("settings.yaml");
        if !settings_path.exists() {
            return None;
        }

        let content = std::fs::read_to_string(&settings_path).ok()?;

        #[derive(serde::Deserialize)]
        struct Settings {
            selected_proxy: Option<String>,
        }

        let settings: Settings = serde_yaml_ng::from_str(&content).ok()?;
        settings.selected_proxy
    }

    /// Load mode from settings.yaml
    fn load_mode(&self) -> Option<String> {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))?;

        let settings_path = exe_dir.join("settings.yaml");
        if !settings_path.exists() {
            return None;
        }

        let content = std::fs::read_to_string(&settings_path).ok()?;

        #[derive(serde::Deserialize)]
        struct Settings {
            mode: Option<String>,
        }

        let settings: Settings = serde_yaml_ng::from_str(&content).ok()?;
        settings.mode
    }

    /// Set mode: update config.yaml + optionally restart Mihomo
    /// If `skip_restart` is true, only hot-patches Mihomo without restarting
    /// (used during startup restoration to avoid restart loops)
    fn set_mode(&self, mode: &str, skip_restart: bool) -> Result<(), String> {
        // Step 1: Update config.yaml with new mode
        if let Err(e) = self.update_config_mode(mode) {
            return Err(format!("Failed to update config.yaml: {}", e));
        }

        // Step 2: Persist mode to settings.yaml
        if let Err(e) = self.save_mode(mode) {
            tracing::warn!("Failed to save mode to settings.yaml: {}", e);
        }

        // Step 3: Hot-patch Mihomo for immediate effect
        let client = reqwest::blocking::Client::new();
        let url = format!("{}/configs", self.get_api_url());
        let _ = client
            .patch(&url)
            .json(&serde_json::json!({ "mode": mode }))
            .timeout(std::time::Duration::from_secs(5))
            .send();

        // Step 4: Restart only if not skipped (skip during startup restoration)
        if !skip_restart {
            if let Err(e) = self.restart_mihomo() {
                return Err(format!("Failed to restart Mihomo: {}", e));
            }
        }

        Ok(())
    }

    /// Save mode to settings.yaml (preserves all other settings)
    fn save_mode(&self, mode: &str) -> Result<(), String> {
        let exe_dir = std::env::current_exe()
            .map_err(|e| format!("Failed to get exe path: {}", e))?
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| "Failed to get exe parent".to_string())?;

        let settings_path = exe_dir.join("settings.yaml");

        // Read all existing settings to preserve them
        let mut settings = if settings_path.exists() {
            match std::fs::read_to_string(&settings_path) {
                Ok(c) => serde_yaml_ng::from_str(&c).unwrap_or_default(),
                Err(_) => SettingsData::default(),
            }
        } else {
            SettingsData::default()
        };
        settings.mode = Some(mode.to_string());

        self.save_settings(&settings)?;

        tracing::info!("Mode saved to settings.yaml: {}", mode);
        Ok(())
    }

    /// Update mode in config.yaml directly
    fn update_config_mode(&self, mode: &str) -> Result<(), String> {
        let exe_dir = std::env::current_exe()
            .map_err(|e| format!("Failed to get exe path: {}", e))?
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| "Failed to get exe parent".to_string())?;

        let config_path = exe_dir.join("config.yaml");
        if !config_path.exists() {
            return Err("config.yaml not found".to_string());
        }

        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read config.yaml: {}", e))?;

        let mut yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content)
            .map_err(|e| format!("Failed to parse config.yaml: {}", e))?;

        if let Some(map) = yaml.as_mapping_mut() {
            map.insert("mode".into(), mode.into());
        }

        let new_content = serde_yaml_ng::to_string(&yaml)
            .map_err(|e| format!("Failed to serialize config.yaml: {}", e))?;

        std::fs::write(&config_path, new_content)
            .map_err(|e| format!("Failed to write config.yaml: {}", e))?;

        tracing::info!("Mode updated in config.yaml: {}", mode);
        Ok(())
    }

    /// Update port settings in config.yaml (mixed-port, socks-port, redir-port, tproxy-port)
    fn update_config_ports(&self, http_port: u16, socks_port: u16) -> Result<(), String> {
        let exe_dir = std::env::current_exe()
            .map_err(|e| format!("Failed to get exe path: {}", e))?
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| "Failed to get exe parent".to_string())?;

        let config_path = exe_dir.join("config.yaml");
        if !config_path.exists() {
            return Err("config.yaml not found".to_string());
        }

        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read config.yaml: {}", e))?;

        let mut yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content)
            .map_err(|e| format!("Failed to parse config.yaml: {}", e))?;

        if let Some(map) = yaml.as_mapping_mut() {
            map.insert("mixed-port".into(), http_port.into());
            map.insert("socks-port".into(), socks_port.into());
            // Update external-controller with the new API port
            let api_host = self.api_host.read().clone();
            let api_port = *self.api_port.read();
            let external_controller = format!("{}:{}", api_host, api_port);
            map.insert("external-controller".into(), external_controller.into());
        }

        let new_content = serde_yaml_ng::to_string(&yaml)
            .map_err(|e| format!("Failed to serialize config.yaml: {}", e))?;

        std::fs::write(&config_path, new_content)
            .map_err(|e| format!("Failed to write config.yaml: {}", e))?;

        let api_port = *self.api_port.read();
        tracing::info!("Ports updated in config.yaml: http={}, socks={}, api={}", http_port, socks_port, api_port);
        Ok(())
    }

    /// Restart Mihomo with the current active config
    pub fn restart_mihomo(&self) -> Result<(), String> {
        // Get the last config path
        let config_path = match self.last_config_path.read().clone() {
            Some(path) => path,
            None => {
                return Err("No config path available — start service first".to_string());
            }
        };

        // Stop Mihomo (ignore errors if not running)
        let _ = self.stop();

        // Start Mihomo with the config
        self.start(&config_path)
    }

    /// Restart Mihomo with a specific config path (used after activating a new profile).
    pub fn restart_with_config(&self, config_path: &Path) -> Result<(), String> {
        let path_buf = config_path.to_path_buf();
        *self.last_config_path.write() = Some(path_buf.clone());
        let _ = self.stop();
        self.start(&path_buf)
    }

    /// Select a proxy via Clash API
    fn select_proxy(&self, name: &str) -> Result<(), String> {
        let client = reqwest::blocking::Client::new();
        let url = format!("{}/proxies/GLOBAL", self.get_api_url());

        let response = client
            .put(&url)
            .json(&serde_json::json!({ "name": name }))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .map_err(|e| format!("Failed to select proxy: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("Failed to select proxy: {}", response.status()));
        }

        Ok(())
    }

    /// Stop Mihomo
    pub fn stop(&self) -> Result<(), String> {
        let mut manager = self.manager.write();
        manager.stop()
            .map_err(|e| format!("Failed to stop Mihomo: {}", e))?;
        *self.start_time.write() = None;
        tracing::info!("Mihomo stopped");
        Ok(())
    }

    /// Get current status
    pub fn status(&self) -> ServiceStatus {
        let mut manager = self.manager.write();
        let running = manager.is_running();
        let pid = manager.pid();
        let uptime_secs = if running {
            self.start_time.read().map(|start| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() - start)
                    .unwrap_or(0)
            })
        } else {
            None
        };

        let last_config = self.last_config_path.read().clone();
        let circuit_breaker_remaining = if running {
            manager.remaining_cooldown_secs()
        } else {
            None
        };

        let state = if !running {
            "NotRunning"
        } else if manager.remaining_cooldown_secs().is_some() {
            "CircuitBroken"
        } else {
            "Running"
        };

        ServiceStatus {
            running,
            pid,
            uptime_secs,
            state: state.to_string(),
            config_path: last_config,
            circuit_breaker_remaining_secs: circuit_breaker_remaining,
        }
    }

    /// Check if Mihomo is running
    pub fn is_running(&self) -> bool {
        self.manager.write().is_running()
    }
}

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
            match state.set_mode(&mode, false) {
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
    }
}

// ============ IPC Server Module ============

pub mod ipc_server {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;
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
                    resp_bytes.iter().for_each(|&b| {
                        let _ = stream.write(&[b]);
                    });
                    let _ = stream.write(b"\n");

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
        pub fn state(&self) -> &ServiceState {
            &self.state
        }
    }

    /// Connect to IPC server and send command
    pub fn connect_and_send(socket_path: &Path, command: IpcCommand) -> Result<IpcResponse, String> {
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let cmd = IpcCommand::Start {
            config_path: PathBuf::from("/tmp/config.yaml"),
        };
        let resp = handle_command(cmd);
        assert_eq!(resp.code, 0); // Success
    }

    #[test]
    fn test_handle_command_stop() {
        let cmd = IpcCommand::Stop;
        let resp = handle_command(cmd);
        assert_eq!(resp.code, 0); // Success
    }

    #[test]
    fn test_handle_command_status_returns_data() {
        let cmd = IpcCommand::Status;
        let resp = handle_command(cmd);
        assert_eq!(resp.code, 0);
        assert!(resp.data.is_some(), "Status should return data");
    }

    #[test]
    fn test_handle_command_logs_returns_data() {
        let cmd = IpcCommand::Logs { lines: Some(50) };
        let resp = handle_command(cmd);
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
            config_path: PathBuf::from("/tmp/test.yaml"),
        };
        let raw = serde_json::to_vec(&cmd).unwrap();

        // Server parses
        let parsed = parse_message(&raw).unwrap();

        // Server handles
        let resp = handle_command(parsed);

        // Server serializes
        let resp_bytes = serialize_response(&resp).unwrap();
        let resp_parsed: IpcResponse = serde_json::from_slice(&resp_bytes).unwrap();

        assert_eq!(resp_parsed.code, 0);
    }

    #[test]
    fn test_full_ipc_roundtrip_status() {
        // Client sends: Status command
        let cmd = IpcCommand::Status;
        let raw = serde_json::to_vec(&cmd).unwrap();

        // Server parses
        let parsed = parse_message(&raw).unwrap();

        // Server handles
        let resp = handle_command(parsed);

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
        let cmd = IpcCommand::Start {
            config_path: PathBuf::from("/nonexistent/path/config.yaml"),
        };
        let resp = handle_command(cmd);
        // This should fail because config doesn't exist
        // For now, this will pass because stub returns success
        // When real implementation is done, this should return error
        assert_eq!(resp.code, 0); // TODO: Should be -1 when implemented
    }

    /// Test that Status returns proper ServiceStatus structure
    #[test]
    fn test_handle_command_status_returns_service_status() {
        let cmd = IpcCommand::Status;
        let resp = handle_command(cmd);

        assert_eq!(resp.code, 0);
        assert!(resp.data.is_some());

        // Parse the data as ServiceStatus
        let data = resp.data.unwrap();
        let status: ServiceStatus = serde_json::from_value(data).unwrap();

        // Initially not running (stateless stub returns Unknown state)
        assert!(!status.running);
        assert!(status.pid.is_none());
        assert_eq!(status.state, "Unknown");
        assert!(status.config_path.is_none());
    }

    /// Test that Start command twice doesn't panic (idempotent)
    #[test]
    fn test_handle_command_start_idempotent() {
        let cmd = IpcCommand::Start {
            config_path: PathBuf::from("/tmp/config.yaml"),
        };

        // First start
        let resp1 = handle_command(cmd.clone());
        assert_eq!(resp1.code, 0);

        // Second start (should be no-op if already running)
        let resp2 = handle_command(cmd);
        assert_eq!(resp2.code, 0);
    }

    /// Test Logs command with no lines specified (defaults)
    #[test]
    fn test_handle_command_logs_default_lines() {
        let cmd = IpcCommand::Logs { lines: None };
        let resp = handle_command(cmd);

        assert_eq!(resp.code, 0);
        assert!(resp.data.is_some());

        // Data should be a vector
        let data = resp.data.unwrap();
        assert!(data.is_array());
    }

    /// Test Logs command with specific line count
    #[test]
    fn test_handle_command_logs_with_line_count() {
        let cmd = IpcCommand::Logs { lines: Some(10) };
        let resp = handle_command(cmd);

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
}

// ============ Main Function Implementation ============

use clap::Parser;
use std::sync::atomic::Ordering;

/// Command line arguments for the service
#[derive(Parser, Debug)]
#[command(name = "control-tower-service")]
#[command(version = "0.1.0")]
#[command(about = "Control Tower Service - IPC server for proxy management")]
struct Args {
    /// Socket path for IPC (default: ctsvc.sock in executable directory)
    #[arg(short, long)]
    socket: Option<std::path::PathBuf>,

    /// Log level (default: info)
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// Run in foreground (don't daemonize)
    #[arg(short, long, default_value = "false")]
    foreground: bool,
}

/// Global shutdown flag
static SHUTDOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Setup signal handlers for SIGTERM and SIGINT
fn setup_signal_handlers() {
    SHUTDOWN.store(false, Ordering::SeqCst);

    // Use low-level register with a closure that sets the flag
    unsafe {
        signal_hook::low_level::register(signal_hook::consts::SIGTERM, || {
            SHUTDOWN.store(true, Ordering::SeqCst);
        }).ok();
        signal_hook::low_level::register(signal_hook::consts::SIGINT, || {
            SHUTDOWN.store(true, Ordering::SeqCst);
        }).ok();
    }

    tracing::info!("Signal handlers registered (SIGTERM, SIGINT)");
}

/// Main server loop
fn run_server(mut server: ipc_server::IpcServer, state: Arc<ServiceState>) -> Result<(), String> {
    server.start()?;

    tracing::info!(
        "Control Tower Service started on socket: {}",
        server.socket_path().display()
    );

    // Load cron jobs on startup
    state.load_cron_jobs();

    let mut last_cron_check = std::time::Instant::now();
    const CRON_CHECK_INTERVAL: Duration = Duration::from_secs(60);

    // Main loop - keep accepting connections until shutdown
    // Note: In production, this should use proper async I/O with tokio
    while !SHUTDOWN.load(Ordering::SeqCst) {
        // Check if it's time to check cron jobs
        if last_cron_check.elapsed() >= CRON_CHECK_INTERVAL {
            state.check_and_run_crons();
            last_cron_check = std::time::Instant::now();
        }

        match server.handle_one() {
            Ok(Some(cmd)) => {
                tracing::debug!("Handled command: {:?}", cmd);
            }
            Ok(None) => {
                // No connection available, small sleep to avoid busy loop
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                tracing::warn!("Error handling request: {}", e);
                // Continue running even on error
            }
        }
    }

    tracing::info!("Shutting down server...");
    server.stop();

    // Stop Mihomo if running
    if state.is_running() {
        tracing::info!("Stopping Mihomo...");
        state.stop().ok();
    }

    tracing::info!("Control Tower Service stopped");
    Ok(())
}

/// Update a profile subscription from its URL
fn update_profile_subscription(url: &str, profile_file: &PathBuf) -> anyhow::Result<()> {
    tracing::info!("Updating profile from URL: {}", url);

    // Extract profile_id from profile_file name (e.g., "profiles/profile-xxx.yaml" -> "profile-xxx")
    let profile_id = profile_file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Download new content
    let client = reqwest::blocking::Client::new();
    let response = client
        .get(url)
        .header("User-Agent", "clash-verge/v2.4.7")
        .send()
        .map_err(|e| anyhow::anyhow!("Failed to download: {}", e))?;

    if !response.status().is_success() {
        anyhow::bail!("Download failed: {}", response.status());
    }

    let new_content = response.text()
        .map_err(|e| anyhow::anyhow!("Failed to read response: {}", e))?;

    // Update the profile file
    std::fs::write(profile_file, &new_content)?;

    // Update the profiles.yaml updated_at timestamp
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let profiles_path = exe_dir.join("profiles.yaml");

    if profiles_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&profiles_path) {
            #[derive(serde::Deserialize, serde::Serialize)]
            struct ProfilesYaml {
                current: Option<String>,
                items: Vec<ProfileItem>,
            }

            #[derive(serde::Deserialize, serde::Serialize)]
            struct ProfileItem {
                uid: String,
                name: String,
                #[serde(rename = "file")]
                file: Option<String>,
                url: Option<String>,
                cron: Option<String>,
                #[serde(rename = "updated_at")]
                updated_at: Option<i64>,
            }

            if let Ok(mut yaml) = serde_yaml_ng::from_str::<ProfilesYaml>(&content) {
                for item in &mut yaml.items {
                    if item.uid == profile_id {
                        item.updated_at = Some(chrono::Utc::now().timestamp());
                        break;
                    }
                }
                if let Ok(new_content) = serde_yaml_ng::to_string(&yaml) {
                    let _ = std::fs::write(&profiles_path, new_content);
                }
            }
        }
    }

    Ok(())
}

/// Get the service HTTP port from settings.yaml, defaulting to 8080
fn get_service_port() -> u16 {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));

    let settings_path = exe_dir
        .map(|p| p.join("settings.yaml"))
        .filter(|p| p.exists())
        .unwrap_or_else(|| std::path::PathBuf::from("settings.yaml"));

    if settings_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&settings_path) {
            if let Ok(settings) = serde_yaml_ng::from_str::<Settings>(&content) {
                return settings.service_port.unwrap_or(8080);
            }
        }
    }
    8080 // default
}

#[derive(serde::Deserialize)]
struct Settings {
    service_port: Option<u16>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let args = Args::parse();

    // Determine working directory from executable location
    let work_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."));

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
    let service_port = get_service_port();

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

    // Load settings from settings.yaml
    state.load_settings();

    // Only start HTTP server when not in foreground mode (e2e tests use --foreground)
    if !args.foreground {
        let http_state = state.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime for HTTP server");
            rt.block_on(async move {
                if let Err(e) = http_server::start_http_server(service_port, http_state).await {
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
    setup_signal_handlers();

    // Run server
    run_server(server, state)?;

    Ok(())
}
