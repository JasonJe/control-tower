//! Control Tower Settings Module
//! Handles configuration loading from settings.yaml

use anyhow::Result;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;

/// Settings file name
const SETTINGS_FILE: &str = "settings.yaml";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Settings {
    /// Working directory for all configs (default: ~/.config/control-tower/)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<PathBuf>,

    /// Mihomo binary path (default: auto-detect)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mihomo_path: Option<PathBuf>,

    /// Mihomo API port (default: 9090)
    #[serde(default = "default_api_port")]
    pub api_port: u16,

    /// HTTP proxy port (default: 7890)
    #[serde(default = "default_http_port")]
    pub http_port: u16,

    /// SOCKS5 proxy port (default: 7891)
    #[serde(default = "default_socks_port")]
    pub socks_port: u16,

    /// Control Tower Service port (default: 8080)
    #[serde(default = "default_service_port")]
    pub service_port: u16,

    /// Enable TUN mode (default: false)
    #[serde(default)]
    pub tun_enabled: bool,

    /// Log level (default: info)
    #[serde(default = "default_log_level")]
    pub log_level: String,

    /// Currently selected proxy name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_proxy: Option<String>,

    /// Proxy mode (rule/global/direct)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

fn default_api_port() -> u16 { 9090 }
fn default_http_port() -> u16 { 7890 }
fn default_socks_port() -> u16 { 7891 }
fn default_service_port() -> u16 { 8080 }
fn default_log_level() -> String { "info".to_string() }

impl Default for Settings {
    fn default() -> Self {
        Self {
            working_dir: None,
            mihomo_path: None,
            api_port: default_api_port(),
            http_port: default_http_port(),
            socks_port: default_socks_port(),
            service_port: default_service_port(),
            tun_enabled: false,
            log_level: default_log_level(),
            selected_proxy: None,
            mode: None,
        }
    }
}

/// Custom config path set via --config flag
static CUSTOM_CONFIG_PATH: Lazy<RwLock<Option<PathBuf>>> = Lazy::new(|| RwLock::new(None));

/// Global settings instance
static SETTINGS: Lazy<RwLock<Settings>> = Lazy::new(|| {
    RwLock::new(load_settings().unwrap_or_default())
});

/// Get the executable's directory
fn get_exe_dir() -> Option<PathBuf> {
    env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
}

/// Get config directory (~/.config/control-tower)
fn get_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("control-tower"))
}

/// Set custom config path from --config flag
pub fn set_config_path(path: PathBuf) {
    if let Ok(mut guard) = CUSTOM_CONFIG_PATH.write() {
        *guard = Some(path);
    }
}

/// Find the settings file path
/// Priority: 1. Custom path from --config, 2. ./settings.yaml (next to executable), 3. ~/.config/control-tower/settings.yaml
fn find_settings_path() -> Option<PathBuf> {
    // First check custom path
    if let Ok(guard) = CUSTOM_CONFIG_PATH.read() {
        if let Some(ref path) = *guard {
            return Some(path.clone());
        }
    }

    // Then check executable directory
    if let Some(exe_dir) = get_exe_dir() {
        let path = exe_dir.join(SETTINGS_FILE);
        if path.exists() {
            return Some(path);
        }
    }

    // Then check ~/.config/control-tower/
    if let Some(config_dir) = get_config_dir() {
        let path = config_dir.join(SETTINGS_FILE);
        if path.exists() {
            return Some(path);
        }
    }

    // Return executable directory path for new file creation
    get_exe_dir().map(|p| p.join(SETTINGS_FILE))
}

/// Load settings from settings.yaml
pub fn load_settings() -> Result<Settings> {
    let settings_path = match find_settings_path() {
        Some(path) => path,
        None => return Ok(Settings::default()),
    };

    if !settings_path.exists() {
        return Ok(Settings::default());
    }

    let content = fs::read_to_string(&settings_path)?;
    let settings: Settings = serde_yaml_ng::from_str(&content)?;
    Ok(settings)
}

/// Reload settings from file
#[allow(dead_code)]
pub fn reload_settings() -> Result<()> {
    let settings = load_settings()?;
    let mut global = SETTINGS.write().map_err(|_| anyhow::anyhow!("Lock poisoned"))?;
    *global = settings;
    Ok(())
}

/// Get current API port
pub fn get_api_port() -> u16 {
    SETTINGS.read().map(|s| s.api_port).unwrap_or(9090)
}

/// Get current HTTP proxy port
#[allow(dead_code)]
pub fn get_http_port() -> u16 {
    SETTINGS.read().map(|s| s.http_port).unwrap_or(7890)
}

/// Get current SOCKS5 proxy port
#[allow(dead_code)]
pub fn get_socks_port() -> u16 {
    SETTINGS.read().map(|s| s.socks_port).unwrap_or(7891)
}

/// Get current Service port
#[allow(dead_code)]
pub fn get_service_port() -> u16 {
    SETTINGS.read().map(|s| s.service_port).unwrap_or(8080)
}

/// Get log level
#[allow(dead_code)]
pub fn get_log_level() -> String {
    SETTINGS.read().map(|s| s.log_level.clone()).unwrap_or_else(|_| "info".to_string())
}

/// Check if TUN is enabled
#[allow(dead_code)]
pub fn is_tun_enabled() -> bool {
    SETTINGS.read().map(|s| s.tun_enabled).unwrap_or(false)
}

/// Get currently selected proxy name
pub fn get_selected_proxy() -> Option<String> {
    SETTINGS.read().ok().and_then(|s| s.selected_proxy.clone())
}

/// Set currently selected proxy name
pub fn set_selected_proxy(name: String) {
    if let Ok(mut settings) = SETTINGS.write() {
        settings.selected_proxy = Some(name);
        // Try to save to file
        if let Some(path) = find_settings_path() {
            if let Ok(content) = serde_yaml_ng::to_string(&*settings) {
                let _ = fs::write(path, content);
            }
        }
    }
}

/// Get proxy mode (rule/global/direct)
pub fn get_mode() -> Option<String> {
    SETTINGS.read().ok().and_then(|s| s.mode.clone())
}

/// Set proxy mode and persist to settings.yaml
pub fn set_mode(mode: String) {
    if let Ok(mut settings) = SETTINGS.write() {
        settings.mode = Some(mode.clone());
        // Try to save to file
        if let Some(path) = find_settings_path() {
            if let Ok(content) = serde_yaml_ng::to_string(&*settings) {
                let _ = fs::write(path, content);
            }
        }
    }
}

/// Get working directory
pub fn get_working_dir() -> Option<PathBuf> {
    if let Ok(guard) = SETTINGS.read() {
        if let Some(ref dir) = guard.working_dir {
            return Some(dir.clone());
        }
    }
    // Default: use executable's directory
    get_exe_dir()
}

/// Get Mihomo path from settings
#[allow(dead_code)]
pub fn get_mihomo_path() -> Option<PathBuf> {
    SETTINGS.read().ok().and_then(|s| s.mihomo_path.clone())
}

/// Initialize settings on startup
pub fn init() {
    // Force the lazy initialization
    let _ = SETTINGS.read().map(|_| ());
}

/// Get the path where settings file should be created
pub fn get_settings_path() -> Option<PathBuf> {
    // If custom path is set, use it
    if let Ok(guard) = CUSTOM_CONFIG_PATH.read() {
        if let Some(ref path) = *guard {
            return Some(path.clone());
        }
    }

    find_settings_path()
}

/// Get the current settings path (same as get_settings_path)
pub fn current_settings_path() -> Option<PathBuf> {
    find_settings_path()
}

/// Build shared ControlTowerPaths from the current settings location.
pub fn shared_paths() -> anyhow::Result<control_tower_service_core::ControlTowerPaths> {
    let settings_path = current_settings_path()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine settings path"))?;
    Ok(control_tower_service_core::ControlTowerPaths::from_settings(
        settings_path,
        get_working_dir(),
    ))
}

/// Create default settings.yaml
pub fn create_default_settings() -> Result<PathBuf> {
    // Use custom path if set
    if let Ok(guard) = CUSTOM_CONFIG_PATH.read() {
        if let Some(ref path) = *guard {
            if !path.exists() {
                let settings = Settings::default();
                let content = serde_yaml_ng::to_string(&settings)?;
                fs::write(path, content)?;
            }
            return Ok(path.clone());
        }
    }

    // Try executable directory first
    if let Some(exe_dir) = get_exe_dir() {
        let settings_path = exe_dir.join(SETTINGS_FILE);
        if !settings_path.exists() {
            let settings = Settings::default();
            let content = serde_yaml_ng::to_string(&settings)?;
            fs::write(&settings_path, content)?;
        }
        return Ok(settings_path);
    }

    anyhow::bail!("Cannot find suitable location for settings file")
}
