//! Header injection into a guest's HTTPS, without the guest holding the secret.
//!
//! E2B's `rules[name].transform.headers` asks for headers to be "injected or
//! overridden" on outbound HTTPS requests to a name. HTTPS is encrypted, so the
//! only place that can do that is one that terminates the guest's TLS. That is
//! what this does, for exactly the connections a rule names and no others:
//!
//! ```text
//!   guest --TLS(leaf for SNI, signed by this CA)--> gateway --TLS(verified)--> upstream
//!                                   ^ headers set here
//! ```
//!
//! The guest trusts the CA because the sandbox was created with it installed.
//! The upstream connection is verified against the public web PKI (plus any
//! extra roots the operator adds), and against the *name the guest asked for*,
//! so a guest that aims an allowed name at an address of its choosing gets a
//! handshake failure rather than a request carrying the secret.
//!
//! The CA's key never leaves the host process and is generated fresh each run;
//! a guest can see the CA certificate and nothing else.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;

use hyper::body::Incoming;
use hyper::header::{HeaderName, HeaderValue};
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use parking_lot::Mutex;
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::network_policy::Headers;

const HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// The provider every TLS object here is built with. Named rather than left to
/// rustls' process default, because the workspace carries one backend (`ring`)
/// and a default chosen by feature unification is how a second one sneaks in.
fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// A certificate authority that exists for one gateway host process.
pub struct Authority {
    issuer: rcgen::CertifiedIssuer<'static, rcgen::KeyPair>,
    ca_der: CertificateDer<'static>,
    ca_pem: String,
    leaves: Mutex<HashMap<String, Arc<CertifiedKey>>>,
    provider: Arc<CryptoProvider>,
}

impl std::fmt::Debug for Authority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Authority").finish_non_exhaustive()
    }
}

impl Authority {
    /// A new CA with a fresh key.
    ///
    /// # Errors
    ///
    /// Key generation or self-signing failed.
    pub fn generate() -> io::Result<Self> {
        let key = rcgen::KeyPair::generate().map_err(io::Error::other)?;
        let mut params =
            rcgen::CertificateParams::new(Vec::<String>::new()).map_err(io::Error::other)?;
        params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Constrained(0));
        params.key_usages = vec![
            rcgen::KeyUsagePurpose::KeyCertSign,
            rcgen::KeyUsagePurpose::CrlSign,
            rcgen::KeyUsagePurpose::DigitalSignature,
        ];
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "HyperMachine sandbox egress CA");
        let issuer = rcgen::CertifiedIssuer::self_signed(params, key).map_err(io::Error::other)?;
        let ca_der = issuer.der().clone();
        let ca_pem = issuer.pem();
        Ok(Self {
            issuer,
            ca_der,
            ca_pem,
            leaves: Mutex::new(HashMap::new()),
            provider: provider(),
        })
    }

    /// The CA certificate, PEM, for a guest's trust store.
    #[must_use]
    pub fn ca_pem(&self) -> &str {
        &self.ca_pem
    }

    /// The CA certificate, DER.
    #[must_use]
    pub fn ca_der(&self) -> &CertificateDer<'static> {
        &self.ca_der
    }

    /// A leaf for `name`, made once and kept.
    fn leaf(&self, name: &str) -> Option<Arc<CertifiedKey>> {
        if let Some(leaf) = self.leaves.lock().get(name) {
            return Some(Arc::clone(leaf));
        }
        let key = rcgen::KeyPair::generate().ok()?;
        let mut params = rcgen::CertificateParams::new(vec![name.to_string()]).ok()?;
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, name);
        params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        let cert = params.signed_by(&key, &self.issuer).ok()?;
        let signing = self
            .provider
            .key_provider
            .load_private_key(PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
                key.serialize_der(),
            )))
            .ok()?;
        let leaf = Arc::new(CertifiedKey::new(
            vec![cert.der().clone(), self.ca_der.clone()],
            signing,
        ));
        let mut leaves = self.leaves.lock();
        // Bounded: a guest can name as many hosts as it likes.
        if leaves.len() >= 1024 {
            leaves.clear();
        }
        leaves.insert(name.to_string(), Arc::clone(&leaf));
        Some(leaf)
    }

    /// A server config presenting a leaf for whatever name the client sends.
    ///
    /// # Errors
    ///
    /// The provider refused the default protocol versions, which it does not.
    pub fn server_config(self: &Arc<Self>) -> io::Result<Arc<ServerConfig>> {
        let mut config = ServerConfig::builder_with_provider(Arc::clone(&self.provider))
            .with_safe_default_protocol_versions()
            .map_err(io::Error::other)?
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(LeafResolver(Arc::clone(self))));
        // HTTP/1.1 only: injection is written for HTTP/1 framing, and a client
        // offered nothing else falls back to it.
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Arc::new(config))
    }
}

#[derive(Debug)]
struct LeafResolver(Arc<Authority>);

impl ResolvesServerCert for LeafResolver {
    fn resolve(&self, hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.0.leaf(hello.server_name()?)
    }
}

/// The config used to verify upstream servers: the public web PKI, plus
/// `extra` roots.
///
/// # Errors
///
/// An extra root that does not parse as a certificate.
pub fn upstream_config(extra: &[CertificateDer<'static>]) -> io::Result<Arc<ClientConfig>> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    for root in extra {
        roots.add(root.clone()).map_err(io::Error::other)?;
    }
    let mut config = ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

/// Terminate `guest`'s TLS as `name`, and relay its requests to `upstream`
/// over verified TLS with `headers` set on each.
///
/// # Errors
///
/// Either handshake, or the HTTP exchange, failed.
pub async fn intercept<G, U>(
    guest: G,
    upstream: U,
    name: &str,
    headers: &Headers,
    server: Arc<ServerConfig>,
    client: Arc<ClientConfig>,
) -> io::Result<()>
where
    G: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    U: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let server_name = ServerName::try_from(name.to_string()).map_err(io::Error::other)?;

    // Bounded, so a client that stops mid-handshake does not hold a task and
    // an upstream connection open indefinitely.
    let guest = tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        tokio_rustls::TlsAcceptor::from(server).accept(guest),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "guest TLS handshake"))??;
    tracing::debug!(
        "egress intercept: guest handshake for {name} done ({:?})",
        guest.get_ref().1.protocol_version()
    );
    let upstream = tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        tokio_rustls::TlsConnector::from(client).connect(server_name, upstream),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "upstream TLS handshake"))??;
    tracing::debug!("egress intercept: upstream handshake for {name} done");
    relay_http(guest, upstream, headers).await
}

/// Relay HTTP/1 requests from `guest` to `upstream`, setting `headers` on
/// each. Used inside TLS by [`intercept`], and bare for plain HTTP -- where
/// the caller must first have established that `upstream` is really the
/// host the rule names, since no certificate will.
///
/// # Errors
///
/// A header that does not parse, or the exchange failed.
pub async fn relay_http<G, U>(guest: G, upstream: U, headers: &Headers) -> io::Result<()>
where
    G: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    U: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    // Parsed once, up front, so a bad rule fails the connection rather than
    // every request on it.
    let mut inject = Vec::with_capacity(headers.len());
    for (k, v) in headers {
        inject.push((
            HeaderName::from_bytes(k.as_bytes()).map_err(io::Error::other)?,
            HeaderValue::from_str(v).map_err(io::Error::other)?,
        ));
    }

    let (sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(upstream))
        .await
        .map_err(io::Error::other)?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            tracing::debug!("egress intercept: upstream connection ended: {e}");
        }
    });
    let sender = Arc::new(tokio::sync::Mutex::new(sender));
    let inject = Arc::new(inject);

    let service = hyper::service::service_fn(move |mut request: Request<Incoming>| {
        let sender = Arc::clone(&sender);
        let inject = Arc::clone(&inject);
        async move {
            for (name, value) in inject.iter() {
                request.headers_mut().insert(name.clone(), value.clone());
            }
            let mut sender = sender.lock().await;
            sender.ready().await?;
            tracing::debug!("egress relay: {} {}", request.method(), request.uri());
            let response: Response<Incoming> = sender.send_request(request).await?;
            tracing::debug!("egress relay: upstream answered {}", response.status());
            Ok::<_, hyper::Error>(response)
        }
    });

    hyper::server::conn::http1::Builder::new()
        .serve_connection(TokioIo::new(guest), service)
        .await
        .map_err(io::Error::other)
}
