//! Config file operations: mode / ports / TUN settings, hot-patch, and YAML updates.

use std::time::Duration;
use reqwest::blocking::Client as BlockingClient;

use crate::ServiceState;

/// Atomically replace config.yaml and keep it owner-only.
///
/// config.yaml can contain proxy credentials. Writing through a `.tmp` file
/// also prevents a crash from leaving Mihomo with a truncated config.
fn write_config_atomically(path: &std::path::Path, content: &str) -> Result<(), String> {
    control_tower_service_core::write_private_atomic(path, content)
        .map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}

/// Validate a complete port set before persisting it.
///
/// Kept as a free function so the settings endpoints can validate the values
/// they are about to save (including a new API/service port) without waiting
/// for `apply-ports` to restart Mihomo.
pub(crate) fn validate_port_set(
    http_port: u16,
    socks_port: u16,
    api_port: u16,
    service_port: u16,
) -> Result<(), String> {
    if http_port == 0 || socks_port == 0 || api_port == 0 || service_port == 0 {
        return Err("Port numbers must be between 1 and 65535".to_string());
    }
    if http_port == socks_port {
        return Err("HTTP port and SOCKS port must be different".to_string());
    }
    if http_port == api_port || socks_port == api_port {
        return Err(format!(
            "Proxy ports must differ from the Mihomo API port ({api_port})"
        ));
    }
    if http_port == service_port || socks_port == service_port {
        return Err(format!(
            "Proxy ports must differ from the Control Tower service port ({service_port})"
        ));
    }
    if api_port == service_port {
        return Err(format!(
            "Mihomo API port and Control Tower service port must be different ({api_port})"
        ));
    }
    Ok(())
}

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
        // Reuse the shared loader: a malformed settings.yaml must not silently
        // be replaced with defaults (that would wipe auth and rules).
        let mut settings = self.get_settings();
        settings.mode = Some(mode.to_string());

        self.save_settings(&settings)?;
        tracing::info!("Mode saved to settings.yaml: {}", mode);
        Ok(())
    }

    /// Update mode in config.yaml directly
    fn update_config_mode(&self, mode: &str) -> Result<(), String> {
        let config_path = crate::settings::resolve_paths().active_config_path;
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

        write_config_atomically(&config_path, &new_content)?;

        tracing::info!("Mode updated in config.yaml: {}", mode);
        Ok(())
    }

    /// Update port settings in config.yaml (`port`, `socks-port`, external-controller)
    fn update_config_ports(&self, http_port: u16, socks_port: u16) -> Result<(), String> {
        let config_path = crate::settings::resolve_paths().active_config_path;
        if !config_path.exists() {
            return Err("config.yaml not found".to_string());
        }

        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read config.yaml: {}", e))?;

        let mut yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&content)
            .map_err(|e| format!("Failed to parse config.yaml: {}", e))?;

        // Exclude a profile's mixed-port when it collides with the ports chosen
        // here; both listeners would otherwise try to bind the same address.
        let existing_mixed = yaml.get("mixed-port").and_then(|v| v.as_u64());

        if let Some(map) = yaml.as_mapping_mut() {
            // `http_port` is Mihomo's `port` (HTTP proxy), not `mixed-port`.
            map.insert("port".into(), http_port.into());
            map.insert("socks-port".into(), socks_port.into());

            if existing_mixed == Some(u64::from(http_port))
                || existing_mixed == Some(u64::from(socks_port))
            {
                map.remove(&serde_yaml_ng::Value::String("mixed-port".to_string()));
                tracing::warn!(
                    "Removed mixed-port {:?}: it collides with http_port/socks_port",
                    existing_mixed
                );
            }

            // Older builds wrote the non-standard `http-port` key, which Mihomo
            // ignores. Clean it up so the file no longer misleads anyone.
            map.remove(&serde_yaml_ng::Value::String("http-port".to_string()));

            let api_host = self.api_host.read().clone();
            let api_port = *self.api_port.read();
            let external_controller = format!("{}:{}", api_host, api_port);
            map.insert("external-controller".into(), external_controller.into());
        }

        let new_content = serde_yaml_ng::to_string(&yaml)
            .map_err(|e| format!("Failed to serialize config.yaml: {}", e))?;

        write_config_atomically(&config_path, &new_content)?;

        let api_port = *self.api_port.read();
        tracing::info!("Ports updated in config.yaml: http={}, socks={}, api={}", http_port, socks_port, api_port);
        Ok(())
    }

    /// Apply port settings: save to settings.yaml, update config.yaml, restart Mihomo
    pub fn apply_port_settings(&self, http_port: u16, socks_port: u16) -> Result<(), String> {
        self.validate_proxy_ports(http_port, socks_port)?;

        // Ports can be configured before any profile is activated. In that case
        // there is no config.yaml yet; still persist the settings and apply
        // them the next time config.yaml is generated.
        let config_exists = crate::settings::resolve_paths().active_config_path.exists();
        if config_exists {
            self.update_config_ports(http_port, socks_port)?;
        } else {
            tracing::info!(
                "config.yaml not found; saving port settings for the next profile activation"
            );
        }

        // Only touch the fields this endpoint owns. `save_settings` merges by
        // `Some`-ness, so anything left `None` keeps the user's existing value.
        // The previous code rebuilt the whole struct and forced `api_host` to
        // 127.0.0.1 and `service_port` to its default, silently reverting a
        // customised API bind address or service port.
        let mut settings = crate::SettingsData::patch();
        settings.http_port = Some(http_port);
        settings.socks_port = Some(socks_port);
        self.save_settings(&settings)?;

        let config_path = self.last_config_path.read().clone();
        if config_path.is_some() {
            self.restart_mihomo()?;
        } else {
            tracing::info!("Mihomo not running, config updated but not restarted");
        }

        tracing::info!("Port settings applied");
        // Keep the desktop proxy pointing at the new ports when the switch is
        // on; otherwise the next page load would fail silently.
        crate::service::system_proxy::refresh_ports(http_port, socks_port);
        Ok(())
    }

    /// Reject port combinations that cannot work.
    ///
    /// The HTTP (`port`) and SOCKS (`socks-port`) listeners are independent, so
    /// they must differ from each other and from the ports already used by the
    /// Mihomo API and by Control Tower itself.
    pub fn validate_proxy_ports(&self, http_port: u16, socks_port: u16) -> Result<(), String> {
        let service_port = self
            .get_settings()
            .service_port
            .unwrap_or(crate::settings::consts::DEFAULT_SERVICE_PORT);
        validate_port_set(http_port, socks_port, *self.api_port.read(), service_port)
    }

    /// Update tun section in config.yaml (does NOT touch dns section)
    fn update_config_tun(&self, tun_enabled: bool) -> Result<(), String> {
        let config_path = crate::settings::resolve_paths().active_config_path;
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

        write_config_atomically(&config_path, &new_content)?;

        Ok(())
    }

    /// Apply DNS settings: write dns section to config.yaml
    pub fn apply_dns_settings(&self, dns_settings: &crate::settings::DnsSettings) -> Result<(), String> {
        let config_path = crate::settings::resolve_paths().active_config_path;
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

        write_config_atomically(&config_path, &new_content)?;

        Ok(())
    }

    /// Apply TUN settings: update config.yaml tun section and settings.yaml tun_enabled
    pub fn apply_tun_settings(&self, tun_enabled: bool) -> Result<(), String> {
        // Same as apply-ports: allow configuring TUN before a profile exists.
        if crate::settings::resolve_paths().active_config_path.exists() {
            self.update_config_tun(tun_enabled)?;
        } else {
            tracing::info!(
                "config.yaml not found; saving tun_enabled for the next profile activation"
            );
        }

        // Same partial-update rule as `apply_port_settings`: do not rewrite
        // unrelated settings (api_host, service_port, auto_test, ...).
        let mut settings = crate::SettingsData::patch();
        settings.tun_enabled = Some(tun_enabled);
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
