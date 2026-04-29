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

/// DNS configuration for config.yaml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsSettings {
    #[serde(default)]
    pub enable: bool,
    #[serde(rename = "enhanced_mode", default = "default_enhanced_mode", skip_serializing_if = "String::is_empty")]
    pub enhanced_mode: String,
    #[serde(rename = "fake_ip_range", default = "default_fake_ip_range", skip_serializing_if = "String::is_empty")]
    pub fake_ip_range: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nameserver: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback: Vec<String>,
    #[serde(rename = "fallback_filter", default, skip_serializing_if = "Option::is_none")]
    pub fallback_filter: Option<FallbackFilter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosts: Vec<HostEntry>,
    #[serde(rename = "nameserver_policy", default, skip_serializing_if = "Vec::is_empty")]
    pub nameserver_policy: Vec<NameserverPolicyEntry>,
}

fn default_enhanced_mode() -> String { "fake-ip".to_string() }
fn default_fake_ip_range() -> String { "198.18.0.1/15".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackFilter {
    #[serde(default)]
    pub geoip: bool,
    #[serde(rename = "geoip_code", default, skip_serializing_if = "Option::is_none")]
    pub geoip_code: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ipcidr: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostEntry {
    #[serde(rename = "host", default)]
    pub host: String,
    #[serde(rename = "ip", default)]
    pub ip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameserverPolicyEntry {
    #[serde(rename = "match_domain", default)]
    pub match_domain: String,
    #[serde(default)]
    pub nameserver: Vec<String>,
}

impl Default for DnsSettings {
    fn default() -> Self {
        Self {
            enable: false,
            enhanced_mode: "fake-ip".to_string(),
            fake_ip_range: "198.18.0.1/15".to_string(),
            nameserver: vec![
                "https://doh.pub/dns-query".to_string(),
                "https://dns.alidns.com/dns-query".to_string(),
            ],
            fallback: vec![
                "https://1.1.1.1/dns-query".to_string(),
                "https://dns.google/dns-query".to_string(),
            ],
            fallback_filter: Some(FallbackFilter {
                geoip: true,
                geoip_code: Some("CN".to_string()),
                ipcidr: vec!["240.0.0.0/4".to_string()],
            }),
            hosts: vec![],
            nameserver_policy: vec![],
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
    #[serde(rename = "auto-update-on-startup", default, skip_serializing_if = "Option::is_none")]
    pub auto_update_on_startup: Option<bool>,
    #[serde(rename = "rule-providers", default, skip_serializing_if = "Option::is_none")]
    pub rule_providers: Option<Vec<RuleProviderConfig>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dns: Option<DnsSettings>,
    #[serde(rename = "connection-history", default, skip_serializing_if = "Option::is_none")]
    pub connection_history: Option<ConnectionHistoryConfig>,
    #[serde(rename = "closed-connections", default, skip_serializing_if = "Vec::is_empty")]
    pub closed_connections: Vec<ClosedConnection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub https: Option<HttpsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthConfig>,
}

/// Connection history configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionHistoryConfig {
    pub enabled: bool,
    #[serde(rename = "max_count", default)]
    pub max_count: u32,
}

/// A closed connection record for history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosedConnection {
    pub id: String,
    pub source_ip: String,
    pub destination: String,
    pub chains: Vec<String>,
    pub upload: u64,
    pub download: u64,
    #[serde(rename = "closed_at")]
    pub closed_at: String,
}

/// HTTPS configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpsConfig {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cert_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_path: Option<String>,
}

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self { enabled: true, password: None }
    }
}

/// Rule provider configuration — re-exported from control-tower-service-core
pub use control_tower_service_core::profiles::RuleProviderConfig;

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
            auto_update_on_startup: None,
            rule_providers: None,
            dns: None,
            connection_history: None,
            closed_connections: vec![],
            https: None,
            auth: None,
        }
    }
}
