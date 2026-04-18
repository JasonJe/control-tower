//! HTTP API handlers for Control Tower Service
//!
//! These handlers are exposed via actix-web and provide the same functionality
//! as the IPC commands, but accessible over HTTP for web clients.

use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::task;

use crate::ServiceState;

/// Standard API response wrapper
#[derive(Debug, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            code: 0,
            message: "success".to_string(),
            data: Some(data),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            code: -1,
            message: message.into(),
            data: None,
        }
    }
}

// ============ Request DTOs ============

#[derive(Debug, Deserialize)]
pub struct SelectProxyRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct SetModeRequest {
    pub mode: String,
}

#[derive(Debug, Deserialize)]
pub struct AddProfileRequest {
    pub url: String,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddRuleRequest {
    #[serde(rename = "type")]
    pub rule_type: String,
    pub value: String,
    pub proxy: String,
}

#[derive(Debug, Deserialize)]
pub struct ProxyDelayRequest {
    // Note: name is extracted from URL path /proxies/{name}/delay, not from body
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

fn default_timeout() -> u64 {
    5000
}

// ============ Helper Functions ============

/// Find settings.yaml path (same logic as CLI's settings.rs)
fn find_settings_path() -> Option<PathBuf> {
    // First check executable directory
    if let Ok(exe_dir) = std::env::current_exe() {
        let exe_dir = exe_dir.parent()?.to_path_buf();
        let path = exe_dir.join("settings.yaml");
        if path.exists() {
            return Some(path);
        }
    }

    // Then check ~/.config/control-tower/
    if let Some(config_dir) = dirs::config_dir() {
        let path = config_dir.join("control-tower").join("settings.yaml");
        if path.exists() {
            return Some(path);
        }
    }

    // Fallback to executable directory for new file creation
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .map(|p| p.join("settings.yaml"))
}

/// Get ControlTowerPaths from settings
fn get_control_tower_paths() -> control_tower_service_core::ControlTowerPaths {
    let settings_path = find_settings_path().unwrap_or_else(|| PathBuf::from("settings.yaml"));
    let working_dir = settings_path
        .parent()
        .map(|p| p.to_path_buf())
        .filter(|p| !p.as_os_str().is_empty());

    control_tower_service_core::ControlTowerPaths::from_settings(
        settings_path,
        working_dir,
    )
}

/// Parse profiles.yaml into a JSON-friendly structure
fn parse_profiles_yaml(content: &str) -> Vec<serde_json::Value> {
    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct ProfilesYaml {
        #[serde(skip)]
        current: Option<String>,
        items: Vec<ProfileItem>,
    }

    #[derive(serde::Deserialize)]
    struct ProfileItem {
        uid: String,
        name: Option<String>,
        #[serde(rename = "file")]
        file: Option<String>,
        url: Option<String>,
        cron: Option<String>,
        #[serde(rename = "updated_at")]
        updated_at: Option<i64>,
    }

    match serde_yaml_ng::from_str::<ProfilesYaml>(content) {
        Ok(yaml) => yaml
            .items
            .into_iter()
            .map(|item| {
                serde_json::json!({
                    "uid": item.uid,
                    "name": item.name,
                    "file": item.file,
                    "url": item.url,
                    "cron": item.cron,
                    "updated_at": item.updated_at,
                })
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

// ============ API Handlers ============

/// GET /api/status - Returns service status
pub async fn service_status(
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    let status = state.status();
    HttpResponse::Ok().json(ApiResponse::success(status))
}

/// GET /api/proxies - Returns Mihomo proxies
pub async fn get_proxies(
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    let state = state.clone();
    match task::spawn_blocking(move || state.get_proxies()).await {
        Ok(Ok(proxies)) => HttpResponse::Ok().json(ApiResponse::success(proxies)),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// POST /api/proxies/select - Select a proxy in GLOBAL group
pub async fn select_proxy(
    state: web::Data<Arc<ServiceState>>,
    body: web::Json<SelectProxyRequest>,
) -> HttpResponse {
    let name = body.name.clone();
    let state = state.clone();
    match task::spawn_blocking(move || state.select_proxy(&name)).await {
        Ok(Ok(())) => HttpResponse::Ok().json(ApiResponse::<()>::success(())),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// GET /api/mode - Get current proxy mode
pub async fn get_mode(
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    let state = state.clone();
    match task::spawn_blocking(move || state.get_proxies()).await {
        Ok(Ok(proxies)) => {
            let mode = proxies
                .get("mode")
                .and_then(|m| m.as_str())
                .unwrap_or("rule")
                .to_string();
            HttpResponse::Ok().json(ApiResponse::success(mode))
        }
        Ok(Err(e)) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// POST /api/mode - Set proxy mode
pub async fn set_mode(
    state: web::Data<Arc<ServiceState>>,
    body: web::Json<SetModeRequest>,
) -> HttpResponse {
    // Validate mode
    let valid_modes = ["rule", "global", "direct"];
    if !valid_modes.contains(&body.mode.as_str()) {
        return HttpResponse::BadRequest().json(
            ApiResponse::<()>::error(format!(
                "Invalid mode '{}'. Must be one of: rule, global, direct",
                body.mode
            )),
        );
    }

    let mode = body.mode.clone();
    let state = state.clone();
    match task::spawn_blocking(move || state.set_mode(&mode)).await {
        Ok(Ok(())) => HttpResponse::Ok().json(ApiResponse::<()>::success(())),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// GET /api/connections - Returns Mihomo connections
pub async fn get_connections(
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    let state = state.clone();
    match task::spawn_blocking(move || state.get_connections()).await {
        Ok(Ok(connections)) => HttpResponse::Ok().json(ApiResponse::success(connections)),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// DELETE /api/connections/{id} - Close a specific connection
pub async fn close_connection(
    state: web::Data<Arc<ServiceState>>,
    path: web::Path<String>,
) -> HttpResponse {
    let id = path.into_inner();
    let state = state.clone();
    match task::spawn_blocking(move || state.close_connection(&id)).await {
        Ok(Ok(())) => HttpResponse::Ok().json(ApiResponse::<()>::success(())),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// GET /api/config - Returns verge.yaml as JSON
pub async fn get_config() -> HttpResponse {
    let paths = get_control_tower_paths();
    let verge_path = &paths.verge_config_path;

    if !verge_path.exists() {
        // Return empty config instead of 404 so the UI handles it gracefully
        return HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({})));
    }

    match std::fs::read_to_string(verge_path) {
        Ok(content) => {
            // Parse YAML to JSON
            match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content) {
                Ok(json) => HttpResponse::Ok().json(ApiResponse::success(json)),
                Err(e) => HttpResponse::InternalServerError()
                    .json(ApiResponse::<()>::error(format!("Failed to parse verge.yaml: {}", e))),
            }
        }
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to read verge.yaml: {}", e))),
    }
}

/// GET /api/rules - Returns list of rules from config.yaml
pub async fn get_rules() -> HttpResponse {
    let paths = get_control_tower_paths();
    let config_path = &paths.active_config_path;

    if !config_path.exists() {
        return HttpResponse::Ok().json(ApiResponse::success(Vec::<String>::new()));
    }

    match std::fs::read_to_string(config_path) {
        Ok(content) => {
            match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content) {
                Ok(yaml) => {
                    let rules: Vec<String> = yaml
                        .get("rules")
                        .and_then(|v| v.as_sequence())
                        .map(|seq| {
                            seq.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    HttpResponse::Ok().json(ApiResponse::success(rules))
                }
                Err(_) => HttpResponse::Ok().json(ApiResponse::success(Vec::<String>::new())),
            }
        }
        Err(_) => HttpResponse::Ok().json(ApiResponse::success(Vec::<String>::new())),
    }
}

/// POST /api/rules - Add a new rule
pub async fn add_rule(body: web::Json<AddRuleRequest>) -> HttpResponse {
    let paths = get_control_tower_paths();
    let store = control_tower_service_core::ActiveConfigStore::new(paths);

    let rule = format!("{},{},{}", body.rule_type.to_uppercase(), body.value, body.proxy);

    match store.append_rule(&rule) {
        Ok(()) => HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
            "rule": rule
        }))),
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to add rule: {}", e))),
    }
}

/// DELETE /api/rules/{index} - Remove a rule by 1-based index
pub async fn delete_rule(path: web::Path<usize>) -> HttpResponse {
    let index = path.into_inner();
    let paths = get_control_tower_paths();
    let store = control_tower_service_core::ActiveConfigStore::new(paths);

    match store.remove_rule(index) {
        Ok(removed) => HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
            "removed": removed
        }))),
        Err(e) => HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error(format!("Failed to remove rule: {}", e))),
    }
}

/// DELETE /api/rules - Clear all rules
pub async fn clear_rules() -> HttpResponse {
    let paths = get_control_tower_paths();
    let store = control_tower_service_core::ActiveConfigStore::new(paths);

    match store.clear_rules() {
        Ok(()) => HttpResponse::Ok().json(ApiResponse::<()>::success(())),
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to clear rules: {}", e))),
    }
}

/// GET /api/profiles - Returns list of profiles
pub async fn get_profiles() -> HttpResponse {
    let paths = get_control_tower_paths();
    let profiles_path = &paths.profiles_path;

    if !profiles_path.exists() {
        return HttpResponse::Ok().json(ApiResponse::success(Vec::<serde_json::Value>::new()));
    }

    match std::fs::read_to_string(profiles_path) {
        Ok(content) => {
            let profiles = parse_profiles_yaml(&content);
            HttpResponse::Ok().json(ApiResponse::success(profiles))
        }
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to read profiles.yaml: {}", e))),
    }
}

/// POST /api/profiles - Add a new profile by downloading from URL
pub async fn add_profile(
    body: web::Json<AddProfileRequest>,
) -> HttpResponse {
    let paths = get_control_tower_paths();
    let profiles_path = &paths.profiles_path;
    let profiles_dir = paths.config_dir.join("profiles");

    // Ensure profiles directory exists
    if let Err(e) = std::fs::create_dir_all(&profiles_dir) {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to create profiles dir: {}", e)));
    }

    // Generate a unique ID for the profile
    let uid = uuid::Uuid::new_v4().to_string()[..8].to_string();
    let profile_file = profiles_dir.join(format!("{}.yaml", uid));

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

    #[derive(serde::Deserialize, serde::Serialize)]
    #[allow(dead_code)]
    struct ProfilesYaml {
        #[serde(skip)]
        current: Option<String>,
        items: Vec<ProfileItem>,
    }

    #[derive(serde::Deserialize, serde::Serialize)]
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

    let mut yaml: ProfilesYaml = serde_yaml_ng::from_str(&profiles_content).unwrap_or(ProfilesYaml {
        current: None,
        items: Vec::new(),
    });

    let new_item = ProfileItem {
        uid: uid.clone(),
        name,
        file: Some(format!("{}.yaml", uid)),
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
    path: web::Path<String>,
) -> HttpResponse {
    let uid = path.into_inner();
    let paths = get_control_tower_paths();
    let profiles_path = &paths.profiles_path;
    let profile_file = paths.config_dir.join("profiles").join(format!("{}.yaml", uid));

    if !profiles_path.exists() {
        return HttpResponse::NotFound().json(ApiResponse::<()>::error("profiles.yaml not found"));
    }

    if !profile_file.exists() {
        return HttpResponse::NotFound().json(ApiResponse::<()>::error(format!("Profile {} not found", uid)));
    }

    // Read and update profiles.yaml
    let content = match std::fs::read_to_string(profiles_path) {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to read profiles.yaml: {}", e)))
        }
    };

    #[derive(serde::Deserialize, serde::Serialize)]
    #[allow(dead_code)]
    struct ProfilesYaml {
        #[serde(skip)]
        current: Option<String>,
        items: Vec<ProfileItem>,
    }

    #[derive(serde::Deserialize, serde::Serialize)]
    struct ProfileItem {
        uid: String,
        name: Option<String>,
        #[serde(rename = "file")]
        file: Option<String>,
        url: Option<String>,
        cron: Option<String>,
        #[serde(rename = "updated_at")]
        updated_at: Option<i64>,
    }

    let mut yaml: ProfilesYaml = match serde_yaml_ng::from_str(&content) {
        Ok(y) => y,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to parse profiles.yaml: {}", e)))
        }
    };

    // Update current marker
    yaml.current = Some(uid.clone());

    // Write back profiles.yaml
    if let Err(e) = std::fs::write(profiles_path, serde_yaml_ng::to_string(&yaml).unwrap_or_default()) {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to update profiles.yaml: {}", e)));
    }

    // Replace active config from profile
    let store = control_tower_service_core::ActiveConfigStore::new(paths.clone());
    if let Err(e) = store.replace_from_profile(&profile_file) {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to activate profile: {}", e)));
    }

    HttpResponse::Ok().json(ApiResponse::<()>::success(()))
}

/// DELETE /api/profiles/{id} - Delete a profile
pub async fn delete_profile(
    path: web::Path<String>,
) -> HttpResponse {
    let uid = path.into_inner();
    let paths = get_control_tower_paths();
    let profiles_path = &paths.profiles_path;
    let profile_file = paths.config_dir.join("profiles").join(format!("{}.yaml", uid));

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

    #[derive(serde::Deserialize, serde::Serialize)]
    #[allow(dead_code)]
    struct ProfilesYaml {
        #[serde(skip)]
        current: Option<String>,
        items: Vec<ProfileItem>,
    }

    #[derive(serde::Deserialize, serde::Serialize)]
    struct ProfileItem {
        uid: String,
        name: Option<String>,
        #[serde(rename = "file")]
        file: Option<String>,
        url: Option<String>,
        cron: Option<String>,
        #[serde(rename = "updated_at")]
        updated_at: Option<i64>,
    }

    let mut yaml: ProfilesYaml = match serde_yaml_ng::from_str(&content) {
        Ok(y) => y,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to parse profiles.yaml: {}", e)))
        }
    };

    // Check if profile exists
    if !yaml.items.iter().any(|item| item.uid == uid) {
        return HttpResponse::NotFound().json(ApiResponse::<()>::error(format!("Profile {} not found", uid)));
    }

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

/// POST /api/service/start - Start Mihomo
pub async fn service_start(
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    let paths = get_control_tower_paths();
    let config_path = paths.active_config_path.clone();
    let state = state.clone();

    match task::spawn_blocking(move || state.start(&config_path)).await {
        Ok(Ok(())) => HttpResponse::Ok().json(ApiResponse::<()>::success(())),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// POST /api/service/stop - Stop Mihomo
pub async fn service_stop(
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    let state = state.clone();
    match task::spawn_blocking(move || state.stop()).await {
        Ok(Ok(())) => HttpResponse::Ok().json(ApiResponse::<()>::success(())),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// GET /api/proxies/{name}/delay - Get proxy delay
pub async fn proxy_delay(
    _state: web::Data<Arc<ServiceState>>,
    path: web::Path<String>,
    query: web::Query<ProxyDelayRequest>,
) -> HttpResponse {
    let name = path.into_inner();

    // Call Mihomo's delay API
    let url = format!(
        "http://127.0.0.1:9090/proxies/{}/delay?timeout={}",
        name, query.timeout
    );

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(query.timeout + 1000))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to create HTTP client: {}", e)))
        }
    };

    match client.get(&url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<serde_json::Value>().await {
                    Ok(data) => HttpResponse::Ok().json(ApiResponse::success(data)),
                    Err(e) => HttpResponse::InternalServerError()
                        .json(ApiResponse::<()>::error(format!("Failed to parse response: {}", e))),
                }
            } else {
                HttpResponse::BadRequest()
                    .json(ApiResponse::<()>::error(format!("Delay check failed: {}", response.status())))
            }
        }
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to check proxy delay: {}", e))),
    }
}
