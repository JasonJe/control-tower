//! Traffic statistics handlers.

use actix_web::HttpResponse;
use tokio::task;

use crate::service::stats;

use super::ApiResponse;

/// GET /api/stats - Aggregated traffic statistics
pub async fn get_stats() -> HttpResponse {
    match task::spawn_blocking(stats::view).await {
        Ok(view) => HttpResponse::Ok().json(ApiResponse::success(view)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// DELETE /api/stats - Clear all accumulated statistics
pub async fn reset_stats() -> HttpResponse {
    match task::spawn_blocking(stats::reset).await {
        Ok(Ok(())) => HttpResponse::Ok().json(ApiResponse::<()>::success(())),
        Ok(Err(e)) => HttpResponse::BadRequest().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}
