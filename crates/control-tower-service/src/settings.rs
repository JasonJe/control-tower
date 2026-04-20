//! Shared settings types

use serde::{Deserialize, Serialize};

/// Settings data structure matching settings.yaml
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
}
