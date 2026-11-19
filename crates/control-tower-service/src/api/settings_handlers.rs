//! Settings handlers: get, update, apply ports/tun

use actix_web::{web, HttpResponse};
use std::sync::Arc;
use tokio::task;

use crate::service::config_ops::validate_port_set;
use crate::settings::consts::{
    DEFAULT_MIHOMO_API_PORT, DEFAULT_MIHOMO_HTTP_PORT, DEFAULT_MIHOMO_SOCKS_PORT,
    DEFAULT_SERVICE_PORT,
};
use crate::settings::SettingsData;

use crate::ServiceState;

use super::{ApiResponse, UpdateSettingsRequest, ApplyPortsRequest, ApplyTunRequest};

/// GET /api/settings - Return current settings
pub async fn get_settings(
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    let mut settings = state.get_settings();

    // Never expose the stored credential. The login protocol verifies
    // `sha256(stored_key + nonce)` and the nonce is handed out publicly, so
    // leaking `auth.password` would let anyone forge a login without knowing
    // the real password.
    if let Some(auth) = settings.auth.as_mut() {
        auth.password = None;
    }

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

    // Validate the effective port set before persisting it. Otherwise a PUT
    // with invalid ports succeeds, then apply-ports rejects it and leaves
    // settings.yaml in a state that will generate a broken config later.
    if body.http_port.is_some()
        || body.socks_port.is_some()
        || body.api_port.is_some()
        || body.service_port.is_some()
    {
        let http_port = body
            .http_port
            .or(current.http_port)
            .unwrap_or(DEFAULT_MIHOMO_HTTP_PORT);
        let socks_port = body
            .socks_port
            .or(current.socks_port)
            .unwrap_or(DEFAULT_MIHOMO_SOCKS_PORT);
        let api_port = body
            .api_port
            .or(current.api_port)
            .unwrap_or(DEFAULT_MIHOMO_API_PORT);
        let service_port = body
            .service_port
            .or(current.service_port)
            .unwrap_or(DEFAULT_SERVICE_PORT);
        if let Err(e) = validate_port_set(http_port, socks_port, api_port, service_port) {
            return HttpResponse::BadRequest().json(ApiResponse::<()>::error(e));
        }
    }

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
        auto_update_on_startup: current.auto_update_on_startup,
        rule_providers: current.rule_providers.clone(),
        dns: current.dns.clone(),
        connection_history: current.connection_history.clone(),
        closed_connections: current.closed_connections.clone(),
        https: None,
        auth: None,
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

    // Report invalid combinations as a client error instead of a 500.
    if let Err(e) = state.validate_proxy_ports(http_port, socks_port) {
        return HttpResponse::BadRequest().json(ApiResponse::<()>::error(e));
    }

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

/// GET /api/settings/dns - Return current DNS settings
pub async fn get_dns_settings(
    state: web::Data<Arc<ServiceState>>,
) -> HttpResponse {
    let settings = state.get_settings();
    HttpResponse::Ok().json(ApiResponse::success(settings.dns))
}

/// PUT /api/settings/dns - Update DNS settings and apply to config.yaml
pub async fn put_dns_settings(
    state: web::Data<Arc<ServiceState>>,
    body: web::Json<crate::settings::DnsSettings>,
) -> HttpResponse {
    let dns_settings = body.into_inner();

    // Apply DNS to config.yaml
    let state_clone = state.clone();
    let dns_for_apply = dns_settings.clone();
    match task::spawn_blocking(move || state_clone.apply_dns_settings(&dns_for_apply)).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            return HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e));
        }
        Err(e) => {
            return HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string()));
        }
    }

    // Save DNS to settings.yaml
    let mut settings = SettingsData::patch();
    settings.dns = Some(dns_settings);

    let state_inner = state.clone();
    match task::spawn_blocking(move || state_inner.save_settings(&settings)).await {
        Ok(Ok(())) => {
            tracing::info!("DNS settings updated via API");
            HttpResponse::Ok().json(ApiResponse::<()>::success(()))
        }
        Ok(Err(e)) => {
            tracing::error!("Failed to save DNS settings: {}", e);
            HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e))
        }
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse::<()>::error(e.to_string())),
    }
}
