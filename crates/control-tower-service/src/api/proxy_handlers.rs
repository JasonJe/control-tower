//! Proxy-related handlers: list, select, mode

use actix_web::{web, HttpRequest, HttpResponse};
use std::sync::Arc;
use tokio::task;

use crate::ServiceState;

use super::{ApiResponse, SelectProxyRequest, SetModeRequest};
use super::helpers::require_auth;

/// GET /api/proxies - Returns Mihomo proxies (protected)
pub async fn get_proxies(
    req: HttpRequest,
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    if require_auth(&req, &state).is_none() {
        return HttpResponse::Unauthorized().json(ApiResponse::<()>::error("Unauthorized"));
    }
    let state = state.clone();
    match task::spawn_blocking(move || state.get_proxies()).await {
        Ok(Ok(proxies)) => HttpResponse::Ok().json(ApiResponse::success(proxies)),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// POST /api/proxies/select - Select a proxy in GLOBAL group (protected)
pub async fn select_proxy(
    req: HttpRequest,
    state: web::Data<Arc<ServiceState>>,
    body: web::Json<SelectProxyRequest>,
) -> HttpResponse {
    if require_auth(&req, &state).is_none() {
        return HttpResponse::Unauthorized().json(ApiResponse::<()>::error("Unauthorized"));
    }
    let name = body.name.clone();
    let state = state.clone();
    match task::spawn_blocking(move || state.select_proxy(&name)).await {
        Ok(Ok(())) => HttpResponse::Ok().json(ApiResponse::<()>::success(())),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// GET /api/mode - Get current proxy mode (protected)
pub async fn get_mode(
    req: HttpRequest,
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    if require_auth(&req, &state).is_none() {
        return HttpResponse::Unauthorized().json(ApiResponse::<()>::error("Unauthorized"));
    }
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

/// POST /api/mode - Set proxy mode (protected)
pub async fn set_mode(
    req: HttpRequest,
    state: web::Data<Arc<ServiceState>>,
    body: web::Json<SetModeRequest>,
) -> HttpResponse {
    if require_auth(&req, &state).is_none() {
        return HttpResponse::Unauthorized().json(ApiResponse::<()>::error("Unauthorized"));
    }
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
