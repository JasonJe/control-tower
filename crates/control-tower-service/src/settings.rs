//! Shared settings types

use serde::{Deserialize, Serialize};

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
    #[serde(rename = "latency_test_mode", default)]
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
    #[serde(default)]
    pub api_host: Option<String>,
    #[serde(rename = "api_port", default)]
    pub api_port: Option<u16>,
    #[serde(rename = "http_port", default)]
    pub http_port: Option<u16>,
    #[serde(rename = "socks_port", default)]
    pub socks_port: Option<u16>,
    #[serde(rename = "service_port", default)]
    pub service_port: Option<u16>,
    #[serde(rename = "tun_enabled", default)]
    pub tun_enabled: Option<bool>,
    #[serde(rename = "log_level", default)]
    pub log_level: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(rename = "latency_test_mode", default)]
    pub latency_test_mode: Option<String>,
    #[serde(default)]
    pub auto_test: Option<AutoTestConfig>,
}

impl Default for SettingsData {
    fn default() -> Self {
        Self {
            api_host: None,
            api_port: None,
            http_port: None,
            socks_port: None,
            service_port: None,
            tun_enabled: None,
            log_level: None,
            mode: None,
            latency_test_mode: None,
            auto_test: Some(AutoTestConfig::default()),
        }
    }
}
