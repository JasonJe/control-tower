//! Settings handlers: get, update, apply ports/tun

use actix_web::{web, HttpResponse};
use std::sync::Arc;
use tokio::task;

use crate::settings::SettingsData;

use crate::ServiceState;

use super::{ApiResponse, UpdateSettingsRequest, ApplyPortsRequest, ApplyTunRequest};

/// GET /api/settings - Return current settings
pub async fn get_settings(
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    let settings = state.get_settings();
    HttpResponse::Ok().json(ApiResponse::success(settings))
}

/// PUT /api/settings - Update and persist settings
pub async fn put_settings(
    state: web::Data<Arc<ServiceState>>,
    body: web::Json<UpdateSettingsRequest>,
) -> HttpResponse {
    // Read current settings to preserve custom_rules and profile_rules_count
    // These are maintained by the rules/profile subsystems and must not be
    // cleared when the user saves settings from the UI.
    let current = state.get_settings();

    let settings = SettingsData {
        api_host: body.api_host.clone(),
        api_port: body.api_port,
        http_port: body.http_port,
        socks_port: body.socks_port,
        mixed_port: body.mixed_port,
        service_port: body.service_port,
        tun_enabled: body.tun_enabled,
        log_level: body.log_level.clone(),
        allow_lan: body.allow_lan,
        ipv6: body.ipv6,
        tcp_concurrent: body.tcp_concurrent,
        mode: body.mode.clone(),
        latency_test_mode: body.latency_test_mode.clone(),
        auto_test: body.auto_test.clone(),
        // Preserve rules-related fields — managed by rules subsystem
        custom_rules: current.custom_rules.clone(),
        profile_rules_count: current.profile_rules_count,
    };

    let state = state.clone();
    match task::spawn_blocking(move || state.save_settings(&settings)).await {
        Ok(Ok(())) => {
            tracing::info!("Settings updated via API");
            HttpResponse::Ok().json(ApiResponse::<()>::success(()))
        }
        Ok(Err(e)) => {
            tracing::error!("Failed to save settings: {}", e);
            HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e))
        }
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// POST /api/settings/apply-ports - Apply port settings and restart Mihomo
pub async fn apply_port_settings(
    state: web::Data<Arc<ServiceState>>,
    body: web::Json<ApplyPortsRequest>,
) -> HttpResponse {
    let http_port = body.http_port;
    let socks_port = body.socks_port;

    let state = state.clone();
    match task::spawn_blocking(move || state.apply_port_settings(http_port, socks_port)).await {
        Ok(Ok(())) => {
            tracing::info!("Port settings applied: http={}, socks={}", http_port, socks_port);
            HttpResponse::Ok().json(ApiResponse::<()>::success(()))
        }
        Ok(Err(e)) => {
            tracing::error!("Failed to apply port settings: {}", e);
            HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e))
        }
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}

/// POST /api/settings/apply-tun - Enable or disable TUN mode
pub async fn apply_tun_settings(
    state: web::Data<Arc<ServiceState>>,
    body: web::Json<ApplyTunRequest>,
) -> HttpResponse {
    let tun_enabled = body.tun_enabled;
    let state = state.clone();
    match task::spawn_blocking(move || state.apply_tun_settings(tun_enabled)).await {
        Ok(Ok(())) => {
            tracing::info!("TUN settings applied: enabled={}", tun_enabled);
            HttpResponse::Ok().json(ApiResponse::<()>::success(()))
        }
        Ok(Err(e)) => {
            tracing::error!("Failed to apply TUN settings: {}", e);
            HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e))
        }
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}
