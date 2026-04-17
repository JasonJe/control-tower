//! Profile management module

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::config::{get_config_dir, load_profiles, save_profiles};
use crate::service::reload_cron;
use crate::ProfileAction;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
    /// Cron schedule for automatic updates (e.g., "0 6 * * *" for daily 6am)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
}

pub async fn handle(action: ProfileAction) -> Result<()> {
    match action {
        ProfileAction::Add { url, name } => add_profile(&url, name.as_deref()).await?,
        ProfileAction::List => list_profiles().await?,
        ProfileAction::Remove { id } => remove_profile(&id).await?,
        ProfileAction::Update { id } => update_profile(id.as_deref()).await?,
        ProfileAction::Activate { id } => activate_profile(&id).await?,
        ProfileAction::Cron { id, schedule } => set_cron(&id, schedule.as_deref()).await?,
    }
    Ok(())
}

async fn add_profile(url: &str, name: Option<&str>) -> Result<()> {
    tracing::info!("Adding profile from URL: {}", url);

    // Download subscription
    let content = download_subscription(url).await?;

    // Generate ID
    let id = format!("profile-{}", Uuid::new_v4());
    let profile_name = name.unwrap_or("Unnamed Profile").to_string();

    // Save subscription content to file
    let config_dir = get_config_dir()?;
    let profiles_dir = config_dir.join("profiles");
    std::fs::create_dir_all(&profiles_dir)?;

    let profile_file = profiles_dir.join(format!("{}.yaml", id));
    std::fs::write(&profile_file, &content)?;

    // Create profile
    let profile = Profile {
        id: id.clone(),
        name: profile_name,
        url: Some(url.to_string()),
        file: Some(profile_file.to_string_lossy().to_string()),
        active: false,
        updated_at: Some(chrono::Utc::now().timestamp()),
        cron: None,
    };

    // Load existing profiles
    let mut profiles = load_profiles().unwrap_or_default();

    // Add new profile
    profiles.push(profile);
    save_profiles(&profiles)?;

    println!("Added profile: {} ({})", id, name.unwrap_or("Unnamed"));
    Ok(())
}

async fn list_profiles() -> Result<()> {
    let profiles = load_profiles()?;

    if profiles.is_empty() {
        println!("No profiles found.");
        return Ok(());
    }

    println!("Profiles:");
    println!("{:<38}  {:<24}  {:<8}  {:<20}  {:<15}", "ID", "Name", "Active", "Last Updated", "Cron");
    println!("{}", "-".repeat(102));

    for p in &profiles {
        let active_str = if p.active { "[*]" } else { "[ ]" };
        let cron_str = truncate_str(p.cron.as_deref().unwrap_or("-"), 15);
        let updated_str = if let Some(ts) = p.updated_at {
            chrono::DateTime::from_timestamp(ts, 0)
                .map(|dt| dt.with_timezone(&chrono::Local))
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| ts.to_string())
        } else {
            "-".to_string()
        };
        println!("{:<38}  {:<24}  {:<8}  {:<20}  {:<15}",
            p.id,
            truncate_str(&p.name, 24),
            active_str,
            updated_str,
            cron_str
        );
    }

    Ok(())
}

async fn remove_profile(id: &str) -> Result<()> {
    tracing::info!("Removing profile: {}", id);

    let mut profiles = load_profiles()?;

    // Find the profile to delete and get its file path
    let profile_to_delete = profiles.iter()
        .find(|p| p.id == id)
        .ok_or_else(|| anyhow::anyhow!("Profile not found: {}", id))?;

    let profile_file = profile_to_delete.file.clone();

    // Remove from profiles list
    profiles.retain(|p| p.id != id);

    // Delete the profile file if exists
    if let Some(ref file) = profile_file {
        let path = PathBuf::from(file);
        if path.exists() {
            std::fs::remove_file(&path)?;
            println!("Deleted profile file: {:?}", path);
        }
    }

    save_profiles(&profiles)?;

    println!("Removed profile: {}", id);

    // User主动删除profile，停止服务并清空配置
    println!("Stopping service and clearing config...");
    let config_dir = get_config_dir()?;

    // Delete config.yaml
    let config_path = config_dir.join("config.yaml");
    if config_path.exists() {
        std::fs::remove_file(&config_path)?;
        println!("Deleted config file: {:?}", config_path);
    }

    crate::service::stop_service().await?;

    Ok(())
}

async fn update_profile(id: Option<&str>) -> Result<()> {
    let mut profiles = load_profiles()?;

    let target_id = if let Some(id) = id {
        id.to_string()
    } else {
        // Find current active profile
        if let Some(active) = profiles.iter().find(|p| p.active) {
            active.id.clone()
        } else {
            anyhow::bail!("No active profile found");
        }
    };

    // Find the profile index
    let profile_idx = profiles.iter().position(|p| p.id == target_id)
        .ok_or_else(|| anyhow::anyhow!("Profile not found: {}", target_id))?;

    let url = profiles[profile_idx].url.as_ref()
        .ok_or_else(|| anyhow::anyhow!("Profile has no URL"))?;

    tracing::info!("Updating profile {} from URL: {}", target_id, url);

    // Download new content
    let content = download_subscription(url).await?;

    // Update file
    if let Some(ref file) = profiles[profile_idx].file {
        std::fs::write(file, &content)?;
    }

    // Update timestamp
    profiles[profile_idx].updated_at = Some(chrono::Utc::now().timestamp());
    save_profiles(&profiles)?;

    println!("Updated profile: {}", target_id);
    Ok(())
}

async fn activate_profile(id: &str) -> Result<()> {
    let mut profiles = load_profiles()?;

    // Find the profile index
    let profile_idx = profiles.iter().position(|p| p.id == id)
        .ok_or_else(|| anyhow::anyhow!("Profile not found: {}", id))?;

    let profile = &profiles[profile_idx];
    let profile_name = profile.name.clone();

    // Copy the profile's config file to config.yaml via centralized store
    if let Some(ref file) = profile.file {
        let profile_file = PathBuf::from(file);
        if !profile_file.exists() {
            anyhow::bail!("Profile config file not found: {}", file);
        }

        let store = control_tower_service_core::ActiveConfigStore::new(
            crate::settings::shared_paths()?,
        );
        store.replace_from_profile(&profile_file)?;
        println!("Config file copied to: {:?}", store.active_config_path());
    } else {
        anyhow::bail!("Profile has no config file");
    }

    // Deactivate all profiles first and activate the selected one
    for p in profiles.iter_mut() {
        p.active = false;
    }
    profiles[profile_idx].active = true;

    save_profiles(&profiles)?;

    println!("Activated profile: {} ({})", id, profile_name);

    // If service is not running, start it first
    if !crate::service::is_clash_api_running() {
        println!("Service not running. Starting service with new configuration...");
        crate::service::start_service().await?;
    } else {
        // Restart service to reload config
        println!("Restarting service to apply new configuration...");
        crate::service::restart_service().await?;
    }

    Ok(())
}

async fn set_cron(id: &str, schedule: Option<&str>) -> Result<()> {
    let mut profiles = load_profiles()?;

    // Find the profile
    let profile = profiles.iter_mut().find(|p| p.id == id)
        .ok_or_else(|| anyhow::anyhow!("Profile not found: {}", id))?;

    match schedule {
        None => {
            // Show current cron setting
            if let Some(ref cron) = profile.cron {
                println!("Cron schedule for profile {}: {}", id, cron);
                println!("  Human readable: {}", cron_to_human(cron));
            } else {
                println!("No cron schedule set for profile: {}", id);
            }
        }
        Some(s) if s.to_lowercase() == "off" => {
            profile.cron = None;
            println!("Disabled automatic updates for profile: {}", id);
            save_profiles(&profiles)?;
            let _ = reload_cron().await;
        }
        Some(s) => {
            // Validate: must be a number representing minutes
            let minutes: u32 = s.parse()
                .map_err(|_| anyhow::anyhow!("Invalid minutes value: {}. Must be a number.", s))?;

            if minutes < 1 || minutes > 10080 { // Max 1 week
                anyhow::bail!("Invalid minutes value: {}. Must be between 1 and 10080 (1 week).", minutes);
            }

            profile.cron = Some(s.to_string());
            println!("Set cron schedule for profile {}: every {} minutes", id, minutes);
            save_profiles(&profiles)?;
            let _ = reload_cron().await;
        }
    }

    Ok(())
}

/// Convert cron expression to human readable description
fn cron_to_human(cron: &str) -> String {
    let cron_lower = cron.to_lowercase();

    if cron_lower == "off" {
        return "Disabled".to_string();
    }

    if let Ok(minutes) = cron.parse::<u32>() {
        if minutes == 1 {
            return "Every 1 minute".to_string();
        }
        return format!("Every {} minutes", minutes);
    }

    cron.to_string()
}

async fn download_subscription(url: &str) -> Result<String> {
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .header("User-Agent", "clash-verge/v2.4.7")
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to download: {}", e))?;

    if !response.status().is_success() {
        anyhow::bail!("Download failed: {}", response.status());
    }

    let text = response.text()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to read response: {}", e))?;

    Ok(text)
}

/// Truncate string to max_width characters, showing start and end with "..." in middle if truncated
fn truncate_str(s: &str, max_width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let char_count = chars.len();
    if char_count <= max_width {
        return s.to_string();
    }

    // Need at least 5 chars: start + "..." + end (3 dots = 3)
    if max_width < 5 {
        return chars[..max_width].iter().collect();
    }

    // Calculate: total = start_len + 3 (dots) + end_len
    // So: start_len + end_len = max_width - 3
    let remaining = max_width - 3;
    let start_len = remaining / 2;
    let end_len = remaining - start_len;

    // Start: first start_len characters
    let start: String = chars[..start_len].iter().collect();
    // End: last end_len characters
    let end: String = chars[char_count - end_len..].iter().collect();

    format!("{}...{}", start, end)
}
