//! Profile handlers: list, add, activate, update, refresh, delete

use actix_web::{web, HttpResponse};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::task;

use crate::ServiceState;
use control_tower_service_core::ProfilesYaml;

use super::{ApiResponse, AddProfileRequest, RefreshRequest};

/// Resolve the on-disk path for a profile file.
///
/// Tries in order:
///   1. `profiles_dir/file_name` (if file_name is set)
///   2. `profiles_dir/{uid[..8]}.yaml` (canonical fallback)
///   3. `profiles_dir/{uid}.yaml` (legacy fallback for old buggy data)
fn resolve_profile_file(profiles_dir: &PathBuf, uid: &str, file_name: Option<&str>) -> PathBuf {
    if let Some(f) = file_name {
        let p = profiles_dir.join(f);
        if p.exists() {
            return p;
        }
    }
    if uid.len() > 8 {
        let short = profiles_dir.join(format!("{}.yaml", &uid[..8]));
        if short.exists() {
            return short;
        }
    }
    profiles_dir.join(format!("{}.yaml", uid))
}

/// GET /api/profiles - Returns list of profiles
pub async fn get_profiles() -> HttpResponse {
    let paths = super::get_control_tower_paths();
    let profiles_path = &paths.profiles_path;

    if !profiles_path.exists() {
        return HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
            "items": Vec::<serde_json::Value>::new(),
            "current": serde_json::Value::Null
        })));
    }

    match std::fs::read_to_string(profiles_path) {
        Ok(content) => {
            let result = super::parse_profiles_yaml_full(&content);
            HttpResponse::Ok().json(ApiResponse::success(result))
        }
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to read profiles.yaml: {}", e))),
    }
}

/// POST /api/profiles - Add a new profile by downloading from URL
pub async fn add_profile(
    body: web::Json<AddProfileRequest>,
) -> HttpResponse {
    let paths = super::get_control_tower_paths();
    let profiles_path = &paths.profiles_path;
    let profiles_dir = paths.config_dir.join("profiles");

    // Ensure profiles directory exists
    if let Err(e) = std::fs::create_dir_all(&profiles_dir) {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to create profiles dir: {}", e)));
    }

    // Generate a unique ID for the profile (use full UUID)
    let uid = uuid::Uuid::new_v4().to_string();
    let profile_file = profiles_dir.join(format!("{}.yaml", &uid[..8]));

    // Download the profile content
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to create HTTP client: {}", e)))
        }
    };

    let response = match client
        .get(&body.url)
        .header("User-Agent", "clash-verge/v2.4.7")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(ApiResponse::<()>::error(format!("Failed to download profile: {}", e)))
        }
    };

    if !response.status().is_success() {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error(format!("Download failed: {}", response.status())));
    }

    let content = match response.text().await {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to read download: {}", e)))
        }
    };

    // Save the profile file
    if let Err(e) = std::fs::write(&profile_file, &content) {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to save profile: {}", e)));
    }

    // Get the profile name from URL or use provided name
    let name = body.name.clone().unwrap_or_else(|| {
        body.url
            .split('/')
            .last()
            .unwrap_or(&uid)
            .trim_end_matches(".yaml")
            .to_string()
    });

    // Update profiles.yaml
    let profiles_content = if profiles_path.exists() {
        std::fs::read_to_string(profiles_path).unwrap_or_default()
    } else {
        String::new()
    };

    use control_tower_service_core::{ProfileItem, ProfilesYaml};

    let mut yaml = serde_yaml_ng::from_str::<ProfilesYaml>(&profiles_content).unwrap_or(ProfilesYaml {
        current: None,
        items: Vec::new(),
    });

    let new_item = ProfileItem {
        uid: uid.clone(),
        name: Some(name),
        file: Some(format!("{}.yaml", &uid[..8])),
        url: Some(body.url.clone()),
        cron: None,
        updated_at: Some(chrono::Utc::now().timestamp()),
    };

    yaml.items.push(new_item);

    if let Err(e) = std::fs::write(profiles_path, serde_yaml_ng::to_string(&yaml).unwrap_or_default()) {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to update profiles.yaml: {}", e)));
    }

    HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
        "uid": uid,
        "file": profile_file.to_string_lossy(),
    })))
}

/// PUT /api/profiles/{id}/activate - Activate a profile
pub async fn activate_profile(
    state: web::Data<Arc<ServiceState>>,
    path: web::Path<String>,
) -> HttpResponse {
    let uid = path.into_inner();
    let paths = super::get_control_tower_paths();
    let profiles_path = &paths.profiles_path;

    if !profiles_path.exists() {
        return HttpResponse::NotFound().json(ApiResponse::<()>::error("profiles.yaml not found"));
    }

    // Read profiles.yaml to find the profile's file path
    let content = match std::fs::read_to_string(profiles_path) {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to read profiles.yaml: {}", e)))
        }
    };

    let yaml: ProfilesYaml = match serde_yaml_ng::from_str(&content) {
        Ok(y) => y,
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(ApiResponse::<()>::error(format!("Invalid profiles.yaml: {}", e)))
        }
    };

    // Find the profile by uid
    let profile_item = match yaml.items.iter().find(|p| p.uid == uid) {
        Some(item) => item,
        None => {
            return HttpResponse::NotFound().json(ApiResponse::<()>::error(format!("Profile {} not found in profiles.yaml", uid)));
        }
    };

    let profiles_dir = paths.config_dir.join("profiles");
    let actual_file = resolve_profile_file(&profiles_dir, &uid, profile_item.file.as_deref());

    // Validate profile content before activation
    let profile_content = match std::fs::read_to_string(&actual_file) {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(ApiResponse::<()>::error(format!("Failed to read profile file: {}", e)))
        }
    };

    // Parse and validate the profile YAML has required fields
    let yaml: serde_yaml_ng::Value = match serde_yaml_ng::from_str(&profile_content) {
        Ok(y) => y,
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(ApiResponse::<()>::error(format!("Invalid YAML in profile: {}", e)))
        }
    };

    // Check for required fields: proxies, mixed-port, or proxy-providers
    let has_proxies = yaml.get("proxies").is_some();
    let has_mixed_port = yaml.get("mixed-port").is_some();
    let has_proxy_providers = yaml.get("proxy-providers").is_some();
    let has_proxy_groups = yaml.get("proxy-groups").is_some();

    if !has_proxies && !has_mixed_port && !has_proxy_providers && !has_proxy_groups {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error("Profile does not contain valid proxy configuration (missing: proxies, mixed-port, proxy-providers, or proxy-groups)".to_string()));
    }

    // Re-parse with Serialize for writing back
    use control_tower_service_core::ProfilesYaml as ProfilesYamlWrite;
    let mut yaml_write: ProfilesYamlWrite = match serde_yaml_ng::from_str(&content) {
        Ok(y) => y,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to parse profiles.yaml: {}", e)))
        }
    };

    // Update current marker
    yaml_write.current = Some(uid.clone());

    // Write back profiles.yaml
    if let Err(e) = std::fs::write(profiles_path, serde_yaml_ng::to_string(&yaml_write).unwrap_or_default()) {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to update profiles.yaml: {}", e)));
    }

    // Replace active config from profile
    let store = control_tower_service_core::ActiveConfigStore::new(paths.clone());
    if let Err(e) = store.replace_from_profile(&actual_file) {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to activate profile: {}", e)));
    }

    // Restart Mihomo to load new config
    let config_path = paths.active_config_path.clone();
    let state = state.clone();
    match task::spawn_blocking(move || state.restart_with_config(&config_path)).await {
        Ok(Ok(())) => HttpResponse::Ok().json(ApiResponse::<()>::success(())),
        Ok(Err(e)) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to restart Mihomo: {}", e))),
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to restart Mihomo: {}", e.to_string()))),
    }
}

/// PATCH /api/profiles/{id} - Update name, URL, and/or cron schedule for a profile.
/// If the URL is changed, the subscription content is re-downloaded and the profile
/// file is overwritten. If the profile is currently active, Mihomo is restarted.
pub async fn update_profile(
    state: web::Data<Arc<ServiceState>>,
    path: web::Path<String>,
    body: web::Json<serde_json::Value>,
) -> HttpResponse {
    let uid = path.into_inner();
    let paths = super::get_control_tower_paths();
    let profiles_path = &paths.profiles_path;
    let profiles_dir = paths.config_dir.join("profiles");

    if !profiles_path.exists() {
        return HttpResponse::NotFound().json(ApiResponse::<()>::error("profiles.yaml not found"));
    }

    // Parse body as serde_json::Value to distinguish "key absent" from "key = null" from "key = ''"
    let obj = match body.as_object() {
        Some(o) => o,
        None => {
            return HttpResponse::BadRequest()
                .json(ApiResponse::<()>::error("Request body must be a JSON object".to_string()))
        }
    };

    // Returns (value_str, was_explicitly_provided)
    fn get_string_field(obj: &serde_json::Map<String, serde_json::Value>, key: &str) -> (Option<String>, bool) {
        if let Some(v) = obj.get(key) {
            if v.is_null() {
                return (None, true); // explicitly null → clear
            }
            if let Some(s) = v.as_str() {
                return (Some(s.to_string()), true); // present with value
            }
        }
        (None, false) // absent or wrong type
    }

    let (name, name_provided) = get_string_field(obj, "name");
    let (url, url_provided) = get_string_field(obj, "url");
    let (cron, cron_provided) = get_string_field(obj, "cron");

    // Validate URL if explicitly provided (even as empty string → clear not allowed for URL)
    if url_provided {
        if let Some(ref u) = url {
            if u.trim().is_empty() {
                return HttpResponse::BadRequest()
                    .json(ApiResponse::<()>::error("URL cannot be empty".to_string()));
            }
        }
    }

    // Read profiles.yaml
    let content = match std::fs::read_to_string(profiles_path) {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to read profiles.yaml: {}", e)))
        }
    };

    let mut yaml = match serde_yaml_ng::from_str::<ProfilesYaml>(&content) {
        Ok(y) => y,
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(ApiResponse::<()>::error(format!("Invalid profiles.yaml: {}", e)))
        }
    };

    // Find the profile item
    let item = yaml.items.iter_mut().find(|i| i.uid == uid);
    let item = match item {
        Some(i) => i,
        None => {
            return HttpResponse::NotFound()
                .json(ApiResponse::<()>::error(format!("Profile {} not found", uid)))
        }
    };

    let is_active = yaml.current.as_ref() == Some(&uid);
    let old_url = item.url.clone();

    // Normalize file field to uid[..8].yaml so all code paths find the same file.
    // Historical bug: item.file may contain full uid instead of uid[..8].
    if uid.len() > 8 {
        let correct_file = format!("{}.yaml", &uid[..8]);
        if item.file.as_ref() != Some(&correct_file) {
            item.file = Some(correct_file);
        }
    }

    // Apply cron: cron_provided means the key was present (even if null/empty → clear)
    if cron_provided {
        let cron_trimmed = cron.as_ref().map(|s| s.trim()).unwrap_or("");
        if cron_trimmed.is_empty() {
            item.cron = None; // null or "" → clear
        } else {
            match cron_trimmed.parse::<u32>() {
                Ok(n) if n >= 1 => { item.cron = Some(cron_trimmed.to_string()); }
                _ => {
                    return HttpResponse::BadRequest()
                        .json(ApiResponse::<()>::error("Cron must be a positive integer (minutes)".to_string()));
                }
            }
        }
    }

    // Apply name: null or "" → clear, otherwise update
    if name_provided {
        item.name = name.filter(|s| !s.trim().is_empty());
    }

    // Apply URL: null or "" → clear, otherwise update (but re-download only if changed)
    let url_changed = url_provided && url.as_ref() != old_url.as_ref();
    if url_provided {
        if url.as_ref().map(|s| s.trim().is_empty()).unwrap_or(true) {
            return HttpResponse::BadRequest()
                .json(ApiResponse::<()>::error("URL cannot be empty".to_string()));
        }
    }

    let profile_file = resolve_profile_file(&profiles_dir, &uid, item.file.as_deref());

    // If URL changed, download new content in a blocking task
    if url_changed {
        let download_url = url.clone().unwrap(); // url_changed means url_provided && Some
        let uid_clone = uid.clone();
        let profile_file_clone = profile_file.clone();
        let profiles_path_clone = profiles_path.clone();

        let result = tokio::task::spawn_blocking(move || {
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {}", e))?;

            let response = client
                .get(&download_url)
                .header("User-Agent", "clash-verge/v2.4.7")
                .send()
                .map_err(|e| anyhow::anyhow!("Failed to download: {}", e))?;

            if !response.status().is_success() {
                anyhow::bail!("Download failed: {}", response.status());
            }

            let new_content = response
                .text()
                .map_err(|e| anyhow::anyhow!("Failed to read response: {}", e))?;

            // Validate it's YAML (not HTML error page)
            if new_content.trim().starts_with('<') {
                anyhow::bail!("Downloaded content looks like HTML, not a valid subscription");
            }

            // Write new content to profile file
            std::fs::write(&profile_file_clone, &new_content)?;

            // Update updated_at and url in profiles.yaml
            let content = std::fs::read_to_string(&profiles_path_clone)?;
            let mut yaml = serde_yaml_ng::from_str::<ProfilesYaml>(&content)?;

            let now = chrono::Utc::now().timestamp();
            for item in &mut yaml.items {
                if item.uid == uid_clone {
                    item.updated_at = Some(now);
                    item.url = Some(download_url.clone());
                    break;
                }
            }

            let new_content_str = serde_yaml_ng::to_string(&yaml)?;
            std::fs::write(&profiles_path_clone, new_content_str)?;

            Ok::<(), anyhow::Error>(())
        })
        .await;

        match result {
            Ok(Ok(())) => {
                tracing::info!("Profile {} URL updated and subscription re-downloaded", uid);
            }
            Ok(Err(e)) => {
                tracing::error!("Failed to update subscription for profile {}: {}", uid, e);
                return HttpResponse::BadRequest()
                    .json(ApiResponse::<()>::error(format!("Failed to download new subscription: {}", e)));
            }
            Err(e) => {
                return HttpResponse::InternalServerError()
                    .json(ApiResponse::<()>::error(format!("Task error: {}", e)));
            }
        }

        // If this profile is currently active, replace active config and restart Mihomo
        if is_active {
            let store = control_tower_service_core::ActiveConfigStore::new(paths.clone());
            if let Err(e) = store.replace_from_profile(&profile_file) {
                return HttpResponse::InternalServerError()
                    .json(ApiResponse::<()>::error(format!("Failed to update active config: {}", e)));
            }

            let config_path = paths.active_config_path.clone();
            let state = state.clone();
            match task::spawn_blocking(move || state.restart_with_config(&config_path)).await {
                Ok(Ok(())) => {
                    tracing::info!("Profile {} activated with new URL, Mihomo restarted", uid);
                }
                Ok(Err(e)) => {
                    return HttpResponse::InternalServerError()
                        .json(ApiResponse::<()>::error(format!("Mihomo restart failed: {}", e)));
                }
                Err(e) => {
                    return HttpResponse::InternalServerError()
                        .json(ApiResponse::<()>::error(format!("Task error: {}", e)));
                }
            }
        }
    } else {
        // No URL change — just write back profiles.yaml with name/cron updates
        if let Err(e) = std::fs::write(profiles_path, serde_yaml_ng::to_string(&yaml).unwrap_or_default()) {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to update profiles.yaml: {}", e)));
        }
    }

    // Reload cron jobs so the scheduler picks up any new schedule
    state.load_cron_jobs();

    tracing::info!("Profile {} updated", uid);
    HttpResponse::Ok().json(ApiResponse::<()>::success(()))
}

/// POST /api/profiles/{id}/refresh - Manually refresh a subscription profile
pub async fn refresh_profile(
    path: web::Path<String>,
    body: Option<web::Json<RefreshRequest>>,
) -> HttpResponse {
    let uid = path.into_inner();
    let paths = super::get_control_tower_paths();
    let profiles_path = &paths.profiles_path;
    let profiles_dir = paths.config_dir.join("profiles");

    // Read profiles.yaml to get URL and file path
    let content = match std::fs::read_to_string(profiles_path) {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to read profiles.yaml: {}", e)))
        }
    };

    use control_tower_service_core::ProfilesYaml as ProfilesYamlRead;
    let yaml: ProfilesYamlRead = match serde_yaml_ng::from_str(&content) {
        Ok(y) => y,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to parse profiles.yaml: {}", e)))
        }
    };

    let profile_item = match yaml.items.iter().find(|i| i.uid == uid) {
        Some(item) => item,
        None => {
            return HttpResponse::NotFound()
                .json(ApiResponse::<()>::error("Profile not found".to_string()))
        }
    };

    let url = match &profile_item.url {
        Some(u) => u.clone(),
        None => {
            return HttpResponse::BadRequest()
                .json(ApiResponse::<()>::error("Profile has no subscription URL".to_string()))
        }
    };

    // Determine profile file path using helper
    let profile_file = resolve_profile_file(&profiles_dir, &uid, profile_item.file.as_deref());

    if !profile_file.exists() {
        return HttpResponse::NotFound()
            .json(ApiResponse::<()>::error("Profile file not found".to_string()));
    }

    // Download new content in a blocking task
    let uid_clone = uid.clone();
    let profile_file_clone = profile_file.clone();
    let profiles_path_clone = profiles_path.clone();
    let use_proxy = body.as_ref().map(|b| b.use_proxy.unwrap_or(false)).unwrap_or(false);
    let http_port = super::get_mihomo_http_port();

    let result = tokio::task::spawn_blocking(move || {
        // Download subscription (optionally through Mihomo proxy)
        let client = if use_proxy {
            // Route through Mihomo HTTP proxy
            let proxy_url = format!("http://127.0.0.1:{}", http_port);
            let proxy = reqwest::Proxy::http(proxy_url)
                .map_err(|e| anyhow::anyhow!("Failed to create proxy: {}", e))?;
            reqwest::blocking::Client::builder()
                .proxy(proxy)
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {}", e))?
        } else {
            reqwest::blocking::Client::new()
        };

        let response = client
            .get(&url)
            .header("User-Agent", "clash-verge/v2.4.7")
            .send()
            .map_err(|e| anyhow::anyhow!("Failed to download: {}", e))?;

        if !response.status().is_success() {
            anyhow::bail!("Download failed: {}", response.status());
        }

        let new_content = response
            .text()
            .map_err(|e| anyhow::anyhow!("Failed to read response: {}", e))?;

        // Update profile file
        std::fs::write(&profile_file_clone, &new_content)?;

        // Update updated_at in profiles.yaml
        let content = std::fs::read_to_string(&profiles_path_clone)?;
        let mut yaml = serde_yaml_ng::from_str::<ProfilesYaml>(&content)?;

        let now = chrono::Utc::now().timestamp();
        for item in &mut yaml.items {
            if item.uid == uid_clone {
                item.updated_at = Some(now);
                break;
            }
        }

        let new_content = serde_yaml_ng::to_string(&yaml)?;
        std::fs::write(&profiles_path_clone, new_content)?;

        Ok::<(), anyhow::Error>(())
    })
    .await;

    match result {
        Ok(Ok(())) => {
            tracing::info!("Profile {} refreshed manually (use_proxy={})", uid, use_proxy);
            HttpResponse::Ok().json(ApiResponse::success(()))
        }
        Ok(Err(e)) => {
            tracing::error!("Failed to refresh profile {}: {}", uid, e);
            HttpResponse::BadRequest().json(ApiResponse::<()>::error(e.to_string()))
        }
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Task error: {}", e))),
    }
}

/// DELETE /api/profiles/{id} - Delete a profile
pub async fn delete_profile(
    path: web::Path<String>,
) -> HttpResponse {
    let uid = path.into_inner();
    let paths = super::get_control_tower_paths();
    let profiles_path = &paths.profiles_path;
    let profiles_dir = paths.config_dir.join("profiles");

    if !profiles_path.exists() {
        return HttpResponse::NotFound().json(ApiResponse::<()>::error("profiles.yaml not found"));
    }

    // Read profiles.yaml
    let content = match std::fs::read_to_string(profiles_path) {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to read profiles.yaml: {}", e)))
        }
    };

    let mut yaml = match serde_yaml_ng::from_str::<ProfilesYaml>(&content) {
        Ok(y) => y,
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(ApiResponse::<()>::error(format!("Invalid profiles.yaml: {}", e)))
        }
    };

    // Find the profile item to get its file path
    let profile_item = match yaml.items.iter().find(|item| item.uid == uid) {
        Some(item) => item,
        None => {
            return HttpResponse::NotFound().json(ApiResponse::<()>::error(format!("Profile {} not found", uid)));
        }
    };

    // Determine actual file path using helper
    let profile_file = resolve_profile_file(&profiles_dir, &uid, profile_item.file.as_deref());

    // Remove from items
    yaml.items.retain(|item| item.uid != uid);

    // Clear current if it was this profile
    if yaml.current.as_ref() == Some(&uid) {
        yaml.current = None;
    }

    // Write back profiles.yaml
    if let Err(e) = std::fs::write(profiles_path, serde_yaml_ng::to_string(&yaml).unwrap_or_default()) {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to update profiles.yaml: {}", e)));
    }

    // Delete the profile file
    if profile_file.exists() {
        if let Err(e) = std::fs::remove_file(&profile_file) {
            tracing::warn!("Failed to delete profile file {:?}: {}", profile_file, e);
        }
    }

    HttpResponse::Ok().json(ApiResponse::<()>::success(()))
}
