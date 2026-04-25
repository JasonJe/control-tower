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

/// DELETE /api/connections/{id} - Close a specific connection
pub async fn close_connection(
    state: web::Data<Arc<ServiceState>>,
    path: web::Path<String>,
) -> HttpResponse {
    let id = path.into_inner();
    let state = state.clone();
    match task::spawn_blocking(move || state.close_connection(&id)).await {
        Ok(Ok(())) => HttpResponse::Ok().json(ApiResponse::<()>::success(())),
        Ok(Err(e)) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}
