//! Web UI server module

use crate::api;
use crate::html;

use actix_web::{web, App, HttpServer, HttpResponse, middleware};
use anyhow::Result;

pub async fn start_web_server(host: &str, port: u16) -> Result<()> {
    tracing::info!("Starting Control Tower Web UI on http://{}:{}", host, port);

    HttpServer::new(|| {
        App::new()
            .wrap(middleware::Logger::default())
            .route("/", web::get().to(index))
            .service(
                web::scope("/api")
                    .route("/profiles", web::get().to(api::get_profiles))
                    .route("/profiles", web::post().to(api::add_profile))
                    .route("/profiles/{id}", web::delete().to(api::delete_profile))
                    .route("/profiles/{id}/activate", web::post().to(api::activate_profile))
                    .route("/profiles/{id}", web::put().to(api::update_profile))
                    .route("/mode", web::get().to(api::get_mode))
                    .route("/mode", web::put().to(api::set_mode))
                    .route("/proxies", web::get().to(api::get_proxies))
                    .route("/proxies/select", web::put().to(api::select_proxy))
                    .route("/connections", web::get().to(api::get_connections))
                    .route("/connections/{id}", web::delete().to(api::close_connection))
                    .route("/service/status", web::get().to(api::service_status))
                    .route("/service/start", web::post().to(api::start_service))
                    .route("/service/stop", web::post().to(api::stop_service))
                    .route("/config", web::get().to(api::get_config))
                    .route("/proxy/delay", web::post().to(api::proxy_delay))
            )
    })
    .bind((host, port))?
    .run()
    .await?;

    Ok(())
}

async fn index() -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(html::INDEX_HTML)
}
