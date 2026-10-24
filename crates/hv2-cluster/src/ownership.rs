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
        if value.is_empty()
            || value.len() > 128
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
/// an administrator, or a deployment without teams. Absent altogether on a
/// node, which serves those routes to its control plane only.
#[derive(Debug, Clone)]
pub struct RequestTeam(pub Option<TeamId>);

impl RequestTeam {
    /// Whether something belonging to `team` is visible to this request.
    #[must_use]
    pub fn sees(&self, team: Option<&TeamId>) -> bool {
        self.0.as_ref().is_none_or(|mine| team == Some(mine))
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
        for value in ["", "alice@example.com", "a/b", "a b", "a\n", "é"] {
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
