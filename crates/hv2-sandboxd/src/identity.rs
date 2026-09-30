//! Workload identity: JWT-SVIDs for sandboxes, minted on the host.
//!
//! E2B's `iam.tokens` registers named tokens for a sandbox, each an audience
//! and a type -- `JWT-SVID`, SPIFFE's JWT identity document, is the only type
//! its API accepts. A network rule then writes `${e2b.identity.tokens.NAME}`
//! into a header it injects, and the egress gateway puts a freshly minted
//! token there on every request. The guest never holds one: it wrote the
//! placeholder, and the gateway, on the host, signed.
//!
//! The token is an ES256 JWT: `sub` is the sandbox's SPIFFE ID,
//! `spiffe://TRUST_DOMAIN/sandbox/ID`; `aud` the registered audience; five
//! minutes to live. `iss` is set when an issuer URL is configured, which is
//! what OIDC federation (AWS STS `AssumeRoleWithWebIdentity`, GCP workload
//! identity federation) matches against -- and those fetch the signing keys
//! from the issuer's `/.well-known/jwks.json`, which nodes and control
//! planes serve.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};
use serde_json::{json, Value};

/// How long a minted token lives. Minted per request, so short costs
/// nothing and bounds what a token leaked from an upstream is worth.
pub const TOKEN_LIFETIME: Duration = Duration::from_secs(300);

/// The only token type E2B's API accepts in this version.
pub const JWT_SVID: &str = "JWT-SVID";

/// A signing key, and what tokens it signs say about their issuer.
pub struct Identity {
    key: EcdsaKeyPair,
    pkcs8: Vec<u8>,
    kid: String,
    issuer: Option<String>,
    trust_domain: String,
    rng: SystemRandom,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("kid", &self.kid)
            .field("issuer", &self.issuer)
            .field("trust_domain", &self.trust_domain)
            .finish_non_exhaustive()
    }
}

impl Identity {
    /// A fresh P-256 key.
    pub fn generate(issuer: Option<String>, trust_domain: String) -> Result<Self, String> {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
            .map_err(|_| "generating the identity key failed".to_string())?;
        Self::from_pkcs8(pkcs8.as_ref(), issuer, trust_domain)
    }

    /// The key in `pkcs8`, as [`Self::pkcs8`] wrote it.
    pub fn from_pkcs8(
        pkcs8: &[u8],
        issuer: Option<String>,
        trust_domain: String,
    ) -> Result<Self, String> {
        let rng = SystemRandom::new();
        let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8, &rng)
            .map_err(|e| format!("the identity key: {e}"))?;
        let mut identity = Self {
            key,
            pkcs8: pkcs8.to_vec(),
            kid: String::new(),
            issuer,
            trust_domain,
            rng,
        };
        identity.kid = identity.thumbprint();
        Ok(identity)
    }

    /// The private key, PKCS#8 DER -- to share with other nodes, and never
    /// with a guest.
    pub fn pkcs8(&self) -> &[u8] {
        &self.pkcs8
    }

    /// `x` and `y` of the public key, base64url.
    fn coordinates(&self) -> (String, String) {
        // An uncompressed point: 0x04, then 32 bytes each of x and y.
        let point = self.key.public_key().as_ref();
        (b64url(&point[1..33]), b64url(&point[33..65]))
    }

    /// The RFC 7638 thumbprint, which is this key's `kid`: stable for the
    /// key, so every node sharing it names it the same.
    fn thumbprint(&self) -> String {
        use sha2::Digest;
        let (x, y) = self.coordinates();
        let canonical = format!(r#"{{"crv":"P-256","kty":"EC","x":"{x}","y":"{y}"}}"#);
        b64url(&sha2::Sha256::digest(canonical.as_bytes()))
    }

    /// The public key as a JWK, for a JWKS.
    pub fn jwk(&self) -> Value {
        let (x, y) = self.coordinates();
        json!({
            "kty": "EC",
            "crv": "P-256",
            "x": x,
            "y": y,
            "use": "sig",
            "alg": "ES256",
            "kid": self.kid,
        })
    }

    /// A sandbox's SPIFFE ID.
    pub fn spiffe_id(&self, sandbox_id: &str) -> String {
        format!("spiffe://{}/sandbox/{sandbox_id}", self.trust_domain)
    }

    /// A JWT-SVID for `sandbox_id`, for `audience`, valid from now for
    /// [`TOKEN_LIFETIME`].
    pub fn mint(&self, sandbox_id: &str, audience: &str) -> Result<String, String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs();
        let header = json!({ "alg": "ES256", "typ": "JWT", "kid": self.kid });
        let mut claims = json!({
            "sub": self.spiffe_id(sandbox_id),
            "aud": [audience],
            "iat": now,
            "nbf": now,
            "exp": now + TOKEN_LIFETIME.as_secs(),
            "jti": uuid::Uuid::new_v4().simple().to_string(),
        });
        if let Some(issuer) = &self.issuer {
            claims["iss"] = json!(issuer);
        }
        let signing_input = format!(
            "{}.{}",
            b64url(header.to_string().as_bytes()),
            b64url(claims.to_string().as_bytes())
        );
        let signature = self
            .key
            .sign(&self.rng, signing_input.as_bytes())
            .map_err(|_| "signing a workload token failed".to_string())?;
        Ok(format!("{signing_input}.{}", b64url(signature.as_ref())))
    }

    /// The OIDC discovery document for `issuer`.
    pub fn discovery(issuer: &str) -> Value {
        let base = issuer.trim_end_matches('/');
        json!({
            "issuer": issuer,
            "jwks_uri": format!("{base}/.well-known/jwks.json"),
            "response_types_supported": ["id_token"],
            "subject_types_supported": ["public"],
            "id_token_signing_alg_values_supported": ["ES256"],
        })
    }
}

/// Whether `name` can be a token name: the SDK reads a placeholder's name as
/// everything up to the next `}`, so braces would let one name reach
/// another's token, and a control character cannot go in a header.
pub fn valid_token_name(name: &str) -> bool {
    !name.is_empty() && !name.chars().any(|c| c == '{' || c == '}' || c.is_control())
}

/// Unpadded base64url, RFC 4648 section 5.
pub fn b64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = match chunk {
            [a, b, c] => u32::from(*a) << 16 | u32::from(*b) << 8 | u32::from(*c),
            [a, b] => u32::from(*a) << 16 | u32::from(*b) << 8,
            [a] => u32::from(*a) << 16,
            _ => unreachable!("chunks of at most three"),
        };
        let emit = chunk.len() + 1;
        for i in 0..emit {
            out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 0x3f) as usize]));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64url_decode(s: &str) -> Vec<u8> {
        let value = |c: u8| -> u32 {
            match c {
                b'A'..=b'Z' => u32::from(c - b'A'),
                b'a'..=b'z' => u32::from(c - b'a') + 26,
                b'0'..=b'9' => u32::from(c - b'0') + 52,
                b'-' => 62,
                b'_' => 63,
                _ => panic!("not base64url"),
            }
        };
        let mut out = Vec::new();
        for chunk in s.as_bytes().chunks(4) {
            let mut n = 0u32;
            for (i, c) in chunk.iter().enumerate() {
                n |= value(*c) << (18 - 6 * i);
            }
            let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
            out.extend_from_slice(&bytes[..chunk.len() - 1]);
        }
        out
    }

    #[test]
    fn base64url_matches_the_rfc_vectors() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg"),
            ("fo", "Zm8"),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg"),
            ("fooba", "Zm9vYmE"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(b64url(plain.as_bytes()), encoded);
            assert_eq!(b64url_decode(encoded), plain.as_bytes());
        }
        assert_eq!(b64url(&[0xfb, 0xff]), "-_8");
    }

    /// A minted token verifies against the published key, and says what it
    /// should: whose it is, for whom, and until when.
    #[test]
    fn a_minted_token_verifies_against_its_jwk() {
        let identity = Identity::generate(Some("https://issuer.example".into()), "hv2.test".into())
            .expect("a key");
        let token = identity
            .mint("sbx-abc123", "sts.amazonaws.com")
            .expect("minted");
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);

        let jwk = identity.jwk();
        let mut point = vec![4u8];
        point.extend(b64url_decode(jwk["x"].as_str().expect("x")));
        point.extend(b64url_decode(jwk["y"].as_str().expect("y")));
        ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_FIXED, point)
            .verify(
                format!("{}.{}", parts[0], parts[1]).as_bytes(),
                &b64url_decode(parts[2]),
            )
            .expect("the signature verifies against the JWK");

        let header: Value = serde_json::from_slice(&b64url_decode(parts[0])).expect("json");
        assert_eq!(header["alg"], "ES256");
        assert_eq!(header["kid"], jwk["kid"]);
        let claims: Value = serde_json::from_slice(&b64url_decode(parts[1])).expect("json");
        assert_eq!(claims["sub"], "spiffe://hv2.test/sandbox/sbx-abc123");
        assert_eq!(claims["aud"], json!(["sts.amazonaws.com"]));
        assert_eq!(claims["iss"], "https://issuer.example");
        assert_eq!(
            claims["exp"].as_u64().expect("exp") - claims["iat"].as_u64().expect("iat"),
            TOKEN_LIFETIME.as_secs()
        );

        // Two tokens, two IDs: minted per request, not reused.
        let again = identity
            .mint("sbx-abc123", "sts.amazonaws.com")
            .expect("minted");
        assert_ne!(token, again);
    }

    #[test]
    fn a_reloaded_key_keeps_its_kid() {
        let identity = Identity::generate(None, "hv2.test".into()).expect("a key");
        let reloaded =
            Identity::from_pkcs8(identity.pkcs8(), None, "hv2.test".into()).expect("reloads");
        assert_eq!(reloaded.jwk(), identity.jwk());
    }

    #[test]
    fn token_names_refuse_braces_and_control_characters() {
        assert!(valid_token_name("aws"));
        assert!(valid_token_name("gcp-prod.1"));
        for bad in ["", "a}b", "a{b", "a\nb", "\u{7f}"] {
            assert!(!valid_token_name(bad), "{bad:?}");
        }
    }
}
