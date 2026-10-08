//! Short-lived DNS TXT ownership proof through an operator-selected HTTPS resolver.
use std::sync::Arc;
use std::time::Duration;

use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tokio::sync::Semaphore;
use zeroize::Zeroizing;

use crate::domains::{DomainBinding, DomainName};

const LIFETIME: u64 = 600;
const MAX_RESPONSE: usize = 64 * 1024;

/// Distinguish a failed proof from a resolver that can be retried later.
#[derive(Debug)]
pub enum VerificationError {
    InvalidProof(&'static str),
    Unavailable(&'static str),
}

impl std::fmt::Display for VerificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidProof(message) | Self::Unavailable(message) => f.write_str(message),
        }
    }
}
impl std::error::Error for VerificationError {}

/// Public instructions for one sandbox/domain pair. The signing key stays off DNS.
#[derive(Debug, Serialize)]
pub struct DomainChallenge {
    pub record_name: String,
    pub record_type: &'static str,
    pub record_value: String,
    pub expires_at: u64,
}

/// DNS queries use a separate client, never the cluster's node credentials.
pub struct DomainVerification {
    key: Zeroizing<Vec<u8>>,
    namespace: String,
    endpoint: reqwest::Url,
    client: reqwest::Client,
    permits: Semaphore,
}

impl std::fmt::Debug for DomainVerification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DomainVerification { signing_key: [redacted] }")
    }
}

impl DomainVerification {
    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.namespace
    }
    /// Read a bounded operator policy with a 32-byte hex signing key.
    ///
    /// # Errors
    /// Reject malformed policy or insecure resolver URLs without exposing its contents.
    pub fn load(path: &std::path::Path) -> Result<Arc<Self>, String> {
        use std::io::Read;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Policy {
            resolver_url: String,
            namespace: String,
            secret_hex: String,
            resolver_ca_file: Option<std::path::PathBuf>,
        }
        let mut raw = Zeroizing::new(String::new());
        std::fs::File::open(path)
            .and_then(|file| file.take(4097).read_to_string(&mut raw))
            .map_err(|_| "could not read DNS verification policy")?;
        if raw.len() > 4096 {
            return Err("DNS verification policy exceeds 4096 bytes".into());
        }
        let policy: Policy =
            serde_json::from_str(&raw).map_err(|_| "invalid DNS verification policy")?;
        let secret = Zeroizing::new(policy.secret_hex);
        if secret.len() != 64 || !secret.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("DNS verification signing key must be 64 hex characters".into());
        }
        let mut key = Zeroizing::new(Vec::with_capacity(32));
        for at in (0..64).step_by(2) {
            key.push(
                u8::from_str_radix(&secret[at..at + 2], 16).map_err(|_| "invalid signing key")?,
            );
        }
        let mut verification = Self::new(&key, &policy.namespace, &policy.resolver_url)?;
        if let Some(path) = policy.resolver_ca_file {
            let mut pem = Vec::new();
            std::fs::File::open(path)
                .and_then(|file| file.take(64 * 1024 + 1).read_to_end(&mut pem))
                .map_err(|_| "could not read DNS resolver CA certificate")?;
            if pem.len() > 64 * 1024 {
                return Err("DNS resolver CA certificate exceeds 64 KiB".into());
            }
            reqwest::Certificate::from_pem(&pem)
                .map_err(|_| "invalid DNS resolver CA certificate")?;
            verification.client = resolver_client(Some(&pem))?;
        }
        Ok(Arc::new(verification))
    }

    /// Configure a trusted DNS JSON resolver using HTTPS with normal certificate validation.
    ///
    /// # Errors
    /// Reject weak keys, ambiguous namespaces or credential-bearing resolver URLs.
    pub fn new(key: &[u8], namespace: &str, endpoint: &str) -> Result<Self, String> {
        let endpoint = reqwest::Url::parse(endpoint).map_err(|_| "invalid DNS resolver URL")?;
        if key.len() != 32
            || namespace.is_empty()
            || namespace.len() > 128
            || !namespace
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || endpoint.scheme() != "https"
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(
                "DNS verification requires a 32-byte key, namespace and plain HTTPS resolver URL"
                    .into(),
            );
        }
        let client = resolver_client(None)?;
        Ok(Self {
            key: Zeroizing::new(key.to_vec()),
            namespace: namespace.into(),
            endpoint,
            client,
            permits: Semaphore::new(16),
        })
    }

    /// Issue instructions for an existing sandbox; the API checks existence separately.
    ///
    /// # Errors
    /// Reject invalid identities, overlong TXT names or clock overflow.
    pub fn challenge(
        &self,
        domain: &DomainName,
        sandbox: &str,
        now: u64,
    ) -> Result<DomainChallenge, String> {
        DomainBinding::new(domain.as_str(), sandbox, 1)?;
        let record_name = format!("_hypermachine-domain.{}", domain.as_str());
        if record_name.len() > 253 {
            return Err("domain is too long for its ownership TXT name".into());
        }
        let expires_at = now
            .checked_add(LIFETIME)
            .ok_or("ownership challenge clock overflow")?;
        Ok(DomainChallenge {
            record_name,
            record_type: "TXT",
            record_value: self.value(domain, sandbox, expires_at),
            expires_at,
        })
    }

    fn value(&self, domain: &DomainName, sandbox: &str, expires: u64) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key).expect("32-byte HMAC key");
        for part in [
            "hypermachine-domain-v1",
            &self.namespace,
            sandbox,
            domain.as_str(),
        ] {
            mac.update(part.as_bytes());
            mac.update(&[0]);
        }
        mac.update(&expires.to_be_bytes());
        let signature: String = mac
            .finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        format!("hypermachine-domain-v1={expires}.{signature}")
    }

    /// Verify the exact current DNS proof before a binding or port update.
    ///
    /// # Errors
    /// Missing/expired proof, resolver errors, oversized answers or exhausted query capacity.
    pub async fn verify(
        &self,
        domain: &DomainName,
        sandbox: &str,
        expires: u64,
    ) -> Result<(), VerificationError> {
        use VerificationError::{InvalidProof, Unavailable};
        let now = clock().map_err(|_| Unavailable("system clock is invalid"))?;
        if expires <= now || expires > now.saturating_add(LIFETIME) {
            return Err(InvalidProof("ownership challenge is expired or invalid"));
        }
        let challenge = self
            .challenge(domain, sandbox, now)
            .map_err(|_| InvalidProof("invalid ownership challenge identity"))?;
        let _permit = self
            .permits
            .try_acquire()
            .map_err(|_| Unavailable("DNS verification query capacity exhausted"))?;
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            let mut response = self
                .client
                .get(self.endpoint.clone())
                .header("accept", "application/dns-json")
                .query(&[("name", challenge.record_name.as_str()), ("type", "TXT")])
                .send()
                .await
                .map_err(|_| Unavailable("DNS resolver request failed"))?;
            if response.status() != reqwest::StatusCode::OK {
                return Err(Unavailable("DNS resolver refused the query"));
            }
            let mut raw = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| Unavailable("DNS resolver body failed"))?
            {
                if chunk.len() > MAX_RESPONSE.saturating_sub(raw.len()) {
                    return Err(Unavailable("DNS resolver response exceeds 64 KiB"));
                }
                raw.extend_from_slice(&chunk);
            }
            let expected = self.value(domain, sandbox, expires);
            if !answer_matches(&raw, &challenge.record_name, &expected) {
                return Err(InvalidProof(
                    "DNS ownership TXT proof is missing or invalid",
                ));
            }
            Ok(())
        })
        .await
        .map_err(|_| Unavailable("DNS verification timed out"))?;
        result?;
        if expires <= clock().map_err(|_| Unavailable("system clock is invalid"))? {
            return Err(InvalidProof(
                "ownership challenge expired during verification",
            ));
        }
        Ok(())
    }
}

fn resolver_client(ca_pem: Option<&[u8]>) -> Result<reqwest::Client, String> {
    hv2_tls::http_client(ca_pem)
        .map_err(|_| "invalid DNS resolver CA certificate".to_string())?
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .connect_timeout(Duration::from_secs(3))
        .build()
        .map_err(|_| "could not configure DNS resolver client".into())
}

fn clock() -> Result<u64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|_| "system clock is before Unix epoch".into())
}

fn answer_matches(raw: &[u8], name: &str, expected: &str) -> bool {
    #[derive(Deserialize)]
    struct Record {
        name: String,
        #[serde(rename = "type")]
        kind: u16,
        data: Option<String>,
    }
    #[derive(Deserialize)]
    struct Answer {
        #[serde(rename = "Status")]
        status: u16,
        #[serde(rename = "TC")]
        truncated: bool,
        #[serde(rename = "Question")]
        question: Vec<Record>,
        #[serde(rename = "Answer", default)]
        answer: Vec<Record>,
    }
    let Ok(answer) = serde_json::from_slice::<Answer>(raw) else {
        return false;
    };
    let same_name = |other: &str| {
        other
            .strip_suffix('.')
            .unwrap_or(other)
            .eq_ignore_ascii_case(name)
    };
    answer.status == 0
        && !answer.truncated
        && answer.question.len() == 1
        && answer.question[0].kind == 16
        && same_name(&answer.question[0].name)
        && answer.answer.iter().any(|r| {
            r.kind == 16
                && same_name(&r.name)
                && r.data.as_deref() == Some(format!("\"{expected}\"").as_str())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    struct ResolverFixture {
        response: Arc<parking_lot::Mutex<(u16, Vec<u8>)>>,
        requests: Arc<parking_lot::Mutex<Vec<String>>>,
        task: tokio::task::JoinHandle<()>,
    }

    async fn resolver_fixture() -> (DomainVerification, ResolverFixture) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        hv2_tls::install_default();
        let issued =
            rcgen::generate_simple_self_signed(vec!["resolver.example.test".into()]).unwrap();
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![issued.cert.der().clone()],
                rustls::pki_types::PrivatePkcs8KeyDer::from(issued.signing_key.serialize_der())
                    .into(),
            )
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let response = Arc::new(parking_lot::Mutex::new((200, b"{}".to_vec())));
        let requests = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let (reply, received) = (response.clone(), requests.clone());
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let task = tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = listener.accept().await else {
                    break;
                };
                let Ok(mut stream) = acceptor.accept(tcp).await else {
                    continue;
                };
                let mut request = Vec::new();
                let mut byte = [0; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") && request.len() < 8192 {
                    let Ok(count) = stream.read(&mut byte).await else {
                        break;
                    };
                    if count == 0 {
                        break;
                    }
                    request.extend_from_slice(&byte[..count]);
                }
                received.lock().push(String::from_utf8(request).unwrap());
                let (status, body) = reply.lock().clone();
                let header=format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/dns-json\r\nContent-Length: {}\r\nConnection: close\r\nLocation: https://other.example.test/\r\n\r\n",body.len());
                let _ = stream.write_all(header.as_bytes()).await;
                let _ = stream.write_all(&body).await;
                let _ = stream.shutdown().await;
            }
        });
        let mut policy = DomainVerification::new(
            &[1; 32],
            "fixture-fleet",
            &format!("https://resolver.example.test:{}/dns-query", address.port()),
        )
        .unwrap();
        policy.client = reqwest::Client::builder()
            .no_proxy()
            .http1_only()
            .add_root_certificate(reqwest::Certificate::from_der(issued.cert.der()).unwrap())
            .resolve("resolver.example.test", address)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        (
            policy,
            ResolverFixture {
                response,
                requests,
                task,
            },
        )
    }

    fn reply(challenge: &DomainChallenge) -> Vec<u8> {
        serde_json::to_vec(&json!({"Status":0,"TC":false,
            "Question":[{"name":challenge.record_name,"type":16}],
            "Answer":[{"name":challenge.record_name,"type":16,"data":format!("\"{}\"",challenge.record_value)}]})).unwrap()
    }

    #[tokio::test]
    async fn trusted_tls_transport_enforces_exact_proof_expiry_redirect_and_size_bounds() {
        let (policy, fixture) = resolver_fixture().await;
        let domain = DomainName::parse("app.example.com").unwrap();
        let challenge = policy
            .challenge(&domain, "sandbox-a", clock().unwrap())
            .unwrap();
        *fixture.response.lock() = (200, reply(&challenge));
        policy
            .verify(&domain, "sandbox-a", challenge.expires_at)
            .await
            .unwrap();
        assert!(policy
            .verify(&domain, "sandbox-b", challenge.expires_at)
            .await
            .is_err());
        assert!(policy.verify(&domain, "sandbox-a", 0).await.is_err());
        assert!(policy.verify(&domain, "sandbox-a", u64::MAX).await.is_err());
        *fixture.response.lock() = (302, reply(&challenge));
        assert!(policy
            .verify(&domain, "sandbox-a", challenge.expires_at)
            .await
            .is_err());
        *fixture.response.lock() = (200, vec![b'x'; MAX_RESPONSE + 1]);
        assert!(policy
            .verify(&domain, "sandbox-a", challenge.expires_at)
            .await
            .is_err());
        let permits = policy.permits.acquire_many(16).await.unwrap();
        assert!(policy
            .verify(&domain, "sandbox-a", challenge.expires_at)
            .await
            .unwrap_err()
            .to_string()
            .contains("capacity"));
        drop(permits);
        for request in fixture.requests.lock().iter() {
            let request = request.to_ascii_lowercase();
            assert!(request.contains("accept: application/dns-json"));
            assert!(
                !request.contains("authorization:")
                    && !request.contains("x-api-key:")
                    && !request.contains("x-hv2-cluster-token:")
            );
        }
        fixture.task.abort();
        let _ = fixture.task.await;
    }

    #[tokio::test]
    async fn ownership_api_refuses_missing_transferred_and_failed_proofs_without_mutating_bindings()
    {
        use crate::control::{ControlConfig, ControlPlane};
        use crate::store::{ClusterStore, MemoryStore};
        let (policy, fixture) = resolver_fixture().await;
        let store = Arc::new(MemoryStore::new());
        for id in ["sandbox-a", "sandbox-b"] {
            store
                .put_sandbox(&crate::store::tests::sandbox(id, "node-a"))
                .await
                .unwrap();
        }
        let control = ControlPlane::new(
            store.clone(),
            ControlConfig {
                api_key: Some("fixture-key".into()),
                api_keys: Vec::new(),
                access_audit: None,
                cluster_token: Some("node-only-secret".into()),
                proxy_port: 5981,
                create_timeout: Duration::from_secs(10),
                identity_issuer: None,
            },
        );
        control
            .require_domain_verification(Arc::new(policy))
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = crate::control::router(control);
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let path = format!("{base}/sandboxes/sandbox-a/domains/app.example.com");
        assert_eq!(
            client
                .get(format!("{path}/challenge"))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
        let challenge: serde_json::Value = client
            .get(format!("{path}/challenge"))
            .header("x-api-key", "fixture-key")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let expires = challenge["expires_at"].as_u64().unwrap();
        assert_eq!(
            client
                .put(&path)
                .header("x-api-key", "fixture-key")
                .json(&json!({"port":8080}))
                .send()
                .await
                .unwrap()
                .status(),
            428
        );
        let body = json!({"port":8080,"challenge_expires_at":expires});
        assert_eq!(
            client
                .put(&path)
                .header("x-api-key", "fixture-key")
                .json(&body)
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        let name = DomainName::parse("app.example.com").unwrap();
        assert!(store.domain(&name).await.unwrap().is_none());
        *fixture.response.lock()=(200,serde_json::to_vec(&json!({"Status":0,"TC":false,
            "Question":[{"name":challenge["record_name"],"type":16}],
            "Answer":[{"name":challenge["record_name"],"type":16,"data":format!("\"{}\"",challenge["record_value"].as_str().unwrap())}]})).unwrap());
        assert_eq!(
            client
                .put(&path)
                .header("x-api-key", "fixture-key")
                .json(&body)
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        assert_eq!(
            client
                .put(format!(
                    "{base}/sandboxes/sandbox-b/domains/app.example.com"
                ))
                .header("x-api-key", "fixture-key")
                .json(&body)
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
        *fixture.response.lock() = (503, b"unavailable".to_vec());
        assert_eq!(
            client
                .put(&path)
                .header("x-api-key", "fixture-key")
                .json(&json!({"port":9090,"challenge_expires_at":expires}))
                .send()
                .await
                .unwrap()
                .status(),
            503
        );
        assert_eq!(store.domain(&name).await.unwrap().unwrap().port(), 8080);
        assert_eq!(
            client
                .delete(&path)
                .header("x-api-key", "fixture-key")
                .send()
                .await
                .unwrap()
                .status(),
            204
        );
        assert!(store.domain(&name).await.unwrap().is_none());
        server.abort();
        let _ = server.await;
        fixture.task.abort();
        let _ = fixture.task.await;
    }
    #[test]
    fn challenges_bind_namespace_key_domain_and_sandbox() {
        let policy =
            DomainVerification::new(&[1; 32], "fleet-a", "https://resolver.example/dns-query")
                .unwrap();
        let domain = DomainName::parse("App.Example.com.").unwrap();
        let challenge = policy.challenge(&domain, "sandbox-a", 100).unwrap();
        assert_eq!(challenge.expires_at, 700);
        assert_eq!(
            challenge.record_name,
            "_hypermachine-domain.app.example.com"
        );
        assert_ne!(
            challenge.record_value,
            policy
                .challenge(&domain, "sandbox-b", 100)
                .unwrap()
                .record_value
        );
        assert_ne!(
            challenge.record_value,
            policy
                .challenge(
                    &DomainName::parse("other.example.com").unwrap(),
                    "sandbox-a",
                    100
                )
                .unwrap()
                .record_value
        );
        for (key, ns) in [([2; 32], "fleet-a"), ([1; 32], "fleet-b")] {
            assert_ne!(
                challenge.record_value,
                DomainVerification::new(&key, ns, "https://resolver.example/dns-query")
                    .unwrap()
                    .challenge(&domain, "sandbox-a", 100)
                    .unwrap()
                    .record_value
            );
        }
        assert!(policy.challenge(&domain, "bad/id", 100).is_err());
        assert!(policy.challenge(&domain, "sandbox-a", u64::MAX).is_err());
    }
    #[test]
    fn resolver_and_secret_configuration_are_restricted() {
        for url in [
            "http://resolver.example/dns-query",
            "https://user@resolver.example/",
            "https://resolver.example/?key=secret",
            "https://resolver.example/#x",
        ] {
            assert!(DomainVerification::new(&[1; 32], "fleet", url).is_err());
        }
        assert!(DomainVerification::new(&[1; 31], "fleet", "https://resolver.example/").is_err());
        assert!(
            DomainVerification::new(&[1; 32], "bad\0namespace", "https://resolver.example/")
                .is_err()
        );
    }

    #[test]
    fn policy_file_reads_are_bounded_and_errors_do_not_expose_key_contents() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("dns-policy.json");
        let secret = "ab".repeat(32);
        let valid = json!({"resolver_url":"https://resolver.example/dns-query","namespace":"fleet","secret_hex":secret});
        std::fs::write(&path, valid.to_string()).unwrap();
        let policy = DomainVerification::load(&path).unwrap();
        assert_eq!(policy.namespace(), "fleet");
        assert!(!format!("{policy:?}").contains(&secret));
        for raw in [
            "x".repeat(4097),
            format!("{{\"secret_hex\":\"{secret}\"}}"),
            "not json".into(),
        ] {
            std::fs::write(&path, raw).unwrap();
            let error = DomainVerification::load(&path).unwrap_err();
            assert!(!error.contains(&secret));
        }
    }
    #[test]
    fn only_exact_txt_answers_to_the_exact_query_are_accepted() {
        let name = "_hypermachine-domain.app.example.com";
        let good = json!({"Status":0,"TC":false,"Question":[{"name":name,"type":16}],"Answer":[{"name":format!("{name}."),"type":16,"data":"\"proof\""}]});
        assert!(answer_matches(
            &serde_json::to_vec(&good).unwrap(),
            name,
            "proof"
        ));
        let mut bad = Vec::new();
        for (key, value) in [
            ("Status", json!(3)),
            ("TC", json!(true)),
            ("Question", json!([])),
            ("Answer", json!([])),
        ] {
            let mut value_doc = good.clone();
            value_doc[key] = value;
            bad.push(value_doc);
        }
        for (key, value) in [
            ("name", json!("other.example.com")),
            ("type", json!(5)),
            ("data", json!("\"proof-extra\"")),
        ] {
            let mut doc = good.clone();
            doc["Answer"][0][key] = value;
            bad.push(doc);
        }
        let mut doc = good.clone();
        doc["Question"][0]["name"] = json!("other.example.com");
        bad.push(doc);
        for doc in bad {
            assert!(!answer_matches(
                &serde_json::to_vec(&doc).unwrap(),
                name,
                "proof"
            ));
        }
        assert!(!answer_matches(b"not json", name, "proof"));
    }
}
