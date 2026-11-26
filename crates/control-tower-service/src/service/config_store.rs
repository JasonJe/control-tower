//! Active-config editing, validation, backups and rollback.
//!
//! Hand-editing `config.yaml` is a common request: the Web UI shows the whole
//! config, but writing it back unchecked could lock the user out of their
//! proxy. Every save therefore goes through validation plus a backup, and the
//! previous contents are restored if Mihomo refuses to reload.
//!
//! Validation itself lives in `control-tower-service-core::config_validate` so
//! the CLI's `config validate` reports exactly the same result.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

pub use control_tower_service_core::config_validate::{validate, ConfigSummary};

/// Maximum number of automatic backups kept on disk.
const MAX_BACKUPS: usize = 20;

pub fn config_path() -> PathBuf {
    crate::settings::resolve_paths().active_config_path
}

pub fn backups_dir() -> PathBuf {
    crate::settings::resolve_paths().config_dir.join("backups")
}

/// One file in the backups directory.
#[derive(Debug, Clone, Serialize)]
pub struct BackupInfo {
    pub id: String,
    pub reason: String,
    pub created_at: String,
    pub size: u64,
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
