//! TLS support for the HyperMachine API server
//!
//! Provides HTTPS via `rustls` with PEM-encoded certificate/key loading.
//! When TLS is configured, the server uses `tokio-rustls` for the TLS layer
//! and `hyper-util` for HTTP/1.1 + HTTP/2 auto-detection per connection.

use std::path::Path;
use std::sync::Arc;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

/// TLS certificate and key configuration
#[derive(Debug, Clone)]
pub struct TlsConfig {
    /// Path to the PEM-encoded certificate chain
    pub cert_path: String,
    /// Path to the PEM-encoded private key
    pub key_path: String,
}

/// Load PEM-encoded certificates from a file
fn load_certs(path: &Path) -> crate::Result<Vec<CertificateDer<'static>>> {
    CertificateDer::pem_file_iter(path)
        .map_err(|e| crate::ApiError::Transport(format!("open cert {}: {}", path.display(), e)))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| crate::ApiError::Transport(format!("parse certs: {}", e)))
}

/// Load a PEM-encoded private key from a file (first key found)
fn load_key(path: &Path) -> crate::Result<PrivateKeyDer<'static>> {
    PrivateKeyDer::from_pem_file(path)
        .map_err(|e| crate::ApiError::Transport(format!("parse key {}: {}", path.display(), e)))
}

/// Build a [`rustls::ServerConfig`] from PEM certificate and key files.
pub fn build_rustls_config(tls: &TlsConfig) -> crate::Result<Arc<rustls::ServerConfig>> {
    let certs = load_certs(Path::new(&tls.cert_path))?;
    let key = load_key(Path::new(&tls.key_path))?;

    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| crate::ApiError::Transport(format!("TLS config error: {}", e)))?;

    // Enable HTTP/2 via ALPN negotiation
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    Ok(Arc::new(config))
}

/// Serve an axum [`Router`](axum::Router) over TLS with graceful shutdown.
///
/// Uses `tokio-rustls` for the TLS handshake and `hyper-util` for
/// HTTP/1.1 + HTTP/2 connection handling (auto-detected via ALPN).
pub async fn serve_tls(
    listener: TcpListener,
    app: axum::Router,
    acceptor: TlsAcceptor,
    shutdown: impl std::future::Future<Output = ()> + Send,
) -> crate::Result<()> {
    use hyper_util::rt::{TokioExecutor, TokioIo};
    use hyper_util::server::conn::auto::Builder;
    use tower::ServiceExt;

    tokio::pin!(shutdown);

    loop {
        let (tcp_stream, remote_addr) = tokio::select! {
            result = listener.accept() => match result {
                Ok(conn) => conn,
                Err(e) => {
                    tracing::warn!("TCP accept error: {}", e);
                    continue;
                }
            },
            _ = &mut shutdown => {
                tracing::info!("TLS server shutting down");
                break;
            }
        };

        let acceptor = acceptor.clone();
        let app = app.clone();

        tokio::spawn(async move {
            let tls_stream = match acceptor.accept(tcp_stream).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::debug!("TLS handshake failed from {}: {}", remote_addr, e);
                    return;
                }
            };

            let io = TokioIo::new(tls_stream);
            let hyper_service =
                hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                    let tower_svc = app.clone();
                    async move {
                        let req = req.map(axum::body::Body::new);
                        tower_svc.oneshot(req).await
                    }
                });

            if let Err(e) = Builder::new(TokioExecutor::new())
                .serve_connection_with_upgrades(io, hyper_service)
                .await
            {
                tracing::debug!("Connection error from {}: {}", remote_addr, e);
            }
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_config_fields() {
        let config = TlsConfig {
            cert_path: "/path/to/cert.pem".to_string(),
            key_path: "/path/to/key.pem".to_string(),
        };
        assert_eq!(config.cert_path, "/path/to/cert.pem");
        assert_eq!(config.key_path, "/path/to/key.pem");
    }

    #[test]
    fn build_config_missing_cert() {
        let config = TlsConfig {
            cert_path: "/nonexistent/cert.pem".to_string(),
            key_path: "/nonexistent/key.pem".to_string(),
        };
        assert!(build_rustls_config(&config).is_err());
    }
}
