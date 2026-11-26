//! Active-config editing, validation, backups and rollback.
//!
//! Hand-editing `config.yaml` is a common request: the Web UI shows the whole
//! config, but writing it back unchecked could lock the user out of their
//! proxy. Every save therefore goes through validation plus a backup, and the
//! previous contents are restored if Mihomo refuses to reload.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use serde_yaml_ng::Value;

/// Maximum number of automatic backups kept on disk.
const MAX_BACKUPS: usize = 20;

pub fn config_path() -> PathBuf {
    crate::settings::resolve_paths().active_config_path
}

pub fn backups_dir() -> PathBuf {
    crate::settings::resolve_paths().config_dir.join("backups")
}

/// Counts extracted from a config, shown in the UI after validation/save.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ConfigSummary {
    pub proxies: usize,
    pub proxy_groups: usize,
    pub rules: usize,
    pub rule_providers: usize,
    pub proxy_providers: usize,
    pub size: usize,
}

/// Result of validating a candidate config.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<ConfigSummary>,
}

/// One file in the backups directory.
#[derive(Debug, Clone, Serialize)]
pub struct BackupInfo {
    pub id: String,
    pub reason: String,
    pub created_at: String,
    pub size: u64,
}

/// Validate a config body without touching disk.
///
/// Structural errors (unparsable YAML, wrong types, unknown group members)
/// are reported as errors; cosmetic/unsupported things are warnings.
pub fn validate(content: &str) -> ValidationReport {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if content.trim().is_empty() {
        errors.push("Config is empty".to_string());
        return ValidationReport {
            ok: false,
            errors,
            warnings,
            summary: None,
        };
    }

    let value: Value = match serde_yaml_ng::from_str(content) {
        Ok(v) => v,
        Err(e) => {
            errors.push(format!("Invalid YAML: {}", e));
            return ValidationReport {
                ok: false,
                errors,
                warnings,
                summary: None,
            };
        }
    };

    let Some(map) = value.as_mapping() else {
        errors.push("Config root must be a mapping (key: value pairs)".to_string());
        return ValidationReport {
            ok: false,
            errors,
            warnings,
            summary: None,
        };
    };

    let section = |name: &str| map.get(Value::String(name.to_string()));

    let mut summary = ConfigSummary {
        size: content.len(),
        ..Default::default()
    };

    // proxies
    let mut proxy_names: BTreeSet<String> = BTreeSet::new();
    if let Some(proxies) = section("proxies") {
        match proxies.as_sequence() {
            Some(list) => {
                summary.proxies = list.len();
                for (index, proxy) in list.iter().enumerate() {
                    let Some(proxy_map) = proxy.as_mapping() else {
                        errors.push(format!("proxies[{}] must be a mapping", index));
                        continue;
                    };
                    let name = proxy_map
                        .get(Value::String("name".to_string()))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if name.is_empty() {
                        errors.push(format!("proxies[{}] is missing `name`", index));
                    } else if !proxy_names.insert(name.to_string()) {
                        errors.push(format!("Duplicate proxy name `{}`", name));
                    }
                    for required in ["type", "server", "port"] {
                        if !proxy_map.contains_key(Value::String(required.to_string())) {
                            errors.push(format!(
                                "proxies[{}] ({}) is missing `{}`",
                                index,
                                if name.is_empty() { "unnamed" } else { name },
                                required
                            ));
                        }
                    }
                }
            }
            None => errors.push("`proxies` must be a list".to_string()),
        }
    }

    // proxy-providers (names usable as group members)
    let mut provider_names: BTreeSet<String> = BTreeSet::new();
    if let Some(providers) = section("proxy-providers") {
        if let Some(mapping) = providers.as_mapping() {
            summary.proxy_providers = mapping.len();
            for key in mapping.keys().filter_map(Value::as_str) {
                provider_names.insert(key.to_string());
            }
        } else {
            warnings.push("`proxy-providers` should be a mapping".to_string());
        }
    }

    // proxy-groups
    let mut group_names: BTreeSet<String> = BTreeSet::new();
    if let Some(groups) = section("proxy-groups") {
        match groups.as_sequence() {
            Some(list) => {
                summary.proxy_groups = list.len();
                for (index, group) in list.iter().enumerate() {
                    let Some(group_map) = group.as_mapping() else {
                        errors.push(format!("proxy-groups[{}] must be a mapping", index));
                        continue;
                    };
                    let name = group_map
                        .get(Value::String("name".to_string()))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if name.is_empty() {
                        errors.push(format!("proxy-groups[{}] is missing `name`", index));
                    } else if !group_names.insert(name.to_string()) {
                        errors.push(format!("Duplicate proxy group name `{}`", name));
                    }
                    if let Some(members) = group_map.get(Value::String("proxies".to_string())) {
                        match members.as_sequence() {
                            Some(members) => {
                                for member in members.iter().filter_map(Value::as_str) {
                                    if !is_known_target(
                                        member,
                                        &proxy_names,
                                        &group_names,
                                        &provider_names,
                                    ) {
                                        warnings.push(format!(
                                            "Group `{}` references unknown target `{}`",
                                            if name.is_empty() { "unnamed" } else { name },
                                            member
                                        ));
                                    }
                                }
                            }
                            None => errors.push(format!(
                                "proxy-groups[{}] `proxies` must be a list",
                                index
                            )),
                        }
                    }
                }
            }
            None => errors.push("`proxy-groups` must be a list".to_string()),
        }
    }

    // rule-providers
    if let Some(providers) = section("rule-providers") {
        if let Some(mapping) = providers.as_mapping() {
            summary.rule_providers = mapping.len();
        } else {
            warnings.push("`rule-providers` should be a mapping".to_string());
        }
    }

    // rules
    if let Some(rules) = section("rules") {
        match rules.as_sequence() {
            Some(list) => {
                summary.rules = list.len();
                let group_names = group_names.clone();
                let provider_names = provider_names.clone();
                for (index, rule) in list.iter().enumerate() {
                    let Some(text) = rule.as_str() else {
                        errors.push(format!("rules[{}] must be a string", index));
                        continue;
                    };
                    if text.trim().is_empty() {
                        errors.push(format!("rules[{}] is empty", index));
                        continue;
                    }
                    // The policy is the last comma-separated field (MATCH has
                    // type and policy only).
                    let policy = text.rsplit(',').next().unwrap_or("").trim();
                    if policy.is_empty() {
                        errors.push(format!("rules[{}] has no policy: `{}`", index, text));
                    } else if !is_known_target(policy, &proxy_names, &group_names, &provider_names)
                    {
                        warnings.push(format!(
                            "rules[{}] targets unknown policy `{}`",
                            index, policy
                        ));
                    }
                }
            }
            None => errors.push("`rules` must be a list of strings".to_string()),
        }
    }

    ValidationReport {
        ok: errors.is_empty(),
        errors,
        warnings,
        summary: Some(summary),
    }
}

fn is_known_target(
    name: &str,
    proxies: &BTreeSet<String>,
    groups: &BTreeSet<String>,
    providers: &BTreeSet<String>,
) -> bool {
    matches!(
        name,
        "DIRECT" | "REJECT" | "REJECT-DROP" | "PASS" | "COMPATIBLE" | "GLOBAL"
    ) || proxies.contains(name)
        || groups.contains(name)
        || providers.contains(name)
}

/// Read the raw active config for the editor.
pub fn read_raw() -> Result<(String, String), String> {
    let path = config_path();
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    let modified = std::fs::metadata(&path)
        .ok()
        .and_then(|m| m.modified().ok())
        .map(|t| chrono::DateTime::<chrono::Local>::from(t).to_rfc3339())
        .unwrap_or_default();
    Ok((content, modified))
}

/// Create a backup of the current config. `reason` is sanitized into the name.
pub fn create_backup(reason: &str) -> Result<BackupInfo, String> {
    let content = std::fs::read_to_string(config_path())
        .map_err(|e| format!("No active config to back up: {}", e))?;
    let dir = backups_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create {}: {}", dir.display(), e))?;
    control_tower_service_core::fsutil::restrict_dir_permissions(&dir);

    let safe_reason: String = reason
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let id = if safe_reason.is_empty() {
        stamp.to_string()
    } else {
        format!("{}-{}", stamp, safe_reason)
    };
    let path = dir.join(format!("config-{}.yaml", id));
    control_tower_service_core::write_private_atomic(&path, &content)
        .map_err(|e| format!("Failed to write backup: {}", e))?;

    let info = BackupInfo {
        id: id.clone(),
        reason: safe_reason,
        created_at: chrono::Local::now().to_rfc3339(),
        size: content.len() as u64,
    };
    prune_backups();
    Ok(info)
}

/// List backups newest first.
pub fn list_backups() -> Vec<BackupInfo> {
    let dir = backups_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut backups: Vec<BackupInfo> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let id = name.strip_prefix("config-")?.strip_suffix(".yaml")?.to_string();
            let meta = entry.metadata().ok()?;
            let created = meta
                .modified()
                .ok()
                .map(|t| chrono::DateTime::<chrono::Local>::from(t).to_rfc3339())
                .unwrap_or_default();
            // id = <stamp>[-<reason>]
            let reason = id
                .split_once('-')
                .and_then(|(_, rest)| rest.split_once('-').map(|(_, r)| r.to_string()))
                .unwrap_or_default();
            Some(BackupInfo {
                id,
                reason,
                created_at: created,
                size: meta.len(),
            })
        })
        .collect();
    backups.sort_by(|a, b| b.id.cmp(&a.id));
    backups
}

pub fn delete_backup(id: &str) -> Result<(), String> {
    let path = backup_path(id)?;
    if !path.exists() {
        return Err("Backup not found".to_string());
    }
    std::fs::remove_file(&path)
        .map_err(|e| format!("Failed to delete {}: {}", path.display(), e))
}

pub fn read_backup(id: &str) -> Result<String, String> {
    let path = backup_path(id)?;
    std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))
}

fn backup_path(id: &str) -> Result<PathBuf, String> {
    // Reject path traversal: ids are file stems, never paths.
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Invalid backup id".to_string());
    }
    Ok(backups_dir().join(format!("config-{}.yaml", id)))
}

fn prune_backups() {
    let backups = list_backups();
    for old in backups.iter().skip(MAX_BACKUPS) {
        let _ = delete_backup(&old.id);
    }
}

/// Write a validated config to disk, keeping the previous content as a backup.
pub fn save(content: &str) -> Result<ConfigSummary, String> {
    let report = validate(content);
    if !report.ok {
        return Err(report.errors.join("; "));
    }
    if config_path().exists() {
        let _ = create_backup("pre-save");
    }
    let path = config_path();
    control_tower_service_core::write_private_atomic(&path, content)
        .map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
    Ok(report.summary.unwrap_or_default())
}

/// Ask the running Mihomo to reload the config file.
pub fn reload(api_url: &str) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
    let url = format!("{}/configs?force=true", api_url);
    let response = client
        .put(&url)
        .json(&serde_json::json!({}))
        .send()
        .map_err(|e| format!("Failed to reload Mihomo: {}", e))?;
    if !response.status().is_success() {
        return Err(format!("Mihomo rejected the config: {}", response.status()));
    }
    Ok(())
}
