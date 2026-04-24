//! ServiceState extension methods — extracted from main.rs to reduce file size.
//!
//! This module contains the `impl ServiceState` block, `AutoTestState`,
//! `LatencyResult`, and the async latency test helpers.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::stream::{self, StreamExt};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use parking_lot::RwLock;
use reqwest::blocking::Client as BlockingClient;
use reqwest::Client as ReqwestClient;
use serde::Deserialize;

use crate::scheduler::{ProfileCronJob, Schedule};
use crate::{AutoTestState, LatencyResult, ServiceState};

/// Max log lines to keep in the ring buffer
const LOG_LINES: usize = 100;

// ============ impl ServiceState ============

impl ServiceState {
    pub fn new() -> Self {
        Self {
            manager: RwLock::new(control_tower_service_core::MihomoManager::new()),
            start_time: RwLock::new(None),
            log_buffer: RwLock::new(VecDeque::with_capacity(LOG_LINES)),
            cron_jobs: RwLock::new(Vec::new()),
            last_config_path: RwLock::new(None),
            api_host: RwLock::new("127.0.0.1".to_string()),
            api_port: RwLock::new(9090),
            auto_test: RwLock::new(AutoTestState::default()),
        }
    }

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

    /// Get the Mihomo API base URL
    pub fn get_api_url(&self) -> String {
        let host = self.api_host.read().clone();
        let port = *self.api_port.read();
        format!("http://{}:{}", host, port)
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
            api_port: Some(9090),
            http_port: Some(7890),
            socks_port: Some(7891),
            service_port: Some(8080),
            tun_enabled: Some(false),
            log_level: Some("info".to_string()),
            allow_lan: Some(true),
            ipv6: Some(true),
            tcp_concurrent: Some(false),
            mode: Some("rule".to_string()),
            latency_test_mode: Some("http".to_string()),
            auto_test: None,
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

    /// Hot-patch Mihomo via PATCH /configs for immediate effect.
    fn hot_patch_configs(&self, log_level: Option<&str>, allow_lan: Option<bool>,
                         ipv6: Option<bool>, tcp_concurrent: Option<bool>) {
        let client = match BlockingClient::builder()
            .timeout(Duration::from_secs(5))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to create HTTP client: {}", e);
                return;
            }
        };

        let url = format!("{}/configs", self.get_api_url());
        let mut body = serde_json::Map::new();

        if let Some(v) = log_level {
            body.insert("log-level".into(), v.into());
        }
        if let Some(v) = allow_lan {
            body.insert("allow-lan".into(), serde_json::json!(v));
        }
        if let Some(v) = ipv6 {
            body.insert("ipv6".into(), serde_json::json!(v));
        }
        if let Some(v) = tcp_concurrent {
            body.insert("tcp-concurrent".into(), serde_json::json!(v));
        }

        if body.is_empty() {
            return;
        }

        match client.patch(&url).json(&body).send() {
            Ok(resp) => {
                if resp.status().is_success() {
                    tracing::info!("Hot-patched Mihomo configs");
                } else {
                    tracing::warn!("PATCH /configs returned {}", resp.status());
                }
            }
            Err(e) => {
                tracing::warn!("PATCH /configs failed: {}", e);
            }
        }
    }

    /// Save settings to settings.yaml (preserving existing values for None fields)
    pub fn save_settings(&self, new_settings: &crate::SettingsData) -> Result<(), String> {
        let exe_dir = control_tower_service_core::exe_dir();
        let settings_path = exe_dir.join("settings.yaml");

        // Hot-patch runtime first for immediate effect
        self.hot_patch_configs(
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

        let yaml_str = serde_yaml_ng::to_string(&merged)
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
        self.update_config_ports(http_port, socks_port)?;

        let tun_enabled = self.load_tun_enabled();
        let current_settings = self.get_settings();
        let settings = crate::SettingsData {
            api_host: Some("127.0.0.1".to_string()),
            api_port: Some(*self.api_port.read()),
            http_port: Some(http_port),
            socks_port: Some(socks_port),
            service_port: Some(8080),
            tun_enabled,
            log_level: None,
            allow_lan: None,
            ipv6: None,
            tcp_concurrent: None,
            mode: None,
            latency_test_mode: current_settings.latency_test_mode,
            auto_test: None,
        };
        self.save_settings(&settings)?;

        let config_path = self.last_config_path.read().clone();
        if config_path.is_some() {
            self.restart_mihomo()?;
        } else {
            tracing::info!("Mihomo not running, config updated but not restarted");
        }

        tracing::info!("Port settings applied");
        Ok(())
    }

    fn load_tun_enabled(&self) -> Option<bool> {
        let exe_dir = control_tower_service_core::exe_dir();
        let settings_path = exe_dir.join("settings.yaml");
        if let Ok(content) = std::fs::read_to_string(&settings_path) {
            #[derive(Deserialize)]
            struct Settings {
                #[serde(rename = "tun_enabled", default)]
                tun_enabled: Option<bool>,
            }
            if let Ok(settings) = serde_yaml_ng::from_str::<Settings>(&content) {
                return settings.tun_enabled;
            }
        }
        None
    }

    fn load_http_port(&self) -> Option<u16> {
        let exe_dir = control_tower_service_core::exe_dir();
        let settings_path = exe_dir.join("settings.yaml");
        if let Ok(content) = std::fs::read_to_string(&settings_path) {
            #[derive(Deserialize)]
            struct Settings {
                #[serde(rename = "http_port", default)]
                http_port: Option<u16>,
            }
            if let Ok(settings) = serde_yaml_ng::from_str::<Settings>(&content) {
                return settings.http_port;
            }
        }
        None
    }

    fn load_socks_port(&self) -> Option<u16> {
        let exe_dir = control_tower_service_core::exe_dir();
        let settings_path = exe_dir.join("settings.yaml");
        if let Ok(content) = std::fs::read_to_string(&settings_path) {
            #[derive(Deserialize)]
            struct Settings {
                #[serde(rename = "socks_port", default)]
                socks_port: Option<u16>,
            }
            if let Ok(settings) = serde_yaml_ng::from_str::<Settings>(&content) {
                return settings.socks_port;
            }
        }
        None
    }

    /// Apply TUN settings: update config.yaml tun section and settings.yaml tun_enabled
    pub fn apply_tun_settings(&self, tun_enabled: bool) -> Result<(), String> {
        self.update_config_tun(tun_enabled)?;

        let http_port = self.load_http_port();
        let socks_port = self.load_socks_port();
        let current_settings = self.get_settings();
        let settings = crate::SettingsData {
            api_host: Some("127.0.0.1".to_string()),
            api_port: Some(*self.api_port.read()),
            http_port,
            socks_port,
            service_port: Some(8080),
            tun_enabled: Some(tun_enabled),
            log_level: None,
            allow_lan: None,
            ipv6: None,
            tcp_concurrent: None,
            mode: current_settings.mode,
            latency_test_mode: current_settings.latency_test_mode,
            auto_test: current_settings.auto_test,
        };
        self.save_settings(&settings)?;

        let config_path = self.last_config_path.read().clone();
        if config_path.is_some() {
            self.restart_mihomo()?;
        } else {
            tracing::info!("Mihomo not running, tun config updated but not restarted");
        }

        tracing::info!("TUN settings applied: enabled={}", tun_enabled);
        Ok(())
    }

    /// Update tun section in config.yaml
    fn update_config_tun(&self, tun_enabled: bool) -> Result<(), String> {
        let exe_dir = control_tower_service_core::exe_dir();
        let config_path = exe_dir.join("config.yaml");
        if !config_path.exists() {
            return Err("config.yaml not found".to_string());
        }

        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read config.yaml: {}", e))?;

        let mut yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content)
            .map_err(|e| format!("Failed to parse config.yaml: {}", e))?;

        if tun_enabled {
            let mut tun_map = serde_yaml_ng::Mapping::new();
            tun_map.insert("enable".into(), true.into());
            tun_map.insert("stack".into(), "gvisor".into());
            tun_map.insert("name".into(), "mihomo".into());
            tun_map.insert("mtu".into(), (9000 as i64).into());
            tun_map.insert("auto-route".into(), true.into());
            tun_map.insert("auto-detect-interface".into(), true.into());
            let dns_hijack: Vec<serde_yaml_ng::Value> = vec!["udp://0.0.0.0:53".into()];
            tun_map.insert("dns-hijack".into(), dns_hijack.into());

            if let Some(map) = yaml.as_mapping_mut() {
                if !map.contains_key("dns") {
                    let mut dns_map = serde_yaml_ng::Mapping::new();
                    dns_map.insert("enable".into(), true.into());
                    dns_map.insert("listen".into(), "0.0.0.0:53".into());
                    dns_map.insert("enhanced-mode".into(), "fake-ip".into());
                    dns_map.insert("fake-ip-range".into(), "198.18.0.1/15".into());
                    dns_map.insert("default-nameserver".into(),
                        serde_yaml_ng::Sequence::from_iter(
                            ["223.5.5.5", "119.29.29.29", "114.114.114.114"]
                                .iter().map(|s| (*s).into())
                        ).into()
                    );
                    dns_map.insert("nameserver".into(),
                        serde_yaml_ng::Sequence::from_iter(
                            ["https://doh.pub/dns-query", "https://dns.alidns.com/dns-query"]
                                .iter().map(|s| (*s).into())
                        ).into()
                    );
                    dns_map.insert("fallback".into(),
                        serde_yaml_ng::Sequence::from_iter(
                            ["https://1.1.1.1/dns-query", "https://dns.google/dns-query"]
                                .iter().map(|s| (*s).into())
                        ).into()
                    );
                    map.insert("dns".into(), dns_map.into());
                } else {
                    if let Some(dns_val) = map.get_mut("dns") {
                        if let Some(dns_map) = dns_val.as_mapping_mut() {
                            dns_map.insert("enable".into(), true.into());
                        }
                    }
                }
                map.insert("tun".into(), tun_map.into());
            }
        } else {
            if let Some(map) = yaml.as_mapping_mut() {
                map.remove(&serde_yaml_ng::Value::String("tun".into()));
            }
        }

        let new_content = serde_yaml_ng::to_string(&yaml)
            .map_err(|e| format!("Failed to serialize config.yaml: {}", e))?;

        std::fs::write(&config_path, new_content)
            .map_err(|e| format!("Failed to write config.yaml: {}", e))?;

        Ok(())
    }

    /// Append a log line
    pub fn append_log(&self, line: impl Into<String>) {
        let mut buffer = self.log_buffer.write();
        if buffer.len() >= LOG_LINES {
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
        let exe_dir = control_tower_service_core::exe_dir();
        let profiles_path = exe_dir.join("profiles.yaml");

        if !profiles_path.exists() {
            tracing::info!("No profiles.yaml found, no cron jobs to load");
            return;
        }

        let content = match std::fs::read_to_string(&profiles_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to read profiles.yaml: {}", e);
                return;
            }
        };

        #[derive(Deserialize)]
        struct ProfilesYaml {
            items: Vec<ProfileItem>,
        }

        #[derive(Deserialize)]
        struct ProfileItem {
            uid: String,
            file: Option<String>,
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
                    let job = ProfileCronJob::new(item.uid.clone(), item.file.clone(), item.url.clone(), schedule.clone());
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

                let exe_dir = control_tower_service_core::exe_dir();
                let profiles_dir = exe_dir.join("profiles");
                let profile_file = match job.file.as_ref() {
                    Some(f) => profiles_dir.join(f),
                    None => profiles_dir.join(format!("{}.yaml", profile_id)),
                };

                if let Some(url) = url {
                    if profile_file.exists() {
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
        drop(self.manager.read());
        let url = format!("{}/proxies", self.get_api_url());
        let response = BlockingClient::new()
            .get(&url)
            .send()
            .map_err(|e| format!("Failed to query Clash API: {}", e))?;
        if !response.status().is_success() {
            return Err(format!("Clash API returned error: {}", response.status()));
        }
        let text = response.text().map_err(|e| format!("Failed to read response body: {}", e))?;
        let proxies: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse Clash response: {}", e))?;
        Ok(proxies)
    }

    /// Get configs from Clash API (includes current mode)
    pub fn get_configs(&self) -> Result<serde_json::Value, String> {
        drop(self.manager.read());
        let url = format!("{}/configs", self.get_api_url());
        let response = BlockingClient::new()
            .get(&url)
            .send()
            .map_err(|e| format!("Failed to query Clash API: {}", e))?;
        if !response.status().is_success() {
            return Err(format!("Clash API returned error: {}", response.status()));
        }
        let text = response.text().map_err(|e| format!("Failed to read response body: {}", e))?;
        let configs: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse Clash response: {}", e))?;
        Ok(configs)
    }

    /// Get connections from Clash API
    pub fn get_connections(&self) -> Result<serde_json::Value, String> {
        drop(self.manager.read());
        let url = format!("{}/connections", self.get_api_url());
        let response = BlockingClient::new()
            .get(&url)
            .send()
            .map_err(|e| format!("Failed to query Clash API: {}", e))?;
        if !response.status().is_success() {
            return Err(format!("Clash API returned error: {}", response.status()));
        }
        let text = response.text().map_err(|e| format!("Failed to read response body: {}", e))?;
        let connections: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse Clash response: {}", e))?;
        Ok(connections)
    }

    /// Close a specific connection by ID
    pub fn close_connection(&self, id: &str) -> Result<(), String> {
        drop(self.manager.read());
        let url = format!("{}/connections/{}", self.get_api_url(), id);
        let response = BlockingClient::new()
            .delete(&url)
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

        Ok(())
    }

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

    /// Set mode: update config.yaml + hot-patch Mihomo (no restart)
    pub(crate) fn set_mode(&self, mode: &str) -> Result<(), String> {
        if let Err(e) = self.update_config_mode(mode) {
            return Err(format!("Failed to update config.yaml: {}", e));
        }
        if let Err(e) = self.save_mode(mode) {
            tracing::warn!("Failed to save mode to settings.yaml: {}", e);
        }

        let client = BlockingClient::new();
        let url = format!("{}/configs", self.get_api_url());
        let resp = client
            .patch(&url)
            .json(&serde_json::json!({ "mode": mode }))
            .timeout(Duration::from_secs(5))
            .send()
            .map_err(|e| format!("PATCH /configs failed: {}", e))?;
        if !resp.status().is_success() {
            tracing::warn!("PATCH /configs returned {}", resp.status());
        }
        Ok(())
    }

    /// Save mode to settings.yaml (preserves all other settings)
    fn save_mode(&self, mode: &str) -> Result<(), String> {
        let exe_dir = control_tower_service_core::exe_dir();
        let settings_path = exe_dir.join("settings.yaml");

        let mut settings = if settings_path.exists() {
            match std::fs::read_to_string(&settings_path) {
                Ok(c) => serde_yaml_ng::from_str(&c).unwrap_or_default(),
                Err(_) => crate::SettingsData::default(),
            }
        } else {
            crate::SettingsData::default()
        };
        settings.mode = Some(mode.to_string());

        self.save_settings(&settings)?;
        tracing::info!("Mode saved to settings.yaml: {}", mode);
        Ok(())
    }

    /// Update mode in config.yaml directly
    fn update_config_mode(&self, mode: &str) -> Result<(), String> {
        let exe_dir = control_tower_service_core::exe_dir();
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

    /// Update port settings in config.yaml (mixed-port, socks-port, external-controller)
    fn update_config_ports(&self, http_port: u16, socks_port: u16) -> Result<(), String> {
        let exe_dir = control_tower_service_core::exe_dir();
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

    /// Select a proxy via Clash API
    pub(crate) fn select_proxy(&self, name: &str) -> Result<(), String> {
        let client = BlockingClient::new();
        let url = format!("{}/proxies/GLOBAL", self.get_api_url());

        let response = client
            .put(&url)
            .json(&serde_json::json!({ "name": name }))
            .timeout(Duration::from_secs(5))
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

    /// Run automatic latency test on all proxies.
    /// Returns the number of proxies tested.
    pub fn run_auto_latency_test(&self) -> usize {
        let settings = self.get_settings();
        let auto_test_cfg = match settings.auto_test {
            Some(cfg) => cfg,
            None => {
                tracing::debug!("Auto test config not found, skipping");
                return 0;
            }
        };

        if !auto_test_cfg.enabled {
            return 0;
        }

        {
            let mut st = self.auto_test.write();
            st.last_test_at = Some(chrono::Utc::now().timestamp());
        }

        if !self.is_running() {
            tracing::warn!("Mihomo not running, skipping auto latency test");
            return 0;
        }

        let api_url = self.get_api_url();

        let client = match BlockingClient::builder()
            .timeout(Duration::from_secs(15))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to create HTTP client: {}", e);
                return 0;
            }
        };

        let proxies_url = format!("{}/proxies", api_url);
        let proxies_response = match client.get(&proxies_url).send() {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Failed to fetch proxies for latency test: {}", e);
                return 0;
            }
        };

        if !proxies_response.status().is_success() {
            tracing::warn!("Failed to get proxies: {}", proxies_response.status());
            return 0;
        }

        let proxies_data: serde_json::Value = match proxies_response.json() {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("Failed to parse proxies response: {}", e);
                return 0;
            }
        };

        let global_all = match proxies_data
            .get("proxies")
            .and_then(|p| p.get("GLOBAL"))
            .and_then(|g| g.get("all"))
            .and_then(|a| a.as_array())
        {
            Some(a) => a,
            None => {
                tracing::warn!("GLOBAL.all not found in proxies response");
                return 0;
            }
        };

        let skip_count = 3;
        let all_names: Vec<String> = global_all
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();

        let nodes_to_test: Vec<&str> = all_names
            .iter()
            .skip(skip_count)
            .map(|s| s.as_str())
            .collect();

        if nodes_to_test.is_empty() {
            tracing::debug!("No proxy nodes to test");
            let mut state = self.auto_test.write();
            state.enabled = true;
            state.interval_secs = (auto_test_cfg.interval_minutes as u64) * 60;
            state.last_test_at = Some(chrono::Utc::now().timestamp());
            state.fastest = None;
            state.results.clear();
            return 0;
        }

        let timeout_ms = 8000;
        let latency_mode = auto_test_cfg.latency_test_mode.as_deref().unwrap_or("http");
        let total_count = nodes_to_test.len();
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                tracing::error!("Failed to create tokio runtime for latency test: {}", e);
                return 0;
            }
        };
        let results: Vec<LatencyResult> = rt.block_on(test_concurrent_latency(
            api_url,
            nodes_to_test,
            timeout_ms,
            latency_mode,
        ));

        let fastest_name = results.iter()
            .filter(|r| r.latency.is_some())
            .min_by_key(|r| r.latency.unwrap())
            .map(|r| r.name.clone())
            .unwrap_or_else(|| "N/A".to_string());

        let mut state = self.auto_test.write();
        state.enabled = true;
        state.interval_secs = (auto_test_cfg.interval_minutes as u64) * 60;
        state.last_test_at = Some(chrono::Utc::now().timestamp());
        state.fastest = results.iter()
            .filter(|r| r.latency.is_some())
            .min_by_key(|r| r.latency.unwrap())
            .cloned();
        state.results = results;

        tracing::info!("Auto latency test completed, tested {} nodes, fastest: {}",
            total_count, fastest_name);
        self.append_log(format!("Auto test: {} nodes tested, fastest={}", total_count, fastest_name));
        total_count
    }
}

// ============ Standalone async helpers ============

/// Test a single proxy node's latency asynchronously.
#[allow(dead_code)]
pub(crate) async fn test_node_latency(
    client: &ReqwestClient,
    api_url: &str,
    node_name: &str,
    timeout_ms: u64,
    latency_mode: &str,
) -> LatencyResult {
    let delay_url = if latency_mode == "http" {
        format!(
            "{}/proxies/{}/delay?url={}&timeout={}",
            api_url,
            utf8_percent_encode(node_name, NON_ALPHANUMERIC),
            "http%3A%2F%2Fcp.cloudflare.com%2Fgenerate_204",
            timeout_ms
        )
    } else {
        format!(
            "{}/proxies/{}/delay?timeout={}",
            api_url,
            utf8_percent_encode(node_name, NON_ALPHANUMERIC),
            timeout_ms
        )
    };

    match client.get(&delay_url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<serde_json::Value>().await {
                    Ok(data) => {
                        let delay = data.get("delay").and_then(|v| v.as_i64()).unwrap_or(-1);
                        if delay >= 0 {
                            LatencyResult {
                                name: node_name.to_string(),
                                latency: Some(delay),
                                error: None,
                            }
                        } else {
                            LatencyResult {
                                name: node_name.to_string(),
                                latency: None,
                                error: Some("Negative delay".into()),
                            }
                        }
                    }
                    Err(_) => LatencyResult {
                        name: node_name.to_string(),
                        latency: None,
                        error: Some("Parse error".into()),
                    },
                }
            } else {
                LatencyResult {
                    name: node_name.to_string(),
                    latency: None,
                    error: Some(format!("Mihomo error (HTTP {})", response.status().as_u16())),
                }
            }
        }
        Err(_) => LatencyResult {
            name: node_name.to_string(),
            latency: None,
            error: Some("Timeout".into()),
        },
    }
}

/// Run concurrent latency tests on all proxy nodes.
/// Semaphore limits concurrency to MAX_CONCURRENT (avoids overwhelming Mihomo).
pub(crate) async fn test_concurrent_latency(
    api_url: String,
    nodes_to_test: Vec<&str>,
    timeout_ms: u64,
    latency_mode: &str,
) -> Vec<LatencyResult> {
    const MAX_CONCURRENT: usize = 20;
    let sem = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT));

    let client = match ReqwestClient::builder()
        .timeout(Duration::from_millis(timeout_ms as u64 + 3000))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Failed to create async HTTP client: {}", e);
            return nodes_to_test
                .iter()
                .map(|name| LatencyResult {
                    name: (*name).to_string(),
                    latency: None,
                    error: Some("Client init failed".into()),
                })
                .collect();
        }
    };

    let api_url_for_task = api_url.clone();
    let latency_mode_for_task = latency_mode.to_string();
    let timeout_ms_for_task = timeout_ms;

    let results: Vec<LatencyResult> = stream::iter(nodes_to_test)
        .map(|node_name| {
            let sem = sem.clone();
            let client = client.clone();
            let api_url = api_url_for_task.clone();
            let latency_mode = latency_mode_for_task.clone();
            let node_name = node_name.to_string();
            async move {
                let _permit = sem.acquire().await.expect("semaphore not closed");
                test_node_latency(
                    &client,
                    &api_url,
                    &node_name,
                    timeout_ms_for_task,
                    &latency_mode,
                )
                .await
            }
        })
        .buffer_unordered(MAX_CONCURRENT)
        .collect()
        .await;

    results
}

/// Update a profile subscription (download new content and write to file).
/// Used by cron job auto-update.
pub(crate) fn update_profile_subscription(url: &str, profile_file: &std::path::Path) -> Result<(), String> {
    let response = BlockingClient::new()
        .get(url)
        .timeout(Duration::from_secs(60))
        .send()
        .map_err(|e| format!("Failed to fetch subscription: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Subscription fetch failed: {}", response.status()));
    }

    let new_content = response.text()
        .map_err(|e| format!("Failed to read subscription content: {}", e))?;

    // Basic validation: reject HTML responses (common for errors / CAPTCHAs)
    let trimmed = new_content.trim_start();
    if trimmed.starts_with('<') || trimmed.starts_with("<!") {
        return Err("Subscription returned HTML — likely a block page".to_string());
    }

    // Basic YAML structure check
    if !new_content.contains("proxies:")
       && !new_content.contains("proxy-providers:")
       && !new_content.contains("mixed-port:")
    {
        return Err("Subscription content does not look like a Clash config".to_string());
    }

    std::fs::write(profile_file, &new_content)
        .map_err(|e| format!("Failed to write profile file: {}", e))?;

    Ok(())
}