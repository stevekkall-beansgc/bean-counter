//! Trust is an explicit choice. The connector never loads platform roots.
use std::sync::Arc;

use rustls::{
    pki_types::{pem::PemObject, CertificateDer},
    ClientConfig, RootCertStore,
};
use tokio_postgres_rustls::MakeRustlsConnect;

pub enum Trust<'a> {
    Public,
    PemOnly(&'a [u8]),
}

#[derive(Debug, PartialEq, Eq)]
pub enum TrustError {
    EmptyOrOversized,
    InvalidPem,
    InvalidCertificate,
}

// Also used by the deterministic public-root exclusion test. This is the exact
// store passed into Rustls, not a parallel diagnostic representation.
pub(crate) fn root_store(trust: Trust<'_>) -> Result<RootCertStore, TrustError> {
    let mut roots = RootCertStore::empty();
    match trust {
        Trust::Public => roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned()),
        Trust::PemOnly(pem) => {
            if pem.is_empty() || pem.len() > 65_536 {
                return Err(TrustError::EmptyOrOversized);
            }
            for cert in CertificateDer::pem_slice_iter(pem) {
                let cert = cert.map_err(|_| TrustError::InvalidPem)?;
                roots
                    .add(cert)
                    .map_err(|_| TrustError::InvalidCertificate)?;
            }
            if roots.is_empty() {
                return Err(TrustError::InvalidPem);
            }
        }
    }
    Ok(roots)
}

pub fn connector(trust: Trust<'_>) -> Result<MakeRustlsConnect, TrustError> {
    let config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("Ring supports the statically enabled protocol versions")
            .with_root_certificates(root_store(trust)?)
            .with_no_client_auth();
    Ok(MakeRustlsConnect::new(config))
}
