//! Shared settings types

use serde::{Deserialize, Serialize};

/// Default port constants for Mihomo and Control Tower service.
pub mod consts {
    /// Mihomo external controller port (Secret API port)
    pub const DEFAULT_MIHOMO_API_PORT: u16 = 9090;
    /// Mihomo HTTP proxy port
    pub const DEFAULT_MIHOMO_HTTP_PORT: u16 = 7890;
    /// Mihomo SOCKS5 proxy port
    pub const DEFAULT_MIHOMO_SOCKS_PORT: u16 = 7891;
    /// Control Tower service HTTP port
    pub const DEFAULT_SERVICE_PORT: u16 = 8080;
    /// Maximum cron schedule interval in minutes (7 days)
    pub const MAX_CRON_INTERVAL_MINS: u32 = 10080;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_test_config_default() {
        let cfg = AutoTestConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.interval_minutes, 15);
    }

    #[test]
    fn test_settings_data_has_auto_test() {
        let settings = SettingsData::default();
        assert!(settings.auto_test.is_some());
        assert!(!settings.auto_test.as_ref().unwrap().enabled);
    }
}

/// Auto test configuration for proxy latency testing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoTestConfig {
    pub enabled: bool,
    pub interval_minutes: u32,
    #[serde(rename = "latency_test_mode", default, skip_serializing_if = "Option::is_none")]
    pub latency_test_mode: Option<String>,
}

impl Default for AutoTestConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_minutes: 15,
            latency_test_mode: None,
        }
    }
}

/// Settings data structure matching settings.yaml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_host: Option<String>,
    #[serde(rename = "api_port", default, skip_serializing_if = "Option::is_none")]
    pub api_port: Option<u16>,
    #[serde(rename = "http_port", default, skip_serializing_if = "Option::is_none")]
    pub http_port: Option<u16>,
    #[serde(rename = "socks_port", default, skip_serializing_if = "Option::is_none")]
    pub socks_port: Option<u16>,
    #[serde(rename = "mixed_port", default, skip_serializing_if = "Option::is_none")]
    pub mixed_port: Option<u16>,
    #[serde(rename = "service_port", default, skip_serializing_if = "Option::is_none")]
    pub service_port: Option<u16>,
    #[serde(rename = "tun_enabled", default, skip_serializing_if = "Option::is_none")]
    pub tun_enabled: Option<bool>,
    #[serde(rename = "log_level", default, skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
    #[serde(rename = "allow_lan", default, skip_serializing_if = "Option::is_none")]
    pub allow_lan: Option<bool>,
    #[serde(rename = "ipv6", default, skip_serializing_if = "Option::is_none")]
    pub ipv6: Option<bool>,
    #[serde(rename = "tcp_concurrent", default, skip_serializing_if = "Option::is_none")]
    pub tcp_concurrent: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(rename = "latency_test_mode", default, skip_serializing_if = "Option::is_none")]
    pub latency_test_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_test: Option<AutoTestConfig>,
    #[serde(rename = "custom-rules", default, skip_serializing_if = "Option::is_none")]
    pub custom_rules: Option<Vec<String>>,
    #[serde(rename = "profile-rules-count", default, skip_serializing_if = "Option::is_none")]
    pub profile_rules_count: Option<usize>,
}

impl Default for SettingsData {
    fn default() -> Self {
        Self {
            api_host: None,
            api_port: None,
            http_port: None,
            socks_port: None,
            mixed_port: None,
            service_port: None,
            tun_enabled: None,
            log_level: None,
            allow_lan: None,
            ipv6: None,
            tcp_concurrent: None,
            mode: None,
            latency_test_mode: None,
            auto_test: Some(AutoTestConfig::default()),
            custom_rules: None,
            profile_rules_count: None,
        }
    }
}
