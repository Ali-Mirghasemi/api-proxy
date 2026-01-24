//! TLS certificate management utilities.
//!
//! This module provides helper functions for ensuring that a valid
//! TLS certificate and private key exist for HTTPS servers.
//!
//! If the configured certificate or key file is missing, a new
//! **self-signed certificate** is generated automatically using
//! the `rcgen` crate.
//!
//! This behavior is intended to:
//! - Simplify local development
//! - Enable HTTPS without manual certificate generation
//! - Provide a safe default for internal or private deployments

use rcgen::{
    CertificateParams, DistinguishedName, DnType, IsCa,
    KeyUsagePurpose, SanType, KeyPair,
};
use std::fs;
use std::net::IpAddr;
use std::path::Path;
use log::{info, warn};
use crate::errors::Error;

/// Ensure that a TLS certificate and private key exist.
///
/// If both `cert_path` and `key_path` already exist, they are reused.
/// Otherwise, a new self-signed certificate is generated and written
/// to disk.
///
/// # Arguments
/// - `cert_path`   — Path to the PEM-encoded certificate file
/// - `key_path`    — Path to the PEM-encoded private key file
/// - `common_name` — Common Name (CN) used for the certificate
///
/// # Errors
/// Returns an error if:
/// - Certificate generation fails
/// - Files cannot be written to disk
///
/// # Logging
/// - Logs an `info` message when using existing certificates
/// - Logs a `warn` message when generating new certificates
pub fn ensure_cert(
    cert_path: &str,
    key_path: &str,
    common_name: &str,
) -> Result<(), Error> {
    if Path::new(cert_path).exists() && Path::new(key_path).exists() {
        info!("Using existing cert: {}", cert_path);
        return Ok(());
    }

    warn!("Generating new self-signed certificate for {}", common_name);

    let (cert_pem, key_pem) = generate_self_signed(common_name)?;

    write(cert_path, cert_pem.as_bytes())?;
    write(key_path, key_pem.as_bytes())?;
    Ok(())
}

/// Generate a self-signed TLS certificate.
///
/// The generated certificate:
/// - Uses the provided Common Name (CN)
/// - Includes `localhost` and `127.0.0.1` as Subject Alternative Names
/// - Is valid for ~50 years
/// - Is suitable for HTTPS server authentication
///
/// # Returns
/// A tuple of `(certificate_pem, private_key_pem)`.
///
/// # Errors
/// Returns an error if key generation or certificate signing fails.
fn generate_self_signed(
    common_name: &str,
) -> Result<(String, String), Error> {
    let key_pair = KeyPair::generate()?;

    let mut params = CertificateParams::new(vec![
        common_name.to_string(),
        "localhost".to_string(),
    ])?;

    // Subject
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, common_name);
    params.distinguished_name = dn;

    // SANs
    params.subject_alt_names.push(SanType::IpAddress(
        IpAddr::from([127, 0, 0, 1]),
    ));

    // Server cert (not CA)
    params.is_ca = IsCa::NoCa;
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];

    // Long validity (~50 years)
    params.not_before = rcgen::date_time_ymd(2024, 1, 1);
    params.not_after  = rcgen::date_time_ymd(2074, 1, 1);

    let cert = params.self_signed(&key_pair)?;

    Ok((
        cert.pem(),
        key_pair.serialize_pem(),
    ))
}

/// Write data to a file, creating parent directories if necessary.
///
/// # Arguments
/// - `path` — File path to write
/// - `data` — Data to write
///
/// # Errors
/// Returns an error if directory creation or file writing fails.
fn write(path: &str, data: &[u8]) -> Result<(), Error> {
    if let Some(dir) = Path::new(path).parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, data)?;
    Ok(())
}
