//! Connection handlers: list connections, close connection

use actix_web::{web, HttpResponse};
use std::sync::Arc;
use tokio::task;

use crate::ServiceState;

use super::ApiResponse;

/// GET /api/connections - Returns Mihomo connections
pub async fn get_connections(
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    let state = state.clone();
    match task::spawn_blocking(move || state.get_connections()).await {
        Ok(Ok(connections)) => HttpResponse::Ok().json(ApiResponse::success(connections)),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// DELETE /api/connections/{id} - Close a specific connection and record to history
pub async fn close_connection(
    state: web::Data<Arc<ServiceState>>,
    path: web::Path<String>,
) -> HttpResponse {
    let id = path.into_inner();
    let id_for_info = id.clone();
    let id_for_close = id.clone();
    let state_for_info = state.clone();
    let state_for_close = state.clone();
    let state_for_history = state.clone();

    // Get connection info before closing for history record
    let connection_info = task::spawn_blocking(move || {
        state_for_info.get_connection_info(&id_for_info)
    }).await;

    let close_result = task::spawn_blocking(move || state_for_close.close_connection(&id_for_close)).await;

    // Record to history regardless of close result
    if let Ok(Ok(Some(conn))) = connection_info {
        let closed = crate::settings::ClosedConnection {
            id: conn.id,
            source_ip: conn.source_ip,
            destination: conn.destination,
            chains: conn.chains,
            upload: conn.upload,
            download: conn.download,
            closed_at: chrono::Utc::now().to_rfc3339(),
        };
        let _ = task::spawn_blocking(move || {
            state_for_history.record_closed_connection(closed)
        }).await;
    }

    match close_result {
        Ok(Ok(())) => HttpResponse::Ok().json(ApiResponse::<()>::success(())),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// GET /api/connections/history - Returns closed connection history
pub async fn get_connection_history(
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    let settings = state.get_settings();
    let history = settings.closed_connections;
    HttpResponse::Ok().json(ApiResponse::success(history))
}

/// DELETE /api/connections/history - Clear connection history
pub async fn clear_connection_history(
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    let settings = crate::SettingsData {
        closed_connections: vec![],
        ..Default::default()
    };
    let state_inner = state.clone();
    match task::spawn_blocking(move || state_inner.save_settings(&settings)).await {
        Ok(Ok(())) => HttpResponse::Ok().json(ApiResponse::<()>::success(())),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}
