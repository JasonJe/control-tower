//! Shared types for profiles.yaml parsing and serialization.
//!
//! `ProfileItem` and `ProfilesYaml` are used across CLI, service, and Web handler
//! to read/write the profiles manifest.  Having a single definition avoids
//! drift when fields are added.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Options for profile subscription download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileDownloadOptions {
    #[serde(rename = "user_agent", skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    /// Extra HTTP headers sent when downloading the subscription.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(rename = "timeout_seconds", skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
    #[serde(rename = "with_proxy", skip_serializing_if = "Option::is_none")]
    pub with_proxy: Option<bool>,
    #[serde(rename = "danger_accept_invalid_certs", skip_serializing_if = "Option::is_none")]
    pub danger_accept_invalid_certs: Option<bool>,
}

impl Default for ProfileDownloadOptions {
    fn default() -> Self {
        Self {
            user_agent: None,
            headers: None,
            timeout_seconds: None,
            with_proxy: None,
            danger_accept_invalid_certs: None,
        }
    }
}

impl ProfileDownloadOptions {
    /// User-Agent to use, falling back to the caller-provided default.
    pub fn user_agent_or<'a>(&'a self, default: &'a str) -> &'a str {
        self.user_agent.as_deref().unwrap_or(default)
    }

    /// Custom headers as (name, value) pairs. Invalid names are skipped by the
    /// underlying HTTP client when they are applied.
    pub fn header_pairs(&self) -> impl Iterator<Item = (&str, &str)> {
        self.headers
            .iter()
            .flatten()
            .map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Download timeout, falling back to the caller-provided default.
    pub fn timeout_or(&self, default: u64) -> u64 {
        self.timeout_seconds.unwrap_or(default)
    }

    /// Whether a custom header with this name is configured.
    ///
    /// Header names are case-insensitive, so callers must use this instead of
    /// blindly adding a default `User-Agent`: reqwest's `RequestBuilder::header`
    /// *appends*, and two `User-Agent` values make many subscription servers
    /// reject the request.
    pub fn has_header(&self, name: &str) -> bool {
        self.headers
            .as_ref()
            .map(|headers| headers.keys().any(|k| k.eq_ignore_ascii_case(name)))
            .unwrap_or(false)
    }
}

/// One entry inside profiles.yaml.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileItem {
    pub uid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "file", skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
    #[serde(rename = "updated_at")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<ProfileDownloadOptions>,
    /// Profile type: "remote" (default, subscription), "local" (file only), "script" (execute script), "merge" (combine profiles)
    #[serde(rename = "type", default = "default_profile_type")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    /// Script content for script-type profiles (executed to generate config)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    /// List of profile UIDs to merge for merge-type profiles
    #[serde(rename = "merge", deserialize_with = "deserialize_null_as_empty_vec", default)]
    pub merge: Vec<String>,
}

/// Custom deserializer that treats YAML null as an empty Vec
fn deserialize_null_as_empty_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NullOrVec {
        Null,
        Vec(Vec<String>),
    }
    match NullOrVec::deserialize(deserializer)? {
        NullOrVec::Null => Ok(vec![]),
        NullOrVec::Vec(v) => Ok(v),
    }
}

fn default_profile_type() -> Option<String> { Some("remote".to_string()) }

/// The top-level profiles.yaml structure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfilesYaml {
    #[serde(default)]
    pub current: Option<String>,
    pub items: Vec<ProfileItem>,
}

/// Rule provider configuration matching Mihomo's rule-providers schema.
/// This is stored in settings.yaml and written to config.yaml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleProviderConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub behavior: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default = "default_interval")]
    pub interval: u32,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub lazy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
}

fn default_interval() -> u32 { 86400 }
fn default_true() -> bool { true }
fn is_true(value: &bool) -> bool { *value }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_options_roundtrip_with_custom_headers() {
        let mut headers = BTreeMap::new();
        headers.insert("X-Token".to_string(), "abc".to_string());
        let options = ProfileDownloadOptions {
            user_agent: Some("custom-agent".to_string()),
            headers: Some(headers),
            ..Default::default()
        };

        let yaml = serde_yaml_ng::to_string(&options).unwrap();
        assert!(yaml.contains("X-Token"));
        let parsed: ProfileDownloadOptions = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(parsed, options);
        assert_eq!(parsed.user_agent_or("default"), "custom-agent");
        assert_eq!(parsed.timeout_or(30), 30);
    }

    #[test]
    fn download_options_defaults_are_transparent() {
        let options = ProfileDownloadOptions::default();
        assert_eq!(options.user_agent_or("default"), "default");
        assert_eq!(options.header_pairs().count(), 0);
        assert_eq!(options.timeout_or(30), 30);
    }
}
