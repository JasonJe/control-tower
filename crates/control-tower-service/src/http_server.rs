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
            // API routes (configured in api module)
            .service(api::configure_routes())
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
