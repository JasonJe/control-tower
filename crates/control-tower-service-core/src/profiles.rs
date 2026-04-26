//! Shared types for profiles.yaml parsing and serialization.
//!
//! `ProfileItem` and `ProfilesYaml` are used across CLI, service, and Web handler
//! to read/write the profiles manifest.  Having a single definition avoids
//! drift when fields are added.

use serde::{Deserialize, Serialize};

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
}

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
