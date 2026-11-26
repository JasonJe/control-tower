//! Shared types for profiles.yaml parsing and serialization.
//!
//! `ProfileItem` and `ProfilesYaml` are used across CLI, service, and Web handler
//! to read/write the profiles manifest.  Having a single definition avoids
//! drift when fields are added.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

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

    /// Effective `User-Agent` to send: the dedicated `user_agent` option wins
    /// over a `User-Agent` entry in `headers`, which in turn wins over the
    /// caller's default.
    ///
    /// Older versions gave `headers.user-agent` priority and skipped the
    /// dedicated option entirely, so editing `options.user_agent` appeared to
    /// have no effect when a profile also had a custom header.
    pub fn effective_user_agent<'a>(&'a self, default: &'a str) -> &'a str {
        if let Some(ua) = self.user_agent.as_deref() {
            return ua;
        }
        self.headers
            .as_ref()
            .and_then(|headers| {
                headers
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
                    .map(|(_, value)| value.as_str())
            })
            .unwrap_or(default)
    }

    /// Custom headers excluding `User-Agent`, which is sent separately via
    /// [`Self::effective_user_agent`] so only one value is ever emitted.
    pub fn header_pairs_without_user_agent(&self) -> impl Iterator<Item = (&str, &str)> {
        self.header_pairs()
            .filter(|(name, _)| !name.eq_ignore_ascii_case("user-agent"))
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
    /// Bytes uploaded according to the provider's `subscription-userinfo`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload: Option<u64>,
    /// Bytes downloaded according to the provider's `subscription-userinfo`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download: Option<u64>,
    /// Total quota in bytes (0 or absent when the provider does not report it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    /// Unix timestamp when the subscription expires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expire: Option<i64>,
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

/// Serialize profiles.yaml read-modify-write cycles inside this process.
///
/// Handlers create their own `ProfilesYaml` copies, so without a shared lock a
/// later writer would silently drop an earlier writer's change (for example a
/// cron update racing a rename in the Web UI).
fn profiles_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Take an exclusive advisory lock shared with other processes (the CLI).
///
/// The lock lives on a sibling `.lock` file: `profiles.yaml` itself is replaced
/// by an atomic rename, so a process waiting on the old inode would never see
/// the new one.
#[cfg(unix)]
fn lock_across_processes(path: &Path) -> std::io::Result<Option<std::fs::File>> {
    use std::os::unix::io::AsRawFd;

    let lock_path = path.with_extension("lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(Some(file))
}

#[cfg(not(unix))]
fn lock_across_processes(_path: &Path) -> std::io::Result<Option<std::fs::File>> {
    Ok(None)
}

/// Apply `f` to the current profiles.yaml and write the result atomically.
///
/// The file is read *inside* the lock, so concurrent callers apply their change
/// to the latest content instead of overwriting each other. Dropping the
/// returned handle releases both the in-process mutex and the file lock.
pub fn update_profiles<T>(
    path: &Path,
    f: impl FnOnce(&mut ProfilesYaml) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let _guard = profiles_lock();

    // The advisory lock lives next to profiles.yaml, so the directory must
    // exist before it can be opened.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Dropping `_file_guard` releases the advisory lock.
    let _file_guard = lock_across_processes(path)?;

    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };

    let mut yaml: ProfilesYaml = if content.trim().is_empty() {
        ProfilesYaml {
            current: None,
            items: Vec::new(),
        }
    } else {
        serde_yaml_ng::from_str(&content)?
    };

    let result = f(&mut yaml)?;
    let serialized = serde_yaml_ng::to_string(&yaml)?;
    crate::write_private_atomic(path, &serialized)?;
    Ok(result)
}

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

    #[test]
    fn effective_user_agent_prefers_dedicated_option() {
        let mut headers = BTreeMap::new();
        headers.insert("user-agent".to_string(), "old-agent".to_string());
        headers.insert("x-token".to_string(), "secret".to_string());
        let options = ProfileDownloadOptions {
            user_agent: Some("new-agent".to_string()),
            headers: Some(headers),
            ..Default::default()
        };

        assert_eq!(options.effective_user_agent("default"), "new-agent");
        // The stale UA header must not be sent a second time.
        let pairs: Vec<_> = options.header_pairs_without_user_agent().collect();
        assert_eq!(pairs, vec![("x-token", "secret")]);

        let headers_only = ProfileDownloadOptions {
            headers: Some(BTreeMap::from([(
                "USER-AGENT".to_string(),
                "header-agent".to_string(),
            )])),
            ..Default::default()
        };
        assert_eq!(headers_only.effective_user_agent("default"), "header-agent");
        assert_eq!(headers_only.header_pairs_without_user_agent().count(), 0);
    }

    fn test_item(uid: &str) -> ProfileItem {
        ProfileItem {
            uid: uid.to_string(),
            name: None,
            file: None,
            url: None,
            cron: None,
            updated_at: None,
            options: None,
            type_: None,
            script: None,
            merge: Vec::new(),
            upload: None,
            download: None,
            total: None,
            expire: None,
        }
    }

    #[test]
    fn update_profiles_does_not_lose_concurrent_updates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.yaml");

        let mut handles = Vec::new();
        for worker in 0..8 {
            let path = path.clone();
            handles.push(std::thread::spawn(move || {
                for i in 0..5 {
                    let uid = format!("{}-{}", worker, i);
                    update_profiles(&path, |yaml| {
                        yaml.items.push(test_item(&uid));
                        Ok(())
                    })
                    .unwrap();
                }
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }

        let content = std::fs::read_to_string(&path).unwrap();
        let yaml: ProfilesYaml = serde_yaml_ng::from_str(&content).unwrap();
        assert_eq!(yaml.items.len(), 40, "concurrent updates must not be lost");
    }

    #[test]
    fn update_profiles_creates_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.yaml");

        update_profiles(&path, |yaml| {
            yaml.current = Some("a".to_string());
            yaml.items.push(test_item("a"));
            Ok(())
        })
        .unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("current: a"));
    }
}
