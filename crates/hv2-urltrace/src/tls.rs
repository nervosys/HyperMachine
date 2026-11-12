use std::sync::{Arc, Mutex};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    ClientConfig, ClientConnection, DigitallySignedStruct, RootCertStore, SignatureScheme,
    StreamOwned,
};
use sha2::{Digest, Sha256};
use x509_parser::prelude::{FromDer, GeneralName, X509Certificate};

use crate::net::DeadlineStream;
use crate::report::{cap, CertInfo, TlsInfo, CAP_SHORT};

pub const MAX_CERTS: usize = 8;
pub const MAX_SAN: usize = 64;

pub type TlsStream = StreamOwned<ClientConnection, DeadlineStream>;

#[derive(Debug, Default)]
struct VerifyState {
    outcome: Option<Result<(), String>>,
    chain: Vec<Vec<u8>>,
}

#[derive(Debug)]
struct RecordingVerifier {
    inner: Arc<WebPkiServerVerifier>,
    state: Arc<Mutex<VerifyState>>,
}

impl ServerCertVerifier for RecordingVerifier {
    // The verdict is recorded and then overruled: an analyst needs the response from a host with a bad certificate.
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let verdict = self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        );
        if let Ok(mut state) = self.state.lock() {
            state.outcome = Some(match &verdict {
                Ok(_) => Ok(()),
                Err(e) => Err(e.to_string()),
            });
            state.chain = std::iter::once(end_entity)
                .chain(intermediates.iter())
                .take(MAX_CERTS + 1)
                .map(|c| c.as_ref().to_vec())
                .collect();
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(Sha256::digest(data).as_slice())
}

pub fn describe_cert(der: &[u8]) -> CertInfo {
    let mut info = CertInfo {
        sha256: sha256_hex(der),
        subject: None,
        issuer: None,
        serial: None,
        not_before: None,
        not_after: None,
        san_dns: Vec::new(),
        san_dns_truncated: false,
        parse_error: None,
    };
    match X509Certificate::from_der(der) {
        Ok((_, cert)) => {
            info.subject = Some(cap(&cert.subject().to_string(), CAP_SHORT));
            info.issuer = Some(cap(&cert.issuer().to_string(), CAP_SHORT));
            info.serial = Some(hex(cert.raw_serial()));
            info.not_before = Some(cert.validity().not_before.timestamp());
            info.not_after = Some(cert.validity().not_after.timestamp());
            match cert.subject_alternative_name() {
                Ok(Some(san)) => {
                    for name in &san.value.general_names {
                        if let GeneralName::DNSName(dns) = name {
                            if info.san_dns.len() >= MAX_SAN {
                                info.san_dns_truncated = true;
                                break;
                            }
                            info.san_dns.push(cap(dns, CAP_SHORT));
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => info.parse_error = Some(format!("subjectAltName: {e}")),
            }
        }
        Err(e) => info.parse_error = Some(cap(&e.to_string(), CAP_SHORT)),
    }
    info
}

fn build_config(state: Arc<Mutex<VerifyState>>) -> Result<ClientConfig, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let inner = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider.clone())
        .build()
        .map_err(|e| format!("building the certificate verifier: {e}"))?;
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(rustls::ALL_VERSIONS)
        .map_err(|e| format!("selecting protocol versions: {e}"))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(RecordingVerifier { inner, state }))
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}

pub struct Handshake {
    pub info: TlsInfo,
    pub stream: Result<TlsStream, String>,
}

pub fn connect(host: &str, mut tcp: DeadlineStream) -> Handshake {
    let state = Arc::new(Mutex::new(VerifyState::default()));
    let mut info = TlsInfo::default();
    let result = (|| {
        let config = build_config(state.clone())?;
        let name = ServerName::try_from(host.to_owned())
            .map_err(|e| format!("invalid server name {host:?}: {e}"))?;
        let mut conn = ClientConnection::new(Arc::new(config), name)
            .map_err(|e| format!("starting the TLS connection: {e}"))?;
        while conn.is_handshaking() {
            conn.complete_io(&mut tcp)
                .map_err(|e| format!("TLS handshake: {e}"))?;
        }
        Ok::<_, String>((conn, tcp))
    })();
    let stream = result.map(|(conn, tcp)| {
        info.protocol_version = conn.protocol_version().map(|v| format!("{v:?}"));
        info.cipher_suite = conn
            .negotiated_cipher_suite()
            .map(|s| format!("{:?}", s.suite()));
        info.alpn = conn
            .alpn_protocol()
            .map(|p| cap(&String::from_utf8_lossy(p), CAP_SHORT));
        StreamOwned::new(conn, tcp)
    });
    if let Ok(state) = state.lock() {
        match &state.outcome {
            Some(Ok(())) => info.verified = Some(true),
            Some(Err(e)) => {
                info.verified = Some(false);
                info.verification_error = Some(cap(e, CAP_SHORT));
            }
            None => {}
        }
        info.certificates_truncated = state.chain.len() > MAX_CERTS;
        info.certificates = state
            .chain
            .iter()
            .take(MAX_CERTS)
            .map(|der| describe_cert(der))
            .collect();
    }
    Handshake { info, stream }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_hex() {
        assert_eq!(hex(&[0, 255, 16]), "00ff10");
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn garbage_certificate_is_recorded_not_fatal() {
        let info = describe_cert(b"not a certificate");
        assert!(info.parse_error.is_some());
        assert_eq!(info.sha256.len(), 64);
        assert!(info.subject.is_none());
    }
}
