//! Single sign-on: verifying what an OpenID Connect provider says about a
//! person, and the session the control plane gives them for it.
//!
//! This module is the part that has to be right before anything is wired to
//! it, so it does no I/O:
//!
//! - [`Jwks`] holds a provider's signing keys, as its `jwks_uri` publishes
//!   them: RSA (`RS256`) and P-256 (`ES256`). Nothing else is accepted --
//!   in particular not `none`, and not `HS256`, which would let anyone who
//!   knows the client secret mint an ID token.
//! - [`verify_id_token`] checks an ID token against those keys and the rules
//!   OpenID Connect Core section 3.1.3.7 sets: the issuer is exactly the one
//!   configured, the client is in the audience, it has not expired, and the
//!   nonce is the one this login sent.
//! - [`SessionKey`] mints and checks the control plane's own session token, a
//!   MAC over who and until when. Stateless: a session ends when it expires,
//!   or at once when its person leaves the [`Members`] file.
//! - [`Members`] says what a verified email may do: its principal, team, role
//!   and scopes, as an API key policy would. An email nobody listed signs in
//!   to nothing.
//!
//! Every signature and MAC goes through hv2-core's IronCrypto primitives.

use std::collections::BTreeMap;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hv2_core::crypto::asymmetric::{
    EcCurve, EcPublicKey, RsaKeySize, RsaPublicKey, Signature, SignatureAlgorithm,
};
use hv2_core::crypto::{FipsCrypto, FipsMode};
use serde::Deserialize;
use serde_json::Value;

/// How far a clock may disagree with the provider's, either way.
pub const CLOCK_LEEWAY_SECS: i64 = 60;

/// Why a token was not accepted. Never carries the token or a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SsoError {
    /// Not three base64url parts, or a header or payload that is not JSON.
    Malformed(&'static str),
    /// An algorithm this module does not accept.
    Algorithm(String),
    /// No key with the token's `kid`, or none of the algorithm's kind.
    UnknownKey,
    /// The signature does not verify.
    Signature,
    /// A claim that is missing or wrong, named.
    Claim(&'static str),
    /// A configuration or key that could never verify anything.
    Config(String),
}

impl std::fmt::Display for SsoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(what) => write!(f, "malformed token: {what}"),
            Self::Algorithm(alg) => write!(f, "algorithm {alg:?} is not accepted"),
            Self::UnknownKey => write!(f, "no signing key matches the token"),
            Self::Signature => write!(f, "signature does not verify"),
            Self::Claim(claim) => write!(f, "claim {claim} is missing or wrong"),
            Self::Config(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SsoError {}

fn decode(part: &str, what: &'static str) -> Result<Vec<u8>, SsoError> {
    URL_SAFE_NO_PAD
        .decode(part)
        .map_err(|_| SsoError::Malformed(what))
}

fn crypto() -> Result<FipsCrypto, SsoError> {
    FipsCrypto::new(FipsMode::Enabled).map_err(|e| SsoError::Config(e.to_string()))
}

/// One signing key.
#[derive(Debug, Clone)]
enum Key {
    Rsa(RsaPublicKey),
    P256(EcPublicKey),
}

#[derive(Debug, Clone)]
struct Jwk {
    kid: Option<String>,
    key: Key,
}

/// A provider's signing keys.
#[derive(Debug, Clone, Default)]
pub struct Jwks {
    keys: Vec<Jwk>,
}

#[derive(Deserialize)]
struct RawJwks {
    keys: Vec<RawJwk>,
}

#[derive(Deserialize)]
struct RawJwk {
    kty: String,
    kid: Option<String>,
    #[serde(rename = "use")]
    usage: Option<String>,
    crv: Option<String>,
    n: Option<String>,
    e: Option<String>,
    x: Option<String>,
    y: Option<String>,
}

impl Jwks {
    /// Parse a JWK Set. Keys this module cannot use -- encryption keys,
    /// other curves, other types -- are skipped rather than refused, since a
    /// provider publishes those alongside its signing keys.
    ///
    /// # Errors
    /// Not a JWK Set, or one with no usable signing key.
    pub fn from_json(json: &str) -> Result<Self, SsoError> {
        let raw: RawJwks =
            serde_json::from_str(json).map_err(|_| SsoError::Config("not a JWK Set".into()))?;
        let mut keys = Vec::new();
        for raw in raw.keys {
            if raw.usage.as_deref().is_some_and(|u| u != "sig") {
                continue;
            }
            let key = match raw.kty.as_str() {
                "RSA" => {
                    let (Some(n), Some(e)) = (raw.n, raw.e) else {
                        continue;
                    };
                    let n = decode(&n, "RSA modulus")?;
                    let e = decode(&e, "RSA exponent")?;
                    let size = match n.len() * 8 {
                        2048 => RsaKeySize::Rsa2048,
                        3072 => RsaKeySize::Rsa3072,
                        4096 => RsaKeySize::Rsa4096,
                        // Shorter is weak, and other sizes are not ones a
                        // provider uses.
                        _ => continue,
                    };
                    Key::Rsa(RsaPublicKey { n, e, size })
                }
                "EC" if raw.crv.as_deref() == Some("P-256") => {
                    let (Some(x), Some(y)) = (raw.x, raw.y) else {
                        continue;
                    };
                    let x = decode(&x, "EC x")?;
                    let y = decode(&y, "EC y")?;
                    if x.len() != 32 || y.len() != 32 {
                        continue;
                    }
                    Key::P256(EcPublicKey {
                        x,
                        y,
                        curve: EcCurve::P256,
                    })
                }
                _ => continue,
            };
            keys.push(Jwk { kid: raw.kid, key });
        }
        if keys.is_empty() {
            return Err(SsoError::Config(
                "the JWK Set has no RSA or P-256 signing key".into(),
            ));
        }
        Ok(Self { keys })
    }

    /// Whether a key has this `kid`: a token naming one that is not here may
    /// be signed by a key the provider rotated in since the set was fetched.
    #[must_use]
    pub fn has_kid(&self, kid: &str) -> bool {
        self.keys.iter().any(|k| k.kid.as_deref() == Some(kid))
    }
}

/// The parts of a compact JWS, its header parsed.
struct Jws<'a> {
    signing_input: &'a str,
    header: Value,
    payload: Vec<u8>,
    signature: Vec<u8>,
}

fn split(token: &str) -> Result<Jws<'_>, SsoError> {
    let mut parts = token.split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(SsoError::Malformed("a JWS has three parts"));
    };
    let header: Value = serde_json::from_slice(&decode(header, "header")?)
        .map_err(|_| SsoError::Malformed("header is not JSON"))?;
    Ok(Jws {
        signing_input: &token[..token.rfind('.').expect("three parts")],
        header,
        payload: decode(payload, "payload")?,
        signature: decode(signature, "signature")?,
    })
}

/// The `kid` a token's header names, if it parses that far. For deciding
/// whether to refetch keys; [`verify_jws`] is what decides anything else.
#[must_use]
pub fn token_kid(token: &str) -> Option<String> {
    split(token).ok()?.header["kid"]
        .as_str()
        .map(str::to_string)
}

/// Verify a compact JWS against `jwks` and return its payload.
///
/// The key is the one the header's `kid` names, or -- when the header names
/// none -- the only key of the algorithm's kind; two candidates and no `kid`
/// is refused rather than tried in turn.
///
/// # Errors
/// [`SsoError`] for anything but a valid `RS256` or `ES256` signature by a key
/// in the set.
pub fn verify_jws(token: &str, jwks: &Jwks) -> Result<Vec<u8>, SsoError> {
    let jws = split(token)?;
    let alg = jws.header["alg"]
        .as_str()
        .ok_or(SsoError::Malformed("header has no alg"))?;
    let rsa = match alg {
        "RS256" => true,
        "ES256" => false,
        other => return Err(SsoError::Algorithm(other.to_string())),
    };
    let kid = jws.header["kid"].as_str();
    let mut candidates = jwks.keys.iter().filter(|k| {
        matches!((&k.key, rsa), (Key::Rsa(_), true) | (Key::P256(_), false))
            && kid.is_none_or(|kid| k.kid.as_deref() == Some(kid))
    });
    let key = match (candidates.next(), candidates.next()) {
        (Some(key), None) => key,
        (Some(_), Some(_)) if kid.is_none() => return Err(SsoError::UnknownKey),
        (Some(key), Some(_)) => key,
        (None, _) => return Err(SsoError::UnknownKey),
    };
    let crypto = crypto()?;
    let valid = match &key.key {
        Key::Rsa(public) => crypto.rsa_verify(
            public,
            jws.signing_input.as_bytes(),
            &Signature {
                data: jws.signature,
                algorithm: SignatureAlgorithm::RsaPkcs1Sha256,
            },
        ),
        Key::P256(public) => crypto.ecdsa_verify(
            public,
            jws.signing_input.as_bytes(),
            &Signature {
                data: jws.signature,
                algorithm: SignatureAlgorithm::EcdsaP256Sha256,
            },
        ),
    }
    .map_err(|e| SsoError::Config(e.to_string()))?;
    if valid {
        Ok(jws.payload)
    } else {
        Err(SsoError::Signature)
    }
}

/// What a sign-in establishes about a person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The provider's stable subject identifier.
    pub subject: String,
    /// Their email, which the provider says it verified -- lowercased, the
    /// form [`Members`] is keyed by.
    pub email: String,
}

/// What an ID token must say to be accepted.
#[derive(Debug, Clone)]
pub struct IdTokenRules<'a> {
    /// Exactly the `issuer` the provider's discovery document names.
    pub issuer: &'a str,
    /// This control plane's client ID at the provider.
    pub client_id: &'a str,
    /// The nonce this login sent, if it sent one -- a browser login always
    /// does; a token exchanged from a device grant may not.
    pub nonce: Option<&'a str>,
    /// Unix seconds now.
    pub now: i64,
}

/// Verify an OpenID Connect ID token and say who it is about.
///
/// Beyond the signature: `iss` is exactly the configured issuer; `aud` is
/// or contains the client ID, and when it lists others, `azp` is the client
/// ID; `exp` is in the future and `iat` not in it (each within
/// [`CLOCK_LEEWAY_SECS`]); `nonce` matches when one was sent; `email` is
/// present and `email_verified` is true -- an unverified address is a claim
/// anyone can make about themselves.
///
/// # Errors
/// [`SsoError`] naming what failed.
pub fn verify_id_token(
    token: &str,
    jwks: &Jwks,
    rules: &IdTokenRules<'_>,
) -> Result<Identity, SsoError> {
    let payload = verify_jws(token, jwks)?;
    let claims: Value =
        serde_json::from_slice(&payload).map_err(|_| SsoError::Malformed("payload is not JSON"))?;
    if claims["iss"].as_str() != Some(rules.issuer) {
        return Err(SsoError::Claim("iss"));
    }
    let audiences: Vec<&str> = match &claims["aud"] {
        Value::String(one) => vec![one.as_str()],
        Value::Array(many) => many.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    if !audiences.contains(&rules.client_id) {
        return Err(SsoError::Claim("aud"));
    }
    if audiences.len() > 1 && claims["azp"].as_str() != Some(rules.client_id) {
        return Err(SsoError::Claim("azp"));
    }
    let exp = claims["exp"].as_i64().ok_or(SsoError::Claim("exp"))?;
    if exp + CLOCK_LEEWAY_SECS <= rules.now {
        return Err(SsoError::Claim("exp"));
    }
    if claims["iat"]
        .as_i64()
        .is_none_or(|iat| iat > rules.now + CLOCK_LEEWAY_SECS)
    {
        return Err(SsoError::Claim("iat"));
    }
    if let Some(nonce) = rules.nonce {
        let sent = claims["nonce"].as_str().unwrap_or_default();
        if !bool::from(subtle::ConstantTimeEq::ct_eq(
            sent.as_bytes(),
            nonce.as_bytes(),
        )) {
            return Err(SsoError::Claim("nonce"));
        }
    }
    let subject = claims["sub"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or(SsoError::Claim("sub"))?;
    let email = claims["email"]
        .as_str()
        .filter(|e| e.contains('@'))
        .ok_or(SsoError::Claim("email"))?;
    // Some providers send the boolean as a string.
    let verified = match &claims["email_verified"] {
        Value::Bool(b) => *b,
        Value::String(s) => s == "true",
        _ => false,
    };
    if !verified {
        return Err(SsoError::Claim("email_verified"));
    }
    Ok(Identity {
        subject: subject.to_string(),
        email: email.to_ascii_lowercase(),
    })
}

/// The control plane's key for its own session tokens.
pub struct SessionKey(zeroize::Zeroizing<Vec<u8>>);

impl std::fmt::Debug for SessionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionKey(..)")
    }
}

/// What a session token says.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
pub struct Session {
    /// The verified email it was issued for.
    pub email: String,
    /// Unix seconds it was issued and when it ends.
    pub iat: i64,
    pub exp: i64,
}

const SESSION_PREFIX: &str = "hms1";

impl SessionKey {
    /// A key of at least 32 bytes.
    ///
    /// # Errors
    /// A shorter key.
    pub fn new(bytes: Vec<u8>) -> Result<Self, SsoError> {
        if bytes.len() < 32 {
            return Err(SsoError::Config(
                "a session key needs at least 32 bytes".into(),
            ));
        }
        Ok(Self(zeroize::Zeroizing::new(bytes)))
    }

    fn mac(&self, body: &str) -> Result<[u8; 32], SsoError> {
        crypto()?
            .hmac_sha256(&self.0, body.as_bytes())
            .map_err(|e| SsoError::Config(e.to_string()))
    }

    /// A token for `email`, valid for `ttl_secs` from `now`.
    ///
    /// # Errors
    /// Only if the MAC primitive fails.
    pub fn mint(&self, email: &str, now: i64, ttl_secs: i64) -> Result<String, SsoError> {
        let session = Session {
            email: email.to_ascii_lowercase(),
            iat: now,
            exp: now + ttl_secs,
        };
        let body = format!(
            "{SESSION_PREFIX}.{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&session).expect("serializable"))
        );
        let mac = self.mac(&body)?;
        Ok(format!("{body}.{}", URL_SAFE_NO_PAD.encode(mac)))
    }

    /// The session a token carries, if this key made it and it has not
    /// ended.
    ///
    /// # Errors
    /// [`SsoError`] for a token this key did not make, or one that expired.
    pub fn verify(&self, token: &str, now: i64) -> Result<Session, SsoError> {
        let (body, mac) = token
            .rsplit_once('.')
            .ok_or(SsoError::Malformed("not a session token"))?;
        let Some(payload) = body.strip_prefix(&format!("{SESSION_PREFIX}.")) else {
            return Err(SsoError::Malformed("not a session token"));
        };
        let expected = self.mac(body)?;
        let given = decode(mac, "session MAC")?;
        if !bool::from(subtle::ConstantTimeEq::ct_eq(
            given.as_slice(),
            &expected[..],
        )) {
            return Err(SsoError::Signature);
        }
        let session: Session = serde_json::from_slice(&decode(payload, "session")?)
            .map_err(|_| SsoError::Malformed("session is not JSON"))?;
        if session.exp <= now {
            return Err(SsoError::Claim("exp"));
        }
        Ok(session)
    }
}

/// Who may sign in, and as what.
#[derive(Debug, Clone, Default)]
pub struct Members {
    by_email: BTreeMap<String, Member>,
}

/// One person: what a session for their email may do. The same shape as an
/// API key policy entry, keyed by verified email rather than a digest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub email: String,
    pub scopes: std::collections::BTreeSet<crate::keys::ApiScope>,
    #[serde(default)]
    pub role: crate::keys::ApiRole,
    pub principal_id: crate::ownership::OwnerId,
    #[serde(default)]
    pub team_id: Option<crate::ownership::TeamId>,
}

impl Members {
    /// Parse a members document: a JSON array of [`Member`]s, at most 1024,
    /// emails unique ignoring case.
    ///
    /// # Errors
    /// Malformed JSON, an unknown field or scope, an email without `@`, no
    /// scopes, or a duplicate email.
    pub fn from_json(json: &str) -> Result<Self, String> {
        if json.len() > crate::keys::MAX_POLICY_BYTES {
            return Err("members document exceeds 1 MiB".into());
        }
        let raw: Vec<Member> = serde_json::from_str(json).map_err(|e| {
            format!(
                "invalid members JSON at line {}, column {}",
                e.line(),
                e.column()
            )
        })?;
        if raw.len() > 1024 {
            return Err("members document lists more than 1024 people".into());
        }
        let mut by_email = BTreeMap::new();
        for mut member in raw {
            member.email = member.email.to_ascii_lowercase();
            if !member.email.contains('@') || member.email.len() > 254 {
                return Err("every member needs an email address".into());
            }
            if member.scopes.is_empty() {
                return Err(format!("{} has no scopes", member.email));
            }
            if by_email.insert(member.email.clone(), member).is_some() {
                return Err("a member is listed twice".into());
            }
        }
        Ok(Self { by_email })
    }

    /// The member with this verified email.
    #[must_use]
    pub fn get(&self, email: &str) -> Option<&Member> {
        self.by_email.get(&email.to_ascii_lowercase())
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.by_email.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_email.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 7515 Appendix A.2 (RS256) and A.3 (ES256): the keys and tokens as
    // the RFC prints them. Known answers from outside this codebase, so a
    // verifier that is wrong in a way its own tests share still fails here.
    const RFC_RSA_N: &str = "ofgWCuLjybRlzo0tZWJjNiuSfb4p4fAkd_wWJcyQoTbji9k0l8W26mPddxHmfHQp-Vaw-4qPCJrcS2mJPMEzP1Pt0Bm4d4QlL-yRT-SFd2lZS-pCgNMsD1W_YpRPEwOWvG6b32690r2jZ47soMZo9wGzjb_7OMg0LOL-bSf63kpaSHSXndS5z5rexMdbBYUsLA9e-KXBdQOS-UTo7WTBEMa2R2CapHg665xsmtdVMTBQY4uDZlxvb3qCo5ZwKh9kG4LT6_I5IhlJH7aGhyxXFvUK-DWNmoudF8NAco9_h9iaGNj8q2ethFkMLs91kzk2PAcDTW9gb54h4FRWyuXpoQ";
    const RFC_RS256: &str = "eyJhbGciOiJSUzI1NiJ9.eyJpc3MiOiJqb2UiLA0KICJleHAiOjEzMDA4MTkzODAsDQogImh0dHA6Ly9leGFtcGxlLmNvbS9pc19yb290Ijp0cnVlfQ.cC4hiUPoj9Eetdgtv3hF80EGrhuB__dzERat0XF9g2VtQgr9PJbu3XOiZj5RZmh7AAuHIm4Bh-0Qc_lF5YKt_O8W2Fp5jujGbds9uJdbF9CUAr7t1dnZcAcQjbKBYNX4BAynRFdiuB--f_nZLgrnbyTyWzO75vRK5h6xBArLIARNPvkSjtQBMHlb1L07Qe7K0GarZRmB_eSN9383LcOLn6_dO--xi12jzDwusC-eOkHWEsqtFZESc6BfI7noOPqvhJ1phCnvWh6IeYI2w9QOYEUipUTI8np6LbgGY9Fs98rqVt5AXLIhWkWywlVmtVrBp0igcN_IoypGlUPQGe77Rw";
    const RFC_EC_X: &str = "f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU";
    const RFC_EC_Y: &str = "x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0";
    const RFC_ES256: &str = "eyJhbGciOiJFUzI1NiJ9.eyJpc3MiOiJqb2UiLA0KICJleHAiOjEzMDA4MTkzODAsDQogImh0dHA6Ly9leGFtcGxlLmNvbS9pc19yb290Ijp0cnVlfQ.DtEhU3ljbEg8L38VWAfUAqOyKAM6-Xx-F4GawxaepmXFCgfTjDxw5djxLa8ISlSApmWQxfKTUJqPP3-Kg6NU1Q";

    fn rfc_rsa() -> Jwks {
        Jwks::from_json(&format!(
            r#"{{"keys":[{{"kty":"RSA","n":"{RFC_RSA_N}","e":"AQAB"}}]}}"#
        ))
        .unwrap()
    }

    fn rfc_ec() -> Jwks {
        Jwks::from_json(&format!(
            r#"{{"keys":[{{"kty":"EC","crv":"P-256","x":"{RFC_EC_X}","y":"{RFC_EC_Y}"}}]}}"#
        ))
        .unwrap()
    }

    const RFC_PAYLOAD: &[u8] =
        b"{\"iss\":\"joe\",\r\n \"exp\":1300819380,\r\n \"http://example.com/is_root\":true}";

    #[test]
    fn rfc_7515_rs256_verifies() {
        assert_eq!(verify_jws(RFC_RS256, &rfc_rsa()).unwrap(), RFC_PAYLOAD);
    }

    #[test]
    fn rfc_7515_es256_verifies() {
        assert_eq!(verify_jws(RFC_ES256, &rfc_ec()).unwrap(), RFC_PAYLOAD);
    }

    /// Flip one bit anywhere -- signing input or signature -- and it fails.
    #[test]
    fn a_changed_token_does_not_verify() {
        for (token, keys) in [(RFC_RS256, rfc_rsa()), (RFC_ES256, rfc_ec())] {
            let last = token.rfind('.').unwrap();
            for index in [5, last - 3, token.len() - 3] {
                let mut bytes = token.as_bytes().to_vec();
                // Swap for another base64url character, so it still parses.
                bytes[index] = if bytes[index] == b'A' { b'B' } else { b'A' };
                let changed = String::from_utf8(bytes).unwrap();
                assert!(verify_jws(&changed, &keys).is_err(), "index {index}");
            }
        }
    }

    #[test]
    fn a_token_is_checked_against_its_own_kind_of_key_only() {
        assert_eq!(verify_jws(RFC_RS256, &rfc_ec()), Err(SsoError::UnknownKey));
        assert_eq!(verify_jws(RFC_ES256, &rfc_rsa()), Err(SsoError::UnknownKey));
    }

    fn header(alg: &str) -> String {
        URL_SAFE_NO_PAD.encode(format!(r#"{{"alg":"{alg}"}}"#))
    }

    /// `none` and the MAC algorithms are refused before any key is looked
    /// at: the classic forgeries.
    #[test]
    fn unsigned_and_mac_tokens_are_refused() {
        let payload = URL_SAFE_NO_PAD.encode(RFC_PAYLOAD);
        for alg in ["none", "HS256", "PS256", "ES512"] {
            let token = format!("{}.{payload}.", header(alg));
            assert_eq!(
                verify_jws(&token, &rfc_rsa()),
                Err(SsoError::Algorithm(alg.into()))
            );
        }
        assert!(matches!(
            verify_jws("only.two", &rfc_rsa()),
            Err(SsoError::Malformed(_))
        ));
    }

    #[test]
    fn a_named_kid_must_be_in_the_set() {
        let keys = Jwks::from_json(&format!(
            r#"{{"keys":[{{"kty":"RSA","kid":"k1","n":"{RFC_RSA_N}","e":"AQAB"}}]}}"#
        ))
        .unwrap();
        assert!(keys.has_kid("k1"));
        let mut parts = RFC_RS256.split('.');
        let kidded = format!(
            "{}.{}.{}",
            URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256","kid":"k2"}"#),
            parts.nth(1).unwrap(),
            parts.next().unwrap()
        );
        assert_eq!(token_kid(&kidded).as_deref(), Some("k2"));
        assert_eq!(verify_jws(&kidded, &keys), Err(SsoError::UnknownKey));
    }

    #[test]
    fn unusable_keys_are_skipped_and_an_empty_set_refused() {
        assert!(Jwks::from_json(r#"{"keys":[{"kty":"oct","k":"AAAA"}]}"#).is_err());
        assert!(Jwks::from_json(&format!(
            r#"{{"keys":[{{"kty":"RSA","use":"enc","n":"{RFC_RSA_N}","e":"AQAB"}}]}}"#
        ))
        .is_err());
        // A 1024-bit modulus is too weak to be offered.
        let short = URL_SAFE_NO_PAD.encode([0xc5u8; 128]);
        assert!(Jwks::from_json(&format!(
            r#"{{"keys":[{{"kty":"RSA","n":"{short}","e":"AQAB"}}]}}"#
        ))
        .is_err());
        assert!(Jwks::from_json("not json").is_err());
    }

    /// An ID token, signed for real, for the claim rules: P-256 keys from
    /// hv2-core, so these tests can change claims and re-sign.
    struct Provider {
        key: hv2_core::crypto::asymmetric::EcPrivateKey,
        jwks: Jwks,
    }

    impl Provider {
        fn new() -> Self {
            let crypto = crypto().unwrap();
            let key = crypto.generate_ecdsa_keypair(EcCurve::P256).unwrap();
            let public = crypto.ecdsa_public_key(&key);
            let jwks = Jwks::from_json(&format!(
                r#"{{"keys":[{{"kty":"EC","crv":"P-256","kid":"test","x":"{}","y":"{}"}}]}}"#,
                URL_SAFE_NO_PAD.encode(&public.x),
                URL_SAFE_NO_PAD.encode(&public.y)
            ))
            .unwrap();
            Self { key, jwks }
        }

        fn sign(&self, claims: &Value) -> String {
            let input = format!(
                "{}.{}",
                URL_SAFE_NO_PAD.encode(r#"{"alg":"ES256","kid":"test"}"#),
                URL_SAFE_NO_PAD.encode(claims.to_string())
            );
            let signature = crypto()
                .unwrap()
                .ecdsa_sign(&self.key, input.as_bytes())
                .unwrap();
            format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.data))
        }
    }

    const NOW: i64 = 1_800_000_000;

    fn good() -> Value {
        serde_json::json!({
            "iss": "https://idp.example",
            "aud": "hypermachine",
            "sub": "user-1",
            "email": "Alice@Example.com",
            "email_verified": true,
            "exp": NOW + 300,
            "iat": NOW - 5,
            "nonce": "n-123",
        })
    }

    fn rules() -> IdTokenRules<'static> {
        IdTokenRules {
            issuer: "https://idp.example",
            client_id: "hypermachine",
            nonce: Some("n-123"),
            now: NOW,
        }
    }

    #[test]
    fn a_good_id_token_names_its_verified_email() {
        let provider = Provider::new();
        let identity = verify_id_token(&provider.sign(&good()), &provider.jwks, &rules()).unwrap();
        assert_eq!(
            identity,
            Identity {
                subject: "user-1".into(),
                email: "alice@example.com".into()
            }
        );
    }

    /// Each rule, broken alone, refuses the token and names the claim.
    #[test]
    fn every_claim_rule_is_enforced() {
        let provider = Provider::new();
        type Change = Box<dyn Fn(&mut Value)>;
        let cases: Vec<(&str, Change)> = vec![
            (
                "iss",
                Box::new(|c| c["iss"] = "https://idp.example/".into()),
            ),
            ("aud", Box::new(|c| c["aud"] = "someone-else".into())),
            (
                "azp",
                Box::new(|c| c["aud"] = serde_json::json!(["hypermachine", "other"])),
            ),
            ("exp", Box::new(|c| c["exp"] = (NOW - 61).into())),
            ("iat", Box::new(|c| c["iat"] = (NOW + 61).into())),
            ("nonce", Box::new(|c| c["nonce"] = "n-999".into())),
            ("sub", Box::new(|c| c["sub"] = "".into())),
            ("email", Box::new(|c| c["email"] = Value::Null)),
            (
                "email_verified",
                Box::new(|c| c["email_verified"] = false.into()),
            ),
            (
                "email_verified",
                Box::new(|c| {
                    c.as_object_mut().unwrap().remove("email_verified");
                }),
            ),
        ];
        for (claim, change) in cases {
            let mut claims = good();
            change(&mut claims);
            assert_eq!(
                verify_id_token(&provider.sign(&claims), &provider.jwks, &rules()),
                Err(SsoError::Claim(claim)),
                "{claim}"
            );
        }
        // Within the leeway is fine; a listed audience with the right azp too.
        let mut claims = good();
        claims["exp"] = (NOW - 30).into();
        claims["aud"] = serde_json::json!(["hypermachine", "other"]);
        claims["azp"] = "hypermachine".into();
        claims["email_verified"] = "true".into();
        assert!(verify_id_token(&provider.sign(&claims), &provider.jwks, &rules()).is_ok());
        // No nonce expected, none checked.
        let mut claims = good();
        claims.as_object_mut().unwrap().remove("nonce");
        let rules = IdTokenRules {
            nonce: None,
            ..rules()
        };
        assert!(verify_id_token(&provider.sign(&claims), &provider.jwks, &rules).is_ok());
    }

    #[test]
    fn another_providers_signature_is_refused() {
        let ours = Provider::new();
        let theirs = Provider::new();
        assert_eq!(
            verify_id_token(&theirs.sign(&good()), &ours.jwks, &rules()),
            Err(SsoError::Signature)
        );
    }

    #[test]
    fn a_session_round_trips_until_it_expires() {
        let key = SessionKey::new(vec![7u8; 32]).unwrap();
        let token = key.mint("Alice@Example.com", NOW, 3600).unwrap();
        assert!(token.starts_with("hms1."));
        let session = key.verify(&token, NOW + 10).unwrap();
        assert_eq!(session.email, "alice@example.com");
        assert_eq!(session.exp, NOW + 3600);
        assert_eq!(key.verify(&token, NOW + 3600), Err(SsoError::Claim("exp")));
    }

    #[test]
    fn a_session_from_another_key_or_changed_is_refused() {
        let key = SessionKey::new(vec![7u8; 32]).unwrap();
        let other = SessionKey::new(vec![8u8; 32]).unwrap();
        let token = key.mint("alice@example.com", NOW, 3600).unwrap();
        assert_eq!(other.verify(&token, NOW), Err(SsoError::Signature));
        // Someone else's email, with the original MAC.
        let (_, mac) = token.rsplit_once('.').unwrap();
        let forged_body = format!(
            "hms1.{}",
            URL_SAFE_NO_PAD.encode(br#"{"email":"mallory@example.com","iat":0,"exp":9999999999}"#)
        );
        assert_eq!(
            key.verify(&format!("{forged_body}.{mac}"), NOW),
            Err(SsoError::Signature)
        );
        assert!(key.verify("not-a-session", NOW).is_err());
        assert!(SessionKey::new(vec![1u8; 31]).is_err());
    }

    #[test]
    fn members_are_keyed_by_email_ignoring_case() {
        let members = Members::from_json(
            r#"[{"email":"Alice@Example.com","scopes":["sandboxes"],"principal_id":"alice","team_id":"red"},
                {"email":"bob@example.com","scopes":["inventory"],"role":"observer","principal_id":"bob"}]"#,
        )
        .unwrap();
        assert_eq!(members.len(), 2);
        let alice = members.get("ALICE@example.com").unwrap();
        assert_eq!(alice.principal_id.as_str(), "alice");
        assert_eq!(alice.team_id.as_ref().unwrap().as_str(), "red");
        assert!(members.get("carol@example.com").is_none());
        for bad in [
            r#"[{"email":"a@x","scopes":["sandboxes"],"principal_id":"a"},{"email":"A@X","scopes":["sandboxes"],"principal_id":"b"}]"#,
            r#"[{"email":"no-at","scopes":["sandboxes"],"principal_id":"a"}]"#,
            r#"[{"email":"a@x","scopes":[],"principal_id":"a"}]"#,
            r#"[{"email":"a@x","scopes":["sandboxes"],"principal_id":"a","extra":1}]"#,
            r#"[{"email":"a@x","scopes":["root"],"principal_id":"a"}]"#,
        ] {
            assert!(Members::from_json(bad).is_err(), "{bad}");
        }
    }
}
