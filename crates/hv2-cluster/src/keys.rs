//! Operator-provisioned, expiring API keys. Policies hold SHA-256 digests,
//! never plaintext credentials. Scopes apply to the whole configured team.

use axum::http::Method;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use subtle::ConstantTimeEq;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiScope {
    Admin,
    Inventory,
    Sandboxes,
    Templates,
    Volumes,
    Events,
}

#[derive(Clone)]
pub struct ApiKeyPolicy {
    digest: [u8; 32],
    expires_at: i64,
    scopes: BTreeSet<ApiScope>,
}

impl std::fmt::Debug for ApiKeyPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiKeyPolicy")
            .field("expires_at", &self.expires_at)
            .field("scopes", &self.scopes)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPolicy {
    sha256: String,
    expires_at: i64,
    scopes: BTreeSet<ApiScope>,
}

impl ApiKeyPolicy {
    /// Parse a nonempty policy array. Expiry is Unix time in seconds.
    ///
    /// # Errors
    /// Reject malformed JSON, unknown scopes/fields, invalid hashes, duplicate
    /// credentials, empty scopes, nonpositive expiry or more than 256 policies.
    pub fn from_json(json: &str) -> Result<Vec<Self>, String> {
        let raw: Vec<RawPolicy> = serde_json::from_str(json).map_err(|error| {
            format!("invalid API key policy JSON at line {}, column {}; check required fields and scopes",
                error.line(), error.column())
        })?;
        if raw.is_empty() || raw.len() > 256 {
            return Err("API key policy needs 1-256 entries".into());
        }
        let mut seen = BTreeSet::new();
        raw.into_iter()
            .map(|raw| {
                if raw.sha256.len() != 64 || !raw.sha256.bytes().all(|c| c.is_ascii_hexdigit()) {
                    return Err("API key sha256 must contain 64 hexadecimal characters".into());
                }
                let mut digest = [0u8; 32];
                for (index, byte) in digest.iter_mut().enumerate() {
                    *byte = u8::from_str_radix(&raw.sha256[index * 2..index * 2 + 2], 16)
                        .map_err(|_| "invalid API key digest")?;
                }
                if digest == <[u8; 32]>::from(Sha256::digest(b"")) || !seen.insert(digest) {
                    return Err("empty or duplicate API key credential".into());
                }
                if raw.expires_at <= 0 || raw.scopes.is_empty() {
                    return Err("API key expiry must be positive and scopes nonempty".into());
                }
                Ok(Self {
                    digest,
                    expires_at: raw.expires_at,
                    scopes: raw.scopes,
                })
            })
            .collect()
    }

    pub(crate) fn matches(&self, digest: &[u8; 32], now: i64) -> bool {
        bool::from(self.digest.ct_eq(digest)) && now < self.expires_at
    }

    pub(crate) fn permits(&self, method: &Method, path: &str) -> bool {
        if self.scopes.contains(&ApiScope::Admin) {
            return true;
        }
        if self.scopes.contains(&ApiScope::Inventory)
            && (*method == Method::GET || *method == Method::HEAD)
            && matches!(
                path,
                "/sandboxes"
                    | "/v2/sandboxes"
                    | "/templates"
                    | "/sandboxes/metrics"
                    | "/cluster/nodes"
            )
        {
            return true;
        }
        let parts: Vec<_> = path.trim_start_matches('/').split('/').collect();
        let family = match parts.as_slice() {
            ["v2", "sandboxes", ..] | ["sandboxes", ..] => ApiScope::Sandboxes,
            ["v2" | "v3", "templates", ..] | ["templates" | "snapshots", ..] => ApiScope::Templates,
            ["volumes", ..] => ApiScope::Volumes,
            ["events", ..] => ApiScope::Events,
            _ => return false,
        };
        if self.scopes.contains(&family) {
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy(scope: &str) -> ApiKeyPolicy {
        let hash: String = Sha256::digest(b"fixture-key")
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        ApiKeyPolicy::from_json(&format!(
            r#"[{{"sha256":"{hash}","expires_at":100,"scopes":["{scope}"]}}]"#
        ))
        .unwrap()
        .remove(0)
    }
    #[test]
    fn expiry_is_enforced_at_the_exact_boundary() {
        let policy = policy("admin");
        let digest = Sha256::digest(b"fixture-key").into();
        assert!(policy.matches(&digest, 99));
        assert!(!policy.matches(&digest, 100));
        assert!(!policy.matches(&Sha256::digest(b"wrong").into(), 99));
    }
    #[test]
    fn inventory_cannot_obtain_access_tokens_or_modify_resources() {
        let policy = policy("inventory");
        assert!(policy.permits(&Method::GET, "/sandboxes"));
        assert!(policy.permits(&Method::GET, "/cluster/nodes"));
        for path in [
            "/sandboxes/box",
            "/sandboxes/box/connect",
            "/volumes",
            "/volumes/id",
            "/events/webhooks",
            "/templates/id/files/hash",
            "/sandboxes/box/exec",
        ] {
            assert!(!policy.permits(&Method::GET, path), "{path}");
        }
        assert!(!policy.permits(&Method::POST, "/sandboxes"));
        assert!(!policy.permits(&Method::DELETE, "/templates"));
    }
    #[test]
    fn capability_scopes_do_not_cross_resource_families() {
        let policy = policy("sandboxes");
        assert!(policy.permits(&Method::POST, "/v2/sandboxes"));
        assert!(policy.permits(&Method::POST, "/sandboxes/box/exec"));
        assert!(!policy.permits(&Method::POST, "/v3/templates"));
        assert!(!policy.permits(&Method::GET, "/sandboxes-other"));
        assert!(!policy.permits(&Method::GET, "/events/sandboxes"));
    }
    #[test]
    fn malformed_policy_never_silently_disables_authentication() {
        for json in [
            "[]",
            "{}",
            r#"[{"sha256":"bad","expires_at":100,"scopes":["admin"]}]"#,
        ] {
            assert!(ApiKeyPolicy::from_json(json).is_err());
        }
        let hash: String = Sha256::digest(b"fixture-key")
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        for tail in [
            r#""expires_at":0,"scopes":["admin"]"#,
            r#""expires_at":100,"scopes":[]"#,
            r#""expires_at":100,"scopes":["unknown"]"#,
            r#""expires_at":100,"scopes":["admin"],"key":"plaintext""#,
        ] {
            assert!(
                ApiKeyPolicy::from_json(&format!(r#"[{{"sha256":"{hash}",{tail}}}]"#)).is_err()
            );
        }
        let row = format!(r#"{{"sha256":"{hash}","expires_at":100,"scopes":["admin"]}}"#);
        assert!(ApiKeyPolicy::from_json(&format!("[{row},{row}]")).is_err());
        let unknown =
            format!(r#"[{{"sha256":"{hash}","expires_at":100,"scopes":["accidental-secret"]}}]"#);
        assert!(!ApiKeyPolicy::from_json(&unknown)
            .unwrap_err()
            .contains("accidental-secret"));
        let empty = Sha256::digest(b"")
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert!(ApiKeyPolicy::from_json(&format!(
            r#"[{{"sha256":"{empty}","expires_at":100,"scopes":["admin"]}}]"#
        ))
        .is_err());
        let excessive = format!("[{}]", vec![row; 257].join(","));
        assert!(ApiKeyPolicy::from_json(&excessive)
            .unwrap_err()
            .contains("1-256"));
    }
}
