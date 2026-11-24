//! Profile management module

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use control_tower_service_core::ProfileDownloadOptions;

use crate::config::{get_config_dir, load_profiles};
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
    /// Download options such as custom headers. Preserved across CLI writes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<ProfileDownloadOptions>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub profile_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merge: Vec<String>,
}

pub async fn handle(action: ProfileAction) -> Result<()> {
    match action {
        ProfileAction::Add { url, name, headers } => {
            add_profile(&url, name.as_deref(), &headers).await?
        }
        ProfileAction::List => list_profiles().await?,
        ProfileAction::Remove { id } => remove_profile(&id).await?,
        ProfileAction::Update { id, headers } => update_profile(id.as_deref(), &headers).await?,
        ProfileAction::Activate { id } => activate_profile(&id).await?,
        ProfileAction::Cron { id, schedule } => set_cron(&id, schedule.as_deref()).await?,
    }
    Ok(())
}

async fn add_profile(url: &str, name: Option<&str>, headers: &[String]) -> Result<()> {
    tracing::info!("Adding profile from URL: {}", url);

    let options = build_download_options(headers)?;

    // Download subscription
    let content = download_subscription(url, options.as_ref()).await?;

    // Generate ID
    let id = format!("profile-{}", Uuid::new_v4());
    let profile_name = name.unwrap_or("Unnamed Profile").to_string();

    // Save subscription content to file
    let config_dir = get_config_dir()?;
    let profiles_dir = config_dir.join("profiles");
    std::fs::create_dir_all(&profiles_dir)?;

    let profile_file = profiles_dir.join(format!("{}.yaml", id));
    control_tower_service_core::write_private_atomic(&profile_file, &content)?;

    // Create profile
    let item = control_tower_service_core::ProfileItem {
        uid: id.clone(),
        name: Some(profile_name),
        url: Some(url.to_string()),
        file: Some(profile_file.to_string_lossy().to_string()),
        cron: None,
        updated_at: Some(chrono::Utc::now().timestamp()),
        options,
        type_: Some("remote".to_string()),
        script: None,
        merge: Vec::new(),
    };

    crate::config::update_profiles_file(|yaml| {
        yaml.items.push(item);
        Ok(())
    })?;

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

    // Remove the entry under the profiles.yaml lock and return its file path.
    let profile_file = crate::config::update_profiles_file(|yaml| {
        let item = yaml
            .items
            .iter()
            .find(|p| p.uid == id)
            .ok_or_else(|| anyhow::anyhow!("Profile not found: {}", id))?;
        let file = item.file.clone();

        yaml.items.retain(|p| p.uid != id);
        if yaml.current.as_deref() == Some(id) {
            yaml.current = None;
        }
        Ok(file)
    })?;

    // Delete the profile file if exists
    if let Some(ref file) = profile_file {
        let path = PathBuf::from(file);
        if path.exists() {
            std::fs::remove_file(&path)?;
            println!("Deleted profile file: {:?}", path);
        }
    }

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

async fn update_profile(id: Option<&str>, headers: &[String]) -> Result<()> {
    let profiles = load_profiles()?;

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

    let url = profiles[profile_idx]
        .url
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Profile has no URL"))?;

    tracing::info!("Updating profile {} from URL: {}", target_id, url);

    // Reuse stored download options (headers, timeout, ...). An explicit
    // --header on the command line replaces the stored header set.
    let mut options = profiles[profile_idx]
        .options
        .clone()
        .unwrap_or_default();
    if !headers.is_empty() {
        options.headers = Some(parse_headers(headers)?);
    }

    // Download new content. Surface the effective User-Agent so a subscription
    // source that filters by UA can be debugged with `ctctl profile update`.
    println!(
        "Updating {} from {} (User-Agent: {})",
        target_id,
        url,
        options.effective_user_agent("clash-verge/v2.4.7")
    );
    let content = download_subscription(&url, Some(&options)).await?;

    // Update file
    if let Some(ref file) = profiles[profile_idx].file {
        control_tower_service_core::write_private_atomic(std::path::Path::new(file), &content)?;
    }

    // Persist options so later updates (and cron jobs) keep using the headers.
    // Only write the field when it was already present or the user supplied
    // headers, otherwise every update would add a noisy `options: {}` block.
    let persist_options = profiles[profile_idx].options.is_some() || !headers.is_empty();
    let options_for_write = options;

    // Update timestamp and options in the manifest under the shared lock.
    crate::config::update_profiles_file(|yaml| {
        if let Some(item) = yaml.items.iter_mut().find(|item| item.uid == target_id) {
            if persist_options {
                item.options = Some(options_for_write);
            }
            item.updated_at = Some(chrono::Utc::now().timestamp());
        }
        Ok(())
    })?;

    println!("Updated profile: {}", target_id);
    Ok(())
}

async fn activate_profile(id: &str) -> Result<()> {
    let profiles = load_profiles()?;

    let profile = profiles
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| anyhow::anyhow!("Profile not found: {}", id))?;
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
        store.replace_from_profile(&profile_file, &[], Default::default())?;
        println!("Config file copied to: {:?}", store.active_config_path());
    } else {
        anyhow::bail!("Profile has no config file");
    }

    // Mark the selected profile as current.
    crate::config::update_profiles_file(|yaml| {
        if !yaml.items.iter().any(|p| p.uid == id) {
            anyhow::bail!("Profile not found: {}", id);
        }
        yaml.current = Some(id.to_string());
        Ok(())
    })?;

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
    // Display mode: show the current value without touching the file.
    let Some(schedule) = schedule else {
        let profiles = load_profiles()?;
        let profile = profiles
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| anyhow::anyhow!("Profile not found: {}", id))?;
        match profile.cron {
            Some(ref cron) => {
                println!("Cron schedule for profile {}: {}", id, cron);
                println!("  Human readable: {}", cron_to_human(cron));
            }
            None => println!("No cron schedule set for profile: {}", id),
        }
        return Ok(());
    };

    // Validate before writing so a typo does not modify profiles.yaml.
    let new_cron: Option<String> = if schedule.eq_ignore_ascii_case("off") {
        None
    } else {
        let minutes: u32 = schedule.parse().map_err(|_| {
            anyhow::anyhow!("Invalid minutes value: {}. Must be a number.", schedule)
        })?;
        if !(1..=10080).contains(&minutes) {
            anyhow::bail!(
                "Invalid minutes value: {}. Must be between 1 and 10080 (1 week).",
                minutes
            );
        }
        Some(schedule.to_string())
    };

    crate::config::update_profiles_file(|yaml| {
        let item = yaml
            .items
            .iter_mut()
            .find(|p| p.uid == id)
            .ok_or_else(|| anyhow::anyhow!("Profile not found: {}", id))?;
        item.cron = new_cron.clone();
        Ok(())
    })?;

    match new_cron {
        Some(ref cron) => {
            let minutes: u32 = cron.parse().unwrap_or(0);
            println!("Set cron schedule for profile {}: every {} minutes", id, minutes);
        }
        None => println!("Disabled automatic updates for profile: {}", id),
    }

    let _ = reload_cron().await;
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

/// Parse `KEY: VALUE` command-line headers into a header map.
fn parse_headers(headers: &[String]) -> Result<std::collections::BTreeMap<String, String>> {
    let mut map = std::collections::BTreeMap::new();
    for raw in headers {
        let (name, value) = raw
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("Invalid header '{}': expected 'KEY: VALUE'", raw))?;
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() {
            anyhow::bail!("Invalid header '{}': header name is empty", raw);
        }
        if name.contains(['\r', '\n']) || value.contains(['\r', '\n']) {
            anyhow::bail!("Invalid header '{}': line breaks are not allowed", raw);
        }
        map.insert(name.to_string(), value.to_string());
    }
    Ok(map)
}

/// Build download options from CLI headers (None when no headers were given).
fn build_download_options(headers: &[String]) -> Result<Option<ProfileDownloadOptions>> {
    if headers.is_empty() {
        return Ok(None);
    }
    Ok(Some(ProfileDownloadOptions {
        headers: Some(parse_headers(headers)?),
        ..Default::default()
    }))
}

async fn download_subscription(
    url: &str,
    options: Option<&ProfileDownloadOptions>,
) -> Result<String> {
    let timeout = options.map(|o| o.timeout_or(30)).unwrap_or(30);
    let with_proxy = options.and_then(|o| o.with_proxy).unwrap_or(false);
    let danger_accept_invalid_certs = options
        .and_then(|o| o.danger_accept_invalid_certs)
        .unwrap_or(false);

    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout))
        .danger_accept_invalid_certs(danger_accept_invalid_certs);
    if with_proxy {
        let proxy_url = format!("http://127.0.0.1:{}", crate::settings::get_http_port());
        builder = builder.proxy(
            reqwest::Proxy::http(&proxy_url)
                .map_err(|e| anyhow::anyhow!("Failed to create proxy: {}", e))?,
        );
    }
    let client = builder
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {}", e))?;

    let user_agent = options
        .map(|o| o.effective_user_agent("clash-verge/v2.4.7"))
        .unwrap_or("clash-verge/v2.4.7");
    let mut request = client.get(url);
    request = request.header("User-Agent", user_agent);
    if let Some(options) = options {
        for (name, value) in options.header_pairs_without_user_agent() {
            request = request.header(name, value);
        }
    }

    let response = request
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to download: {}", e))?;

    if !response.status().is_success() {
        anyhow::bail!("Download failed: {}", response.status());
    }

    Ok(control_tower_service_core::download::text_limited(
        response,
        control_tower_service_core::download::MAX_SUBSCRIPTION_BYTES,
    )
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_custom_headers() {
        let headers = vec![
            "X-Token: abc".to_string(),
            "Authorization:Bearer x:y".to_string(),
        ];
        let map = parse_headers(&headers).unwrap();
        assert_eq!(map.get("X-Token").map(String::as_str), Some("abc"));
        assert_eq!(
            map.get("Authorization").map(String::as_str),
            Some("Bearer x:y")
        );
    }

    #[test]
    fn rejects_invalid_header() {
        assert!(parse_headers(&["NoColon".to_string()]).is_err());
        assert!(parse_headers(&[": value".to_string()]).is_err());
        assert!(parse_headers(&["X-Test: a\r\nInjected: b".to_string()]).is_err());
    }

    #[test]
    fn empty_header_list_produces_no_options() {
        assert!(build_download_options(&[]).unwrap().is_none());
    }
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
