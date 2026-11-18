//! Authentication handlers: status, login, setup-password, nonce, logout

use actix_web::{web, HttpResponse};
use std::sync::Arc;
use tokio::task;

use crate::ServiceState;
use super::{session_token, ApiResponse, SESSION_COOKIE};

/// GET /api/auth/nonce - Returns a random nonce for challenge-response
pub async fn get_nonce(
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    let settings = state.get_settings();

    // A nonce is only meaningful when auth is actually being enforced
    // (enabled *and* a password has been configured). Missing/disabled auth
    // must not report a login challenge.
    if !super::auth_required(&settings) {
        return HttpResponse::Ok().json(ApiResponse::<()>::error("Auth not configured or no password set"));
    }

    // Generate random nonce (32 bytes hex string)
    let nonce: String = (0..32)
        .map(|_| {
            let b = rand::random::<u8>();
            format!("{:02x}", b)
        })
        .collect();

    // Store nonce in memory so the login handler can verify it was issued by
    // us, and consume it on first use (single-use challenge).
    state.issue_nonce(nonce.clone());

    // Return nonce to client
    HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
        "nonce": nonce
    })))
}

/// GET /api/auth/status - Returns current auth state
pub async fn get_auth_status(
    req: actix_web::HttpRequest,
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    let settings = state.get_settings();

    // Get auth config. A missing `auth` section means auth is off (the UI
    // should go straight to the main view instead of forcing setup/login).
    let auth_config = settings.auth.as_ref();
    let auth_enabled = auth_config.map(|a| a.enabled).unwrap_or(false);
    let password_set = auth_config
        .and_then(|a| a.password.as_ref())
        .map(|p| !p.trim().is_empty())
        .unwrap_or(false);

    // Check if client has a valid bearer token or session cookie. This is a
    // read-only probe, so the CSRF header is not required here.
    let authenticated = session_token(req.headers())
        .map(|(_, token)| state.verify_session(&token))
        .unwrap_or(false);

    HttpResponse::Ok().json(serde_json::json!({
        "code": 0,
        "data": {
            "auth_enabled": auth_enabled,
            "password_set": password_set,
            "authenticated": authenticated
        }
    }))
}

/// POST /api/auth/logout - Invalidate session token and clear the cookie
pub async fn logout(
    req: actix_web::HttpRequest,
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    if let Some((_, token)) = session_token(req.headers()) {
        state.remove_session(&token);
    }

    let mut res = HttpResponse::Ok().json(ApiResponse::<()>::success(()));
    if let Err(e) = res.add_cookie(&session_cookie(String::new(), is_https(&req), true)) {
        tracing::warn!("Failed to expire session cookie: {}", e);
    }
    res
}

/// POST /api/auth/login - Verify password using challenge-response
///
/// Flow:
/// 1. Client calls GET /api/auth/nonce to get a random nonce
/// 2. Client computes: key = argon2(password, nonce), hash = sha256(key + nonce)
/// 3. Client calls POST /api/auth/login with { hash, nonce }
/// 4. Server retrieves stored key (argon2 of password), computes expected_hash = sha256(stored_key + nonce)
/// 5. Server compares hash == expected_hash
///
/// This way the actual password is never transmitted.
pub async fn login(
    req: actix_web::HttpRequest,
    state: web::Data<Arc<ServiceState>>,
    body: web::Json<LoginRequest>,
) -> HttpResponse {
    let settings = state.get_settings();

    let auth_config = match settings.auth.as_ref() {
        Some(a) => a,
        None => return HttpResponse::Ok().json(ApiResponse::<()>::error("Auth not configured")),
    };

    // If password not set, return error
    let Some(stored_key) = &auth_config.password else {
        return HttpResponse::Ok().json(ApiResponse::<()>::error("Password not set"));
    };

    if stored_key.is_empty() {
        return HttpResponse::Ok().json(ApiResponse::<()>::error("Password not set"));
    }

    // Verify using challenge-response
    // Client sends: hash = sha256(key + nonce) where key = argon2(password, nonce)
    // We compute expected_hash = sha256(stored_key + nonce) and compare
    let client_hash = body.hash.as_str();
    let nonce = body.nonce.as_str();

    // The nonce must have been issued by us and must not have been used before.
    // Consume it even when the password is wrong so a failed attempt cannot be
    // replayed with the same challenge.
    if !state.consume_nonce(nonce) {
        tracing::warn!("Failed login attempt - unknown or expired nonce");
        return HttpResponse::Unauthorized()
            .json(ApiResponse::<()>::error("Invalid or expired nonce"));
    }

    // Compute expected hash: sha256(stored_key + nonce)
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(stored_key.as_bytes());
    hasher.update(nonce.as_bytes());
    let expected_hash = format!("{:x}", hasher.finalize());

    if client_hash == expected_hash {
        tracing::info!("User logged in successfully");
        let token = state.create_session();
        // The Web UI keeps the token in an httpOnly cookie so page scripts
        // (including a hypothetical XSS) cannot read it. CLI/API clients keep
        // using the bearer token returned in the body.
        let mut res = HttpResponse::Ok().json(ApiResponse::success(
            serde_json::json!({ "token": token }),
        ));
        if let Err(e) = res.add_cookie(&session_cookie(token, is_https(&req), false)) {
            // Without the cookie the browser session would silently fall back
            // to the login screen, so make the failure visible in the log.
            tracing::warn!("Failed to set session cookie: {}", e);
        }
        res
    } else {
        tracing::warn!("Failed login attempt - invalid password hash");
        HttpResponse::Ok().json(ApiResponse::<()>::error("Invalid password"))
    }
}

/// Whether this request arrived over TLS (controls the cookie `Secure` flag).
fn is_https(req: &actix_web::HttpRequest) -> bool {
    req.connection_info().scheme().eq_ignore_ascii_case("https")
}

/// Build the session cookie.
///
/// `clear` produces an expired cookie with the same attributes so the browser
/// removes it on logout.
fn session_cookie(token: String, secure: bool, clear: bool) -> actix_web::cookie::Cookie<'static> {
    use actix_web::cookie::{time::Duration, Cookie, SameSite};

    let max_age = if clear {
        Duration::seconds(0)
    } else {
        Duration::seconds(crate::session::SESSION_TTL_SECS as i64)
    };

    let mut cookie = Cookie::build(SESSION_COOKIE, token)
        .http_only(true)
        .same_site(SameSite::Strict)
        .path("/")
        .max_age(max_age)
        .finish();
    cookie.set_secure(secure);
    cookie
}

/// POST /api/auth/setup-password - Set initial password
pub async fn setup_password(
    req: actix_web::HttpRequest,
    state: web::Data<Arc<ServiceState>>,
    body: web::Json<SetupPasswordRequest>,
) -> HttpResponse {
    // First-run setup is unauthenticated by definition. Restrict it to loopback
    // so somebody on the LAN cannot claim the password before the operator does.
    match req.peer_addr() {
        Some(addr) if is_loopback_addr(addr.ip()) => {}
        _ => {
            return HttpResponse::Forbidden().json(ApiResponse::<()>::error(
                "Initial password can only be set from localhost. Run the setup on the machine hosting the service, or edit settings.yaml manually.",
            ))
        }
    }

    // The client sends the PBKDF2-derived key, never the plaintext password:
    // this endpoint may be reached over plain HTTP, and the password must not
    // be exposed on the wire.
    let key = body.key.trim();
    if key.len() != 64 || !key.chars().all(|c| c.is_ascii_hexdigit()) {
        return HttpResponse::BadRequest()
            .json(ApiResponse::<()>::error("Invalid password key"));
    }
    let key = key.to_ascii_lowercase();

    // This endpoint is reachable without a token (first-run setup), so it must
    // never be able to overwrite an existing credential. Otherwise anyone who
    // can reach the port could reset the password and log in.
    let mut new_settings = state.get_settings();
    let password_already_set = new_settings
        .auth
        .as_ref()
        .and_then(|a| a.password.as_deref())
        .map(|p| !p.trim().is_empty())
        .unwrap_or(false);
    if password_already_set {
        return HttpResponse::Forbidden().json(ApiResponse::<()>::error(
            "Password already set. Edit settings.yaml to reset it.",
        ));
    }

    // Save to settings (store the derived key, not the password)
    let auth = new_settings.auth.get_or_insert_with(|| crate::settings::AuthConfig {
        enabled: true,
        password: None,
    });
    auth.password = Some(key);

    match task::spawn_blocking(move || state.save_settings(&new_settings)).await {
        Ok(Ok(())) => {
            tracing::info!("Password set successfully");
            HttpResponse::Ok().json(ApiResponse::<()>::success(()))
        }
        Ok(Err(e)) => {
            tracing::error!("Failed to save password: {}", e);
            HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e))
        }
        Err(e) => {
            tracing::error!("Task failed: {}", e);
            HttpResponse::InternalServerError().json(ApiResponse::<()>::error("Internal error"))
        }
    }
}

#[derive(serde::Deserialize)]
pub struct LoginRequest {
    hash: String,
    nonce: String,
}

#[derive(serde::Deserialize)]
pub struct SetupPasswordRequest {
    /// PBKDF2(password, "control-tower-auth-key-v1") as a 64-char hex string.
    key: String,
}

/// Whether an address is loopback, including IPv4-mapped IPv6 addresses.
fn is_loopback_addr(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_loopback(),
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.to_ipv4_mapped()
                    .map(|v4| v4.is_loopback())
                    .unwrap_or(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_loopback_addr;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn loopback_addresses_are_recognized() {
        assert!(is_loopback_addr(IpAddr::V4(Ipv4Addr::LOCALHOST)));
        assert!(is_loopback_addr(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert!(is_loopback_addr("::ffff:127.0.0.1".parse().unwrap()));
        assert!(!is_loopback_addr("192.168.1.10".parse().unwrap()));
        assert!(!is_loopback_addr("::ffff:192.168.1.10".parse().unwrap()));
    }
}