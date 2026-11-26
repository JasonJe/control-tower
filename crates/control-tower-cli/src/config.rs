//! Configuration management module

use anyhow::Result;
use std::fs;
use std::path::PathBuf;

use control_tower_service_core::ProfilesYaml;

use crate::profile::Profile;

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
