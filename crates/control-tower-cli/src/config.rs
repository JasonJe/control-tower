//! Configuration management module

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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

    #[derive(Deserialize)]
    struct ProfilesYaml {
        current: Option<String>,
        items: Vec<ProfileItem>,
    }

    #[derive(Deserialize)]
    struct ProfileItem {
        uid: String,
        name: String,
        #[serde(rename = "file")]
        file: Option<String>,
        url: Option<String>,
        cron: Option<String>,
        #[serde(rename = "updated_at")]
        updated_at: Option<i64>,
    }

    let yaml: ProfilesYaml = serde_yaml_ng::from_str(&content)?;

    let profiles: Vec<Profile> = yaml.items.into_iter().map(|item| {
        let active = yaml.current.as_ref().map(|c| c == &item.uid).unwrap_or(false);
        Profile {
            id: item.uid,
            name: item.name,
            url: item.url,
            file: item.file,
            active,
            updated_at: item.updated_at,
            cron: item.cron,
        }
    }).collect();

    Ok(profiles)
}

pub fn save_profiles(profiles: &[Profile]) -> Result<()> {
    let config_dir = get_config_dir()?;
    fs::create_dir_all(&config_dir)?;

    #[derive(Serialize)]
    struct ProfilesYaml {
        current: Option<String>,
        items: Vec<ProfileItem>,
    }

    #[derive(Serialize)]
    struct ProfileItem {
        uid: String,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        file: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cron: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        updated_at: Option<i64>,
    }

    let current = profiles.iter().find(|p| p.active).map(|p| p.id.clone());

    let items: Vec<ProfileItem> = profiles.iter().map(|p| ProfileItem {
        uid: p.id.clone(),
        name: p.name.clone(),
        file: p.file.clone(),
        url: p.url.clone(),
        cron: p.cron.clone(),
        updated_at: p.updated_at,
    }).collect();

    let yaml = ProfilesYaml { current, items };
    let content = serde_yaml_ng::to_string(&yaml)?;

    let profiles_path = config_dir.join("profiles.yaml");
    fs::write(profiles_path, content)?;

    Ok(())
}
