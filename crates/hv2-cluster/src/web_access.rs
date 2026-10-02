//! Separate, expiring browser credentials for operator-protected guest URLs.
use axum::http::HeaderMap;
use base64::Engine;
use parking_lot::RwLock;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

/// Guest-visible identity comes from policy, never from a client header.
pub const IDENTITY_HEADER: &str = "x-hypermachine-user";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct User {
    subject: String,
    sha256: String,
    expires_at: i64,
}

struct Credential {
    subject: String,
    digest: [u8; 32],
    expires_at: i64,
}

/// An atomic policy replacement never disables an enabled private proxy.
pub struct WebAccessPolicy {
    users: RwLock<Vec<Credential>>,
}

impl std::fmt::Debug for WebAccessPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebAccessPolicy")
            .field("user_count", &self.users.read().len())
            .finish_non_exhaustive()
    }
}

fn parse(json: &str) -> Result<Vec<Credential>, String> {
    if json.len() > crate::keys::MAX_POLICY_BYTES {
        return Err("web access policy exceeds 1 MiB".into());
    }
    let users: Vec<User> = serde_json::from_str(json)
        .map_err(|_| "invalid web access policy JSON; check subject, sha256 and expires_at")?;
    if users.is_empty() || users.len() > 256 {
        return Err("web access policy needs 1-256 users".into());
    }
    let mut subjects = BTreeSet::new();
    let mut digests = BTreeSet::new();
    users
        .into_iter()
        .map(|user| {
            if user.subject.is_empty()
                || user.subject.len() > 128
                || !user
                    .subject
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._@+-".contains(&c))
                || !subjects.insert(user.subject.clone())
            {
                return Err(
                    "web access subjects must be unique, 1-128 ASCII letters/digits or ._@+-"
                        .into(),
                );
            }
            if user.sha256.len() != 64
                || !user.sha256.bytes().all(|c| c.is_ascii_hexdigit())
                || user.expires_at <= 0
            {
                return Err("web access needs a SHA-256 hex digest and positive expiry".into());
            }
            let mut digest = [0; 32];
            for (index, value) in digest.iter_mut().enumerate() {
                *value = u8::from_str_radix(&user.sha256[index * 2..index * 2 + 2], 16)
                    .map_err(|_| "invalid web access digest")?;
            }
            if digest == <[u8; 32]>::from(Sha256::digest(b"")) || !digests.insert(digest) {
                return Err("web access credentials must be nonempty and unique".into());
            }
            Ok(Credential {
                subject: user.subject,
                digest,
                expires_at: user.expires_at,
            })
        })
        .collect()
}

impl WebAccessPolicy {
    /// Load the entire policy, rejecting malformed or empty configurations.
    ///
    /// # Errors
    /// Returns credential-free errors for invalid policy documents.
    pub fn from_json(json: &str) -> Result<Self, String> {
        Ok(Self {
            users: RwLock::new(parse(json)?),
        })
    }

    /// Replace all credentials atomically; invalid replacements preserve policy.
    ///
    /// # Errors
    /// Returns credential-free errors for invalid policy documents.
    pub fn replace(&self, json: &str) -> Result<(), String> {
        let users = parse(json)?;
        *self.users.write() = users;
        Ok(())
    }

    /// Authenticate HTTP Basic login against request-time expiry and key digest.
    #[must_use]
    pub fn identity(&self, headers: &HeaderMap, now: i64) -> Option<String> {
        let mut values = headers.get_all(axum::http::header::AUTHORIZATION).iter();
        let value = values.next()?.to_str().ok()?;
        if values.next().is_some() || value.len() > 8192 {
            return None;
        }
        let (scheme, encoded) = value.split_once(' ')?;
        if !scheme.eq_ignore_ascii_case("Basic") {
            return None;
        }
        let decoded = Zeroizing::new(
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .ok()?,
        );
        let (subject, password) = std::str::from_utf8(&decoded).ok()?.split_once(':')?;
        if password.is_empty() || password.len() > 4096 {
            return None;
        }
        let digest: [u8; 32] = Sha256::digest(password.as_bytes()).into();
        self.users
            .read()
            .iter()
            .find(|user| {
                user.subject == subject
                    && now < user.expires_at
                    && bool::from(user.digest.ct_eq(&digest))
            })
            .map(|user| user.subject.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy(password: &str) -> String {
        let digest: String = Sha256::digest(password.as_bytes())
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect();
        format!(r#"[{{"subject":"alice@example.test","sha256":"{digest}","expires_at":100}}]"#)
    }
    fn headers(password: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let encoded = base64::engine::general_purpose::STANDARD
            .encode(format!("alice@example.test:{password}"));
        headers.insert("authorization", format!("Basic {encoded}").parse().unwrap());
        headers
    }
    #[test]
    fn expiry_rotation_and_invalid_replacements_fail_closed() {
        let policy = WebAccessPolicy::from_json(&policy("secret-a")).unwrap();
        assert_eq!(
            policy.identity(&headers("secret-a"), 99).as_deref(),
            Some("alice@example.test")
        );
        assert_eq!(policy.identity(&headers("secret-a"), 100), None);
        assert_eq!(policy.identity(&headers("secret-b"), 99), None);
        assert!(policy.replace("[]").is_err());
        assert!(policy.identity(&headers("secret-a"), 99).is_some());
        policy.replace(&super::tests::policy("secret-b")).unwrap();
        assert!(policy.identity(&headers("secret-a"), 99).is_none());
        assert!(policy.identity(&headers("secret-b"), 99).is_some());
        assert!(!format!("{policy:?}").contains("secret"));
    }
    #[test]
    fn malformed_duplicate_and_oversized_auth_are_refused() {
        let policy = WebAccessPolicy::from_json(&policy("secret-a")).unwrap();
        let mut duplicate = headers("secret-a");
        duplicate.append("authorization", duplicate["authorization"].clone());
        assert!(policy.identity(&duplicate, 99).is_none());
        for value in ["Bearer abc", "Basic !!!", "Basic Og==", ""] {
            let mut headers = HeaderMap::new();
            headers.insert("authorization", value.parse().unwrap());
            assert!(policy.identity(&headers, 99).is_none());
        }
        assert!(policy.identity(&headers(&"x".repeat(9000)), 99).is_none());
        for json in [
            "[]",
            "{}",
            "private-secret",
            r#"[{"subject":"a:b","sha256":"secret","expires_at":99}]"#,
        ] {
            let error = WebAccessPolicy::from_json(json).unwrap_err();
            assert!(!error.contains("private-secret"));
        }
    }
}
