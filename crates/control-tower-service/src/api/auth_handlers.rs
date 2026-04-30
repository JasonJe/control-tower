//! Authentication handlers: status, login, setup-password, nonce, logout

use actix_web::{web, HttpResponse};
use std::sync::Arc;
use tokio::task;

use crate::ServiceState;
use super::ApiResponse;

/// GET /api/auth/nonce - Returns a random nonce for challenge-response
pub async fn get_nonce(
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    let settings = state.get_settings();

    // Check if auth is enabled and password is set
    let auth_config = settings.auth.as_ref();
    let auth_enabled = auth_config.map(|a| a.enabled).unwrap_or(true);
    let password_set = auth_config
        .and_then(|a| a.password.as_ref())
        .map(|p| !p.is_empty())
        .unwrap_or(false);

    if !auth_enabled || !password_set {
        return HttpResponse::Ok().json(ApiResponse::<()>::error("Auth not configured or no password set"));
    }

    // Generate random nonce (32 bytes hex string)
    let nonce: String = (0..32)
        .map(|_| {
            let b = rand::random::<u8>();
            format!("{:02x}", b)
        })
        .collect();

    // Store nonce in memory (simple in-memory for now, could be Redis in production)
    // For now, just return the nonce - client will send it back with the hash
    // Server will need to use the same nonce to verify

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

    // Get auth config
    let auth_config = settings.auth.as_ref();
    let auth_enabled = auth_config.map(|a| a.enabled).unwrap_or(true);
    let password_set = auth_config
        .and_then(|a| a.password.as_ref())
        .map(|p| !p.is_empty())
        .unwrap_or(false);

    // Check if client has valid session token
    let authenticated = check_session_token(&req, &state);

    HttpResponse::Ok().json(serde_json::json!({
        "code": 0,
        "data": {
            "auth_enabled": auth_enabled,
            "password_set": password_set,
            "authenticated": authenticated
        }
    }))
}

/// Extract and verify session token from Authorization header
fn check_session_token(req: &actix_web::HttpRequest, state: &ServiceState) -> bool {
    if let Some(auth_header) = req.headers().get("Authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                return state.verify_session(token);
            }
        }
    }
    false
}

/// POST /api/auth/logout - Invalidate session token
pub async fn logout(
    req: actix_web::HttpRequest,
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    if let Some(auth_header) = req.headers().get("Authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                state.remove_session(token);
                return HttpResponse::Ok().json(ApiResponse::<()>::success(()));
            }
        }
    }
    HttpResponse::Ok().json(ApiResponse::<()>::success(()))
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

    // Compute expected hash: sha256(stored_key + nonce)
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(stored_key.as_bytes());
    hasher.update(nonce.as_bytes());
    let expected_hash = format!("{:x}", hasher.finalize());

    if client_hash == expected_hash {
        tracing::info!("User logged in successfully");
        let token = state.create_session();
        HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({ "token": token })))
    } else {
        tracing::warn!("Failed login attempt - invalid password hash");
        HttpResponse::Ok().json(ApiResponse::<()>::error("Invalid password"))
    }
}

/// POST /api/auth/setup-password - Set initial password
pub async fn setup_password(
    state: web::Data<Arc<ServiceState>>,
    body: web::Json<SetupPasswordRequest>,
) -> HttpResponse {
    // Validate password length
    if body.password.len() < 6 {
        return HttpResponse::Ok().json(ApiResponse::<()>::error("Password must be at least 6 characters"));
    }

    // Derive key using PBKDF2-SHA256: key = PBKDF2(password, salt, 100k iterations)
    // This MUST match the client's key derivation (see index.html loginSubmit)
    let salt = b"control-tower-auth-key-v1";
    let key = pbkdf2_hash(&body.password, salt);

    // Save to settings (store the derived key, not the password)
    let mut new_settings = state.get_settings();
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

/// Derive a key from password using PBKDF2-SHA256
/// Matches the client's key derivation (100k iterations, 32 bytes)
fn pbkdf2_hash(password: &str, salt: &[u8]) -> String {
    use pbkdf2::pbkdf2_hmac_array;
    use sha2::Sha256;

    const ITERATIONS: u32 = 100_000;
    const KEY_LEN: usize = 32;

    let key: [u8; KEY_LEN] = pbkdf2_hmac_array::<Sha256, KEY_LEN>(
        password.as_bytes(),
        salt,
        ITERATIONS,
    );

    // Convert to hex string for storage
    key.iter().map(|b| format!("{:02x}", b)).collect()
}

#[derive(serde::Deserialize)]
pub struct LoginRequest {
    hash: String,
    nonce: String,
}

#[derive(serde::Deserialize)]
pub struct SetupPasswordRequest {
    password: String,
}