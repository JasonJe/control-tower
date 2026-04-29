//! Authentication handlers: status, login, setup-password

use actix_web::{web, HttpResponse};
use std::sync::Arc;
use tokio::task;

use crate::ServiceState;
use super::ApiResponse;

/// GET /api/auth/status - Returns current auth state
pub async fn get_auth_status(
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

    // TODO: implement session tracking
    let authenticated = false;

    HttpResponse::Ok().json(serde_json::json!({
        "code": 0,
        "data": {
            "auth_enabled": auth_enabled,
            "password_set": password_set,
            "authenticated": authenticated
        }
    }))
}

/// POST /api/auth/login - Verify password
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
    let Some(password_hash) = &auth_config.password else {
        return HttpResponse::Ok().json(ApiResponse::<()>::error("Password not set"));
    };

    if password_hash.is_empty() {
        return HttpResponse::Ok().json(ApiResponse::<()>::error("Password not set"));
    }

    // Verify password with bcrypt
    match bcrypt::verify(&body.password, password_hash) {
        Ok(true) => {
            tracing::info!("User logged in successfully");
            HttpResponse::Ok().json(ApiResponse::<()>::success(()))
        }
        Ok(false) => {
            tracing::warn!("Failed login attempt - invalid password");
            HttpResponse::Ok().json(ApiResponse::<()>::error("Invalid password"))
        }
        Err(e) => {
            tracing::error!("Password verification failed: {}", e);
            HttpResponse::InternalServerError().json(ApiResponse::<()>::error("Verification failed"))
        }
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

    // Hash password with bcrypt (cost = 12)
    let hash = match bcrypt::hash(&body.password, 12) {
        Ok(h) => h,
        Err(e) => {
            tracing::error!("Failed to hash password: {}", e);
            return HttpResponse::InternalServerError().json(ApiResponse::<()>::error("Failed to hash password"));
        }
    };

    // Save to settings
    let mut new_settings = state.get_settings();
    let auth = new_settings.auth.get_or_insert_with(|| crate::settings::AuthConfig {
        enabled: true,
        password: None,
    });
    auth.password = Some(hash);

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
    password: String,
}

#[derive(serde::Deserialize)]
pub struct SetupPasswordRequest {
    password: String,
}