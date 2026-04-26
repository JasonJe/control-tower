//! Service lifecycle handlers: status, start, stop

use actix_web::{web, HttpResponse};
use std::sync::Arc;
use tokio::task;

use crate::ServiceState;

use super::ApiResponse;

/// GET /api/status - Returns service status
pub async fn service_status(
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    let status = state.status();
    HttpResponse::Ok().json(ApiResponse::success(status))
}

/// POST /api/service/start - Start Mihomo
pub async fn service_start(
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    let paths = super::get_control_tower_paths();
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

/// POST /api/service/reload - Trigger Mihomo config hot-reload
pub async fn service_reload(
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    let state = state.clone();
    match task::spawn_blocking(move || state.reload_config()).await {
        Ok(Ok(())) => HttpResponse::Ok().json(ApiResponse::<()>::success(())),
        Ok(Err(e)) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Reload failed: {}", e))),
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Reload task error: {}", e))),
    }
}
