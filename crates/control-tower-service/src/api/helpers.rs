//! Helper functions for HTTP API handlers

use std::path::PathBuf;
use actix_web::HttpRequest;
use std::sync::Arc;

use crate::ServiceState;

use crate::settings::consts::DEFAULT_MIHOMO_HTTP_PORT;

/// Find settings.yaml path (same logic as the rest of the service)
pub fn find_settings_path() -> Option<PathBuf> {
    Some(crate::settings::resolve_settings_path())
}

/// Get Mihomo HTTP proxy port from settings.yaml, defaulting to DEFAULT_MIHOMO_HTTP_PORT
pub fn get_mihomo_http_port() -> u16 {
    #[derive(serde::Deserialize)]
    struct Settings {
        #[serde(rename = "http_port", default)]
        http_port: Option<u16>,
    }
    if let Some(path) = find_settings_path() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(settings) = serde_yaml_ng::from_str::<Settings>(&content) {
                return settings.http_port.unwrap_or(DEFAULT_MIHOMO_HTTP_PORT);
            }
        }
    }
    DEFAULT_MIHOMO_HTTP_PORT
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

/// Update profile_rules_count in settings.yaml, preserving all other fields.
pub fn update_profile_rules_count(count: usize) -> anyhow::Result<()> {
    use crate::settings::SettingsData;

    let settings_path = find_settings_path().unwrap_or_else(|| PathBuf::from("settings.yaml"));
    let content = if settings_path.exists() {
        std::fs::read_to_string(&settings_path)?
    } else {
        String::new()
    };

    let mut settings: SettingsData = serde_yaml_ng::from_str(&content)
        .unwrap_or_else(|_| SettingsData::default());

    settings.profile_rules_count = Some(count);

    let yaml_str = serde_yaml_ng::to_string(&settings)?;
    let temp = settings_path.with_extension("yaml.tmp");
    std::fs::write(&temp, &yaml_str)?;
    std::fs::rename(&temp, &settings_path)?;
    Ok(())
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
                    // Never expose custom header values (subscription tokens,
                    // Authorization, ...) through the API. The Web UI does not
                    // need them, and PATCH preserves stored headers when the
                    // `headers` field is omitted.
                    let mut options = item.options;
                    if let Some(opts) = options.as_mut() {
                        opts.headers = None;
                    }
                    serde_json::json!({
                        "uid": item.uid,
                        "name": item.name,
                        "file": item.file,
                        "url": item.url,
                        "cron": item.cron,
                        "updated_at": item.updated_at,
                        "type": item.type_,
                        "script": item.script,
                        "merge": item.merge,
                        "options": options,
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

/// Whether API requests must carry a valid session token.
///
/// Auth is only enforced when it is explicitly enabled **and** a password has
/// been configured. A missing `auth` section (the default settings layout),
/// `auth.enabled: false`, or an enabled-but-not-yet-initialized auth config
/// must not lock the API behind a 401 — otherwise the Web UI cannot even reach
/// the setup-password endpoint and `ctctl`/API clients get stuck.
pub fn auth_required(settings: &crate::SettingsData) -> bool {
    match settings.auth.as_ref() {
        Some(auth) => {
            auth.enabled
                && auth
                    .password
                    .as_deref()
                    .map(|p| !p.trim().is_empty())
                    .unwrap_or(false)
        }
        None => false,
    }
}

/// Check if an `Authorization` header value grants access.
///
/// Split out from [`check_auth`] so the global middleware (which only has a
/// `ServiceRequest`) can reuse the exact same rules.
pub fn check_auth_header(state: &Arc<crate::ServiceState>, auth_header: Option<&str>) -> bool {
    // Auth disabled or not configured yet: allow all.
    let settings = state.get_settings();
    if !auth_required(&settings) {
        return true;
    }

    auth_header
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(|token| state.verify_session(token))
        .unwrap_or(false)
}

/// Check if request has valid auth token
/// Returns true if authenticated (or auth is disabled), false otherwise
pub fn check_auth(req: &HttpRequest, state: &Arc<crate::ServiceState>) -> bool {
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|value| value.to_str().ok());
    check_auth_header(state, auth_header)
}

/// Require auth helper - returns Some(()) if authenticated, None otherwise
pub fn require_auth(req: &HttpRequest, state: &Arc<ServiceState>) -> Option<()> {
    if check_auth(req, state) {
        Some(())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{AuthConfig, SettingsData};

    #[test]
    fn auth_is_not_required_when_section_is_missing() {
        let settings = SettingsData::default();
        assert!(!auth_required(&settings));
    }

    #[test]
    fn auth_is_not_required_when_disabled() {
        let settings = SettingsData {
            auth: Some(AuthConfig {
                enabled: false,
                password: Some("hash".to_string()),
            }),
            ..SettingsData::default()
        };
        assert!(!auth_required(&settings));
    }

    #[test]
    fn auth_is_not_required_before_password_is_set() {
        let settings = SettingsData {
            auth: Some(AuthConfig {
                enabled: true,
                password: None,
            }),
            ..SettingsData::default()
        };
        assert!(!auth_required(&settings));
    }

    #[test]
    fn auth_is_required_when_enabled_and_password_set() {
        let settings = SettingsData {
            auth: Some(AuthConfig {
                enabled: true,
                password: Some("hash".to_string()),
            }),
            ..SettingsData::default()
        };
        assert!(auth_required(&settings));
    }
}
