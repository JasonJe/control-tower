//! Service lifecycle handlers: status, start, stop

use actix_web::{web, HttpRequest, HttpResponse};
use std::sync::Arc;
use tokio::task;

use crate::ServiceState;

use super::ApiResponse;
use super::helpers::require_auth;

/// GET /api/status - Returns service status (protected)
pub async fn service_status(
    req: HttpRequest,
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    if require_auth(&req, &state).is_none() {
        return HttpResponse::Unauthorized().json(ApiResponse::<()>::error("Unauthorized"));
    }
    let status = state.status();
    HttpResponse::Ok().json(ApiResponse::success(status))
}

/// POST /api/service/start - Start Mihomo (protected)
pub async fn service_start(
    req: HttpRequest,
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    if require_auth(&req, &state).is_none() {
        return HttpResponse::Unauthorized().json(ApiResponse::<()>::error("Unauthorized"));
    }
    let paths = super::get_control_tower_paths();
    let config_path = paths.active_config_path.clone();
    let state = state.clone();

    match task::spawn_blocking(move || state.start(&config_path)).await {
        Ok(Ok(())) => HttpResponse::Ok().json(ApiResponse::<()>::success(())),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// POST /api/service/stop - Stop Mihomo (protected)
pub async fn service_stop(
    req: HttpRequest,
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    if require_auth(&req, &state).is_none() {
        return HttpResponse::Unauthorized().json(ApiResponse::<()>::error("Unauthorized"));
    }
    let state = state.clone();
    match task::spawn_blocking(move || state.stop()).await {
        Ok(Ok(())) => HttpResponse::Ok().json(ApiResponse::<()>::success(())),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// POST /api/service/reload - Trigger Mihomo config hot-reload (protected)
pub async fn service_reload(
    req: HttpRequest,
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    if require_auth(&req, &state).is_none() {
        return HttpResponse::Unauthorized().json(ApiResponse::<()>::error("Unauthorized"));
    }
    let state = state.clone();
    match task::spawn_blocking(move || state.reload_config()).await {
        Ok(Ok(())) => HttpResponse::Ok().json(ApiResponse::<()>::success(())),
        Ok(Err(e)) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Reload failed: {}", e))),
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Reload task error: {}", e))),
    }
}
