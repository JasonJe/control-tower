//! Service lifecycle: start / stop / restart / status

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::ServiceState;

impl ServiceState {
    /// Start Mihomo with the given config path
    pub fn start(&self, config_path: &PathBuf) -> Result<(), String> {
        let mut manager = self.manager.write();
        if manager.is_running() {
            tracing::info!("Mihomo already running, skipping start");
            return Ok(());
        }

        manager.start(config_path)
            .map_err(|e| format!("Failed to start Mihomo: {}", e))?;

        let start_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        *self.start_time.write() = Some(start_time);
        *self.last_config_path.write() = Some(config_path.clone());

        tracing::info!("Mihomo started successfully");

        drop(manager);
        if let Some(proxy_name) = self.load_selected_proxy() {
            if let Err(e) = self.select_proxy(&proxy_name) {
                tracing::warn!("Failed to auto-select proxy {}: {}", proxy_name, e);
            } else {
                tracing::info!("Auto-selected proxy: {}", proxy_name);
            }
        }

        if let Some(mode) = self.load_mode() {
            if let Err(e) = self.set_mode(&mode) {
                tracing::warn!("Failed to restore mode {}: {}", mode, e);
            } else {
                tracing::info!("Restored mode: {}", mode);
            }
        }

        // Ensure allow-lan=true at runtime so proxy ports bind to all interfaces.
        // We must read allow-lan from config.yaml (active config), not settings.yaml,
        // because settings.yaml may have stale allow_lan values from before the
        // profile was activated, which would override the profile's own setting.
        let allow_lan = {
            let exe_dir = control_tower_service_core::exe_dir();
            let config_path = exe_dir.join("config.yaml");
            if let Ok(content) = std::fs::read_to_string(&config_path) {
                #[derive(Deserialize)]
                struct Config {
                    #[serde(rename = "allow-lan", default)]
                    allow_lan: Option<bool>,
                }
                serde_yaml_ng::from_str::<Config>(&content)
                    .ok()
                    .and_then(|c| c.allow_lan)
            } else {
                None
            }
        };

        if let Some(allow_lan_val) = allow_lan {
            self.hot_patch_runtime(None, Some(allow_lan_val), None, None);
            tracing::info!("Hot-patched runtime allow-lan={} from config.yaml", allow_lan_val);
        } else {
            // Default to allow-lan=true for external proxy access
            self.hot_patch_runtime(None, Some(true), None, None);
            tracing::info!("Hot-patched runtime allow-lan=true (default)");
        }

        Ok(())
    }

    /// Load selected proxy name from settings.yaml
    fn load_selected_proxy(&self) -> Option<String> {
        let exe_dir = control_tower_service_core::exe_dir();
        let settings_path = exe_dir.join("settings.yaml");
        if !settings_path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(&settings_path).ok()?;

        #[derive(Deserialize)]
        struct Settings {
            selected_proxy: Option<String>,
        }

        let settings: Settings = serde_yaml_ng::from_str(&content).ok()?;
        settings.selected_proxy
    }

    /// Load mode from settings.yaml
    fn load_mode(&self) -> Option<String> {
        let exe_dir = control_tower_service_core::exe_dir();
        let settings_path = exe_dir.join("settings.yaml");
        if !settings_path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(&settings_path).ok()?;

        #[derive(Deserialize)]
        struct Settings {
            mode: Option<String>,
        }

        let settings: Settings = serde_yaml_ng::from_str(&content).ok()?;
        settings.mode
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
    pub fn status(&self) -> crate::ServiceStatus {
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

        crate::ServiceStatus {
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

    /// Restart Mihomo with the current active config
    pub fn restart_mihomo(&self) -> Result<(), String> {
        let config_path = match self.last_config_path.read().clone() {
            Some(path) => path,
            None => {
                return Err("No config path available — start service first".to_string());
            }
        };

        let _ = self.stop();
        self.start(&config_path)
    }

    /// Restart Mihomo with a specific config path (used after activating a new profile).
    pub fn restart_with_config(&self, config_path: &Path) -> Result<(), String> {
        let path_buf = config_path.to_path_buf();
        *self.last_config_path.write() = Some(path_buf.clone());
        let _ = self.stop();
        self.start(&path_buf)
    }
}
