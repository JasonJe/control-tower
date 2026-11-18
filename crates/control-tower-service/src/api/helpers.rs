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
    crate::service::settings_ops::invalidate_settings_cache();
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

/// Name of the httpOnly session cookie used by the Web UI.
pub const SESSION_COOKIE: &str = "ct_session";

/// Header the Web UI must send on cookie-authenticated requests.
///
/// Browsers attach cookies automatically, so a cookie alone cannot prove the
/// request originated from our own page. A custom header cannot be added
/// cross-origin without a CORS preflight (which this server does not grant),
/// which blocks CSRF even from same-site origins such as another port on
/// 127.0.0.1.
pub const CSRF_HEADER: &str = "X-Requested-With";
pub const CSRF_HEADER_VALUE: &str = "control-tower";

/// Where the session token in a request came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenSource {
    /// `Authorization: Bearer <token>` — used by `ctctl`/scripts.
    Header,
    /// httpOnly cookie — used by the Web UI, requires the CSRF header.
    Cookie,
}

/// Read a single value from a raw `Cookie` header.
pub fn cookie_value<'a>(cookie_header: Option<&'a str>, name: &str) -> Option<&'a str> {
    cookie_header?.split(';').find_map(|pair| {
        let (key, value) = pair.trim().split_once('=')?;
        if key == name { Some(value) } else { None }
    })
}

/// Extract the session token from the `Authorization` header or the session
/// cookie. The header wins when both are present.
pub fn session_token(
    headers: &actix_web::http::header::HeaderMap,
) -> Option<(TokenSource, String)> {
    let bearer = headers
        .get("Authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|token| !token.is_empty());
    if let Some(token) = bearer {
        return Some((TokenSource::Header, token.to_string()));
    }

    let cookie_header = headers.get("Cookie").and_then(|value| value.to_str().ok());
    cookie_value(cookie_header, SESSION_COOKIE)
        .filter(|token| !token.is_empty())
        .map(|token| (TokenSource::Cookie, token.to_string()))
}

/// Verify the credentials carried by a request.
///
/// Auth is a no-op unless it is enabled and a password is configured. Bearer
/// tokens are accepted from any client; the cookie path additionally requires
/// [`CSRF_HEADER`] so a foreign page cannot drive the API with the user's
/// session cookie.
pub fn check_request_auth(
    state: &Arc<crate::ServiceState>,
    headers: &actix_web::http::header::HeaderMap,
) -> bool {
    match session_token(headers) {
        Some((TokenSource::Header, token)) => check_token(state, Some(&token)),
        Some((TokenSource::Cookie, token)) => {
            let csrf_ok = headers
                .get(CSRF_HEADER)
                .and_then(|value| value.to_str().ok())
                == Some(CSRF_HEADER_VALUE);
            csrf_ok && check_token(state, Some(&token))
        }
        // No credentials offered: allowed only when auth is not required.
        None => check_token(state, None),
    }
}

/// Verify a raw session token. `None` is denied only when auth is required.
pub fn check_token(state: &Arc<crate::ServiceState>, token: Option<&str>) -> bool {
    // Auth disabled or not configured yet: allow all.
    let settings = state.get_settings();
    if !auth_required(&settings) {
        return true;
    }

    token.map(|token| state.verify_session(token)).unwrap_or(false)
}

/// Check if request has valid auth token
/// Returns true if authenticated (or auth is disabled), false otherwise
pub fn check_auth(req: &HttpRequest, state: &Arc<crate::ServiceState>) -> bool {
    check_request_auth(state, req.headers())
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
    fn cookie_value_is_parsed() {
        assert_eq!(
            cookie_value(Some("a=1; ct_session=abc; b=2"), SESSION_COOKIE),
            Some("abc")
        );
        assert_eq!(cookie_value(Some("other=1"), SESSION_COOKIE), None);
        assert_eq!(cookie_value(None, SESSION_COOKIE), None);
        // The cookie name must match exactly, not as a prefix.
        assert_eq!(cookie_value(Some("xct_session=abc"), SESSION_COOKIE), None);
    }

    #[test]
    fn session_token_prefers_authorization_header() {
        use actix_web::http::header::{HeaderMap, HeaderName, HeaderValue};

        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("Bearer header-token"),
        );
        headers.insert(
            HeaderName::from_static("cookie"),
            HeaderValue::from_static("ct_session=cookie-token"),
        );

        let (source, token) = session_token(&headers).expect("token");
        assert_eq!(source, TokenSource::Header);
        assert_eq!(token, "header-token");
    }

    #[test]
    fn session_token_falls_back_to_cookie() {
        use actix_web::http::header::{HeaderMap, HeaderName, HeaderValue};

        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("cookie"),
            HeaderValue::from_static("a=1; ct_session=cookie-token"),
        );

        let (source, token) = session_token(&headers).expect("token");
        assert_eq!(source, TokenSource::Cookie);
        assert_eq!(token, "cookie-token");
    }

    #[test]
    fn session_token_absent_without_header_or_cookie() {
        assert!(session_token(&actix_web::http::header::HeaderMap::new()).is_none());
    }

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
