//! Embedded Web UI HTML template

use std::sync::OnceLock;

pub const INDEX_HTML: &str = include_str!("../ui/index.html");

/// Render the UI with the crate version substituted.
///
/// Keeping the version in `Cargo.toml` as the single source of truth avoids a
/// stale footer when `cargo release` bumps the workspace.
pub fn index_html() -> &'static str {
    static RENDERED: OnceLock<String> = OnceLock::new();
    RENDERED.get_or_init(|| INDEX_HTML.replace("{{VERSION}}", env!("CARGO_PKG_VERSION")))
}

/// Vendored CryptoJS (PBKDF2 / SHA-256). The Web UI must not depend on a
/// third-party CDN: the login flow breaks when the page is reached through a
/// NAT/offline network where cdnjs.cloudflare.com is unreachable.
pub const CRYPTO_JS: &str = include_str!("../ui/crypto-js.min.js");
