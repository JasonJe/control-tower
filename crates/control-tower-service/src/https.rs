//! HTTPS certificate generation using rcgen

use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa,
};
use std::fs;
use std::io::Cursor;
use std::path::Path;

/// Install default crypto provider for rustls
pub fn install_crypto_provider() {
    // Use ring as the crypto provider
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Generate self-signed certificate for HTTPS
#[allow(dead_code)]
pub fn generate_self_signed_cert(cert_path: &Path, key_path: &Path) -> Result<(), String> {
    if cert_path.exists() && key_path.exists() {
        tracing::info!("HTTPS certificates already exist at {:?}, {:?}", cert_path, key_path);
        return Ok(());
    }

    tracing::info!("Generating self-signed HTTPS certificates...");

    // Create directory if needed
    if let Some(parent) = cert_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create cert dir: {}", e))?;
    }

    // Generate certificate using CertificateParams::new which accepts String directly for SANs
    let mut params = CertificateParams::new(vec!["localhost".to_string()]).map_err(|e| format!("Failed to create params: {}", e))?;

    // Set the Common Name in the distinguished name
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "localhost");
    params.distinguished_name = dn;

    // Set as CA for self-signed cert
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);

    // Use Certificate::self_signed to generate the cert
    let key_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519).map_err(|e| format!("Failed to generate key pair: {}", e))?;
    let cert = params.self_signed(&key_pair).map_err(|e| format!("Failed to create cert: {}", e))?;

    // Write certificate
    let cert_pem = cert.pem();
    fs::write(cert_path, &cert_pem).map_err(|e| format!("Failed to write cert: {}", e))?;

    // Write private key
    let key_pem = key_pair.serialize_pem();
    fs::write(key_path, &key_pem).map_err(|e| format!("Failed to write key: {}", e))?;

    tracing::info!("HTTPS certificates generated successfully");
    Ok(())
}

/// Load certificate and key for rustls
pub fn load_tls_config(cert_path: &Path, key_path: &Path) -> Result<rustls::ServerConfig, String> {
    let cert_data = fs::read(cert_path).map_err(|e| format!("Failed to read cert: {}", e))?;
    let key_data = fs::read(key_path).map_err(|e| format!("Failed to read key: {}", e))?;

    // rustls_pemfile::certs expects &mut dyn io::BufRead
    let mut cert_reader = Cursor::new(cert_data);
    let certs: Vec<rustls::pki_types::CertificateDer> = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<Vec<_>, std::io::Error>>()
        .map_err(|e| format!("Failed to parse cert: {}", e))?
        .into_iter()
        .map(rustls::pki_types::CertificateDer::from)
        .collect();

    let mut key_reader = Cursor::new(key_data);
    let key = rustls_pemfile::private_key(&mut key_reader)
        .map_err(|e| format!("Failed to parse key: {}", e))?
        .ok_or_else(|| "No private key found".to_string())?;

    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| format!("Failed to build TLS config: {}", e))?;

    Ok(config)
}