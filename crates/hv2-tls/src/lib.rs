//! The one place HyperMachine chooses its TLS crypto provider.
//!
//! Every TLS endpoint (the API, the sandbox proxy, node mTLS, the egress
//! interception relay) and every outbound HTTPS client asks here, so a build
//! is all one thing or all the other:
//!
//! - By default, `ring`, as the workspace has always used.
//! - With the `fips` feature, AWS-LC's FIPS 140-3 validated module through
//!   `aws-lc-rs`, as `rustls::crypto::default_fips_provider` configures it:
//!   AES-GCM suites only, and the P-256, P-384 and X25519+ML-KEM-768 groups.
//!   ChaCha20-Poly1305 and plain X25519 are not offered, so they cannot be
//!   negotiated.
//!
//! [`require_fips`] is what a binary's `--fips` calls: it refuses to run a
//! build that is not the FIPS build, rather than run one that only claims to
//! be. What this crate does not cover yet -- non-TLS primitives elsewhere in
//! the workspace -- is listed in `docs/FIPS.md`.

use std::sync::Arc;

use rustls::crypto::CryptoProvider;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;

/// Whether this is the FIPS build.
pub const FIPS_BUILD: bool = cfg!(feature = "fips");

/// The crypto provider every TLS configuration in this build uses.
#[must_use]
pub fn provider() -> Arc<CryptoProvider> {
    #[cfg(feature = "fips")]
    {
        Arc::new(rustls::crypto::default_fips_provider())
    }
    #[cfg(not(feature = "fips"))]
    {
        Arc::new(rustls::crypto::ring::default_provider())
    }
}

/// Make [`provider`] the process default, for code that builds a TLS
/// configuration without naming one. Idempotent; the first install wins.
pub fn install_default() {
    let _ = CryptoProvider::clone(&provider()).install_default();
}

/// Whether TLS in this process runs in FIPS mode: the FIPS build, and its
/// provider reporting itself as such (AWS-LC's power-on self tests passed).
#[must_use]
pub fn fips_active() -> bool {
    FIPS_BUILD && provider().fips()
}

/// For a binary's `--fips`: refuse unless TLS will run in FIPS mode.
///
/// # Errors
/// A build without the `fips` feature, or a provider that does not report
/// FIPS mode.
pub fn require_fips() -> Result<(), String> {
    if !FIPS_BUILD {
        return Err(
            "--fips needs the FIPS build (compiled with --features fips on AWS-LC's validated module)"
                .into(),
        );
    }
    if !provider().fips() {
        return Err("the TLS provider is not in FIPS mode".into());
    }
    install_default();
    Ok(())
}

/// A client configuration on [`provider`]: the bundled Web PKI roots, plus
/// any PEM roots in `extra_roots_pem`.
///
/// # Errors
/// Unparseable extra roots.
pub fn client_config(extra_roots_pem: Option<&[u8]>) -> Result<rustls::ClientConfig, String> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(pem) = extra_roots_pem {
        for certificate in CertificateDer::pem_slice_iter(pem) {
            let certificate = certificate.map_err(|e| format!("extra root: {e}"))?;
            roots
                .add(certificate)
                .map_err(|e| format!("extra root: {e}"))?;
        }
    }
    rustls::ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())
        .map(|builder| builder.with_root_certificates(roots).with_no_client_auth())
}

/// A reqwest client builder whose TLS runs on [`provider`].
///
/// In the default build this is `reqwest::Client::builder()`, unchanged:
/// reqwest's own `ring` configuration is the same provider. In the FIPS build
/// it carries a preconfigured [`client_config`], since reqwest would
/// otherwise build its own on `ring`. Extra roots go in `extra_roots_pem`,
/// not `add_root_certificate`, which a preconfigured configuration ignores.
///
/// # Errors
/// Unparseable extra roots.
pub fn http_client(extra_roots_pem: Option<&[u8]>) -> Result<reqwest::ClientBuilder, String> {
    if FIPS_BUILD {
        return Ok(
            reqwest::Client::builder().use_preconfigured_tls(client_config(extra_roots_pem)?)
        );
    }
    let mut builder = reqwest::Client::builder();
    if let Some(pem) = extra_roots_pem {
        for certificate in
            reqwest::Certificate::from_pem_bundle(pem).map_err(|e| format!("extra root: {e}"))?
        {
            builder = builder.add_root_certificate(certificate);
        }
    }
    Ok(builder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_and_its_provider_agree() {
        assert_eq!(fips_active(), FIPS_BUILD);
        assert_eq!(require_fips().is_ok(), FIPS_BUILD);
    }

    /// The FIPS build offers nothing outside FIPS 140-3: no ChaCha20, no plain
    /// X25519.
    #[test]
    fn the_fips_build_offers_only_approved_suites_and_groups() {
        let provider = provider();
        let suites: Vec<String> = provider
            .cipher_suites
            .iter()
            .map(|s| format!("{:?}", s.suite()))
            .collect();
        let groups: Vec<String> = provider
            .kx_groups
            .iter()
            .map(|g| format!("{:?}", g.name()))
            .collect();
        if FIPS_BUILD {
            assert!(suites.iter().all(|s| s.contains("AES")), "{suites:?}");
            assert!(!groups.iter().any(|g| g == "X25519"), "{groups:?}");
        } else {
            assert!(suites.iter().any(|s| s.contains("CHACHA20")));
        }
        assert!(!suites.is_empty() && !groups.is_empty());
    }

    #[test]
    fn a_client_config_takes_extra_roots_and_refuses_garbage() {
        assert!(client_config(None).is_ok());
        assert!(http_client(None).is_ok());
        assert!(client_config(Some(
            b"-----BEGIN CERTIFICATE-----\nnot base64\n-----END CERTIFICATE-----\n"
        ))
        .is_err());
    }
}
