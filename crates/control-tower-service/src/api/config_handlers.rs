//! Config editor handlers: raw read, validate, save with rollback, backups.

use actix_web::{web, HttpResponse};
use serde::Deserialize;
use std::sync::Arc;
use tokio::task;

use crate::service::config_store;
use crate::ServiceState;

use super::ApiResponse;

#[derive(Deserialize)]
pub struct SaveConfigRequest {
    pub content: String,
}

#[derive(Deserialize)]
pub struct BackupRequest {
    #[serde(default)]
    pub reason: Option<String>,
}

/// GET /api/config/raw - Return the active config.yaml as text
pub async fn get_config_raw() -> HttpResponse {
    match task::spawn_blocking(config_store::read_raw).await {
        Ok(Ok((content, modified))) => {
            HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
                "content": content,
                "modified": modified,
                "path": config_store::config_path().display().to_string(),
            })))
        }
        Ok(Err(e)) => HttpResponse::NotFound().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// POST /api/config/validate - Validate a candidate config without saving it
pub async fn validate_config(body: web::Json<SaveConfigRequest>) -> HttpResponse {
    let content = body.into_inner().content;
    match task::spawn_blocking(move || config_store::validate(&content)).await {
        Ok(report) => HttpResponse::Ok().json(ApiResponse::success(report)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// PUT /api/config - Save a validated config and hot-reload Mihomo
///
/// If Mihomo rejects the new config, the previous contents are written back and
/// reloaded so a bad edit can never leave the proxy broken.
pub async fn put_config(
    state: web::Data<Arc<ServiceState>>,
    body: web::Json<SaveConfigRequest>,
) -> HttpResponse {
    let content = body.into_inner().content;
    let api_url = state.get_api_url();
    let running = state.is_running();
    match task::spawn_blocking(move || save_and_reload(&content, &api_url, running)).await {
        Ok(Ok(summary)) => HttpResponse::Ok().json(ApiResponse::success(summary)),
        Ok(Err(e)) => HttpResponse::BadRequest().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// GET /api/config/backups - List config backups, newest first
pub async fn list_backups() -> HttpResponse {
    match task::spawn_blocking(config_store::list_backups).await {
        Ok(backups) => HttpResponse::Ok().json(ApiResponse::success(backups)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// POST /api/config/backups - Create a backup now
pub async fn create_backup(body: web::Json<BackupRequest>) -> HttpResponse {
    let reason = body
        .into_inner()
        .reason
        .unwrap_or_else(|| "manual".to_string());
    match task::spawn_blocking(move || config_store::create_backup(&reason)).await {
        Ok(Ok(info)) => HttpResponse::Ok().json(ApiResponse::success(info)),
        Ok(Err(e)) => HttpResponse::BadRequest().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// POST /api/config/backups/{id}/restore - Restore a backup and reload Mihomo
pub async fn restore_backup(
    state: web::Data<Arc<ServiceState>>,
    path: web::Path<String>,
) -> HttpResponse {
    let id = path.into_inner();
    let api_url = state.get_api_url();
    let running = state.is_running();
    match task::spawn_blocking(move || {
        let content = config_store::read_backup(&id)?;
        save_and_reload(&content, &api_url, running)
    })
    .await
    {
        Ok(Ok(summary)) => HttpResponse::Ok().json(ApiResponse::success(summary)),
        Ok(Err(e)) => HttpResponse::BadRequest().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// DELETE /api/config/backups/{id} - Delete one backup
pub async fn delete_backup(path: web::Path<String>) -> HttpResponse {
    let id = path.into_inner();
    match task::spawn_blocking(move || config_store::delete_backup(&id)).await {
        Ok(Ok(())) => HttpResponse::Ok().json(ApiResponse::<()>::success(())),
        Ok(Err(e)) => HttpResponse::BadRequest().json(ApiResponse::<()>::error(e)),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// Write `content` to the active config and reload Mihomo, rolling back on failure.
fn save_and_reload(
    content: &str,
    api_url: &str,
    running: bool,
) -> Result<config_store::ConfigSummary, String> {
    let path = config_store::config_path();
    let previous = std::fs::read_to_string(&path).ok();

    let summary = config_store::save(content)?;

    if !running {
        return Ok(summary);
    }

    match config_store::reload(api_url) {
        Ok(()) => Ok(summary),
        Err(e) => {
            // Put the old file back: a rejected config can otherwise survive a
            // restart and lock the user out of the Web UI's own proxy.
            if let Some(previous) = previous {
                if let Err(restore_err) =
                    control_tower_service_core::write_private_atomic(&path, &previous)
                {
                    tracing::error!(
                        "Failed to roll back config after reload error: {}",
                        restore_err
                    );
                } else {
                    let _ = config_store::reload(api_url);
                }
            }
            Err(format!("{} (previous config restored)", e))
        }
    }
}
