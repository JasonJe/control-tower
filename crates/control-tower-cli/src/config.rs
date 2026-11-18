//! Configuration management module

use anyhow::Result;
use std::fs;
use std::path::PathBuf;

use control_tower_service_core::{ProfileItem, ProfilesYaml};

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
            }
        })
        .collect();

    Ok(profiles)
}

pub fn save_profiles(profiles: &[Profile]) -> Result<()> {
    let config_dir = get_config_dir()?;
    fs::create_dir_all(&config_dir)?;

    let current = profiles.iter().find(|p| p.active).map(|p| p.id.clone());

    let items: Vec<ProfileItem> = profiles
        .iter()
        .map(|p| ProfileItem {
            uid: p.id.clone(),
            name: Some(p.name.clone()),
            file: p.file.clone(),
            url: p.url.clone(),
            cron: p.cron.clone(),
            updated_at: p.updated_at,
            options: p.options.clone(),
            type_: p.profile_type.clone().or_else(|| Some("remote".to_string())),
            script: p.script.clone(),
            merge: p.merge.clone(),
        })
        .collect();

    let yaml = ProfilesYaml { current, items };
    let content = serde_yaml_ng::to_string(&yaml)?;

    let profiles_path = config_dir.join("profiles.yaml");
    fs::write(profiles_path, content)?;

    Ok(())
}
