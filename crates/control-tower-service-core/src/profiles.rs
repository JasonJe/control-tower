//! Shared types for profiles.yaml parsing and serialization.
//!
//! `ProfileItem` and `ProfilesYaml` are used across CLI, service, and Web handler
//! to read/write the profiles manifest.  Having a single definition avoids
//! drift when fields are added.

use serde::{Deserialize, Serialize};

/// Options for profile subscription download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileDownloadOptions {
    #[serde(rename = "user_agent", skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
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
            timeout_seconds: None,
            with_proxy: None,
            danger_accept_invalid_certs: None,
        }
    }
}

/// One entry inside profiles.yaml.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileItem {
    pub uid: String,
    pub name: Option<String>,
    #[serde(rename = "file")]
    pub file: Option<String>,
    pub url: Option<String>,
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
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub lazy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
}

fn default_interval() -> u32 { 86400 }
fn default_true() -> bool { true }
