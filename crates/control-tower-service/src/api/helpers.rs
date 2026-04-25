//! Helper functions for HTTP API handlers

use std::path::PathBuf;

/// Find settings.yaml path (same logic as CLI's settings.rs)
pub fn find_settings_path() -> Option<PathBuf> {
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
pub fn get_mihomo_http_port() -> u16 {
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
pub fn get_control_tower_paths() -> control_tower_service_core::ControlTowerPaths {
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
pub fn parse_profiles_yaml_full(content: &str) -> serde_json::Value {
    use control_tower_service_core::ProfilesYaml;
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
