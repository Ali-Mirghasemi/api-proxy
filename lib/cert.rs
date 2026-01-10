// src/cert.rs
use rcgen::{
    CertificateParams, DistinguishedName, DnType, IsCa,
    KeyUsagePurpose, SanType, KeyPair,
};
use std::fs;
use std::net::IpAddr;
use std::path::Path;
use log::{info, warn};
use crate::errors::Error;

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

fn write(path: &str, data: &[u8]) -> Result<(), Error> {
    if let Some(dir) = Path::new(path).parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, data)?;
    Ok(())
}
