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

#[derive(Debug, Deserialize)]
pub struct ProxyDelayPostRequest {
    pub name: String,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

fn default_timeout() -> u64 {
    5000
}

#[derive(Debug, Deserialize)]
pub struct UpdateProfileRequest {
    pub cron: Option<String>,
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

/// Get Mihomo HTTP proxy port from settings.yaml, defaulting to 7890
fn get_mihomo_http_port() -> u16 {
    #[derive(serde::Deserialize)]
    struct Settings {
        #[serde(rename = "http_port", default)]
        http_port: Option<u16>,
    }
    if let Some(path) = find_settings_path() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(settings) = serde_yaml_ng::from_str::<Settings>(&content) {
                return settings.http_port.unwrap_or(7890);
            }
        }
    }
    7890
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
fn parse_profiles_yaml_full(content: &str) -> serde_json::Value {
    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct ProfilesYaml {
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
        Ok(yaml) => {
            let items: Vec<serde_json::Value> = yaml
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
                .collect();
            serde_json::json!({
                "items": items,
                "current": yaml.current
            })
        }
        Err(_) => serde_json::json!({
            "items": Vec::<serde_json::Value>::new(),
            "current": serde_json::Value::Null
        }),
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
    match task::spawn_blocking(move || state.get_configs()).await {
        Ok(Ok(configs)) => {
            let mode = configs
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
    match task::spawn_blocking(move || state.set_mode(&mode, false)).await {
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
        return HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
            "items": Vec::<serde_json::Value>::new(),
            "current": serde_json::Value::Null
        })));
    }

    match std::fs::read_to_string(profiles_path) {
        Ok(content) => {
            let result = parse_profiles_yaml_full(&content);
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
    let paths = get_control_tower_paths();
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

    #[derive(serde::Deserialize, serde::Serialize)]
    #[allow(dead_code)]
    struct ProfilesYaml {
        #[serde(default)]
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
    state: web::Data<Arc<ServiceState>>,
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

    // Validate profile content before activation
    let profile_content = match std::fs::read_to_string(&profile_file) {
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
        #[serde(default)]
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

/// PATCH /api/profiles/{id} - Update cron schedule for a profile (only active profile)
pub async fn update_profile(
    state: web::Data<Arc<ServiceState>>,
    path: web::Path<String>,
    body: web::Json<UpdateProfileRequest>,
) -> HttpResponse {
    let uid = path.into_inner();
    let paths = get_control_tower_paths();
    let profiles_path = &paths.profiles_path;

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
        #[serde(default)]
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

    // Only allow updating the active profile
    if yaml.current.as_ref() != Some(&uid) {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error("Only the active profile can be updated".to_string()));
    }

    // Find and update the profile item
    let item = yaml.items.iter_mut().find(|i| i.uid == uid);
    let item = match item {
        Some(i) => i,
        None => {
            return HttpResponse::NotFound()
                .json(ApiResponse::<()>::error(format!("Profile {} not found", uid)))
        }
    };

    // Validate cron if provided
    if let Some(ref cron_str) = body.cron {
        if !cron_str.trim().is_empty() {
            if cron_str.trim().parse::<u32>().is_err() || cron_str.trim().parse::<u32>().ok().map(|n| n < 1).unwrap_or(true) {
                return HttpResponse::BadRequest()
                    .json(ApiResponse::<()>::error("Cron must be a positive integer (minutes)".to_string()));
            }
        }
        item.cron = if cron_str.trim().is_empty() { None } else { Some(cron_str.trim().to_string()) };
    }

    // Write back profiles.yaml
    if let Err(e) = std::fs::write(profiles_path, serde_yaml_ng::to_string(&yaml).unwrap_or_default()) {
        return HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to update profiles.yaml: {}", e)));
    }

    tracing::info!("Profile {} cron updated", uid);

    // Reload cron jobs so the scheduler picks up the new schedule
    state.load_cron_jobs();

    HttpResponse::Ok().json(ApiResponse::<()>::success(()))
}

#[derive(serde::Deserialize)]
pub struct RefreshRequest {
    pub use_proxy: Option<bool>,
}

/// POST /api/profiles/{id}/refresh - Manually refresh a subscription profile
pub async fn refresh_profile(
    path: web::Path<String>,
    body: Option<web::Json<RefreshRequest>>,
) -> HttpResponse {
    let uid = path.into_inner();
    let paths = get_control_tower_paths();
    let profiles_path = &paths.profiles_path;
    let profiles_dir = paths.config_dir.join("profiles");
    let profile_file = profiles_dir.join(format!("{}.yaml", uid));

    // Read profiles.yaml to get URL
    let content = match std::fs::read_to_string(profiles_path) {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to read profiles.yaml: {}", e)))
        }
    };

    #[derive(serde::Deserialize, serde::Serialize)]
    struct ProfilesYaml {
        current: Option<String>,
        items: Vec<ProfileItem>,
    }

    #[derive(serde::Deserialize, serde::Serialize)]
    struct ProfileItem {
        uid: String,
        url: Option<String>,
    }

    let yaml: ProfilesYaml = match serde_yaml_ng::from_str(&content) {
        Ok(y) => y,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to parse profiles.yaml: {}", e)))
        }
    };

    let url = match yaml.items.iter().find(|i| i.uid == uid) {
        Some(item) => item.url.clone(),
        None => {
            return HttpResponse::NotFound()
                .json(ApiResponse::<()>::error("Profile not found".to_string()))
        }
    };

    let url = match url {
        Some(u) => u,
        None => {
            return HttpResponse::BadRequest()
                .json(ApiResponse::<()>::error("Profile has no subscription URL".to_string()))
        }
    };

    if !profile_file.exists() {
        return HttpResponse::NotFound()
            .json(ApiResponse::<()>::error("Profile file not found".to_string()));
    }

    // Download new content in a blocking task
    let uid_clone = uid.clone();
    let profile_file_clone = profile_file.clone();
    let profiles_path_clone = profiles_path.clone();
    let use_proxy = body.as_ref().map(|b| b.use_proxy.unwrap_or(false)).unwrap_or(false);
    let http_port = get_mihomo_http_port();

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
        #[derive(serde::Deserialize, serde::Serialize)]
        struct ProfilesYaml {
            current: Option<String>,
            items: Vec<ProfileItem2>,
        }

        #[derive(serde::Deserialize, serde::Serialize)]
        struct ProfileItem2 {
            uid: String,
            name: Option<String>,
            #[serde(rename = "file")]
            file: Option<String>,
            url: Option<String>,
            cron: Option<String>,
            #[serde(rename = "updated_at")]
            updated_at: Option<i64>,
        }

        let content = std::fs::read_to_string(&profiles_path_clone)?;
        let mut yaml: ProfilesYaml = serde_yaml_ng::from_str(&content)?;

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
        #[serde(default)]
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

/// GET /api/settings - Return current settings
pub async fn get_settings(
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    let settings = state.get_settings();
    HttpResponse::Ok().json(ApiResponse::success(settings))
}

/// Request body for updating settings
#[derive(Debug, Deserialize)]
pub struct UpdateSettingsRequest {
    #[serde(rename = "api_host", default)]
    pub api_host: Option<String>,
    #[serde(rename = "api_port", default)]
    pub api_port: Option<u16>,
    #[serde(rename = "http_port", default)]
    pub http_port: Option<u16>,
    #[serde(rename = "socks_port", default)]
    pub socks_port: Option<u16>,
    #[serde(rename = "service_port", default)]
    pub service_port: Option<u16>,
    #[serde(rename = "tun_enabled", default)]
    pub tun_enabled: Option<bool>,
    #[serde(rename = "log_level", default)]
    pub log_level: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
}

/// PUT /api/settings - Update and persist settings
pub async fn put_settings(
    state: web::Data<Arc<ServiceState>>,
    body: web::Json<UpdateSettingsRequest>,
) -> HttpResponse {
    // Build SettingsData from request
    let settings = crate::settings::SettingsData {
        api_host: body.api_host.clone(),
        api_port: body.api_port,
        http_port: body.http_port,
        socks_port: body.socks_port,
        service_port: body.service_port,
        tun_enabled: body.tun_enabled,
        log_level: body.log_level.clone(),
        mode: body.mode.clone(),
    };

    let state = state.clone();
    match task::spawn_blocking(move || state.save_settings(&settings)).await {
        Ok(Ok(())) => {
            tracing::info!("Settings updated via API");
            HttpResponse::Ok().json(ApiResponse::<()>::success(()))
        }
        Ok(Err(e)) => {
            tracing::error!("Failed to save settings: {}", e);
            HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e))
        }
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// POST /api/settings/apply-ports - Apply port settings and restart Mihomo
#[derive(Debug, Deserialize)]
pub struct ApplyPortsRequest {
    #[serde(rename = "http_port")]
    pub http_port: u16,
    #[serde(rename = "socks_port")]
    pub socks_port: u16,
}

pub async fn apply_port_settings(
    state: web::Data<Arc<ServiceState>>,
    body: web::Json<ApplyPortsRequest>,
) -> HttpResponse {
    let http_port = body.http_port;
    let socks_port = body.socks_port;

    let state = state.clone();
    match task::spawn_blocking(move || state.apply_port_settings(http_port, socks_port)).await {
        Ok(Ok(())) => {
            tracing::info!("Port settings applied: http={}, socks={}", http_port, socks_port);
            HttpResponse::Ok().json(ApiResponse::<()>::success(()))
        }
        Ok(Err(e)) => {
            tracing::error!("Failed to apply port settings: {}", e);
            HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e))
        }
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

#[derive(Debug, Deserialize)]
pub struct ApplyTunRequest {
    pub tun_enabled: bool,
}

/// POST /api/settings/apply-tun - Enable or disable TUN mode
pub async fn apply_tun_settings(
    state: web::Data<Arc<ServiceState>>,
    body: web::Json<ApplyTunRequest>,
) -> HttpResponse {
    let tun_enabled = body.tun_enabled;
    let state = state.clone();
    match task::spawn_blocking(move || state.apply_tun_settings(tun_enabled)).await {
        Ok(Ok(())) => {
            tracing::info!("TUN settings applied: enabled={}", tun_enabled);
            HttpResponse::Ok().json(ApiResponse::<()>::success(()))
        }
        Ok(Err(e)) => {
            tracing::error!("Failed to apply TUN settings: {}", e);
            HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e))
        }
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// GET /api/proxies/{name}/delay - Get proxy delay
pub async fn proxy_delay(
    state: web::Data<Arc<ServiceState>>,
    path: web::Path<String>,
    query: web::Query<ProxyDelayRequest>,
) -> HttpResponse {
    let name = path.into_inner();

    // Call Mihomo's delay API
    let api_url = state.get_api_url();
    let url = format!(
        "{}/proxies/{}/delay?timeout={}",
        api_url, name, query.timeout
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

/// POST /api/proxies/delay - Get proxy delay (JSON body)
pub async fn proxy_delay_post(
    state: web::Data<Arc<ServiceState>>,
    body: web::Json<ProxyDelayPostRequest>,
) -> HttpResponse {
    let name_or_idx = &body.name;
    let timeout_ms = body.timeout;

    // Get the API URL from state
    let api_url = state.get_api_url();

    // Build a single HTTP client for all requests
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(timeout_ms + 2000))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to create HTTP client: {}", e)))
        }
    };

    // Step 1: Resolve name (index -> proxy name) and get current GLOBAL selection
    let proxies_url = format!("{}/proxies", api_url);
    let proxies_response = match client.get(&proxies_url).send().await {
        Ok(r) => r,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to fetch proxies: {}", e)))
        }
    };

    if !proxies_response.status().is_success() {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error(format!("Failed to get proxies: {}", proxies_response.status())));
    }

    let proxies_data: serde_json::Value = match proxies_response.json().await {
        Ok(d) => d,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to parse proxies response: {}", e)))
        }
    };

    // Get GLOBAL.now (current selection) to restore later
    let original_proxy: String = proxies_data
        .get("proxies")
        .and_then(|p| p.get("GLOBAL"))
        .and_then(|g| g.get("now"))
        .and_then(|v| v.as_str())
        .unwrap_or("DIRECT")
        .to_string();

    // Get GLOBAL.all for index resolution
    let global_all = match proxies_data
        .get("proxies")
        .and_then(|p| p.get("GLOBAL"))
        .and_then(|g| g.get("all"))
        .and_then(|a| a.as_array())
    {
        Some(a) => a,
        None => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error("GLOBAL.all not found".to_string()))
        }
    };

    // Resolve proxy name from index or use directly
    let target_proxy = if name_or_idx.chars().all(|c| c.is_ascii_digit()) {
        let idx: usize = match name_or_idx.parse() {
            Ok(i) => i,
            Err(_) => {
                return HttpResponse::BadRequest()
                    .json(ApiResponse::<()>::error(format!("Invalid index: {}", name_or_idx)))
            }
        };
        if idx == 0 || idx > global_all.len() {
            return HttpResponse::BadRequest()
                .json(ApiResponse::<()>::error(format!("Index {} out of range (1-{})", idx, global_all.len())));
        }
        global_all[idx - 1].as_str().unwrap_or(name_or_idx).to_string()
    } else {
        name_or_idx.clone()
    };

    tracing::debug!("Latency test: target={}, original={}", target_proxy, original_proxy);

    // Step 2: Temporarily select the target proxy
    let select_url = format!("{}/proxies/GLOBAL", api_url);
    let select_response = match client
        .put(&select_url)
        .json(&serde_json::json!({ "name": target_proxy }))
        .timeout(std::time::Duration::from_millis(5000))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(ApiResponse::<()>::error(format!("Failed to select proxy: {}", e)))
        }
    };

    if !select_response.status().is_success() {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error(format!("Failed to select proxy {}: {}", target_proxy, select_response.status())));
    }

    // Step 3: Measure HTTP delay through the selected proxy
    let test_url = "http://cp.cloudflare.com/generate_204";
    let start = std::time::Instant::now();

    let http_response = match client
        .get(test_url)
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            // Restore original proxy on error
            let _ = client
                .put(select_url)
                .json(&serde_json::json!({ "name": original_proxy }))
                .timeout(std::time::Duration::from_millis(5000))
                .send()
                .await;
            return HttpResponse::Ok()
                .json(ApiResponse::success(serde_json::json!({ "delay": null, "error": e.to_string() })));
        }
    };

    let elapsed_ms = start.elapsed().as_millis() as i64;

    // Step 4: Restore original GLOBAL selection
    if original_proxy != target_proxy {
        let _ = client
            .put(select_url)
            .json(&serde_json::json!({ "name": original_proxy }))
            .timeout(std::time::Duration::from_millis(5000))
            .send()
            .await;
    }

    // Check if HTTP response is successful (200 or 204)
    let delay = if http_response.status().is_success() || http_response.status().as_u16() == 204 {
        elapsed_ms
    } else {
        return HttpResponse::Ok()
            .json(ApiResponse::success(serde_json::json!({ "delay": null, "error": format!("HTTP {}", http_response.status()) })));
    };

    tracing::debug!("Latency test result: {}ms for {}", delay, target_proxy);
    HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({ "delay": delay })))
}

/// GET /api/logs - Read log files (ctsvc or mihomo)
pub async fn get_logs(
    query: web::Query<LogsQuery>,
) -> HttpResponse {
    let log_dir = std::path::PathBuf::from("/opt/ctsvc/logs");
    let lines = query.lines.unwrap_or(200).min(1000);

    let file_path = match query.source.as_deref() {
        Some("mihomo") => log_dir.join("mihomo.log"),
        _ => {
            let entries = std::fs::read_dir(&log_dir).ok()
                .into_iter().flatten().flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("ctsvc.log."))
                .max_by_key(|e| e.file_name().to_string_lossy().to_string());
            match entries {
                Some(e) => e.path(),
                None => log_dir.join("ctsvc.log"),
            }
        }
    };

    if !file_path.exists() {
        return HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
            "items": Vec::<String>::new(),
            "source": query.source.clone().unwrap_or_else(|| "ctsvc".to_string()),
            "total_lines": 0
        })));
    }

    let content = std::fs::read_to_string(&file_path).unwrap_or_default();
    let all_lines: Vec<&str> = content.lines().collect();
    let total_lines = all_lines.len();
    let start = if all_lines.len() > lines { all_lines.len() - lines } else { 0 };
    let items: Vec<String> = all_lines[start..].iter().map(|s| s.to_string()).collect();

    HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
        "items": items,
        "source": query.source.clone().unwrap_or_else(|| "ctsvc".to_string()),
        "total_lines": total_lines
    })))
}

#[derive(Deserialize)]
pub struct LogsQuery {
    pub source: Option<String>,
    pub lines: Option<usize>,
}
