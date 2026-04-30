//! Settings management: load_settings / get_settings / save_settings / get_latency_test_mode

use serde::Deserialize;

use crate::ServiceState;
use crate::settings::consts::{
    DEFAULT_MIHOMO_API_PORT, DEFAULT_MIHOMO_HTTP_PORT,
    DEFAULT_MIHOMO_SOCKS_PORT, DEFAULT_SERVICE_PORT,
};

impl ServiceState {
    /// Load api_host and api_port from settings.yaml, also sync AutoTestState
    pub fn load_settings(&self) {
        let exe_dir = control_tower_service_core::exe_dir();
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

        #[derive(Deserialize)]
        struct Settings {
            api_host: Option<String>,
            api_port: Option<u16>,
            auto_test: Option<AutoTestYaml>,
        }

        #[derive(Deserialize)]
        struct AutoTestYaml {
            enabled: Option<bool>,
            interval_minutes: Option<u32>,
        }

        match serde_yaml_ng::from_str::<Settings>(&content) {
            Ok(settings) => {
                if let Some(host) = settings.api_host {
                    *self.api_host.write() = host;
                }
                if let Some(port) = settings.api_port {
                    *self.api_port.write() = port;
                }
                if let Some(ref auto_cfg) = settings.auto_test {
                    let mut auto_test = self.auto_test.write();
                    auto_test.enabled = auto_cfg.enabled.unwrap_or(false);
                    auto_test.interval_secs = (auto_cfg.interval_minutes.unwrap_or(5) as u64) * 60;
                }
                tracing::info!("Loaded settings: api_host={}, api_port={}, auto_test.enabled={}",
                    self.api_host.read(), self.api_port.read(), self.auto_test.read().enabled);
            }
            Err(e) => {
                tracing::warn!("Failed to parse settings.yaml: {}", e);
            }
        }
    }

    /// Ensure settings.yaml exists with default values, create if missing
    pub fn ensure_settings_file() {
        let exe_dir = control_tower_service_core::exe_dir();
        let settings_path = exe_dir.join("settings.yaml");

        if settings_path.exists() {
            return;
        }

        let default_settings = crate::SettingsData {
            api_host: Some("127.0.0.1".to_string()),
            api_port: Some(DEFAULT_MIHOMO_API_PORT),
            http_port: Some(DEFAULT_MIHOMO_HTTP_PORT),
            socks_port: Some(DEFAULT_MIHOMO_SOCKS_PORT),
            mixed_port: None,
            service_port: Some(DEFAULT_SERVICE_PORT),
            tun_enabled: Some(false),
            log_level: Some("info".to_string()),
            allow_lan: Some(true),
            ipv6: Some(true),
            tcp_concurrent: Some(false),
            mode: Some("rule".to_string()),
            latency_test_mode: Some("http".to_string()),
            auto_test: None,
            custom_rules: None,
            profile_rules_count: None,
            auto_update_on_startup: None,
            rule_providers: None,
            dns: None,
            connection_history: None,
            closed_connections: vec![],
            https: None,
            auth: None,
        };

        let yaml = serde_yaml_ng::to_string(&default_settings)
            .unwrap_or_else(|e| {
                tracing::error!("Serialized settings failed: {}", e);
                format!("api_host: 127.0.0.1\napi_port: {DEFAULT_MIHOMO_API_PORT}\nhttp_port: {DEFAULT_MIHOMO_HTTP_PORT}\nsocks_port: {DEFAULT_MIHOMO_SOCKS_PORT}\nservice_port: {DEFAULT_SERVICE_PORT}\ntun_enabled: false\nlog_level: info\nmode: rule\n")
            });

        if let Err(e) = std::fs::write(&settings_path, yaml) {
            tracing::error!("Failed to create default settings.yaml: {}", e);
        } else {
            tracing::info!("Created default settings.yaml");
        }
    }

    /// Get all settings from settings.yaml
    pub fn get_settings(&self) -> crate::SettingsData {
        let exe_dir = control_tower_service_core::exe_dir();
        let settings_path = exe_dir.join("settings.yaml");
        if !settings_path.exists() {
            return crate::SettingsData::default();
        }

        let content = match std::fs::read_to_string(&settings_path) {
            Ok(c) => c,
            Err(_) => return crate::SettingsData::default(),
        };

        serde_yaml_ng::from_str(&content).unwrap_or_default()
    }

    /// Get latency test mode from settings (default: "http")
    pub fn get_latency_test_mode(&self) -> String {
        self.get_settings()
            .latency_test_mode
            .unwrap_or_else(|| "http".to_string())
    }

    /// Save settings to settings.yaml (preserving existing values for None fields)
    pub fn save_settings(&self, new_settings: &crate::SettingsData) -> Result<(), String> {
        let exe_dir = control_tower_service_core::exe_dir();
        let settings_path = exe_dir.join("settings.yaml");

        // Hot-patch runtime first for immediate effect
        self.hot_patch_runtime(
            new_settings.log_level.as_deref(),
            new_settings.allow_lan,
            new_settings.ipv6,
            new_settings.tcp_concurrent,
        );

        // Merge with existing settings to preserve values for None fields
        let mut merged = if settings_path.exists() {
            match std::fs::read_to_string(&settings_path) {
                Ok(c) => serde_yaml_ng::from_str::<crate::SettingsData>(&c).unwrap_or_default(),
                Err(_) => crate::SettingsData::default(),
            }
        } else {
            crate::SettingsData::default()
        };

        if new_settings.api_host.is_some() { merged.api_host = new_settings.api_host.clone(); }
        if new_settings.api_port.is_some() { merged.api_port = new_settings.api_port; }
        if new_settings.http_port.is_some() { merged.http_port = new_settings.http_port; }
        if new_settings.socks_port.is_some() { merged.socks_port = new_settings.socks_port; }
        if new_settings.service_port.is_some() { merged.service_port = new_settings.service_port; }
        if new_settings.tun_enabled.is_some() { merged.tun_enabled = new_settings.tun_enabled; }
        if new_settings.log_level.is_some() { merged.log_level = new_settings.log_level.clone(); }
        if new_settings.allow_lan.is_some() { merged.allow_lan = new_settings.allow_lan; }
        if new_settings.ipv6.is_some() { merged.ipv6 = new_settings.ipv6; }
        if new_settings.tcp_concurrent.is_some() { merged.tcp_concurrent = new_settings.tcp_concurrent; }
        if new_settings.mode.is_some() { merged.mode = new_settings.mode.clone(); }
        if new_settings.latency_test_mode.is_some() { merged.latency_test_mode = new_settings.latency_test_mode.clone(); }
        if new_settings.auto_test.is_some() { merged.auto_test = new_settings.auto_test.clone(); }
        if new_settings.custom_rules.is_some() { merged.custom_rules = new_settings.custom_rules.clone(); }
        if new_settings.profile_rules_count.is_some() { merged.profile_rules_count = new_settings.profile_rules_count; }
        if new_settings.auto_update_on_startup.is_some() { merged.auto_update_on_startup = new_settings.auto_update_on_startup; }
        if new_settings.rule_providers.is_some() { merged.rule_providers = new_settings.rule_providers.clone(); }
        if new_settings.dns.is_some() { merged.dns = new_settings.dns.clone(); }
        if new_settings.connection_history.is_some() { merged.connection_history = new_settings.connection_history.clone(); }
        if new_settings.auth.is_some() { merged.auth = new_settings.auth.clone(); }
        // closed_connections: always use new value if provided, otherwise keep existing
        if !new_settings.closed_connections.is_empty() {
            merged.closed_connections = new_settings.closed_connections.clone();
        }

        let yaml_str = serde_yaml_ng::to_string(&merged)
            .map_err(|e| format!("Failed to serialize settings: {}", e))?;

        std::fs::write(&settings_path, yaml_str)
            .map_err(|e| format!("Failed to write settings.yaml: {}", e))?;

        // Reload settings into state
        self.load_settings();

        tracing::info!("Settings saved to settings.yaml");
        Ok(())
    }

    /// Record a closed connection to history in connection_history.yaml
    pub fn record_closed_connection(&self, closed: crate::settings::ClosedConnection) -> Result<(), String> {
        let exe_dir = control_tower_service_core::exe_dir();
        let history_path = exe_dir.join("connection_history.yaml");

        // Load existing history from separate file
        let mut connections: Vec<crate::settings::ClosedConnection> = if history_path.exists() {
            match std::fs::read_to_string(&history_path) {
                Ok(c) => serde_yaml_ng::from_str(&c).unwrap_or_default(),
                Err(_) => vec![],
            }
        } else {
            vec![]
        };

        // Get max_count from connection_history config (in settings.yaml)
        let settings_path = exe_dir.join("settings.yaml");
        let max_count = if settings_path.exists() {
            match std::fs::read_to_string(&settings_path) {
                Ok(c) => {
                    #[derive(serde::Deserialize)]
                    struct ConnHistoryConfig { connection_history: Option<crate::settings::ConnectionHistoryConfig> }
                    match serde_yaml_ng::from_str::<ConnHistoryConfig>(&c) {
                        Ok(cfg) => cfg.connection_history.map(|c| c.max_count as usize).unwrap_or(500),
                        Err(_) => 500,
                    }
                }
                Err(_) => 500,
            }
        } else {
            500
        };

        // Prepend new closed connection
        connections.insert(0, closed);

        // Trim to max_count
        if connections.len() > max_count {
            connections.truncate(max_count);
        }

        let yaml_str = serde_yaml_ng::to_string(&connections)
            .map_err(|e| format!("Failed to serialize history: {}", e))?;

        std::fs::write(&history_path, yaml_str)
            .map_err(|e| format!("Failed to write connection_history.yaml: {}", e))?;

        tracing::debug!("Recorded closed connection to history");
        Ok(())
    }

    /// Get connection history from connection_history.yaml
    pub fn get_connection_history(&self) -> Vec<crate::settings::ClosedConnection> {
        let exe_dir = control_tower_service_core::exe_dir();
        let history_path = exe_dir.join("connection_history.yaml");
        if !history_path.exists() {
            return vec![];
        }
        match std::fs::read_to_string(&history_path) {
            Ok(c) => serde_yaml_ng::from_str(&c).unwrap_or_default(),
            Err(_) => vec![],
        }
    }

    /// Clear connection history file
    pub fn clear_connection_history_file(&self) -> Result<(), String> {
        let exe_dir = control_tower_service_core::exe_dir();
        let history_path = exe_dir.join("connection_history.yaml");
        if history_path.exists() {
            std::fs::remove_file(&history_path)
                .map_err(|e| format!("Failed to remove connection_history.yaml: {}", e))?;
        }
        Ok(())
    }
}
