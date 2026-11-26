//! Configuration management module

use anyhow::Result;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

use control_tower_service_core::{config_validate, ProfilesYaml};

use crate::profile::Profile;
use crate::{BackupAction, ConfigAction};

pub fn get_config_dir() -> Result<PathBuf> {
    Ok(crate::settings::shared_paths()?.config_dir)
}

pub fn load_profiles() -> Result<Vec<Profile>> {
    let config_dir = get_config_dir()?;
    let profiles_path = config_dir.join("profiles.yaml");

    if !profiles_path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&profiles_path)?;
    let yaml: ProfilesYaml = serde_yaml_ng::from_str(&content)?;

    let profiles: Vec<Profile> = yaml
        .items
        .into_iter()
        .map(|item| {
            let active = yaml.current.as_ref() == Some(&item.uid);
            Profile {
                id: item.uid,
                name: item.name.unwrap_or_else(|| "Unnamed Profile".to_string()),
                url: item.url,
                file: item.file,
                active,
                updated_at: item.updated_at,
                cron: item.cron,
                options: item.options,
                profile_type: item.type_,
                script: item.script,
                merge: item.merge,
                upload: item.upload,
                download: item.download,
                total: item.total,
                expire: item.expire,
            }
        })
        .collect();

    Ok(profiles)
}

/// Apply a change to profiles.yaml under the shared lock.
///
/// The CLI and the service can both write this file. `update_profiles`
/// re-reads it inside the lock, so a change made by the service (for example a
/// cron refresh) is not silently overwritten by a stale CLI snapshot.
pub fn update_profiles_file<T>(
    f: impl FnOnce(&mut ProfilesYaml) -> Result<T>,
) -> Result<T> {
    let config_dir = get_config_dir()?;
    fs::create_dir_all(&config_dir)?;
    control_tower_service_core::fsutil::restrict_dir_permissions(&config_dir);

    Ok(control_tower_service_core::update_profiles(
        &config_dir.join("profiles.yaml"),
        f,
    )?)
}

// ---------------------------------------------------------------------------
// config show / validate / backup / edit
// ---------------------------------------------------------------------------

/// Maximum number of automatic backups kept on disk (same as the service).
const MAX_BACKUPS: usize = 20;

#[derive(Debug, Clone, Serialize)]
pub struct BackupInfo {
    pub id: String,
    pub reason: String,
    pub created_at: String,
    pub size: u64,
}

fn config_path() -> Result<PathBuf> {
    Ok(crate::settings::shared_paths()?.active_config_path)
}

fn backups_dir() -> Result<PathBuf> {
    Ok(crate::settings::shared_paths()?.config_dir.join("backups"))
}

fn backup_path(id: &str) -> Result<PathBuf> {
    // Reject path traversal: ids are file stems, never paths. This matches
    // the service's config_store::backup_path.
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        anyhow::bail!("Invalid backup id: {}", id);
    }
    Ok(backups_dir()?.join(format!("config-{}.yaml", id)))
}

fn modified_string(path: &Path) -> String {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .map(|t| chrono::DateTime::<chrono::Local>::from(t).to_rfc3339())
        .unwrap_or_default()
}

pub fn handle(action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::Show { json } => show(json),
        ConfigAction::Validate { path } => validate(path.as_deref()),
        ConfigAction::Backup { action } => match action {
            BackupAction::List { json } => list_backups(json),
            BackupAction::Create { reason } => {
                let info = create_backup(reason.as_deref().unwrap_or("manual"))?;
                println!("Backup created: {} ({})", info.id, info.created_at);
                Ok(())
            }
            BackupAction::Restore { id } => restore(&id),
            BackupAction::Delete { id } => {
                let path = backup_path(&id)?;
                if !path.exists() {
                    anyhow::bail!("Backup not found: {}", id);
                }
                fs::remove_file(&path)?;
                println!("Deleted backup: {}", id);
                Ok(())
            }
        },
        ConfigAction::Edit => edit(),
    }
}

fn show(json: bool) -> Result<()> {
    let path = config_path()?;
    let content = fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path.display(), e))?;
    if json {
        let report = config_validate::validate(&content);
        let value = serde_json::json!({
            "path": path.display().to_string(),
            "modified": modified_string(&path),
            "content": content,
            "validation": report,
        });
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    print!("{}", content);
    if !content.ends_with('\n') {
        println!();
    }
    eprintln!("Config: {}", path.display());
    Ok(())
}

fn validate(path: Option<&Path>) -> Result<()> {
    let path = match path {
        Some(p) => p.to_path_buf(),
        None => config_path()?,
    };
    let content = fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path.display(), e))?;
    let report = config_validate::validate(&content);

    println!("Validating: {}", path.display());
    println!("Result: {}", if report.ok { "OK" } else { "INVALID" });
    if let Some(summary) = &report.summary {
        println!(
            "Proxies: {}  Groups: {}  Rules: {}  Rule providers: {}  Proxy providers: {}  Size: {} bytes",
            summary.proxies,
            summary.proxy_groups,
            summary.rules,
            summary.rule_providers,
            summary.proxy_providers,
            summary.size
        );
    }
    for warning in &report.warnings {
        println!("Warning: {}", warning);
    }
    for error in &report.errors {
        println!("Error: {}", error);
    }
    if report.ok {
        println!("\nTip: use `ctctl config edit` to apply changes safely (backup + reload + rollback).");
        Ok(())
    } else {
        anyhow::bail!("Config validation failed")
    }
}

fn list_backups(json: bool) -> Result<()> {
    let dir = backups_dir()?;
    let mut backups: Vec<BackupInfo> = fs::read_dir(&dir)
        .into_iter()
        .flatten()
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

    if json {
        println!("{}", serde_json::to_string_pretty(&backups)?);
        return Ok(());
    }
    if backups.is_empty() {
        println!("No backups found in {}", dir.display());
        return Ok(());
    }
    println!("Backups ({}):", backups.len());
    println!("{:<44}  {:<14}  {:>10}  {}", "ID", "Reason", "Size", "Created");
    println!("{}", "-".repeat(96));
    for backup in &backups {
        println!(
            "{:<44}  {:<14}  {:>10}  {}",
            backup.id, backup.reason, backup.size, backup.created_at
        );
    }
    println!("\nRestore with: ctctl config backup restore <ID>");
    Ok(())
}

fn create_backup(reason: &str) -> Result<BackupInfo> {
    let path = config_path()?;
    let content = fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("No active config to back up: {}", e))?;
    let dir = backups_dir()?;
    fs::create_dir_all(&dir)?;
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
    let backup_file = dir.join(format!("config-{}.yaml", id));
    control_tower_service_core::write_private_atomic(&backup_file, &content)?;

    // Keep the same retention policy as the service.
    let mut existing: Vec<PathBuf> = fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("config-") && name.ends_with(".yaml") {
                Some(entry.path())
            } else {
                None
            }
        })
        .collect();
    existing.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    for old in existing.into_iter().skip(MAX_BACKUPS) {
        let _ = fs::remove_file(old);
    }

    Ok(BackupInfo {
        id,
        reason: safe_reason,
        created_at: chrono::Local::now().to_rfc3339(),
        size: content.len() as u64,
    })
}

fn restore(id: &str) -> Result<()> {
    let backup = backup_path(id)?;
    let content = fs::read_to_string(&backup)
        .map_err(|e| anyhow::anyhow!("Failed to read backup {}: {}", id, e))?;
    let report = config_validate::validate(&content);
    if !report.ok {
        anyhow::bail!("Backup {} is invalid: {}", id, report.errors.join("; "));
    }

    if crate::service::ipc_available() {
        let summary = crate::service::restore_config(id)?;
        println!("Restored backup: {}", id);
        if let Some(s) = summary.as_object() {
            println!(
                "Proxies: {}  Groups: {}  Rules: {}",
                s.get("proxies").and_then(|v| v.as_u64()).unwrap_or(0),
                s.get("proxy_groups").and_then(|v| v.as_u64()).unwrap_or(0),
                s.get("rules").and_then(|v| v.as_u64()).unwrap_or(0)
            );
        }
        return Ok(());
    }

    // Service is offline: still restore the file, but tell the user to restart.
    create_backup("before-restore")?;
    let path = config_path()?;
    control_tower_service_core::write_private_atomic(&path, &content)?;
    println!("Restored backup {} to {} (service offline)", id, path.display());
    println!("Run `ctctl service start` or restart ctsvc once the service is available.");
    Ok(())
}

fn edit() -> Result<()> {
    let path = config_path()?;
    let original = fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path.display(), e))?;

    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());
    let dir = get_config_dir()?;
    let tmp = dir.join(format!(".config-edit-{}-{}", std::process::id(), chrono::Local::now().format("%H%M%S")));
    control_tower_service_core::write_private_atomic(&tmp, &original)?;

    let status = std::process::Command::new(&editor)
        .arg(&tmp)
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to launch editor `{}`: {}", editor, e))?;
    if !status.success() {
        let _ = fs::remove_file(&tmp);
        anyhow::bail!("Editor exited with an error; config was not changed");
    }

    let edited = fs::read_to_string(&tmp)?;
    let _ = fs::remove_file(&tmp);
    if edited == original {
        println!("No changes made.");
        return Ok(());
    }

    let report = config_validate::validate(&edited);
    if !report.ok {
        for error in &report.errors {
            println!("Error: {}", error);
        }
        anyhow::bail!("Edited config is invalid; nothing was saved");
    }
    for warning in &report.warnings {
        println!("Warning: {}", warning);
    }

    if !crate::service::ipc_available() {
        anyhow::bail!("Service socket not found; start ctsvc before editing the active config");
    }
    let summary = crate::service::save_config(&edited)?;
    println!("Config saved and applied.");
    if let Some(s) = summary.as_object() {
        println!(
            "Proxies: {}  Groups: {}  Rules: {}",
            s.get("proxies").and_then(|v| v.as_u64()).unwrap_or(0),
            s.get("proxy_groups").and_then(|v| v.as_u64()).unwrap_or(0),
            s.get("rules").and_then(|v| v.as_u64()).unwrap_or(0)
        );
    }
    println!("A backup of the previous config was created automatically.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_path_rejects_traversal() {
        assert!(backup_path("../../etc/passwd").is_err());
        assert!(backup_path("20260101-000000-manual").is_ok());
    }
}
