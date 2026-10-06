//! Operator-provisioned, expiring API keys. Policies hold SHA-256 digests,
//! never plaintext credentials. Scopes apply to the whole configured team.

use axum::http::Method;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use subtle::ConstantTimeEq;

/// Maximum policy document size accepted at startup and during rotation.
pub const MAX_POLICY_BYTES: usize = 1_048_576;

/// Read a UTF-8 policy document with bounded allocation.
///
/// # Errors
/// Returns credential-free errors for unreadable, oversized or non-UTF-8 files.
pub fn read_policy_file(path: impl AsRef<std::path::Path>) -> Result<String, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|_| "could not read API key policy file")?;
    let mut bytes = Vec::new();
    file.take((MAX_POLICY_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "could not read API key policy file")?;
    if bytes.len() > MAX_POLICY_BYTES {
        return Err("API key policy document exceeds 1 MiB".into());
    }
    String::from_utf8(bytes).map_err(|_| "API key policy document must be UTF-8".into())
}

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

/// A role limits capabilities independently of the resource scopes.
/// Omitted roles preserve the existing operator policy behavior.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiRole {
    #[default]
    Operator,
    Observer,
}

#[derive(Clone)]
pub struct ApiKeyPolicy {
    digest: [u8; 32],
    expires_at: i64,
    scopes: BTreeSet<ApiScope>,
    role: ApiRole,
    principal_id: Option<crate::ownership::OwnerId>,
}

impl std::fmt::Debug for ApiKeyPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiKeyPolicy")
            .field("expires_at", &self.expires_at)
            .field("scopes", &self.scopes)
            .field("role", &self.role)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPolicy {
    sha256: String,
    expires_at: i64,
    scopes: BTreeSet<ApiScope>,
    #[serde(default)]
    role: ApiRole,
    #[serde(default)]
    principal_id: Option<crate::ownership::OwnerId>,
}

impl ApiKeyPolicy {
    /// Parse a nonempty policy array. Expiry is Unix time in seconds.
    ///
    /// # Errors
    /// Reject malformed JSON, unknown scopes/fields, invalid hashes, duplicate
    /// credentials, empty scopes, nonpositive expiry or more than 256 policies.
    pub fn from_json(json: &str) -> Result<Vec<Self>, String> {
        if json.len() > MAX_POLICY_BYTES {
            return Err("API key policy document exceeds 1 MiB".into());
        }
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
                    role: raw.role,
                    principal_id: raw.principal_id,
                })
            })
            .collect()
    }

    /// Reject a legacy admin credential also present in scoped policies.
    ///
    /// # Errors
    /// Returns a credential-free error when a policy duplicates the admin key,
    /// including when that policy has already expired.
    pub fn validate_legacy_admin(policies: &[Self], admin: Option<&str>) -> Result<(), String> {
        if let Some(admin) = admin {
            let digest: [u8; 32] = Sha256::digest(admin.as_bytes()).into();
            if policies.iter().any(|policy| policy.has_digest(&digest)) {
                return Err("legacy admin credential must differ from every scoped API key".into());
            }
        }
        Ok(())
    }

    /// Operator-provisioned stable principal; not derived from credential bytes.
    pub fn principal_id(&self) -> Option<&crate::ownership::OwnerId> {
        self.principal_id.as_ref()
    }

    pub(crate) fn has_digest(&self, digest: &[u8; 32]) -> bool {
        bool::from(self.digest.ct_eq(digest))
    }

    pub(crate) fn matches(&self, digest: &[u8; 32], now: i64) -> bool {
        self.has_digest(digest) && now < self.expires_at
    }

    pub(crate) fn permits(&self, method: &Method, path: &str) -> bool {
        // GET is not sufficient: detail, upload URLs and TCP upgrades carry
        // writable capabilities. Fail closed for every route not listed here.
        let inventory_read = (*method == Method::GET || *method == Method::HEAD)
            && matches!(
                path,
                "/sandboxes"
                    | "/v2/sandboxes"
                    | "/templates"
                    | "/sandboxes/metrics"
                    | "/cluster/nodes"
            );
        if self.role == ApiRole::Observer && !inventory_read {
            return false;
        }
        if self.scopes.contains(&ApiScope::Admin) {
            return true;
        }
        if self.scopes.contains(&ApiScope::Inventory) && inventory_read {
            return true;
        }
        let parts: Vec<_> = path.trim_start_matches('/').split('/').collect();
        let family = match parts.as_slice() {
            ["v2", "sandboxes", ..] | ["sandboxes" | "sandbox-names", ..] => ApiScope::Sandboxes,
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
    #[test]
    fn policy_reads_and_parsing_are_bounded_without_content_in_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("policies.json");
        let oversized = "s".repeat(MAX_POLICY_BYTES + 1);
        std::fs::write(&path, &oversized).unwrap();
        assert_eq!(
            read_policy_file(&path).unwrap_err(),
            "API key policy document exceeds 1 MiB"
        );
        assert_eq!(
            ApiKeyPolicy::from_json(&oversized).unwrap_err(),
            "API key policy document exceeds 1 MiB"
        );
        std::fs::write(&path, [255]).unwrap();
        assert_eq!(
            read_policy_file(&path).unwrap_err(),
            "API key policy document must be UTF-8"
        );
        std::fs::write(&path, "[]").unwrap();
        assert_eq!(read_policy_file(&path).unwrap(), "[]");
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            read_policy_file(&path).unwrap_err(),
            "could not read API key policy file"
        );
    }

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
    fn configured_principal_survives_key_rotation_and_rejects_invalid_labels() {
        let entry = |key: &str, principal: &str| serde_json::json!({
            "sha256": Sha256::digest(key.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
            "expires_at": 100, "scopes": ["sandboxes"], "principal_id": principal});
        let first = ApiKeyPolicy::from_json(&serde_json::json!([entry("first-key", "principal-a")]).to_string()).unwrap();
        let rotated = ApiKeyPolicy::from_json(&serde_json::json!([entry("rotated-key", "principal-a")]).to_string()).unwrap();
        assert_eq!(first[0].principal_id(), rotated[0].principal_id());
        assert!(!first[0].has_digest(&Sha256::digest(b"rotated-key").into()));
        assert!(rotated[0].has_digest(&Sha256::digest(b"rotated-key").into()));
        for invalid in ["", "a/b", "a b", "secret\n"] {
            let error = ApiKeyPolicy::from_json(&serde_json::json!([entry("first-key", invalid)]).to_string()).unwrap_err();
            assert!(!error.contains("first-key"));
        }
        assert!(policy("sandboxes").principal_id().is_none());
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
    fn observer_role_caps_even_admin_scope_and_defaults_preserve_operators() {
        let mut observer = policy("admin");
        observer.role = ApiRole::Observer;
        for path in [
            "/sandboxes",
            "/v2/sandboxes",
            "/templates",
            "/sandboxes/metrics",
            "/cluster/nodes",
        ] {
            assert!(observer.permits(&Method::GET, path));
            assert!(observer.permits(&Method::HEAD, path));
            for method in [
                Method::POST,
                Method::PUT,
                Method::DELETE,
                Method::PATCH,
                Method::OPTIONS,
            ] {
                assert!(!observer.permits(&method, path));
            }
        }
        for path in [
            "/sandboxes/id",
            "/sandboxes/id/connect",
            "/sandboxes/id/ports/22/tcp",
            "/sandbox-names/name",
            "/volumes",
            "/volumes/id",
            "/templates/id/files/hash",
            "/events",
            "/unknown",
            "/sandboxes/",
        ] {
            assert!(!observer.permits(&Method::GET, path), "{path}");
        }
        assert!(policy("admin").permits(&Method::DELETE, "/sandboxes/id"));
        observer.scopes = BTreeSet::from([ApiScope::Sandboxes]);
        assert!(observer.permits(&Method::GET, "/sandboxes"));
        assert!(!observer.permits(&Method::GET, "/templates"));
    }

    #[test]
    fn legacy_admin_collision_is_rejected_even_for_expired_policies() {
        let policy = policy("inventory");
        let error =
            ApiKeyPolicy::validate_legacy_admin(std::slice::from_ref(&policy), Some("fixture-key"))
                .unwrap_err();
        assert!(!error.contains("fixture-key"));
        assert!(ApiKeyPolicy::validate_legacy_admin(
            std::slice::from_ref(&policy),
            Some("different-admin")
        )
        .is_ok());
        assert!(ApiKeyPolicy::validate_legacy_admin(&[policy], None).is_ok());
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
            "/sandboxes/box/ports/8080/tcp",
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
        assert!(policy.permits(&Method::GET, "/sandboxes/box/ports/8080/tcp"));
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
            r#""expires_at":100,"scopes":["admin"],"role":"unknown""#,
            r#""expires_at":100,"scopes":["admin"],"role":null"#,
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
