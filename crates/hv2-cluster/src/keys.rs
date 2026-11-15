//! Operator-provisioned, expiring API keys. Policies hold SHA-256 digests,
//! never plaintext credentials.
//!
//! Without `team_id` on any policy, scopes apply to the one configured team,
//! as they always have. Once any policy names a team the deployment is
//! multi-tenant: every key that is not an administrator must name its team
//! and its principal, and reaches only that team's sandboxes -- all of them,
//! as its role allows. Administrators stay global.

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
    Machines,
    /// A team's own telemetry destination: `/telemetry`.
    Telemetry,
}

/// A role limits capabilities independently of the resource scopes.
/// Omitted roles preserve the existing operator policy behavior.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiRole {
    /// Also written `member`, the team name for the same role.
    #[default]
    #[serde(alias = "member")]
    Operator,
    /// Also written `viewer`, the team name for the same role.
    #[serde(alias = "viewer")]
    Observer,
    /// An operator that also manages its own team: it may adopt an owner for
    /// a sandbox of its team (`POST /sandboxes/{id}/owner`). It needs a
    /// `team_id`, may not hold the `admin` scope, and is never an
    /// administrator.
    #[serde(rename = "team-admin")]
    TeamAdmin,
}

/// A named bundle of scopes (and a role) that a policy entry may use instead
/// of listing scopes by hand. Written as `"preset": "<name>"` on the entry.
///
/// Explicit `scopes` on the same entry are added to the preset's, except
/// with [`ApiPreset::ReadOnly`], which accepts none beyond its own: read-only
/// stays read-only. An explicit `role` may lower a preset's role to
/// `observer` but never raise `read-only` to `operator` or `team-admin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApiPreset {
    /// Scopes `inventory`, `events`, `telemetry`; role `observer`.
    ReadOnly,
    /// Scopes `sandboxes`, `templates`, `volumes`, `events`, `inventory`; role `operator`.
    Developer,
    /// Scopes `machines`, `inventory`, `events`; role `operator`.
    Machines,
    /// Scope `admin`; role `operator`.
    Admin,
}

impl ApiPreset {
    /// The scopes this preset stands for.
    #[must_use]
    pub fn scopes(self) -> BTreeSet<ApiScope> {
        use ApiScope as S;
        let list: &[ApiScope] = match self {
            // Only inventory: the observer role refuses every route but the
            // inventory GETs, so any other scope here would promise reads it
            // cannot make.
            Self::ReadOnly => &[S::Inventory],
            Self::Developer => &[
                S::Sandboxes,
                S::Templates,
                S::Volumes,
                S::Events,
                S::Inventory,
            ],
            Self::Machines => &[S::Machines, S::Inventory, S::Events],
            Self::Admin => &[S::Admin],
        };
        list.iter().copied().collect()
    }

    /// The role this preset stands for.
    #[must_use]
    pub fn role(self) -> ApiRole {
        match self {
            Self::ReadOnly => ApiRole::Observer,
            Self::Developer | Self::Machines | Self::Admin => ApiRole::Operator,
        }
    }
}

/// Deserialize a present field as a value, so an explicit `null` is an error
/// rather than "absent"; absence is `None` through `#[serde(default)]`.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[derive(Clone)]
pub struct ApiKeyPolicy {
    digest: [u8; 32],
    expires_at: i64,
    scopes: BTreeSet<ApiScope>,
    role: ApiRole,
    principal_id: Option<crate::ownership::OwnerId>,
    team_id: Option<crate::ownership::TeamId>,
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
    #[serde(default)]
    scopes: BTreeSet<ApiScope>,
    #[serde(default, deserialize_with = "present")]
    role: Option<ApiRole>,
    #[serde(default, deserialize_with = "present")]
    preset: Option<ApiPreset>,
    #[serde(default)]
    principal_id: Option<crate::ownership::OwnerId>,
    #[serde(default)]
    team_id: Option<crate::ownership::TeamId>,
}

impl RawPolicy {
    /// The effective scopes and role once any preset is expanded.
    fn expand(&self) -> Result<(BTreeSet<ApiScope>, ApiRole), String> {
        let Some(preset) = self.preset else {
            return Ok((self.scopes.clone(), self.role.unwrap_or_default()));
        };
        if preset == ApiPreset::ReadOnly && self.role == Some(ApiRole::TeamAdmin) {
            return Err(
                "API key preset read-only is an observer; role team-admin cannot raise it".into(),
            );
        }
        let base = preset.scopes();
        if preset == ApiPreset::ReadOnly {
            if self.role == Some(ApiRole::Operator) {
                return Err(
                    "API key preset read-only is an observer; role operator cannot raise it".into(),
                );
            }
            if !self.scopes.is_subset(&base) {
                return Err("API key preset read-only accepts no scopes beyond its own".into());
            }
        }
        let role = match (preset.role(), self.role) {
            (_, Some(ApiRole::Observer)) => ApiRole::Observer,
            (_, Some(ApiRole::TeamAdmin)) => ApiRole::TeamAdmin,
            (role, _) => role,
        };
        Ok((base.union(&self.scopes).copied().collect(), role))
    }
}

impl ApiKeyPolicy {
    /// Parse a nonempty policy array. Expiry is Unix time in seconds.
    ///
    /// # Errors
    /// Reject malformed JSON, unknown scopes/fields, invalid hashes, duplicate
    /// credentials, empty scopes (after any `preset` expands), unknown presets,
    /// read-only presets given extra scopes or an operator role, nonpositive expiry or more than 256 policies.
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
        let teams = raw.iter().any(|raw| raw.team_id.is_some());
        let policies: Vec<Self> = raw
            .into_iter()
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
                let (scopes, role) = raw.expand()?;
                if role == ApiRole::TeamAdmin {
                    if raw.team_id.is_none() {
                        return Err("API key role team-admin needs a team_id".into());
                    }
                    if scopes.contains(&ApiScope::Admin) {
                        return Err(
                            "API key role team-admin cannot hold the admin scope, which is cluster-wide"
                                .into(),
                        );
                    }
                }
                if raw.expires_at <= 0 || scopes.is_empty() {
                    return Err("API key expiry must be positive and scopes nonempty".into());
                }
                Ok(Self {
                    digest,
                    expires_at: raw.expires_at,
                    scopes,
                    role,
                    principal_id: raw.principal_id,
                    team_id: raw.team_id,
                })
            })
            .collect::<Result<_, String>>()?;
        // Half a tenancy is none: a key with no team in a multi-tenant file
        // would be global by omission, which is the one mistake that matters.
        if teams
            && policies.iter().any(|policy| {
                !policy.is_administrator()
                    && (policy.team_id.is_none() || policy.principal_id.is_none())
            })
        {
            return Err(
                "once any API key names a team_id, every non-administrator key needs both team_id and principal_id"
                    .into(),
            );
        }
        Ok(policies)
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

    /// The policy a signed-in member acts under: their scopes, role,
    /// principal and team, until their session ends. Its digest is all
    /// zeros, which no key hashes to, so no `X-API-Key` ever matches it.
    pub(crate) fn for_member(member: &crate::sso::Member, session_expires_at: i64) -> Self {
        Self {
            digest: [0u8; 32],
            expires_at: session_expires_at,
            scopes: member.scopes.clone(),
            role: member.role,
            principal_id: Some(member.principal_id.clone()),
            team_id: member.team_id.clone(),
        }
    }

    /// Operator-provisioned stable principal; not derived from credential bytes.
    pub(crate) fn is_administrator(&self) -> bool {
        self.role == ApiRole::Operator && self.scopes.contains(&ApiScope::Admin)
    }

    /// A team administrator: operator rights plus owner adoption, on its own
    /// team's sandboxes only (the caller checks the team).
    pub(crate) fn is_team_admin(&self) -> bool {
        self.role == ApiRole::TeamAdmin && self.team_id.is_some()
    }

    pub fn principal_id(&self) -> Option<&crate::ownership::OwnerId> {
        self.principal_id.as_ref()
    }

    /// The team this key acts for, unless it is an administrator, which is
    /// global whatever its policy says.
    pub fn team_id(&self) -> Option<&crate::ownership::TeamId> {
        self.team_id.as_ref().filter(|_| !self.is_administrator())
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
        if matches!(parts.as_slice(), ["sandboxes", _, "owner"]) {
            // Only a team administrator, and only for POST; the handler
            // checks that the sandbox is in its team.
            return self.is_team_admin()
                && *method == Method::POST
                && self.scopes.contains(&ApiScope::Sandboxes);
        }
        let family = match parts.as_slice() {
            ["v2", "sandboxes", ..] | ["sandboxes" | "sandbox-names", ..] => ApiScope::Sandboxes,
            ["v2" | "v3", "templates", ..] | ["templates" | "snapshots", ..] => ApiScope::Templates,
            ["volumes", ..] => ApiScope::Volumes,
            ["machines", ..] => ApiScope::Machines,
            ["telemetry"] => ApiScope::Telemetry,
            ["events", ..] => ApiScope::Events,
            _ => return false,
        };
        self.scopes.contains(&family)
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

    fn digest(key: &str) -> String {
        Sha256::digest(key.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// One key with a team makes the file multi-tenant, and then a key with
    /// no team would be global by omission: refused, as is a team key with no
    /// principal. Administrators are exempt, and global whatever they say.
    #[test]
    fn a_multi_tenant_file_leaves_no_key_without_a_team() {
        let entry = |key: &str, scope: &str, extra: &str| {
            format!(
                r#"{{"sha256":"{}","expires_at":100,"scopes":["{scope}"]{extra}}}"#,
                digest(key)
            )
        };
        let red = entry(
            "red",
            "sandboxes",
            r#","team_id":"red","principal_id":"alice""#,
        );
        let admin = entry("admin", "admin", r#","team_id":"red""#);
        let loose = entry("loose", "sandboxes", r#","principal_id":"bob""#);
        let anonymous_member = entry("anon", "sandboxes", r#","team_id":"red""#);

        let policies = ApiKeyPolicy::from_json(&format!("[{red},{admin}]")).unwrap();
        assert_eq!(policies[0].team_id().unwrap().as_str(), "red");
        assert!(
            policies[1].team_id().is_none(),
            "an administrator is global"
        );

        for bad in [loose, anonymous_member] {
            assert!(ApiKeyPolicy::from_json(&format!("[{red},{bad}]"))
                .unwrap_err()
                .contains("team_id and principal_id"));
        }
        // Without any team, nothing changes.
        let single = entry("solo", "sandboxes", r#","principal_id":"bob""#);
        assert!(ApiKeyPolicy::from_json(&format!("[{single}]")).unwrap()[0]
            .team_id()
            .is_none());
        assert!(ApiKeyPolicy::from_json(&format!(
            "[{}]",
            entry(
                "x",
                "sandboxes",
                r#","team_id":"has space","principal_id":"p""#
            )
        ))
        .is_err());
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
        let entry = |key: &str, principal: &str| {
            serde_json::json!({
            "sha256": Sha256::digest(key.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
            "expires_at": 100, "scopes": ["sandboxes"], "principal_id": principal})
        };
        let first = ApiKeyPolicy::from_json(
            &serde_json::json!([entry("first-key", "principal-a")]).to_string(),
        )
        .unwrap();
        let rotated = ApiKeyPolicy::from_json(
            &serde_json::json!([entry("rotated-key", "principal-a")]).to_string(),
        )
        .unwrap();
        assert_eq!(first[0].principal_id(), rotated[0].principal_id());
        assert!(!first[0].has_digest(&Sha256::digest(b"rotated-key").into()));
        assert!(rotated[0].has_digest(&Sha256::digest(b"rotated-key").into()));
        for invalid in ["", "a/b", "a b", "secret\n"] {
            let error = ApiKeyPolicy::from_json(
                &serde_json::json!([entry("first-key", invalid)]).to_string(),
            )
            .unwrap_err();
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

    fn preset_policy(extra: &str) -> Result<ApiKeyPolicy, String> {
        ApiKeyPolicy::from_json(&format!(
            r#"[{{"sha256":"{}","expires_at":100,{extra}}}]"#,
            digest("preset-key")
        ))
        .map(|mut policies| policies.remove(0))
    }

    #[test]
    fn each_preset_expands_to_its_scopes_and_role() {
        use ApiScope as S;
        for (name, scopes, role) in [
            ("read-only", vec![S::Inventory], ApiRole::Observer),
            (
                "developer",
                vec![
                    S::Sandboxes,
                    S::Templates,
                    S::Volumes,
                    S::Events,
                    S::Inventory,
                ],
                ApiRole::Operator,
            ),
            (
                "machines",
                vec![S::Machines, S::Inventory, S::Events],
                ApiRole::Operator,
            ),
            ("admin", vec![S::Admin], ApiRole::Operator),
        ] {
            let policy = preset_policy(&format!(r#""preset":"{name}""#)).unwrap();
            assert_eq!(policy.scopes, scopes.into_iter().collect(), "{name}");
            assert_eq!(policy.role, role, "{name}");
        }
        assert!(preset_policy(r#""preset":"admin""#)
            .unwrap()
            .is_administrator());
    }

    #[test]
    fn presets_gate_requests_by_what_they_name() {
        let ro = preset_policy(r#""preset":"read-only""#).unwrap();
        assert!(ro.permits(&Method::GET, "/sandboxes"));
        assert!(ro.permits(&Method::GET, "/cluster/nodes"));
        assert!(!ro.permits(&Method::POST, "/v2/sandboxes"));
        assert!(!ro.permits(&Method::POST, "/sandboxes/box/exec"));
        assert!(!ro.permits(&Method::GET, "/volumes"));
        let dev = preset_policy(r#""preset":"developer""#).unwrap();
        assert!(dev.permits(&Method::POST, "/v2/sandboxes"));
        assert!(dev.permits(&Method::POST, "/volumes"));
        assert!(!dev.permits(&Method::GET, "/machines"));
        let machines = preset_policy(r#""preset":"machines""#).unwrap();
        assert!(machines.permits(&Method::POST, "/machines/web-01/exec"));
        assert!(!machines.permits(&Method::POST, "/v2/sandboxes"));
        assert!(preset_policy(r#""preset":"admin""#)
            .unwrap()
            .permits(&Method::DELETE, "/volumes/id"));
    }

    #[test]
    fn explicit_scopes_are_added_to_a_preset_except_for_read_only() {
        let policy = preset_policy(r#""preset":"machines","scopes":["volumes","events"]"#).unwrap();
        assert_eq!(
            policy.scopes,
            BTreeSet::from([
                ApiScope::Machines,
                ApiScope::Inventory,
                ApiScope::Events,
                ApiScope::Volumes
            ])
        );
        // An explicit observer role lowers a preset's role; it never raises one.
        let lowered = preset_policy(r#""preset":"developer","role":"observer""#).unwrap();
        assert_eq!(lowered.role, ApiRole::Observer);
        assert_eq!(
            preset_policy(r#""preset":"developer","role":"operator""#)
                .unwrap()
                .role,
            ApiRole::Operator
        );
        // Read-only may restate its own scopes or role, and nothing more.
        let same =
            preset_policy(r#""preset":"read-only","scopes":["inventory"],"role":"observer""#)
                .unwrap();
        assert_eq!(same.scopes, ApiPreset::ReadOnly.scopes());
        assert!(
            preset_policy(r#""preset":"read-only","scopes":["sandboxes"]"#)
                .unwrap_err()
                .contains("beyond its own")
        );
        assert!(preset_policy(r#""preset":"read-only","scopes":["admin","events"]"#).is_err());
        assert!(preset_policy(r#""preset":"read-only","scopes":["events"]"#).is_err());
        assert!(preset_policy(r#""preset":"read-only","role":"operator""#)
            .unwrap_err()
            .contains("cannot raise"));
    }

    #[test]
    fn unknown_or_null_presets_are_rejected_and_absent_ones_change_nothing() {
        for bad in [
            r#""preset":"superuser""#,
            r#""preset":"Read-Only""#,
            r#""preset":"read_only""#,
            r#""preset":null"#,
            r#""preset":"""#,
        ] {
            let error = preset_policy(&format!(r#"{bad},"scopes":["events"]"#)).unwrap_err();
            assert!(
                error.contains("invalid API key policy JSON"),
                "{bad}: {error}"
            );
        }
        assert!(preset_policy(r#""scopes":[]"#).is_err());
        assert!(preset_policy(r#""role":"observer""#).is_err());
        let plain = preset_policy(r#""scopes":["sandboxes"]"#).unwrap();
        assert_eq!(plain.scopes, BTreeSet::from([ApiScope::Sandboxes]));
        assert_eq!(plain.role, ApiRole::Operator);
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
        assert!(!policy.permits(&Method::GET, "/machines"));
        assert!(!policy.permits(&Method::POST, "/machines/web-01/exec"));
    }
    #[test]
    fn the_machines_scope_reaches_machines_and_nothing_else() {
        let policy = policy("machines");
        assert!(policy.permits(&Method::GET, "/machines"));
        assert!(policy.permits(&Method::POST, "/machines"));
        assert!(policy.permits(&Method::POST, "/machines/web-01/exec"));
        assert!(policy.permits(&Method::DELETE, "/machines/web-01"));
        assert!(!policy.permits(&Method::GET, "/machines-other"));
        assert!(!policy.permits(&Method::GET, "/sandboxes"));
        assert!(!policy.permits(&Method::GET, "/volumes"));
        // An inventory key lists sandboxes and nodes, not machines: a
        // machine's description carries its network rules.
        assert!(!self::policy("inventory").permits(&Method::GET, "/machines"));
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

    #[test]
    fn team_names_are_aliases_of_the_existing_roles() {
        for (name, role) in [
            ("viewer", ApiRole::Observer),
            ("observer", ApiRole::Observer),
            ("member", ApiRole::Operator),
            ("operator", ApiRole::Operator),
        ] {
            let policy =
                preset_policy(&format!(r#""scopes":["inventory"],"role":"{name}""#)).unwrap();
            assert_eq!(policy.role, role, "{name}");
        }
        let viewer = preset_policy(r#""scopes":["sandboxes"],"role":"viewer""#).unwrap();
        assert!(!viewer.permits(&Method::POST, "/sandboxes/box/exec"));
        let member = preset_policy(r#""scopes":["sandboxes"],"role":"member""#).unwrap();
        assert!(member.permits(&Method::POST, "/sandboxes/box/exec"));
        assert!(!member.permits(&Method::POST, "/sandboxes/box/owner"));
        assert!(!member.is_team_admin());
    }

    #[test]
    fn a_team_admin_needs_a_team_and_never_holds_admin() {
        let team = r#""team_id":"red","principal_id":"lead""#;
        let ok = preset_policy(&format!(
            r#""scopes":["sandboxes"],"role":"team-admin",{team}"#
        ))
        .unwrap();
        assert_eq!(ok.role, ApiRole::TeamAdmin);
        assert!(ok.is_team_admin());
        assert!(!ok.is_administrator());
        assert_eq!(ok.team_id().unwrap().as_str(), "red");
        // A preset may carry it too.
        let dev = preset_policy(&format!(
            r#""preset":"developer","role":"team-admin",{team}"#
        ))
        .unwrap();
        assert!(dev.is_team_admin() && dev.permits(&Method::POST, "/v2/sandboxes"));
        // No team, no team-admin: the whole file is refused.
        for bad in [
            r#""scopes":["sandboxes"],"role":"team-admin""#.to_string(),
            r#""scopes":["sandboxes"],"role":"team-admin","principal_id":"lead""#.to_string(),
            r#""preset":"developer","role":"team-admin""#.to_string(),
        ] {
            assert!(
                preset_policy(&bad).unwrap_err().contains("needs a team_id"),
                "{bad}"
            );
        }
        // The admin scope is cluster-wide, which a team-scoped role is not.
        for bad in [
            format!(r#""scopes":["admin"],"role":"team-admin",{team}"#),
            format!(r#""preset":"admin","role":"team-admin",{team}"#),
            format!(r#""preset":"developer","scopes":["admin"],"role":"team-admin",{team}"#),
        ] {
            assert!(
                preset_policy(&bad).unwrap_err().contains("admin scope"),
                "{bad}"
            );
        }
        // Read-only stays read-only.
        assert!(preset_policy(&format!(
            r#""preset":"read-only","role":"team-admin",{team}"#
        ))
        .unwrap_err()
        .contains("cannot raise"));
        // The same entry in a file with other teams.
        let file = format!(
            r#"[{{"sha256":"{}","expires_at":100,"scopes":["sandboxes"],"role":"team-admin",{team}}},
               {{"sha256":"{}","expires_at":100,"scopes":["sandboxes"]}}]"#,
            digest("a"),
            digest("b")
        );
        assert!(ApiKeyPolicy::from_json(&file)
            .unwrap_err()
            .contains("team_id and principal_id"));
    }

    #[test]
    fn a_team_admin_gains_owner_adoption_and_no_cluster_route() {
        let team = r#""team_id":"red","principal_id":"lead""#;
        let lead = preset_policy(&format!(
            r#""scopes":["sandboxes","inventory"],"role":"team-admin",{team}"#
        ))
        .unwrap();
        let operator =
            preset_policy(&format!(r#""scopes":["sandboxes","inventory"],{team}"#)).unwrap();
        // Everything an operator with the same scopes does...
        for (method, path) in [
            (Method::GET, "/sandboxes"),
            (Method::GET, "/cluster/nodes"),
            (Method::POST, "/v2/sandboxes"),
            (Method::DELETE, "/sandboxes/box"),
            (Method::POST, "/sandboxes/box/exec"),
            (Method::GET, "/volumes"),
            (Method::POST, "/machines/m/exec"),
            (Method::POST, "/templates"),
            (Method::GET, "/cluster/events"),
            (
                Method::POST,
                "/cluster/nodes/n/sandboxes/box/registration/reconcile",
            ),
            (Method::GET, "/cluster/nodes/n/registrations/pending"),
        ] {
            assert_eq!(
                lead.permits(&method, path),
                operator.permits(&method, path),
                "{method} {path}"
            );
        }
        // ...plus owner adoption (POST only), which the operator is refused.
        assert!(lead.permits(&Method::POST, "/sandboxes/box/owner"));
        assert!(!lead.permits(&Method::GET, "/sandboxes/box/owner"));
        assert!(!lead.permits(&Method::DELETE, "/sandboxes/box/owner"));
        assert!(!operator.permits(&Method::POST, "/sandboxes/box/owner"));
        // The scope still gates it, and node routes stay out of reach.
        let inventory = preset_policy(&format!(
            r#""scopes":["inventory"],"role":"team-admin",{team}"#
        ))
        .unwrap();
        assert!(!inventory.permits(&Method::POST, "/sandboxes/box/owner"));
        assert!(!lead.permits(&Method::GET, "/cluster/nodes/n/registrations/pending"));
        assert!(!lead.is_administrator());
    }
}
