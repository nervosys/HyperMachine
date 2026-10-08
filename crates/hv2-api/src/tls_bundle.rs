//! Atomically reloadable SNI certificate bundles for certificate deploy hooks.
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::server::{ClientHello, ResolvesServerCert, ResolvesServerCertUsingSni};
use rustls::sign::CertifiedKey;
use serde::Deserialize;

const MAX_MANIFEST: usize = 1024 * 1024;
const MAX_CERT: usize = 1024 * 1024;
const MAX_KEY: usize = 64 * 1024;
const MAX_PEM_TOTAL: usize = 8 * 1024 * 1024;
const MAX_MAPPED_CHAINS: usize = 8 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pair {
    cert_path: String,
    key_path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NamedPair {
    names: Vec<String>,
    cert_path: String,
    key_path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    default: Option<Pair>,
    certificates: Vec<NamedPair>,
}

struct Bundle {
    sni: ResolvesServerCertUsingSni,
    fallback: Option<Arc<CertifiedKey>>,
    names: usize,
}

/// One validated generation is visible to every new TLS handshake.
/// Existing connections retain the certificate they negotiated.
pub struct TlsBundle {
    active: parking_lot::RwLock<Arc<Bundle>>,
    /// Serialize reloads so concurrent callers cannot publish out of order.
    reload: parking_lot::Mutex<()>,
}

impl std::fmt::Debug for TlsBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsBundle")
            .field("names", &self.active.read().names)
            .field("private_keys", &"[redacted]")
            .finish()
    }
}

impl TlsBundle {
    /// Load the complete initial generation before listening.
    ///
    /// # Errors
    /// Reject malformed manifests, oversized inputs, invalid names or certificate/key pairs.
    pub fn load(path: &Path) -> Result<Arc<Self>, String> {
        Ok(Arc::new(Self {
            active: parking_lot::RwLock::new(Arc::new(read_bundle(path)?)),
            reload: parking_lot::Mutex::new(()),
        }))
    }

    /// Validate the full proposed generation, then replace the resolver atomically.
    /// A rejected reload leaves the active generation unchanged.
    ///
    /// # Errors
    /// As [`Self::load`]; no partial generation is published.
    pub fn replace_from_file(&self, path: &Path) -> Result<(), String> {
        let _reload = self.reload.lock();
        let proposed = Arc::new(read_bundle(path)?);
        *self.active.write() = proposed;
        Ok(())
    }

    /// Build a workload-proxy configuration supporting HTTP/1.1 and HTTP/2.
    #[must_use]
    pub fn server_config(self: &Arc<Self>) -> rustls::ServerConfig {
        hv2_tls::install_default();
        let mut config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_cert_resolver(self.clone());
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        // A resumed TLS session can omit a certificate entirely. Disable server
        // session resumption so a new connection observes the active generation.
        config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
        config.send_tls13_tickets = 0;
        config
    }
}

impl ResolvesServerCert for TlsBundle {
    fn resolve(&self, hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let bundle = self.active.read().clone();
        bundle
            .sni
            .resolve(hello)
            .or_else(|| bundle.fallback.clone())
    }
}

fn bounded(path: &Path, maximum: usize) -> Result<Vec<u8>, String> {
    if !std::fs::metadata(path)
        .map_err(|_| "could not inspect TLS bundle input")?
        .is_file()
    {
        return Err("TLS bundle input must be a regular file".into());
    }
    let file = std::fs::File::open(path).map_err(|_| "could not open TLS bundle input")?;
    if !file
        .metadata()
        .map_err(|_| "could not inspect TLS bundle input")?
        .is_file()
    {
        return Err("TLS bundle input must be a regular file".into());
    }
    let mut raw = Vec::new();
    file.take(maximum as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|_| "could not read TLS bundle input")?;
    if raw.len() > maximum {
        return Err("TLS bundle input exceeds its size limit".into());
    }
    Ok(raw)
}

fn pair(pair: &Pair, total: &mut usize) -> Result<CertifiedKey, String> {
    if !Path::new(&pair.cert_path).is_absolute() || !Path::new(&pair.key_path).is_absolute() {
        return Err("TLS certificate and key paths must be absolute".into());
    }
    let cert = bounded(Path::new(&pair.cert_path), MAX_CERT)?;
    let key = bounded(Path::new(&pair.key_path), MAX_KEY)?;
    *total = total
        .checked_add(cert.len() + key.len())
        .ok_or("TLS bundle size overflow")?;
    if *total > MAX_PEM_TOTAL {
        return Err("TLS bundle PEM inputs exceed 8 MiB".into());
    }
    let certs = CertificateDer::pem_slice_iter(&cert)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "invalid TLS certificate PEM")?;
    valid_certificates(&certs, x509_parser::time::ASN1Time::now())?;
    let key = PrivateKeyDer::from_pem_slice(&key).map_err(|_| "invalid TLS private key PEM")?;
    let provider = hv2_tls::provider();
    let certified = CertifiedKey::from_der(certs, key, &provider)
        .map_err(|_| "invalid TLS certificate/key pair")?;
    certified
        .keys_match()
        .map_err(|_| "TLS certificate does not match its private key")?;
    Ok(certified)
}

fn valid_certificates(
    certs: &[CertificateDer<'_>],
    now: x509_parser::time::ASN1Time,
) -> Result<(), String> {
    if certs.is_empty() || certs.len() > 16 {
        return Err("TLS chain needs between 1 and 16 certificates".into());
    }
    for der in certs {
        let (remaining, certificate) = x509_parser::parse_x509_certificate(der.as_ref())
            .map_err(|_| "invalid TLS certificate DER")?;
        if !remaining.is_empty() {
            return Err("invalid TLS certificate DER suffix".into());
        }
        if !certificate.validity().is_valid_at(now) {
            return Err("TLS certificate is expired or not yet valid".into());
        }
    }
    Ok(())
}

fn read_bundle(path: &Path) -> Result<Bundle, String> {
    let manifest: Manifest = serde_json::from_slice(&bounded(path, MAX_MANIFEST)?)
        .map_err(|_| "invalid TLS bundle manifest")?;
    if manifest.certificates.len() > 128
        || (manifest.default.is_none() && manifest.certificates.is_empty())
    {
        return Err("TLS bundle needs a certificate and supports at most 128 named pairs".into());
    }
    let mut total = 0;
    let fallback = manifest
        .default
        .as_ref()
        .map(|entry| pair(entry, &mut total).map(Arc::new))
        .transpose()?;
    let mut sni = ResolvesServerCertUsingSni::new();
    let mut names = HashSet::new();
    let mut mapped = fallback.as_ref().map_or(0, |cert| {
        cert.cert.iter().map(|der| der.len()).sum::<usize>()
    });
    for entry in manifest.certificates {
        if entry.names.is_empty() || entry.names.len() > 1024 {
            return Err("TLS certificate needs between 1 and 1024 exact SNI names".into());
        }
        let certified = pair(
            &Pair {
                cert_path: entry.cert_path,
                key_path: entry.key_path,
            },
            &mut total,
        )?;
        // rustls' SNI resolver owns a cloned DER chain per registered name.
        // Account for aliases too, rather than bounding only input file size.
        let bytes = certified
            .cert
            .iter()
            .map(|der| der.len())
            .sum::<usize>()
            .checked_mul(entry.names.len())
            .ok_or("TLS mapped-chain size overflow")?;
        mapped = mapped
            .checked_add(bytes)
            .ok_or("TLS mapped-chain size overflow")?;
        if mapped > MAX_MAPPED_CHAINS {
            return Err("TLS mapped certificate chains exceed 8 MiB".into());
        }
        for name in entry.names {
            let name = name.strip_suffix('.').unwrap_or(&name).to_ascii_lowercase();
            if !matches!(
                ServerName::try_from(name.clone()),
                Ok(ServerName::DnsName(_))
            ) || !name.is_ascii()
                || !name.contains('.')
                || names.len() >= 1024
                || !names.insert(name.clone())
            {
                return Err("invalid, duplicate or excessive TLS SNI name".into());
            }
            sni.add(&name, certified.clone())
                .map_err(|_| "TLS certificate does not cover its configured SNI name")?;
        }
    }
    Ok(Bundle {
        sni,
        fallback,
        names: names.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::process::Command;

    fn certificate(directory: &Path, name: &str, id: &str) -> Pair {
        let cert = directory.join(format!("{id}.pem"));
        let key = directory.join(format!("{id}.key"));
        let output = Command::new("openssl")
            .args([
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-days",
                "1",
                "-subj",
                &format!("/CN={name}"),
                "-addext",
                &format!("subjectAltName=DNS:{name}"),
                "-addext",
                "basicConstraints=critical,CA:FALSE",
                "-keyout",
                key.to_str().unwrap(),
                "-out",
                cert.to_str().unwrap(),
            ])
            .output()
            .expect("openssl is required for TLS bundle integration tests");
        assert!(output.status.success());
        Pair {
            cert_path: cert.to_str().unwrap().into(),
            key_path: key.to_str().unwrap().into(),
        }
    }

    fn manifest(path: &Path, entry: &Pair, name: &str) {
        std::fs::write(path,json!({"certificates":[{"names":[name],"cert_path":entry.cert_path,"key_path":entry.key_path}]}).to_string()).unwrap();
    }

    #[test]
    fn full_generation_validation_refuses_duplicate_names_wrong_sans_and_key_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bundle.json");
        let first = certificate(dir.path(), "app.example.test", "first");
        let second = certificate(dir.path(), "other.example.test", "second");
        let der = CertificateDer::pem_file_iter(Path::new(&first.cert_path))
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        assert!(valid_certificates(
            std::slice::from_ref(&der),
            x509_parser::time::ASN1Time::now()
        )
        .is_ok());
        assert!(valid_certificates(
            std::slice::from_ref(&der),
            x509_parser::time::ASN1Time::from_timestamp(0).unwrap()
        )
        .is_err());
        let after = x509_parser::parse_x509_certificate(&der)
            .unwrap()
            .1
            .validity()
            .not_after
            .timestamp()
            + 1;
        assert!(valid_certificates(
            &[der],
            x509_parser::time::ASN1Time::from_timestamp(after).unwrap()
        )
        .is_err());
        manifest(&path, &first, "App.Example.Test.");
        let bundle = TlsBundle::load(&path).unwrap();
        assert_eq!(bundle.active.read().names, 1);
        manifest(&path, &first, "other.example.test");
        assert!(bundle.replace_from_file(&path).is_err());
        assert_eq!(bundle.active.read().names, 1);
        manifest(
            &path,
            &Pair {
                cert_path: first.cert_path.clone(),
                key_path: second.key_path,
            },
            "app.example.test",
        );
        assert!(bundle.replace_from_file(&path).is_err());
        std::fs::write(&path,json!({"certificates":[{"names":["app.example.test","APP.EXAMPLE.TEST."],"cert_path":first.cert_path,"key_path":first.key_path}]}).to_string()).unwrap();
        assert!(bundle.replace_from_file(&path).is_err());
        std::fs::write(&path, b"{\"certificates\":[]}").unwrap();
        assert!(bundle.replace_from_file(&path).is_err());
        std::fs::write(&path, vec![b'x'; MAX_MANIFEST + 1]).unwrap();
        assert!(bundle.replace_from_file(&path).is_err());
        assert_eq!(bundle.active.read().names, 1);
    }

    #[tokio::test]
    async fn a_new_tls_connection_observes_renewal_and_a_failed_reload_preserves_the_previous_leaf()
    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bundle.json");
        let first = certificate(dir.path(), "app.example.test", "first");
        let second = certificate(dir.path(), "app.example.test", "second");
        let other = certificate(dir.path(), "other.example.test", "other");
        let first_der = CertificateDer::pem_file_iter(Path::new(&first.cert_path))
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        let second_der = CertificateDer::pem_file_iter(Path::new(&second.cert_path))
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        let other_der = CertificateDer::pem_file_iter(Path::new(&other.cert_path))
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        std::fs::write(&path,json!({"certificates":[
            {"names":["app.example.test"],"cert_path":first.cert_path,"key_path":first.key_path},
            {"names":["other.example.test"],"cert_path":other.cert_path,"key_path":other.key_path}
        ]}).to_string()).unwrap();
        let bundle = TlsBundle::load(&path).unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(bundle.server_config()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            loop {
                let (tcp, _) = listener.accept().await.unwrap();
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    if let Ok(mut tls) = acceptor.accept(tcp).await {
                        let mut byte = [0];
                        while tls.read_exact(&mut byte).await.is_ok() {
                            if tls.write_all(&byte).await.is_err() {
                                break;
                            }
                        }
                    }
                });
            }
        });
        let mut roots = rustls::RootCertStore::empty();
        roots.add(first_der.clone()).unwrap();
        roots.add(second_der.clone()).unwrap();
        roots.add(other_der.clone()).unwrap();
        let client = tokio_rustls::TlsConnector::from(Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        ));
        let connect = || async {
            client
                .connect(
                    ServerName::try_from("app.example.test").unwrap().to_owned(),
                    tokio::net::TcpStream::connect(address).await.unwrap(),
                )
                .await
                .unwrap()
        };
        let mut old = connect().await;
        assert_eq!(old.get_ref().1.peer_certificates().unwrap()[0], first_der);
        let other_connection = client
            .connect(
                ServerName::try_from("other.example.test")
                    .unwrap()
                    .to_owned(),
                tokio::net::TcpStream::connect(address).await.unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            other_connection.get_ref().1.peer_certificates().unwrap()[0],
            other_der
        );
        manifest(&path, &second, "app.example.test");
        bundle.replace_from_file(&path).unwrap();
        let renewed = connect().await;
        assert!(client
            .connect(
                ServerName::try_from("other.example.test")
                    .unwrap()
                    .to_owned(),
                tokio::net::TcpStream::connect(address).await.unwrap()
            )
            .await
            .is_err());
        assert_eq!(
            renewed.get_ref().1.peer_certificates().unwrap()[0],
            second_der
        );
        std::fs::write(&path, b"invalid").unwrap();
        assert!(bundle.replace_from_file(&path).is_err());
        let retained = connect().await;
        assert_eq!(
            retained.get_ref().1.peer_certificates().unwrap()[0],
            second_der
        );
        old.write_all(b"x").await.unwrap();
        let mut byte = [0];
        old.read_exact(&mut byte).await.unwrap();
        assert_eq!(byte[0], b'x');
        drop(old);
        drop(renewed);
        drop(retained);
        drop(other_connection);
        server.abort();
        let _ = server.await;
    }

    #[test]
    fn alias_fanout_cannot_duplicate_certificate_chains_beyond_the_memory_budget() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bundle.json");
        let certificate = certificate(dir.path(), "*.example.test", "wildcard");
        let pem = std::fs::read_to_string(&certificate.cert_path).unwrap();
        std::fs::write(&certificate.cert_path, pem.repeat(16)).unwrap();
        let names: Vec<_> = (0..1024).map(|n| format!("n{n}.example.test")).collect();
        std::fs::write(&path,json!({"certificates":[{"names":names,"cert_path":certificate.cert_path,"key_path":certificate.key_path}]}).to_string()).unwrap();
        assert!(TlsBundle::load(&path)
            .unwrap_err()
            .contains("mapped certificate chains"));
    }
}
