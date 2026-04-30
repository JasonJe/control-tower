//! HTTP server setup for Control Tower Service

use actix_web::{web, App, HttpServer, HttpResponse, middleware};
use std::path::PathBuf;
use std::sync::Arc;

use crate::api;
use crate::html;
use crate::ServiceState;

/// Start the HTTP API server on the given port.
pub async fn start_http_server(
    port: u16,
    state: Arc<ServiceState>,
    https_enabled: bool,
    cert_path: Option<PathBuf>,
    key_path: Option<PathBuf>,
) -> std::io::Result<()> {
    let state_data = web::Data::new(state);

    if https_enabled {
        // HTTPS mode
        tracing::info!("Starting HTTPS server on https://0.0.0.0:{}", port);

        // Install crypto provider before loading TLS config
        crate::https::install_crypto_provider();

        let config = crate::https::load_tls_config(
            &cert_path.unwrap_or_else(|| PathBuf::from("cert.pem")),
            &key_path.unwrap_or_else(|| PathBuf::from("key.pem")),
        )
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        HttpServer::new(move || {
            App::new()
                .app_data(state_data.clone())
                .wrap(middleware::Logger::default())
                .route("/", web::get().to(index))
                .service(api::configure_routes())
        })
        .bind_rustls_0_23(("0.0.0.0", port), config)?
        .run()
        .await
    } else {
        // HTTP mode
        tracing::info!("Starting HTTP server on http://0.0.0.0:{}", port);

        HttpServer::new(move || {
            App::new()
                .app_data(state_data.clone())
                .wrap(middleware::Logger::default())
                .route("/", web::get().to(index))
                .service(api::configure_routes())
        })
        .bind(("0.0.0.0", port))?
        .run()
        .await
    }
}

async fn index() -> HttpResponse {
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(html::INDEX_HTML)
}