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
    hv2_tls::provider()
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

/// Explicit operator roots for intercepted upstream TLS. The owned 0600 file
/// and immediate owned 0700 directory follow the private-policy file contract.
/// This never trusts the guest interception CA implicitly. Restart to reload.
#[cfg(target_os = "linux")]
pub fn upstream_roots_from_private_file(
    path: &std::path::Path,
) -> io::Result<Vec<CertificateDer<'static>>> {
    let input = crate::secret_substitution::private_policy_file(path)?;
    upstream_roots_from_pem(&input)
}

fn upstream_roots_from_pem(input: &[u8]) -> io::Result<Vec<CertificateDer<'static>>> {
    use rustls::pki_types::pem::PemObject;
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "invalid upstream root bundle");
    if input.len() > 1024 * 1024 {
        return Err(invalid());
    }
    let mut roots = Vec::new();
    let mut validator = RootCertStore::empty();
    for certificate in CertificateDer::pem_slice_iter(input) {
        let certificate = certificate.map_err(|_| invalid())?;
        if roots.len() == 128 {
            return Err(invalid());
        }
        validator.add(certificate.clone()).map_err(|_| invalid())?;
        roots.push(certificate);
    }
    if roots.is_empty() {
        return Err(invalid());
    }
    Ok(roots)
}

/// Resolves a workload token by name, freshly, for one request: what an
/// injected header's `${e2b.identity.tokens.NAME}` becomes. `None` leaves
/// the placeholder as it was -- a name the sandbox never registered.
pub type TokenSource = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// The placeholder E2B's SDK writes for a workload token.
const TOKEN_PLACEHOLDER: &str = "${e2b.identity.tokens.";

/// `value` with every token placeholder replaced by what `tokens` gives.
///
/// Read the way the SDK defines it: everything between the prefix and the
/// next `}` is the name. Replaced once, left to right, and never rescanned,
/// so a token's own text cannot introduce a placeholder.
fn expand(value: &str, tokens: &TokenSource) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(at) = rest.find(TOKEN_PLACEHOLDER) {
        out.push_str(&rest[..at]);
        let after = &rest[at + TOKEN_PLACEHOLDER.len()..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[at..]);
            return out;
        };
        let name = &after[..end];
        match tokens(name) {
            Some(token) => out.push_str(&token),
            None => {
                tracing::warn!("egress: no workload token named {name:?}; left as written");
                out.push_str(&rest[at..at + TOKEN_PLACEHOLDER.len() + end + 1]);
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
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
    tokens: Option<TokenSource>,
    server: Arc<ServerConfig>,
    client: Arc<ClientConfig>,
) -> io::Result<()>
where
    G: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    U: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    intercept_policy(guest, upstream, name, headers, tokens, server, client, None).await
}

/// Intercept using the same verified upstream TLS handshake before enabling
/// host-bound secret substitution. Failed authentication sends no HTTP request.
#[allow(clippy::too_many_arguments)]
pub async fn intercept_with_secrets<G, U>(
    guest: G,
    upstream: U,
    name: &str,
    headers: &Headers,
    tokens: Option<TokenSource>,
    server: Arc<ServerConfig>,
    client: Arc<ClientConfig>,
    secrets: Arc<crate::secret_substitution::Store>,
) -> io::Result<()>
where
    G: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    U: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    intercept_policy(
        guest,
        upstream,
        name,
        headers,
        tokens,
        server,
        client,
        Some(secrets),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn intercept_policy<G, U>(
    guest: G,
    upstream: U,
    name: &str,
    headers: &Headers,
    tokens: Option<TokenSource>,
    server: Arc<ServerConfig>,
    client: Arc<ClientConfig>,
    secrets: Option<Arc<crate::secret_substitution::Store>>,
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
    if let Some(secrets) = secrets {
        relay_http_with_secrets(guest, upstream, headers, tokens, name.to_string(), secrets).await
    } else {
        relay_http(guest, upstream, headers, tokens).await
    }
}

/// Relay HTTP/1 requests from `guest` to `upstream`, setting `headers` on
/// each. Used inside TLS by [`intercept`], and bare for plain HTTP -- where
/// the caller must first have established that `upstream` is really the
/// host the rule names, since no certificate will.
///
/// # Errors
///
/// A header that does not parse, or the exchange failed.
///
/// A header whose value holds a workload-token placeholder is expanded per
/// request, through `tokens`, so each request carries a token minted for it
/// and the guest -- which only ever wrote the placeholder -- holds none.
pub async fn relay_http<G, U>(
    guest: G,
    upstream: U,
    headers: &Headers,
    tokens: Option<TokenSource>,
) -> io::Result<()>
where
    G: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    U: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    relay_http_policy(guest, upstream, headers, tokens, None).await
}

/// Opt-in host-bound substitution. The caller must establish verified upstream
/// TLS for `hostname` before supplying the stream. No network access is granted.
/// Bodies are collected up to 1 MiB with a ten-second deadline; trailers fail.
pub async fn relay_http_with_secrets<G, U>(
    guest: G,
    upstream: U,
    headers: &Headers,
    tokens: Option<TokenSource>,
    hostname: String,
    secrets: Arc<crate::secret_substitution::Store>,
) -> io::Result<()>
where
    G: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    U: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    relay_http_policy(guest, upstream, headers, tokens, Some((hostname, secrets))).await
}

async fn relay_http_policy<G, U>(
    guest: G,
    upstream: U,
    headers: &Headers,
    tokens: Option<TokenSource>,
    secrets: Option<(String, Arc<crate::secret_substitution::Store>)>,
) -> io::Result<()>
where
    G: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    U: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    use http_body_util::{BodyExt, Either, Full, Limited};
    // Parsed once, up front, so a bad rule fails the connection rather than
    // every request on it. A value with a placeholder is kept as text, and
    // becomes a header value only once its tokens are in.
    let mut inject = Vec::with_capacity(headers.len());
    for (k, v) in headers {
        let name = HeaderName::from_bytes(k.as_bytes()).map_err(io::Error::other)?;
        let value = match (&tokens, v.contains(TOKEN_PLACEHOLDER)) {
            (Some(_), true) => Injected::Minted(v.clone()),
            _ => Injected::Fixed(HeaderValue::from_str(v).map_err(io::Error::other)?),
        };
        inject.push((name, value));
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
        let tokens = tokens.clone();
        let secrets = secrets.clone();
        async move {
            for (name, value) in inject.iter() {
                let value = match (value, &tokens) {
                    (Injected::Fixed(value), _) => value.clone(),
                    (Injected::Minted(template), Some(tokens)) => {
                        match HeaderValue::from_str(&expand(template, tokens)) {
                            Ok(value) => value,
                            Err(e) => {
                                tracing::warn!("egress: a minted header did not parse: {e}");
                                continue;
                            }
                        }
                    }
                    (Injected::Minted(_), None) => continue,
                };
                request.headers_mut().insert(name.clone(), value);
            }
            let request = if let Some((hostname, secrets)) = secrets {
                let (mut parts, body) = request.into_parts();
                let collected = tokio::time::timeout(
                    HANDSHAKE_TIMEOUT,
                    Limited::new(body, 1024 * 1024).collect(),
                )
                .await
                .map_err(|_| io::Error::other("secret request body deadline exceeded"))?
                .map_err(|_| io::Error::other("secret request body collection failed"))?;
                if collected.trailers().is_some() {
                    return Err(io::Error::other("secret request trailers refused"));
                }
                // Hyper has decoded transfer framing; the rewriter supplies
                // the new Content-Length after replacing the complete body.
                parts.headers.remove(hyper::header::TRANSFER_ENCODING);
                let mut buffered = Request::from_parts(parts, collected.to_bytes().to_vec());
                secrets.rewrite_request(&hostname, &mut buffered)?;
                let (parts, body) = buffered.into_parts();
                Request::from_parts(parts, Either::Right(Full::new(bytes::Bytes::from(body))))
            } else {
                request.map(Either::Left)
            };
            let mut sender = sender.lock().await;
            sender.ready().await.map_err(io::Error::other)?;
            // Query strings and paths can carry guest credentials or bound
            // secret placeholders. Keep them out of operator tracing.
            tracing::debug!("egress relay: request method {}", request.method());
            let response: Response<Incoming> = sender
                .send_request(request)
                .await
                .map_err(io::Error::other)?;
            tracing::debug!("egress relay: upstream answered {}", response.status());
            Ok::<_, io::Error>(response)
        }
    });

    hyper::server::conn::http1::Builder::new()
        .serve_connection(TokioIo::new(guest), service)
        .await
        .map_err(io::Error::other)
}

/// A header to set: fixed, or holding placeholders to expand per request.
enum Injected {
    Fixed(HeaderValue),
    Minted(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::client::danger::ServerCertVerifier;

    #[test]
    fn operator_upstream_roots_are_bounded_and_validated() {
        let authority = Authority::generate().unwrap();
        let pem = authority.ca_pem();
        let roots = upstream_roots_from_pem(pem.as_bytes()).unwrap();
        assert_eq!(roots.as_slice(), std::slice::from_ref(authority.ca_der()));
        assert!(upstream_config(&roots).is_ok());
        assert!(upstream_roots_from_pem(b"").is_err());
        assert!(upstream_roots_from_pem(
            b"-----BEGIN CERTIFICATE-----\ninvalid\n-----END CERTIFICATE-----"
        )
        .is_err());
        assert!(upstream_roots_from_pem(
            b"-----BEGIN CERTIFICATE-----\nAA==\n-----END CERTIFICATE-----"
        )
        .is_err());
        assert!(upstream_roots_from_pem(&vec![b' '; 1024 * 1024 + 1]).is_err());
        assert_eq!(
            upstream_roots_from_pem(pem.repeat(128).as_bytes())
                .unwrap()
                .len(),
            128
        );
        assert!(upstream_roots_from_pem(pem.repeat(129).as_bytes()).is_err());
    }

    #[tokio::test]
    async fn secret_interception_requires_trusted_upstream_tls() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for trusted in [true, false] {
            let guest_ca = Arc::new(Authority::generate().unwrap());
            let upstream_ca = Arc::new(Authority::generate().unwrap());
            let guest_server = guest_ca.server_config().unwrap();
            let guest_client = upstream_config(&[guest_ca.ca_der().clone()]).unwrap();
            let upstream_server = upstream_ca.server_config().unwrap();
            let upstream_client = upstream_config(if trusted {
                std::slice::from_ref(upstream_ca.ca_der())
            } else {
                &[]
            })
            .unwrap();
            let token = format!("hms_{}", "a".repeat(64));
            let secrets = Arc::new(
                crate::secret_substitution::Store::new(vec![crate::secret_substitution::Binding {
                    placeholder: token.clone(),
                    value: b"owned-secret".to_vec(),
                    hosts: vec!["api.example.test".into()],
                }])
                .unwrap(),
            );
            let (client, guest) = tokio::io::duplex(16384);
            let (upstream, server) = tokio::io::duplex(16384);
            let relay = tokio::spawn(async move {
                intercept_with_secrets(
                    guest,
                    upstream,
                    "api.example.test",
                    &Headers::new(),
                    None,
                    guest_server,
                    upstream_client,
                    secrets,
                )
                .await
            });
            let receiver = tokio::spawn(async move {
                let accepted = tokio_rustls::TlsAcceptor::from(upstream_server)
                    .accept(server)
                    .await;
                if !trusted {
                    assert!(accepted.is_err());
                    return;
                }
                let mut server = accepted.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.ends_with(b"\r\n\r\n") {
                    let count = server.read(&mut buffer).await.unwrap();
                    assert!(count > 0 && request.len() + count <= 8192);
                    request.extend_from_slice(&buffer[..count]);
                }
                let request = String::from_utf8(request).unwrap();
                assert!(request.starts_with("GET /?key=owned-secret HTTP/1.1\r\n"));
                assert!(request.contains("x-key: owned-secret\r\n"));
                server
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
                    )
                    .await
                    .unwrap();
                server.shutdown().await.unwrap();
            });
            let mut client = tokio_rustls::TlsConnector::from(guest_client)
                .connect(ServerName::try_from("api.example.test").unwrap(), client)
                .await
                .unwrap();
            client.write_all(format!("GET /?key={token} HTTP/1.1\r\nHost: api.example.test\r\nX-Key: {token}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
            let mut response = Vec::new();
            let read = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                client.read_to_end(&mut response),
            )
            .await
            .unwrap();
            if trusted {
                assert!(response.ends_with(b"OK"));
            } else {
                assert!(response.is_empty());
            }
            let _ = read; // TLS close-notify behavior is independent of request verification.
            let result = tokio::time::timeout(std::time::Duration::from_secs(5), relay)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(result.is_ok(), trusted);
            receiver.await.unwrap();
        }
    }

    #[tokio::test]
    async fn opt_in_secret_relay_rewrites_actual_http_bytes() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let token = format!("hms_{}", "a".repeat(64));
        let secrets = Arc::new(
            crate::secret_substitution::Store::new(vec![crate::secret_substitution::Binding {
                placeholder: token.clone(),
                value: b"secret".to_vec(),
                hosts: vec!["api.example.test".into()],
            }])
            .unwrap(),
        );
        let (mut client, guest) = tokio::io::duplex(8192);
        let (upstream, mut server) = tokio::io::duplex(8192);
        let relay = tokio::spawn(async move {
            relay_http_with_secrets(
                guest,
                upstream,
                &Headers::new(),
                None,
                "api.example.test".into(),
                secrets,
            )
            .await
        });
        let receiver = tokio::spawn(async move {
            let mut received = Vec::new();
            let mut buffer = [0; 1024];
            while !received.ends_with(b"\r\n\r\nsecret") {
                let count = server.read(&mut buffer).await.unwrap();
                assert!(count > 0 && received.len() + count <= 8192);
                received.extend_from_slice(&buffer[..count]);
            }
            let text = String::from_utf8(received).unwrap();
            assert!(text.starts_with("POST /path?key=secret HTTP/1.1\r\n"));
            assert!(text.contains("x-key: secret\r\n"));
            assert!(text.contains("content-length: 6\r\n"));
            server
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                .await
                .unwrap();
        });
        let request = format!(
            "POST /path?key={token} HTTP/1.1\r\nHost: api.example.test\r\nX-Key: {token}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{token}",
            token.len()
        );
        client.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.read_to_end(&mut response),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(response.ends_with(b"OK"));
        receiver.await.unwrap();
        relay.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn open_relay_uses_rotated_and_revoked_policy_on_next_request() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let token = format!("hms_{}", "a".repeat(64));
        let binding = |value: &[u8]| crate::secret_substitution::Binding {
            placeholder: token.clone(),
            value: value.to_vec(),
            hosts: vec!["api.example.test".into()],
        };
        let store =
            Arc::new(crate::secret_substitution::Store::new(vec![binding(b"first")]).unwrap());
        let (mut client, guest) = tokio::io::duplex(8192);
        let (upstream, mut server) = tokio::io::duplex(8192);
        let selected = Arc::clone(&store);
        let relay = tokio::spawn(async move {
            relay_http_with_secrets(
                guest,
                upstream,
                &Headers::new(),
                None,
                "api.example.test".into(),
                selected,
            )
            .await
        });
        let expected = vec!["first".to_string(), "rotated".to_string(), token.clone()];
        let receiver = tokio::spawn(async move {
            for (index, expected) in expected.into_iter().enumerate() {
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                let ending = format!("\r\n\r\n{expected}");
                while !request.ends_with(ending.as_bytes()) {
                    let count = server.read(&mut buffer).await.unwrap();
                    assert!(count > 0 && request.len() + count <= 8192);
                    request.extend_from_slice(&buffer[..count]);
                }
                let text = String::from_utf8(request).unwrap();
                assert!(text.starts_with(&format!("POST /?key={expected} HTTP/1.1\r\n")));
                assert!(text.contains(&format!("x-key: {expected}\r\n")));
                assert!(text.contains(&format!("content-length: {}\r\n", expected.len())));
                let close = if index == 2 {
                    "Connection: close\r\n"
                } else {
                    ""
                };
                server
                    .write_all(
                        format!("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n{close}\r\nOK").as_bytes(),
                    )
                    .await
                    .unwrap();
            }
        });
        for index in 0..3 {
            if index == 1 {
                store.rotate(vec![binding(b"rotated")]).unwrap();
            }
            if index == 2 {
                store.rotate(vec![]).unwrap();
            }
            client.write_all(format!("POST /?key={token} HTTP/1.1\r\nHost: api.example.test\r\nX-Key: {token}\r\nContent-Length: {}\r\n\r\n{token}", token.len()).as_bytes()).await.unwrap();
            let mut response = Vec::new();
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                let mut buffer = [0; 1024];
                while !response.ends_with(b"\r\n\r\nOK") {
                    let count = client.read(&mut buffer).await.unwrap();
                    assert!(count > 0 && response.len() + count <= 8192);
                    response.extend_from_slice(&buffer[..count]);
                }
            })
            .await
            .unwrap();
        }
        receiver.await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), relay)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[test]
    fn placeholders_expand_by_name_and_only_once() {
        let tokens: TokenSource = Arc::new(|name: &str| match name {
            "aws" => Some("TOKEN-${e2b.identity.tokens.gcp}".to_string()),
            "gcp" => Some("G".to_string()),
            _ => None,
        });
        assert_eq!(
            expand("Bearer ${e2b.identity.tokens.aws}", &tokens),
            "Bearer TOKEN-${e2b.identity.tokens.gcp}",
            "a token's text is not rescanned"
        );
        assert_eq!(
            expand(
                "${e2b.identity.tokens.gcp},${e2b.identity.tokens.gcp}",
                &tokens
            ),
            "G,G"
        );
        assert_eq!(
            expand("x ${e2b.identity.tokens.unknown} y", &tokens),
            "x ${e2b.identity.tokens.unknown} y",
            "an unregistered name is left as written"
        );
        assert_eq!(
            expand("${e2b.identity.tokens.aws", &tokens),
            "${e2b.identity.tokens.aws"
        );
        assert_eq!(expand("plain", &tokens), "plain");
    }

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
