//! Trusted operator-assigned sandbox owner IDs, independent of API credentials.
use serde::{Deserialize, Serialize};

/// Internal control-to-node header. Client headers never establish ownership.
pub const OWNER_HEADER: &str = "x-hv2-sandbox-owner";

/// Opaque stable principal. Operators must use an identity label, never a secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct OwnerId(String);
impl OwnerId {
    /// Reject empty, non-ASCII, decorated and oversized principal labels.
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        // `.` and `..` are labels by the alphabet but path segments by every
        // URL parser: a team or principal in a URL would climb out of it.
        if value.is_empty()
            || value.len() > 128
            || matches!(value, "." | "..")
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        {
            return Err("owner ID must be a bounded opaque ASCII principal");
        }
        Ok(Self(value.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for OwnerId {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}
impl From<OwnerId> for String {
    fn from(value: OwnerId) -> Self {
        value.0
    }
}

/// Internal control-to-node header carrying the creating key's team. Like
/// [`OWNER_HEADER`], set only by an authenticated control plane.
pub const TEAM_HEADER: &str = "x-hv2-sandbox-team";

/// The team a key and the sandboxes it creates belong to: the isolation
/// boundary between tenants. An operator-assigned label, under the same rules
/// as an [`OwnerId`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TeamId(String);
impl TeamId {
    /// Reject empty, non-ASCII, decorated and oversized team labels.
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        OwnerId::parse(value)
            .map(|owner| Self(owner.0))
            .map_err(|_| "team ID must be a bounded opaque ASCII label")
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The scope of a key that has a principal and no team: a team of one.
    /// Everything such a key creates -- events, webhooks, machines, volumes,
    /// snapshots, environments, telemetry -- is recorded under it, so the
    /// team checks that fence tenants fence principals as well. Derived from
    /// the principal by a hash, so the label is a valid team and reveals
    /// nothing a team label could not.
    #[must_use]
    pub fn personal(owner: &OwnerId) -> Self {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(owner.as_str().as_bytes());
        let hex: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
        Self(format!("{PERSONAL_PREFIX}{hex}"))
    }

    /// Whether this is a principal's own scope ([`Self::personal`]). A real
    /// team may not use such a label: it would be that principal's.
    #[must_use]
    pub fn is_personal(&self) -> bool {
        self.0.starts_with(PERSONAL_PREFIX)
    }
}

/// What every [`TeamId::personal`] label starts with.
const PERSONAL_PREFIX: &str = "principal.";

/// What the name of a template built under a principal's scope starts with.
/// Nodes keep one template namespace for the whole node, with no owner on a
/// build; the control plane puts the scope in the name instead, and lets no
/// one but that scope (and an administrator) name such a template.
pub const TEMPLATE_SCOPE_PREFIX: &str = "scope.";

impl TeamId {
    /// The prefix of every template this scope builds -- `scope.`, sixteen hex
    /// digits of the principal's hash, and `--` -- or `None` for a real team,
    /// which builds none. It takes 24 of a node's 64 template-name
    /// characters. It is not a secret: the control plane refuses any name
    /// under a scope that is not the caller's own.
    #[must_use]
    pub fn template_prefix(&self) -> Option<String> {
        let hex = self.0.strip_prefix(PERSONAL_PREFIX)?;
        Some(format!(
            "{TEMPLATE_SCOPE_PREFIX}{}--",
            &hex[..hex.len().min(16)]
        ))
    }
}

impl TryFrom<String> for TeamId {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}
impl From<TeamId> for String {
    fn from(value: TeamId) -> Self {
        value.0
    }
}

/// The team a request acts for, set by the control plane's authentication
/// for routes served outside it (events and webhooks). `None` is every team:
/// an administrator, or a deployment without teams. A key with a principal
/// and no team acts for [`TeamId::personal`], so it sees only what it made.
/// Absent altogether on a
/// node, which serves those routes to its control plane only.
#[derive(Debug, Clone)]
pub struct RequestTeam(pub Option<TeamId>);

impl RequestTeam {
    /// Whether something belonging to `team` is visible to this request.
    #[must_use]
    pub fn sees(&self, team: Option<&TeamId>) -> bool {
        self.0.as_ref().is_none_or(|mine| team == Some(mine))
    }

    /// The prefix of the templates this request builds: a principal's scope.
    #[must_use]
    pub fn template_prefix(&self) -> Option<String> {
        self.0.as_ref().and_then(TeamId::template_prefix)
    }

    /// Whether this request may name `name` (a template name, untagged): any
    /// name that is not under a principal's scope, and under one only its
    /// own. An administrator names every one.
    #[must_use]
    pub fn may_name_template(&self, name: &str) -> bool {
        !name.starts_with(TEMPLATE_SCOPE_PREFIX)
            || self.0.is_none()
            || self
                .template_prefix()
                .is_some_and(|prefix| name.starts_with(&prefix))
    }

    /// The template `requested` means to this request, for starting a
    /// sandbox or a build from it: a principal's own template of that name
    /// when `exists` says there is one, else the name as given. `None` for a
    /// name under someone else's scope.
    pub fn resolve_template(
        &self,
        requested: &str,
        exists: impl Fn(&str) -> bool,
    ) -> Option<String> {
        let name = crate::model::untagged(requested);
        if !self.may_name_template(name) {
            return None;
        }
        if let Some(prefix) = self.template_prefix() {
            if !name.starts_with(&prefix) {
                let scoped = format!("{prefix}{name}");
                if exists(&scoped) {
                    return Some(scoped);
                }
            }
        }
        Some(name.to_owned())
    }

    /// The name a build `requested` is made under: the principal's scope
    /// before it. `None` for a name under someone else's scope.
    #[must_use]
    pub fn build_template_name(&self, requested: &str) -> Option<String> {
        let name = crate::model::untagged(requested);
        if !self.may_name_template(name) {
            return None;
        }
        Some(match self.template_prefix() {
            Some(prefix) if !name.starts_with(&prefix) => format!("{prefix}{name}"),
            _ => name.to_owned(),
        })
    }

    /// Whether a template belonging to `owner` may be used, listed or
    /// started from: an operator's template (no owner) by every team, a
    /// team's snapshot by that team alone.
    #[must_use]
    pub fn may_use_template(&self, owner: Option<&TeamId>) -> bool {
        owner.is_none() || self.sees(owner)
    }

    /// As [`Self::may_use_template`], and also a team its owner shared it
    /// with: `shared_with` lists them.
    #[must_use]
    pub fn may_use_shared_template(&self, owner: Option<&TeamId>, shared_with: &[TeamId]) -> bool {
        self.may_use_template(owner) || self.0.as_ref().is_some_and(|me| shared_with.contains(me))
    }
}

/// Atomic legacy adoption; never transfers an existing owner or reservations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerAdoption {
    Adopted,
    AlreadyOwned,
    OwnerConflict,
    PortsPresent,
    SandboxMissing,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owner_labels_are_bounded_and_validated_on_deserialization() {
        for value in ["", ".", "..", "alice@example.com", "a/b", "a b", "a\n", "é"] {
            assert!(OwnerId::parse(value).is_err());
            assert!(serde_json::from_value::<OwnerId>(serde_json::json!(value)).is_err());
        }
        assert!(OwnerId::parse(&"a".repeat(129)).is_err());
        let id = OwnerId::parse("team.user-1_2").unwrap();
        assert_eq!(
            serde_json::from_str::<OwnerId>(&serde_json::to_string(&id).unwrap()).unwrap(),
            id
        );
        assert!(OwnerId::parse(&"a".repeat(128)).is_ok());
    }

    #[test]
    fn a_shared_template_may_be_used_by_the_teams_it_was_shared_with() {
        let team = |t: &str| TeamId::parse(t).unwrap();
        let red = team("red");
        let shared = [team("blue")];
        // An operator's template is every team's, shared or not.
        assert!(RequestTeam(Some(team("green"))).may_use_shared_template(None, &[]));
        // A team's is its own, and a team it was shared with, and no other.
        assert!(RequestTeam(Some(red.clone())).may_use_shared_template(Some(&red), &shared));
        assert!(RequestTeam(Some(team("blue"))).may_use_shared_template(Some(&red), &shared));
        assert!(!RequestTeam(Some(team("green"))).may_use_shared_template(Some(&red), &shared));
        assert!(!RequestTeam(Some(team("blue"))).may_use_shared_template(Some(&red), &[]));
        // A request with no team is an administrator's, and sees everything.
        assert!(RequestTeam(None).may_use_shared_template(Some(&red), &[]));
    }
}
