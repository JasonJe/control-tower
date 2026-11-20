//! HTTP server setup for Control Tower Service

use actix_web::{web, App, HttpServer, HttpResponse, middleware};
use std::path::PathBuf;
use std::sync::Arc;

use crate::api;
use crate::auth_middleware::AuthMiddleware;
use crate::html;
use crate::ServiceState;

/// Security headers added to every response.
///
/// The UI is a single self-contained page with an inline script, so the CSP
/// must allow inline script/style; every other origin is denied. Combined with
/// `frame-ancestors`/`X-Frame-Options` this prevents clickjacking of the
/// cookie-authenticated UI, and `nosniff` stops content-type confusion.
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; \
     script-src 'self' 'unsafe-inline'; \
     style-src 'self' 'unsafe-inline'; \
     img-src 'self' data:; \
     connect-src 'self'; \
     frame-ancestors 'none'; \
     base-uri 'none'; \
     form-action 'self'";

fn security_headers() -> middleware::DefaultHeaders {
    middleware::DefaultHeaders::new()
        .add(("X-Content-Type-Options", "nosniff"))
        .add(("X-Frame-Options", "DENY"))
        .add(("Referrer-Policy", "no-referrer"))
        .add(("Content-Security-Policy", CONTENT_SECURITY_POLICY))
}

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
                .wrap(AuthMiddleware)
                .wrap(middleware::Logger::default())
                .wrap(security_headers())
                .route("/", web::get().to(index))
                .route("/vendor/crypto-js.min.js", web::get().to(crypto_js))
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
                .wrap(AuthMiddleware)
                .wrap(middleware::Logger::default())
                .wrap(security_headers())
                .route("/", web::get().to(index))
                .route("/vendor/crypto-js.min.js", web::get().to(crypto_js))
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
        .body(html::index_html())
}

async fn crypto_js() -> HttpResponse {
    HttpResponse::Ok()
        .content_type("application/javascript; charset=utf-8")
        .insert_header(("Cache-Control", "public, max-age=86400"))
        .body(html::CRYPTO_JS)
}