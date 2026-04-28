//! HTTP API handlers for Control Tower Service
//!
//! These handlers are exposed via actix-web and provide the same functionality
//! as the IPC commands, but accessible over HTTP for web clients.

mod dto;
mod helpers;
mod service_handlers;
mod proxy_handlers;
mod connection_handlers;
mod rules_handlers;
mod profile_handlers;
mod settings_handlers;
mod delay_handlers;
mod logs_handlers;
mod rule_provider_handlers;

pub use dto::*;
pub use helpers::*;
pub use service_handlers::*;
pub use proxy_handlers::*;
pub use connection_handlers::*;
pub use rules_handlers::*;
pub use profile_handlers::*;
pub use settings_handlers::*;
pub use delay_handlers::*;
pub use logs_handlers::*;
pub use rule_provider_handlers::*;

use actix_web::web;
use actix_web::{HttpResponse, Scope};

/// GET /api/config - Returns the active Mihomo config.yaml as JSON
async fn get_config() -> HttpResponse {
    let paths = get_control_tower_paths();
    let config_path = &paths.active_config_path;

    if !config_path.exists() {
        return HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({})));
    }

    match std::fs::read_to_string(config_path) {
        Ok(content) => {
            match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&content) {
                Ok(json) => HttpResponse::Ok().json(ApiResponse::success(json)),
                Err(e) => HttpResponse::InternalServerError()
                    .json(ApiResponse::<()>::error(format!("Failed to parse config.yaml: {}", e))),
            }
        }
        Err(e) => HttpResponse::InternalServerError()
            .json(ApiResponse::<()>::error(format!("Failed to read config.yaml: {}", e))),
    }
}

/// Returns all HTTP API routes as a Scope for use with App::service()
pub fn configure_routes() -> Scope {
    web::scope("/api")
        // Service
        .route("/status", web::get().to(service_status))
        .route("/service/start", web::post().to(service_start))
        .route("/service/stop", web::post().to(service_stop))
        .route("/service/reload", web::post().to(service_reload))
        // Proxies
        .route("/proxies", web::get().to(get_proxies))
        .route("/proxies/select", web::post().to(select_proxy))
        .route("/proxies/{name}/delay", web::get().to(proxy_delay))
        .route("/proxies/delay", web::post().to(proxy_delay_post))
        .route("/proxies/delay-all", web::post().to(proxy_delay_all))
        .route("/proxies/fastest", web::get().to(get_fastest))
        // Mode
        .route("/mode", web::get().to(get_mode))
        .route("/mode", web::post().to(set_mode))
        // Connections
        .route("/connections", web::get().to(get_connections))
        .route("/connections/{id}", web::delete().to(close_connection))
        .route("/connections/history", web::get().to(get_connection_history))
        .route("/connections/history", web::delete().to(clear_connection_history))
        // Config
        .route("/config", web::get().to(get_config))
        // Rules
        .route("/rules", web::get().to(get_rules))
        .route("/rules", web::post().to(add_rule))
        .route("/rules", web::delete().to(clear_rules))
        .route("/rules/{source}/{index}", web::delete().to(delete_rule))
        // Rule Providers
        .route("/rule-providers", web::get().to(get_rule_providers))
        .route("/rule-providers", web::post().to(add_rule_provider))
        .route("/rule-providers/{name}", web::put().to(update_rule_provider))
        .route("/rule-providers/{name}", web::delete().to(delete_rule_provider))
        .route("/rule-providers/refresh-all", web::post().to(refresh_all_rule_providers))
        // Profiles
        .route("/profiles", web::get().to(get_profiles))
        .route("/profiles", web::post().to(add_profile))
        .route("/profiles/{id}/activate", web::put().to(activate_profile))
        .route("/profiles/{id}", web::patch().to(update_profile))
        .route("/profiles/{id}/refresh", web::post().to(refresh_profile))
        .route("/profiles/{id}", web::delete().to(delete_profile))
        .route("/profiles/check-updates", web::post().to(check_all_profiles))
        // Settings
        .route("/settings", web::get().to(get_settings))
        .route("/settings", web::put().to(put_settings))
        .route("/settings/apply-ports", web::post().to(apply_port_settings))
        .route("/settings/apply-tun", web::post().to(apply_tun_settings))
        .route("/settings/dns", web::get().to(get_dns_settings))
        .route("/settings/dns", web::put().to(put_dns_settings))
        // Logs
        .route("/logs", web::get().to(get_logs))
}
