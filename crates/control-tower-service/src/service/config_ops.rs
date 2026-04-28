//! Config file operations: mode / ports / TUN settings, hot-patch, and YAML updates.

use std::time::Duration;
use serde::Deserialize;
use reqwest::blocking::Client as BlockingClient;

use crate::ServiceState;
use crate::settings::consts::DEFAULT_SERVICE_PORT;

/// Hot-patch Mihomo via PATCH /configs for immediate effect (runtime-only, no file write).
fn hot_patch_configs(api_url: &str, log_level: Option<&str>, allow_lan: Option<bool>,
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

    let url = format!("{}/configs", api_url);
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

impl ServiceState {
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

    /// Apply port settings: save to settings.yaml, update config.yaml, restart Mihomo
    pub fn apply_port_settings(&self, http_port: u16, socks_port: u16) -> Result<(), String> {
        self.update_config_ports(http_port, socks_port)?;

        let (_, _, tun_enabled) = self.load_settings_ports();
        let current_settings = self.get_settings();
        let settings = crate::SettingsData {
            api_host: Some("127.0.0.1".to_string()),
            api_port: Some(*self.api_port.read()),
            http_port: Some(http_port),
            socks_port: Some(socks_port),
            mixed_port: current_settings.mixed_port,
            service_port: Some(DEFAULT_SERVICE_PORT),
            tun_enabled,
            log_level: None,
            allow_lan: None,
            ipv6: None,
            tcp_concurrent: None,
            mode: None,
            latency_test_mode: current_settings.latency_test_mode,
            auto_test: None,
            custom_rules: current_settings.custom_rules,
            profile_rules_count: current_settings.profile_rules_count,
            auto_update_on_startup: current_settings.auto_update_on_startup,
            rule_providers: current_settings.rule_providers.clone(),
            dns: current_settings.dns.clone(),
            connection_history: current_settings.connection_history.clone(),
            closed_connections: current_settings.closed_connections.clone(),
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

    /// Load http_port, socks_port, and tun_enabled from settings.yaml in a single read.
    fn load_settings_ports(&self) -> (Option<u16>, Option<u16>, Option<bool>) {
        let exe_dir = control_tower_service_core::exe_dir();
        let settings_path = exe_dir.join("settings.yaml");
        if let Ok(content) = std::fs::read_to_string(&settings_path) {
            #[derive(Deserialize)]
            struct Settings {
                #[serde(rename = "http_port", default)]
                http_port: Option<u16>,
                #[serde(rename = "socks_port", default)]
                socks_port: Option<u16>,
                #[serde(rename = "tun_enabled", default)]
                tun_enabled: Option<bool>,
            }
            if let Ok(settings) = serde_yaml_ng::from_str::<Settings>(&content) {
                return (settings.http_port, settings.socks_port, settings.tun_enabled);
            }
        }
        (None, None, None)
    }

    /// Update tun section in config.yaml (does NOT touch dns section)
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

        if let Some(map) = yaml.as_mapping_mut() {
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
                map.insert("tun".into(), tun_map.into());
            } else {
                map.remove(&serde_yaml_ng::Value::String("tun".into()));
            }
        }

        let new_content = serde_yaml_ng::to_string(&yaml)
            .map_err(|e| format!("Failed to serialize config.yaml: {}", e))?;

        std::fs::write(&config_path, new_content)
            .map_err(|e| format!("Failed to write config.yaml: {}", e))?;

        Ok(())
    }

    /// Apply DNS settings: write dns section to config.yaml
    pub fn apply_dns_settings(&self, dns_settings: &crate::settings::DnsSettings) -> Result<(), String> {
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
            if dns_settings.enable {
                let mut dns_map = serde_yaml_ng::Mapping::new();
                dns_map.insert("enable".into(), true.into());
                dns_map.insert("listen".into(), "0.0.0.0:53".into());
                dns_map.insert("enhanced-mode".into(), dns_settings.enhanced_mode.clone().into());
                dns_map.insert("fake-ip-range".into(), dns_settings.fake_ip_range.clone().into());
                dns_map.insert("default-nameserver".into(),
                    serde_yaml_ng::Sequence::from_iter(
                        ["223.5.5.5", "119.29.29.29", "114.114.114.114"]
                            .iter().map(|s| (*s).into())
                    ).into()
                );
                if !dns_settings.nameserver.is_empty() {
                    dns_map.insert("nameserver".into(),
                        serde_yaml_ng::Sequence::from_iter(
                            dns_settings.nameserver.iter().map(|s| s.clone().into())
                        ).into()
                    );
                }
                if !dns_settings.fallback.is_empty() {
                    dns_map.insert("fallback".into(),
                        serde_yaml_ng::Sequence::from_iter(
                            dns_settings.fallback.iter().map(|s| s.clone().into())
                        ).into()
                    );
                }
                if let Some(ref ff) = dns_settings.fallback_filter {
                    let mut ff_map = serde_yaml_ng::Mapping::new();
                    ff_map.insert("geoip".into(), ff.geoip.into());
                    if let Some(ref code) = ff.geoip_code {
                        ff_map.insert("geoip-code".into(), code.clone().into());
                    }
                    if !ff.ipcidr.is_empty() {
                        ff_map.insert("ipcidr".into(),
                            serde_yaml_ng::Sequence::from_iter(
                                ff.ipcidr.iter().map(|s| s.clone().into())
                            ).into()
                        );
                    }
                    dns_map.insert("fallback-filter".into(), ff_map.into());
                }
                if !dns_settings.hosts.is_empty() {
                    let mut hosts_map = serde_yaml_ng::Mapping::new();
                    for h in &dns_settings.hosts {
                        hosts_map.insert(h.host.clone().into(), h.ip.clone().into());
                    }
                    dns_map.insert("hosts".into(), hosts_map.into());
                }
                if !dns_settings.nameserver_policy.is_empty() {
                    let mut nsp_map = serde_yaml_ng::Mapping::new();
                    for nsp in &dns_settings.nameserver_policy {
                        nsp_map.insert(nsp.match_domain.clone().into(),
                            serde_yaml_ng::Sequence::from_iter(
                                nsp.nameserver.iter().map(|s| s.clone().into())
                            ).into()
                        );
                    }
                    dns_map.insert("nameserver-policy".into(), nsp_map.into());
                }
                map.insert("dns".into(), dns_map.into());
            } else {
                map.remove(&serde_yaml_ng::Value::String("dns".into()));
            }
        }

        let new_content = serde_yaml_ng::to_string(&yaml)
            .map_err(|e| format!("Failed to serialize config.yaml: {}", e))?;

        std::fs::write(&config_path, new_content)
            .map_err(|e| format!("Failed to write config.yaml: {}", e))?;

        Ok(())
    }

    /// Apply TUN settings: update config.yaml tun section and settings.yaml tun_enabled
    pub fn apply_tun_settings(&self, tun_enabled: bool) -> Result<(), String> {
        self.update_config_tun(tun_enabled)?;

        let (http_port, socks_port, _) = self.load_settings_ports();
        let current_settings = self.get_settings();
        let settings = crate::SettingsData {
            api_host: Some("127.0.0.1".to_string()),
            api_port: Some(*self.api_port.read()),
            http_port,
            socks_port,
            mixed_port: current_settings.mixed_port,
            service_port: Some(DEFAULT_SERVICE_PORT),
            tun_enabled: Some(tun_enabled),
            log_level: None,
            allow_lan: None,
            ipv6: None,
            tcp_concurrent: None,
            mode: current_settings.mode,
            latency_test_mode: current_settings.latency_test_mode,
            auto_test: current_settings.auto_test,
            custom_rules: current_settings.custom_rules,
            profile_rules_count: current_settings.profile_rules_count,
            auto_update_on_startup: current_settings.auto_update_on_startup,
            rule_providers: current_settings.rule_providers.clone(),
            dns: current_settings.dns.clone(),
            connection_history: current_settings.connection_history.clone(),
            closed_connections: current_settings.closed_connections.clone(),
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

    /// Hot-patch runtime settings (log-level, allow-lan, ipv6, tcp-concurrent).
    /// Called by save_settings to apply runtime changes immediately.
    pub(crate) fn hot_patch_runtime(&self, log_level: Option<&str>, allow_lan: Option<bool>,
                                   ipv6: Option<bool>, tcp_concurrent: Option<bool>) {
        hot_patch_configs(&self.get_api_url(), log_level, allow_lan, ipv6, tcp_concurrent);
    }
}
