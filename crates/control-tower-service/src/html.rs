//! Embedded Web UI HTML template

pub const INDEX_HTML: &str = include_str!("../ui/index.html");

/// Vendored CryptoJS (PBKDF2 / SHA-256). The Web UI must not depend on a
/// third-party CDN: the login flow breaks when the page is reached through a
/// NAT/offline network where cdnjs.cloudflare.com is unreachable.
pub const CRYPTO_JS: &str = include_str!("../ui/crypto-js.min.js");
