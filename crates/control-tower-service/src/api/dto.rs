//! Request and response DTOs for the HTTP API

use serde::{Deserialize, Serialize};

// ============ Core ApiResponse ============

/// Standard API response wrapper
#[derive(Debug, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            code: 0,
            message: "success".to_string(),
            data: Some(data),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            code: -1,
            message: message.into(),
            data: None,
        }
    }
}

// ============ Proxy DTOs ============

#[derive(Debug, Deserialize)]
pub struct SelectProxyRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct SetModeRequest {
    pub mode: String,
}

#[derive(Debug, Deserialize)]
pub struct ProxyDelayRequest {
    // Note: name is extracted from URL path /proxies/{name}/delay, not from body
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

#[derive(Debug, Deserialize)]
pub struct ProxyDelayPostRequest {
    pub name: String,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    #[serde(default)]
    pub mode: Option<String>,
}

fn default_timeout() -> u64 {
    5000
}

/// Request for batch delay test
#[derive(Debug, Deserialize)]
pub struct ProxyDelayAllRequest {
    pub timeout: Option<u64>,
    pub mode: Option<String>,
}

// ============ Rules DTOs ============

#[derive(Debug, Deserialize)]
pub struct AddRuleRequest {
    #[serde(rename = "type")]
    pub rule_type: String,
    pub value: String,
    pub proxy: String,
}

#[derive(Debug, Deserialize)]
pub struct DeleteRulesRequest {
    #[serde(rename = "source", default)]
    pub source: String,
}

// ============ Profile DTOs ============

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct AddProfileRequest {
    pub url: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub file: Option<String>,
    pub script: Option<String>,
    pub merge: Option<Vec<String>>,
}

impl Default for AddProfileRequest {
    fn default() -> Self {
        Self {
            url: None,
            name: None,
            type_: Some("remote".to_string()),
            file: None,
            script: None,
            merge: None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub use_proxy: Option<bool>,
}

// ============ Settings DTOs ============

#[derive(Debug, Deserialize)]
pub struct UpdateSettingsRequest {
    #[serde(rename = "api_host", default)]
    pub api_host: Option<String>,
    #[serde(rename = "api_port", default)]
    pub api_port: Option<u16>,
    #[serde(rename = "http_port", default)]
    pub http_port: Option<u16>,
    #[serde(rename = "socks_port", default)]
    pub socks_port: Option<u16>,
    #[serde(rename = "mixed_port", default)]
    pub mixed_port: Option<u16>,
    #[serde(rename = "service_port", default)]
    pub service_port: Option<u16>,
    #[serde(rename = "tun_enabled", default)]
    pub tun_enabled: Option<bool>,
    #[serde(rename = "log_level", default)]
    pub log_level: Option<String>,
    #[serde(rename = "allow_lan", default)]
    pub allow_lan: Option<bool>,
    #[serde(rename = "ipv6", default)]
    pub ipv6: Option<bool>,
    #[serde(rename = "tcp_concurrent", default)]
    pub tcp_concurrent: Option<bool>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(rename = "latency_test_mode", default)]
    pub latency_test_mode: Option<String>,
    #[serde(default)]
    pub auto_test: Option<crate::settings::AutoTestConfig>,
    #[serde(rename = "custom-rules", default)]
    #[allow(unused)]
    pub custom_rules: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct ApplyPortsRequest {
    #[serde(rename = "http_port")]
    pub http_port: u16,
    #[serde(rename = "socks_port")]
    pub socks_port: u16,
}

#[derive(Debug, Deserialize)]
pub struct ApplyTunRequest {
    pub tun_enabled: bool,
}

// ============ Logs DTOs ============

#[derive(Debug, Deserialize)]
pub struct LogsQuery {
    pub source: Option<String>,
    pub lines: Option<usize>,
}

// ============ Auto Speed Test DTOs ============

#[derive(Serialize)]
pub struct LatencyResultDto {
    pub name: String,
    pub latency: Option<i64>,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct FastestResponse {
    pub enabled: bool,
    pub interval_minutes: u32,
    pub last_test_at: Option<i64>,
    pub fastest: Option<LatencyResultDto>,
    pub results: Vec<LatencyResultDto>,
}

// ============ Tests ============

#[cfg(test)]
mod fastest_tests {
    use super::*;

    #[test]
    fn test_fastest_response_serialization() {
        let resp = FastestResponse {
            enabled: true,
            interval_minutes: 15,
            last_test_at: Some(1745214725),
            fastest: Some(LatencyResultDto {
                name: "vmess-hk-01".into(),
                latency: Some(127),
                error: None,
            }),
            results: vec![
                LatencyResultDto { name: "vmess-hk-01".into(), latency: Some(127), error: None },
                LatencyResultDto { name: "vmess-sg-02".into(), latency: None, error: Some("Not supported".into()) },
            ],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"enabled\":true"));
        assert!(json.contains("\"interval_minutes\":15"));
        assert!(json.contains("vmess-hk-01"));
        assert!(json.contains("127"));
    }

    #[test]
    fn test_fastest_response_empty_results() {
        let resp = FastestResponse {
            enabled: false,
            interval_minutes: 15,
            last_test_at: None,
            fastest: None,
            results: vec![],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"fastest\":null"));
        assert!(json.contains("\"results\":[]"));
        assert!(json.contains("\"last_test_at\":null"));
    }
}
