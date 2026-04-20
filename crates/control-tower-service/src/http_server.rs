//! HTTP server setup for Control Tower Service

use actix_web::{web, App, HttpServer, HttpResponse, middleware};
use std::sync::Arc;

use crate::api;
use crate::html;
use crate::ServiceState;

/// Start the HTTP API server on the given port.
/// This runs in the same process as the service.
pub async fn start_http_server(port: u16, state: Arc<ServiceState>) -> std::io::Result<()> {
    tracing::info!("Starting HTTP API server on http://0.0.0.0:{}", port);

    let state_data = web::Data::new(state);

    HttpServer::new(move || {
        App::new()
            .app_data(state_data.clone())
            .wrap(middleware::Logger::default())
            // Web UI
            .route("/", web::get().to(index))
            // API routes
            .service(
                web::scope("/api")
                    .route("/status", web::get().to(api::service_status))
                    .route("/profiles", web::get().to(api::get_profiles))
                    .route("/profiles", web::post().to(api::add_profile))
                    .route("/profiles/{id}", web::delete().to(api::delete_profile))
                    .route("/profiles/{id}", web::patch().to(api::update_profile))
                    .route("/profiles/{id}/activate", web::post().to(api::activate_profile))
                    .route("/profiles/{id}/refresh", web::post().to(api::refresh_profile))
                    .route("/mode", web::get().to(api::get_mode))
                    .route("/mode", web::post().to(api::set_mode))
                    .route("/proxies", web::get().to(api::get_proxies))
                    .route("/proxies/select", web::post().to(api::select_proxy))
                    .route("/proxies/{name}/delay", web::get().to(api::proxy_delay))
                    .route("/proxies/delay", web::post().to(api::proxy_delay_post))
                    .route("/proxies/delay-all", web::post().to(api::proxy_delay_all))
                    .route("/connections", web::get().to(api::get_connections))
                    .route("/connections/{id}", web::delete().to(api::close_connection))
                    .route("/service/start", web::post().to(api::service_start))
                    .route("/service/stop", web::post().to(api::service_stop))
                    .route("/settings", web::get().to(api::get_settings))
                    .route("/settings", web::put().to(api::put_settings))
                    .route("/settings/apply-ports", web::post().to(api::apply_port_settings))
                    .route("/settings/apply-tun", web::post().to(api::apply_tun_settings))
                    .route("/config", web::get().to(api::get_config))
                    .route("/rules", web::get().to(api::get_rules))
                    .route("/rules", web::post().to(api::add_rule))
                    .route("/rules", web::delete().to(api::clear_rules))
                    .route("/rules/{index}", web::delete().to(api::delete_rule))
                    .route("/logs", web::get().to(api::get_logs))
            )
    })
    .bind(("0.0.0.0", port))?
    .run()
    .await
}

async fn index() -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(html::INDEX_HTML)
}
