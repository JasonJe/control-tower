//! REST API handlers

use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};
use std::net::TcpStream;
use std::time::Duration;
use uuid::Uuid;

const CLASH_API_HOST: &str = "127.0.0.1";

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            code: 0,
            message: "success".to_string(),
            data: Some(data),
        }
    }

    pub fn error(message: &str) -> ApiResponse<()> {
        ApiResponse {
            code: -1,
            message: message.to_string(),
            data: None,
        }
    }
}

fn is_clash_api_running() -> bool {
    TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], crate::settings::get_api_port())),
        Duration::from_secs(1),
    ).is_ok()
}

fn get_config_dir() -> std::path::PathBuf {
    crate::settings::shared_paths()
        .map(|paths| paths.config_dir)
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}

#[derive(Debug, Deserialize)]
pub struct AddProfileRequest {
    pub url: String,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SetModeRequest {
    pub mode: String,
}

#[derive(Debug, Deserialize)]
pub struct SelectProxyRequest {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
}

pub async fn get_profiles() -> HttpResponse {
    let config_dir = get_config_dir();
    let profiles_path = config_dir.join("profiles.yaml");

    if !profiles_path.exists() {
        return HttpResponse::Ok().json(ApiResponse::<Vec<Profile>>::success(vec![]));
    }

    let content = match std::fs::read_to_string(&profiles_path) {
        Ok(c) => c,
        Err(_) => return HttpResponse::Ok().json(ApiResponse::<Vec<Profile>>::success(vec![])),
    };

    #[derive(Deserialize)]
    struct ProfilesYaml {
        current: Option<String>,
        items: Vec<ProfileItem>,
    }

    #[derive(Deserialize)]
    struct ProfileItem {
        uid: String,
        name: String,
        file: Option<String>,
        url: Option<String>,
    }

    let yaml: ProfilesYaml = match serde_yaml_ng::from_str(&content) {
        Ok(y) => y,
        Err(_) => return HttpResponse::Ok().json(ApiResponse::<Vec<Profile>>::success(vec![])),
    };

    let profiles: Vec<Profile> = yaml.items.into_iter().map(|item| {
        let active = yaml.current.as_ref().map(|c| c == &item.uid).unwrap_or(false);
        Profile {
            id: item.uid,
            name: item.name,
            url: item.url,
            file: item.file,
            active,
            cron: None,
        }
    }).collect();

    HttpResponse::Ok().json(ApiResponse::success(profiles))
}

pub async fn add_profile(req: web::Json<AddProfileRequest>) -> HttpResponse {
    let url = &req.url;
    let name = req.name.as_deref().unwrap_or("Unnamed Profile");

    // Download subscription
    let client = reqwest::Client::new();
    let response = match client
        .get(url)
        .header("User-Agent", "clash-verge/v2.4.7")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return HttpResponse::BadRequest().json(ApiResponse::<()>::error(&e.to_string())),
    };

    if !response.status().is_success() {
        return HttpResponse::BadRequest().json(ApiResponse::<()>::error("Download failed"));
    }

    let content = match response.text().await {
        Ok(c) => c,
        Err(e) => return HttpResponse::BadRequest().json(ApiResponse::<()>::error(&e.to_string())),
    };

    // Generate ID and save file
    let id = format!("profile-{}", Uuid::new_v4());
    let config_dir = get_config_dir();
    let profiles_dir = config_dir.join("profiles");
    let _ = std::fs::create_dir_all(&profiles_dir);

    let profile_file = profiles_dir.join(format!("{}.yaml", id));
    if let Err(e) = std::fs::write(&profile_file, &content) {
        return HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e.to_string()));
    }

    // Add to profiles list
    let profiles_path = config_dir.join("profiles.yaml");
    #[derive(Serialize, Deserialize)]
    struct ProfilesYaml {
        current: Option<String>,
        items: Vec<ProfileItem>,
    }
    #[derive(Serialize, Deserialize)]
    struct ProfileItem {
        uid: String,
        name: String,
        #[serde(rename = "file")]
        file: Option<String>,
        url: Option<String>,
    }

    let mut yaml: ProfilesYaml = if profiles_path.exists() {
        let content = std::fs::read_to_string(&profiles_path).unwrap_or_default();
        serde_yaml_ng::from_str(&content).unwrap_or(ProfilesYaml { current: None, items: vec![] })
    } else {
        ProfilesYaml { current: None, items: vec![] }
    };

    yaml.items.push(ProfileItem {
        uid: id.clone(),
        name: name.to_string(),
        file: Some(profile_file.to_string_lossy().to_string()),
        url: Some(url.clone()),
    });

    let yaml_content = serde_yaml_ng::to_string(&yaml).unwrap_or_default();
    if let Err(e) = std::fs::write(&profiles_path, yaml_content) {
        return HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e.to_string()));
    }

    HttpResponse::Ok().json(ApiResponse::success(Profile {
        id,
        name: name.to_string(),
        url: Some(url.clone()),
        file: None,
        active: false,
        cron: None,
    }))
}

pub async fn delete_profile(path: web::Path<String>) -> HttpResponse {
    let id = path.into_inner();
    let config_dir = get_config_dir();
    let profiles_path = config_dir.join("profiles.yaml");

    if !profiles_path.exists() {
        return HttpResponse::NotFound().json(ApiResponse::<()>::error("Profile not found"));
    }

    let content = std::fs::read_to_string(&profiles_path).unwrap_or_default();

    #[derive(Serialize, Deserialize)]
    struct ProfilesYaml {
        current: Option<String>,
        items: Vec<ProfileItem>,
    }
    #[derive(Serialize, Deserialize)]
    struct ProfileItem {
        uid: String,
        name: String,
        #[serde(rename = "file")]
        file: Option<String>,
        url: Option<String>,
    }

    let mut yaml: ProfilesYaml = match serde_yaml_ng::from_str(&content) {
        Ok(y) => y,
        Err(_) => return HttpResponse::NotFound().json(ApiResponse::<()>::error("Parse error")),
    };

    let initial_len = yaml.items.len();
    yaml.items.retain(|p| p.uid != id);

    if yaml.items.len() == initial_len {
        return HttpResponse::NotFound().json(ApiResponse::<()>::error("Profile not found"));
    }

    let yaml_content = serde_yaml_ng::to_string(&yaml).unwrap_or_default();
    if let Err(e) = std::fs::write(&profiles_path, yaml_content) {
        return HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e.to_string()));
    }

    HttpResponse::Ok().json(ApiResponse::success(()))
}

pub async fn activate_profile(path: web::Path<String>) -> HttpResponse {
    let id = path.into_inner();
    let profiles_path = get_config_dir().join("profiles.yaml");

    if !profiles_path.exists() {
        return HttpResponse::NotFound().json(ApiResponse::<()>::error("Profile not found"));
    }

    let content = match std::fs::read_to_string(&profiles_path) {
        Ok(c) => c,
        Err(_) => return HttpResponse::NotFound().json(ApiResponse::<()>::error("Profile not found")),
    };

    #[derive(Serialize, Deserialize)]
    struct ProfilesYaml {
        current: Option<String>,
        items: Vec<ProfileItem>,
    }
    #[derive(Serialize, Deserialize)]
    struct ProfileItem {
        uid: String,
        name: String,
        #[serde(rename = "file")]
        file: Option<String>,
        url: Option<String>,
    }

    let mut yaml: ProfilesYaml = match serde_yaml_ng::from_str(&content) {
        Ok(y) => y,
        Err(_) => return HttpResponse::NotFound().json(ApiResponse::<()>::error("Parse error")),
    };

    // Find the profile item to get its file path
    let profile_item = yaml.items.iter().find(|p| p.uid == id);
    let profile_file_path = match profile_item {
        Some(item) => item.file.clone(),
        None => return HttpResponse::NotFound().json(ApiResponse::<()>::error("Profile not found")),
    };

    // Mark profile as current in profiles.yaml
    yaml.current = Some(id.clone());
    let yaml_content = serde_yaml_ng::to_string(&yaml).unwrap_or_default();
    if let Err(e) = std::fs::write(&profiles_path, yaml_content) {
        return HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e.to_string()));
    }

    // Replace active config with profile content via ActiveConfigStore
    if let Some(ref file) = profile_file_path {
        let profile_file = std::path::PathBuf::from(file);
        if profile_file.exists() {
            let store = match crate::settings::shared_paths() {
                Ok(paths) => control_tower_service_core::ActiveConfigStore::new(paths),
                Err(e) => return HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e.to_string())),
            };
            if let Err(e) = store.replace_from_profile(&profile_file) {
                return HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e.to_string()));
            }
        }
    }

    HttpResponse::Ok().json(ApiResponse::success(()))
}

pub async fn get_mode() -> HttpResponse {
    if !is_clash_api_running() {
        return HttpResponse::Ok().json(ApiResponse::success("rule"));
    }

    let url = format!("http://{}:{}/proxies", CLASH_API_HOST, crate::settings::get_api_port());
    let client = reqwest::Client::new();

    if let Ok(response) = client.get(&url).timeout(Duration::from_secs(5)).send().await {
        if response.status().is_success() {
            if let Ok(json) = response.json::<serde_json::Value>().await {
                let mode = json.get("mode").and_then(|v| v.as_str()).unwrap_or("rule");
                return HttpResponse::Ok().json(ApiResponse::success(mode));
            }
        }
    }

    HttpResponse::Ok().json(ApiResponse::success("rule"))
}

pub async fn set_mode(req: web::Json<SetModeRequest>) -> HttpResponse {
    let mode = &req.mode;
    let valid_modes = ["rule", "global", "direct"];

    if !valid_modes.contains(&mode.as_str()) {
        return HttpResponse::BadRequest().json(ApiResponse::<()>::error("Invalid mode"));
    }

    match crate::service::set_mode(mode).await {
        Ok(()) => HttpResponse::Ok().json(ApiResponse::success(())),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e.to_string())),
    }
}

pub async fn get_proxies() -> HttpResponse {
    if !is_clash_api_running() {
        return HttpResponse::Ok().json(ApiResponse::<()>::error("Service not running"));
    }

    let url = format!("http://{}:{}/proxies", CLASH_API_HOST, crate::settings::get_api_port());
    let client = reqwest::Client::new();

    match client.get(&url).timeout(Duration::from_secs(5)).send().await {
        Ok(response) => {
            if response.status().is_success() {
                if let Ok(json) = response.json::<serde_json::Value>().await {
                    return HttpResponse::Ok().json(ApiResponse::success(json));
                }
            }
        }
        Err(e) => return HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e.to_string())),
    }

    HttpResponse::InternalServerError().json(ApiResponse::<()>::error("Failed to get proxies"))
}

pub async fn select_proxy(req: web::Json<SelectProxyRequest>) -> HttpResponse {
    match crate::service::select_proxy(&req.name).await {
        Ok(()) => HttpResponse::Ok().json(ApiResponse::success(())),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e.to_string())),
    }
}

pub async fn get_connections() -> HttpResponse {
    match crate::service::get_connections_via_ipc().await {
        Ok(json) => HttpResponse::Ok().json(ApiResponse::success(json)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e.to_string())),
    }
}

pub async fn close_connection(path: web::Path<String>) -> HttpResponse {
    let id = path.into_inner();
    match crate::service::close_connection_via_ipc(&id).await {
        Ok(()) => HttpResponse::Ok().json(ApiResponse::success(())),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e.to_string())),
    }
}

#[derive(Serialize)]
pub struct ServiceStatus {
    pub running: bool,
    pub mode: String,
}

pub async fn service_status() -> HttpResponse {
    let running = is_clash_api_running();

    let mode = if running {
        let url = format!("http://{}:{}/proxies", CLASH_API_HOST, crate::settings::get_api_port());
        let client = reqwest::Client::new();
        match client.get(&url).timeout(Duration::from_secs(2)).send().await {
            Ok(response) => {
                if let Ok(json) = response.json::<serde_json::Value>().await {
                    json.get("mode").and_then(|v| v.as_str()).map(String::from).unwrap_or_else(|| "rule".to_string())
                } else {
                    "rule".to_string()
                }
            }
            Err(_) => "rule".to_string()
        }
    } else {
        "stopped".to_string()
    };

    HttpResponse::Ok().json(ApiResponse::success(ServiceStatus { running, mode }))
}

pub async fn get_config() -> HttpResponse {
    let config_dir = get_config_dir();
    let verge_config = config_dir.join("verge.yaml");

    if !verge_config.exists() {
        return HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({})));
    }

    match std::fs::read_to_string(&verge_config) {
        Ok(content) => {
            let json: serde_json::Value = serde_yaml_ng::from_str(&content).unwrap_or_default();
            HttpResponse::Ok().json(ApiResponse::success(json))
        }
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(&e.to_string())),
    }
}

#[derive(Debug, Deserialize)]
pub struct ProxyDelayRequest {
    pub name: String,
    pub url: Option<String>,
    pub timeout: Option<u64>,
}

pub async fn proxy_delay(req: web::Json<ProxyDelayRequest>) -> HttpResponse {
    if !is_clash_api_running() {
        return HttpResponse::BadRequest().json(ApiResponse::<()>::error("Service not running"));
    }

    let name = &req.name;
    let url = req.url.as_deref().unwrap_or("http://cp.cloudflare.com/generate_204");
    let timeout = req.timeout.unwrap_or(10000);

    let clash_url = format!("http://{}:{}/proxies/{}/delay", CLASH_API_HOST, crate::settings::get_api_port(), name);

    let client = reqwest::Client::new();
    match client
        .get(&clash_url)
        .query(&[("url", url), ("timeout", &timeout.to_string())])
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        Ok(response) => {
            if response.status().is_success() {
                if let Ok(json) = response.json::<serde_json::Value>().await {
                    let delay = json.get("delay").and_then(|v| v.as_u64()).unwrap_or(0);
                    return HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({ "delay": delay })));
                }
            }
            HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({ "delay": 0 })))
        }
        Err(_e) => HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({ "delay": 0 }))),
    }
}

pub async fn update_profile(_path: web::Path<String>) -> HttpResponse {
    // For now, just mark as success - actual update would re-download subscription
    HttpResponse::Ok().json(ApiResponse::success(()))
}

pub async fn start_service() -> HttpResponse {
    // Service start would trigger clash-core here
    // For now, just return success if not running
    if is_clash_api_running() {
        return HttpResponse::Ok().json(ApiResponse::success(()));
    }
    HttpResponse::Ok().json(ApiResponse::success(()))
}

pub async fn stop_service() -> HttpResponse {
    if !is_clash_api_running() {
        return HttpResponse::Ok().json(ApiResponse::success(()));
    }

    // Send shutdown command to Clash
    let url = format!("http://{}:{}/shutdown", CLASH_API_HOST, crate::settings::get_api_port());
    let client = reqwest::Client::new();

    match client.post(&url).timeout(Duration::from_secs(5)).send().await {
        Ok(_) => HttpResponse::Ok().json(ApiResponse::success(())),
        Err(_) => HttpResponse::Ok().json(ApiResponse::success(())),
    }
}
