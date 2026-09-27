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
//! The CA's key never reaches a guest; a guest can see the CA certificate and
//! nothing else. It is generated fresh each run unless the host loads one --
//! hosts that hand guests between them share one, or a guest resumed on a
//! host that is not the one it was created on would distrust every leaf.

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

/// The certificate authority a gateway signs its interception leaves with.
///
/// Fresh per process by default. [`Self::from_pem`] loads one instead, which
/// is what hosts sharing guests need: a guest trusts the CA it was created
/// with, so a guest paused on one host and resumed on another needs the
/// second to sign with the same key.
pub struct Authority {
    issuer: rcgen::Issuer<'static, rcgen::KeyPair>,
    ca_der: CertificateDer<'static>,
    ca_pem: String,
    key_pem: String,
    leaves: Mutex<HashMap<String, Arc<CertifiedKey>>>,
    provider: Arc<CryptoProvider>,
}

/// What the CA's certificate says about itself, the same every time: a
/// leaf's issuer is this name, and a certificate reloaded with its key must
/// issue leaves a guest holding the original certificate accepts.
fn ca_params() -> io::Result<rcgen::CertificateParams> {
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
    Ok(params)
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
        let key_pem = key.serialize_pem();
        let certificate = ca_params()?.self_signed(&key).map_err(io::Error::other)?;
        let ca_pem = certificate.pem();
        Self::assemble(certificate.der().clone(), ca_pem, key_pem, key)
    }

    /// The CA whose certificate and private key are these PEM documents --
    /// as [`Self::ca_pem`] and [`Self::key_pem`] wrote them.
    ///
    /// # Errors
    ///
    /// Either does not parse.
    pub fn from_pem(ca_pem: &str, key_pem: &str) -> io::Result<Self> {
        use rustls::pki_types::pem::PemObject;
        let key = rcgen::KeyPair::from_pem(key_pem).map_err(io::Error::other)?;
        let ca_der = CertificateDer::from_pem_slice(ca_pem.as_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?
            .into_owned();
        Self::assemble(ca_der, ca_pem.to_string(), key_pem.to_string(), key)
    }

    fn assemble(
        ca_der: CertificateDer<'static>,
        ca_pem: String,
        key_pem: String,
        key: rcgen::KeyPair,
    ) -> io::Result<Self> {
        Ok(Self {
            issuer: rcgen::Issuer::new(ca_params()?, key),
            ca_der,
            ca_pem,
            key_pem,
            leaves: Mutex::new(HashMap::new()),
            provider: provider(),
        })
    }

    /// The CA's private key, PEM -- to share it with another host, and
    /// nothing else. A guest must never see it.
    #[must_use]
    pub fn key_pem(&self) -> &str {
        &self.key_pem
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

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::client::danger::ServerCertVerifier;

    /// A CA reloaded from its PEM on another host issues leaves that a guest
    /// holding only the original certificate accepts -- which is the whole
    /// reason to share one.
    #[test]
    fn a_reloaded_authority_issues_leaves_the_original_certificate_verifies() {
        let original = Authority::generate().expect("a CA");
        let reloaded =
            Authority::from_pem(original.ca_pem(), original.key_pem()).expect("it reloads");
        assert_eq!(reloaded.ca_pem(), original.ca_pem());

        let leaf = reloaded.leaf("api.example.com").expect("a leaf");
        let mut roots = RootCertStore::empty();
        roots.add(original.ca_der().clone()).expect("a root");
        let verifier = rustls::client::WebPkiServerVerifier::builder_with_provider(
            Arc::new(roots),
            provider(),
        )
        .build()
        .expect("a verifier");
        let name = ServerName::try_from("api.example.com").expect("a name");
        verifier
            .verify_server_cert(
                &leaf.cert[0],
                &[],
                &name,
                &[],
                rustls::pki_types::UnixTime::now(),
            )
            .expect("the original CA vouches for the reloaded one's leaf");
    }

    #[test]
    fn a_key_that_is_not_pem_is_refused() {
        let original = Authority::generate().expect("a CA");
        assert!(Authority::from_pem(original.ca_pem(), "not a key").is_err());
    }
}
