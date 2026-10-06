//! Owner-scoped private network membership and destination authorization.
//!
//! This model does not allocate addresses or route packets. Callers must load
//! current memberships and sandbox records from an authoritative store and
//! recheck at the destination; serialized route claims are not credentials.
use crate::{model::SandboxRecord, ownership::OwnerId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_NETWORKS_PER_SANDBOX: usize = 8;

/// A single DNS-compatible network tag, scoped by trusted owner identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NetworkTag(String);
impl NetworkTag {
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        if value.is_empty()
            || value.len() > 63
            || !value
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || value.starts_with('-')
            || value.ends_with('-')
        {
            return Err("network tag requires 1-63 lowercase DNS-label characters");
        }
        Ok(Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for NetworkTag {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}
impl From<NetworkTag> for String {
    fn from(value: NetworkTag) -> Self {
        value.0
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}
fn valid_generation(value: &str) -> bool {
    uuid::Uuid::parse_str(value)
        .is_ok_and(|id| id.get_version_num() == 4 && id.to_string() == value)
}

/// Generation changes on every membership replacement, including rejoining.
/// Keep this separate from guest identity so stale route claims cannot survive
/// removal and re-addition of a tag on the same running VM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawMembership")]
pub struct NetworkMembership {
    owner_id: OwnerId,
    sandbox_id: String,
    node_id: String,
    generation: String,
    started_at_ms: u64,
    tags: BTreeSet<NetworkTag>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMembership {
    owner_id: OwnerId,
    sandbox_id: String,
    node_id: String,
    generation: String,
    started_at_ms: u64,
    tags: Vec<NetworkTag>,
}
impl TryFrom<RawMembership> for NetworkMembership {
    type Error = &'static str;
    fn try_from(raw: RawMembership) -> Result<Self, Self::Error> {
        if !valid_id(&raw.sandbox_id)
            || !valid_id(&raw.node_id)
            || !valid_generation(&raw.generation)
            || raw.tags.is_empty()
            || raw.tags.len() > MAX_NETWORKS_PER_SANDBOX
        {
            return Err("invalid private network membership");
        }
        let count = raw.tags.len();
        let tags: BTreeSet<_> = raw.tags.into_iter().collect();
        if tags.len() != count {
            return Err("duplicate network tag");
        }
        Ok(Self {
            owner_id: raw.owner_id,
            sandbox_id: raw.sandbox_id,
            node_id: raw.node_id,
            generation: raw.generation,
            started_at_ms: raw.started_at_ms,
            tags,
        })
    }
}
impl NetworkMembership {
    /// Construct only from a trusted owner-bearing sandbox record. Publication
    /// and liveness are checked separately at connection authorization time.
    pub fn new(record: &SandboxRecord, tags: Vec<NetworkTag>) -> Result<Self, &'static str> {
        Self::try_from(RawMembership {
            owner_id: record
                .owner_id
                .clone()
                .ok_or("ownerless sandbox cannot join a private network")?,
            sandbox_id: record.sandbox_id.clone(),
            node_id: record.node_id.clone(),
            generation: uuid::Uuid::new_v4().to_string(),
            started_at_ms: record.started_at_ms,
            tags,
        })
    }
    pub fn generation(&self) -> &str {
        &self.generation
    }
    pub fn tags(&self) -> &BTreeSet<NetworkTag> {
        &self.tags
    }
}

/// Persisted compare-and-swap revision. Removal retains an owner-bound tombstone
/// with a fresh revision, so an old create request cannot revive membership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawMembershipState")]
pub struct NetworkMembershipState {
    owner_id: OwnerId,
    sandbox_id: String,
    node_id: String,
    started_at_ms: u64,
    revision: String,
    membership: Option<NetworkMembership>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMembershipState {
    owner_id: OwnerId,
    sandbox_id: String,
    node_id: String,
    started_at_ms: u64,
    revision: String,
    membership: Option<NetworkMembership>,
}
impl TryFrom<RawMembershipState> for NetworkMembershipState {
    type Error = &'static str;
    fn try_from(raw: RawMembershipState) -> Result<Self, Self::Error> {
        if !valid_id(&raw.sandbox_id)
            || !valid_id(&raw.node_id)
            || !valid_generation(&raw.revision)
            || raw.membership.as_ref().is_some_and(|m| {
                m.owner_id != raw.owner_id
                    || m.sandbox_id != raw.sandbox_id
                    || m.node_id != raw.node_id
                    || m.started_at_ms != raw.started_at_ms
                    || m.generation != raw.revision
            })
        {
            return Err("invalid private membership revision");
        }
        Ok(Self {
            owner_id: raw.owner_id,
            sandbox_id: raw.sandbox_id,
            node_id: raw.node_id,
            started_at_ms: raw.started_at_ms,
            revision: raw.revision,
            membership: raw.membership,
        })
    }
}
impl NetworkMembershipState {
    /// None removes all tags; an empty tag list is refused rather than confused
    /// with removal. The store verifies the current owner and VM incarnation.
    pub fn new(
        record: &SandboxRecord,
        tags: Option<Vec<NetworkTag>>,
    ) -> Result<Self, &'static str> {
        let membership = tags
            .map(|tags| NetworkMembership::new(record, tags))
            .transpose()?;
        let revision = membership
            .as_ref()
            .map(|m| m.generation.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        Self::try_from(RawMembershipState {
            owner_id: record
                .owner_id
                .clone()
                .ok_or("ownerless sandbox cannot join a private network")?,
            sandbox_id: record.sandbox_id.clone(),
            node_id: record.node_id.clone(),
            started_at_ms: record.started_at_ms,
            revision,
            membership,
        })
    }
    /// Stable caller-selected operation revision enables exact retries after an
    /// uncertain HTTP response. It is an identifier, never an authorization token.
    pub fn with_revision(
        record: &SandboxRecord,
        tags: Option<Vec<NetworkTag>>,
        revision: &str,
    ) -> Result<Self, &'static str> {
        if !valid_generation(revision) {
            return Err("invalid private membership revision");
        }
        let mut state = Self::new(record, tags)?;
        state.revision = revision.into();
        if let Some(member) = &mut state.membership {
            member.generation = revision.into();
        }
        Ok(state)
    }
    pub fn revision(&self) -> &str {
        &self.revision
    }
    pub fn membership(&self) -> Option<&NetworkMembership> {
        self.membership.as_ref()
    }
    pub fn owner_id(&self) -> &OwnerId {
        &self.owner_id
    }
    pub fn sandbox_id(&self) -> &str {
        &self.sandbox_id
    }
    pub(crate) fn matches_record(&self, record: &SandboxRecord) -> bool {
        record.owner_id.as_ref() == Some(&self.owner_id)
            && record.sandbox_id == self.sandbox_id
            && record.node_id == self.node_id
            && record.started_at_ms == self.started_at_ms
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MembershipChange {
    Applied,
    RevisionConflict,
    OwnerConflict,
    SandboxMissing,
    RecordChanged,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MembershipAccess<T> {
    Granted(T),
    OwnerConflict,
    SandboxMissing,
}

/// Destination-bound connection context. Transport authentication must supply
/// the source VM identity; a client cannot establish it by sending these fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivateRouteClaim {
    pub source_id: String,
    pub source_generation: String,
    pub destination_id: String,
    pub destination_generation: String,
    pub network: NetworkTag,
    pub port: u16,
}

/// One atomic shared-store view of both endpoints. Do not assemble this view
/// from independent sandbox and membership reads: those can span revocation,
/// migration or VM replacement. Local publication state remains a node check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateRouteSnapshot {
    pub source_record: SandboxRecord,
    pub destination_record: SandboxRecord,
    source_membership: NetworkMembership,
    destination_membership: NetworkMembership,
}
impl PrivateRouteSnapshot {
    pub(crate) fn checked(
        source_id: &str,
        destination_id: &str,
        source_record: SandboxRecord,
        destination_record: SandboxRecord,
        source_state: NetworkMembershipState,
        destination_state: NetworkMembershipState,
    ) -> Option<Self> {
        if source_record.sandbox_id != source_id
            || destination_record.sandbox_id != destination_id
            || !source_state.matches_record(&source_record)
            || !destination_state.matches_record(&destination_record)
            || source_state.owner_id != destination_state.owner_id
        {
            return None;
        }
        Some(Self {
            source_record,
            destination_record,
            source_membership: source_state.membership?,
            destination_membership: destination_state.membership?,
        })
    }
    /// Make a claim only for a network shared by both current memberships.
    /// This is connection context, not a credential or proof of node liveness.
    pub fn claim(
        &self,
        network: NetworkTag,
        port: u16,
        now_ms: u64,
        source_pending: bool,
        destination_pending: bool,
    ) -> Result<PrivateRouteClaim, &'static str> {
        let claim = PrivateRouteClaim {
            source_id: self.source_record.sandbox_id.clone(),
            source_generation: self.source_membership.generation.clone(),
            destination_id: self.destination_record.sandbox_id.clone(),
            destination_generation: self.destination_membership.generation.clone(),
            network,
            port,
        };
        self.authorize(
            &self.source_record.sandbox_id,
            &claim,
            now_ms,
            source_pending,
            destination_pending,
        )?;
        Ok(claim)
    }
    /// Recheck a previously issued claim at the destination against a fresh
    /// atomic view and actual local publication state, before opening the port.
    pub fn authorize(
        &self,
        authenticated_source_id: &str,
        claim: &PrivateRouteClaim,
        now_ms: u64,
        source_pending: bool,
        destination_pending: bool,
    ) -> Result<(), &'static str> {
        authorize_private_route(
            authenticated_source_id,
            claim,
            NetworkEndpoint {
                membership: &self.source_membership,
                record: &self.source_record,
                registration_pending: source_pending,
            },
            NetworkEndpoint {
                membership: &self.destination_membership,
                record: &self.destination_record,
                registration_pending: destination_pending,
            },
            now_ms,
        )
    }
}

/// An authoritative view of one endpoint, not a client-supplied JSON shape.
/// A store read failure must prevent constructing this view. In particular a
/// committed shared record alone does not clear uncertain local registration.
pub struct NetworkEndpoint<'a> {
    pub membership: &'a NetworkMembership,
    pub record: &'a SandboxRecord,
    pub registration_pending: bool,
}
impl NetworkEndpoint<'_> {
    fn active(&self, now_ms: u64) -> bool {
        let m = self.membership;
        let r = self.record;
        !self.registration_pending
            && !r.paused
            && r.end_at_ms > now_ms
            && r.started_at_ms <= now_ms
            && r.owner_id.as_ref() == Some(&m.owner_id)
            && r.started_at_ms == m.started_at_ms
            && r.sandbox_id == m.sandbox_id
            && r.node_id == m.node_id
    }
}

/// Check both current endpoints against an authenticated source identity and
/// the exact requested destination generation. No metadata field grants access.
/// Call again for each new connection, and revoke existing streams separately.
pub fn authorize_private_route(
    authenticated_source_id: &str,
    claim: &PrivateRouteClaim,
    source: NetworkEndpoint<'_>,
    destination: NetworkEndpoint<'_>,
    now_ms: u64,
) -> Result<(), &'static str> {
    let s = source.membership;
    let d = destination.membership;
    if claim.port == 0
        || authenticated_source_id != s.sandbox_id
        || claim.source_id != s.sandbox_id
        || claim.destination_id != d.sandbox_id
        || claim.source_generation != s.generation
        || claim.destination_generation != d.generation
        || s.owner_id != d.owner_id
        || !s.tags.contains(&claim.network)
        || !d.tags.contains(&claim.network)
        || !source.active(now_ms)
        || !destination.active(now_ms)
    {
        return Err("private route refused");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(id: &str, owner: &str) -> SandboxRecord {
        SandboxRecord {
            sandbox_id: id.into(),
            owner_id: Some(OwnerId::parse(owner).unwrap()),
            node_id: "node-a".into(),
            template_id: "test".into(),
            started_at_ms: 1,
            end_at_ms: 100,
            cpu_count: 1,
            memory_mb: 1024,
            metadata: Default::default(),
            envd_version: "test".into(),
            descriptor: serde_json::json!({}),
            paused: false,
            portable: false,
            volume_mounts: vec![],
        }
    }
    fn member(r: &SandboxRecord, tag: &str) -> NetworkMembership {
        NetworkMembership::new(r, vec![NetworkTag::parse(tag).unwrap()]).unwrap()
    }
    fn claim(s: &NetworkMembership, d: &NetworkMembership) -> PrivateRouteClaim {
        PrivateRouteClaim {
            source_id: s.sandbox_id.clone(),
            source_generation: s.generation.clone(),
            destination_id: d.sandbox_id.clone(),
            destination_generation: d.generation.clone(),
            network: NetworkTag::parse("team").unwrap(),
            port: 8080,
        }
    }
    fn allowed(
        c: &PrivateRouteClaim,
        s: &NetworkMembership,
        sr: &SandboxRecord,
        d: &NetworkMembership,
        dr: &SandboxRecord,
        pending: bool,
    ) -> bool {
        authorize_private_route(
            "source",
            c,
            NetworkEndpoint {
                membership: s,
                record: sr,
                registration_pending: false,
            },
            NetworkEndpoint {
                membership: d,
                record: dr,
                registration_pending: pending,
            },
            50,
        )
        .is_ok()
    }

    #[test]
    fn membership_state_rejects_mismatched_nested_identity_and_incarnation() {
        let r = record("source", "alice");
        let state = NetworkMembershipState::new(&r, Some(vec![NetworkTag::parse("team").unwrap()]))
            .unwrap();
        let value = serde_json::to_value(&state).unwrap();
        assert_eq!(
            serde_json::from_value::<NetworkMembershipState>(value.clone()).unwrap(),
            state
        );
        for (key, replacement) in [
            ("owner_id", serde_json::json!("bob")),
            ("sandbox_id", serde_json::json!("replacement")),
            ("node_id", serde_json::json!("node-b")),
            ("started_at_ms", serde_json::json!(2)),
            (
                "generation",
                serde_json::json!(uuid::Uuid::new_v4().to_string()),
            ),
        ] {
            let mut invalid = value.clone();
            invalid["membership"][key] = replacement;
            assert!(
                serde_json::from_value::<NetworkMembershipState>(invalid).is_err(),
                "{key}"
            );
        }
        let mut invalid = value.clone();
        invalid["revision"] = serde_json::json!("not-a-generation");
        assert!(serde_json::from_value::<NetworkMembershipState>(invalid).is_err());
        let mut invalid = value;
        invalid["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<NetworkMembershipState>(invalid).is_err());
        let removed = NetworkMembershipState::new(&r, None).unwrap();
        assert_ne!(removed.revision(), state.revision());
        assert!(removed.membership().is_none());
        let m = state.membership().unwrap();
        let mut reused = r.clone();
        reused.started_at_ms = 2;
        assert!(
            !NetworkEndpoint {
                membership: m,
                record: &reused,
                registration_pending: false
            }
            .active(50)
        );
    }
    #[test]
    fn tags_are_dns_labels_and_membership_wire_shape_is_strict() {
        for bad in ["", "TEAM", "a.b", "-a", "a-", "a/b", "é"] {
            assert!(NetworkTag::parse(bad).is_err());
        }
        assert!(NetworkTag::parse(&"a".repeat(63)).is_ok());
        assert!(NetworkTag::parse(&"a".repeat(64)).is_err());
        let r = record("source", "alice");
        let m = member(&r, "team");
        let value = serde_json::to_value(&m).unwrap();
        assert_eq!(
            serde_json::from_value::<NetworkMembership>(value.clone()).unwrap(),
            m
        );
        for (key, replacement) in [
            (
                "generation",
                serde_json::json!("00000000-0000-0000-0000-000000000000"),
            ),
            ("sandbox_id", serde_json::json!("../source")),
            ("node_id", serde_json::json!("")),
            ("tags", serde_json::json!([])),
            ("tags", serde_json::json!(["team", "team"])),
            (
                "tags",
                serde_json::json!(["a", "b", "c", "d", "e", "f", "g", "h", "i"]),
            ),
            ("unknown", serde_json::json!(true)),
        ] {
            let mut invalid = value.clone();
            invalid[key] = replacement;
            assert!(
                serde_json::from_value::<NetworkMembership>(invalid).is_err(),
                "{key}"
            );
        }
        let mut legacy = r;
        legacy.owner_id = None;
        legacy.metadata.insert("owner_id".into(), "alice".into());
        assert!(NetworkMembership::new(&legacy, vec![NetworkTag::parse("team").unwrap()]).is_err());
    }
    #[test]
    fn same_tag_never_grants_cross_owner_or_cross_network_access() {
        let s = record("source", "alice");
        let d = record("destination", "alice");
        let sm = member(&s, "team");
        let dm = member(&d, "team");
        let c = claim(&sm, &dm);
        assert!(allowed(&c, &sm, &s, &dm, &d, false));
        let other = record("destination", "bob");
        let other_m = member(&other, "team");
        assert!(!allowed(
            &claim(&sm, &other_m),
            &sm,
            &s,
            &other_m,
            &other,
            false
        ));
        let other_m = member(&d, "other");
        assert!(!allowed(
            &claim(&sm, &other_m),
            &sm,
            &s,
            &other_m,
            &d,
            false
        ));
    }
    #[test]
    fn rejoining_or_replacing_a_destination_invalidates_old_claims() {
        let s = record("source", "alice");
        let d = record("destination", "alice");
        let sm = member(&s, "team");
        let dm = member(&d, "team");
        let c = claim(&sm, &dm);
        let rejoined = member(&d, "team");
        assert!(!allowed(&c, &sm, &s, &rejoined, &d, false));
        let replaced = record("replacement", "alice");
        let rm = member(&replaced, "team");
        assert!(!allowed(&c, &sm, &s, &rm, &replaced, false));
        let source_rejoined = member(&s, "team");
        assert!(!allowed(&c, &source_rejoined, &s, &dm, &d, false));
    }
    #[test]
    fn pending_paused_expired_migrated_and_changed_owner_endpoints_are_refused() {
        let s = record("source", "alice");
        let d = record("destination", "alice");
        let sm = member(&s, "team");
        let dm = member(&d, "team");
        let c = claim(&sm, &dm);
        assert!(!allowed(&c, &sm, &s, &dm, &d, true));
        for source_side in [false, true] {
            for case in 0..6 {
                let mut changed = if source_side { s.clone() } else { d.clone() };
                match case {
                    0 => changed.paused = true,
                    1 => changed.end_at_ms = 50,
                    2 => changed.node_id = "node-b".into(),
                    3 => changed.owner_id = Some(OwnerId::parse("bob").unwrap()),
                    4 => changed.owner_id = None,
                    _ => changed.started_at_ms = 51,
                }
                let (sr, dr) = if source_side {
                    (&changed, &d)
                } else {
                    (&s, &changed)
                };
                assert!(
                    !allowed(&c, &sm, sr, &dm, dr, false),
                    "{source_side}/{case}"
                );
            }
        }
        assert!(
            authorize_private_route(
                "source",
                &c,
                NetworkEndpoint {
                    membership: &sm,
                    record: &s,
                    registration_pending: true
                },
                NetworkEndpoint {
                    membership: &dm,
                    record: &d,
                    registration_pending: false
                },
                50
            )
            .is_err()
        );
    }
    #[test]
    fn forged_source_and_zero_port_are_refused() {
        let s = record("source", "alice");
        let d = record("destination", "alice");
        let sm = member(&s, "team");
        let dm = member(&d, "team");
        let mut c = claim(&sm, &dm);
        assert!(
            authorize_private_route(
                "intruder",
                &c,
                NetworkEndpoint {
                    membership: &sm,
                    record: &s,
                    registration_pending: false
                },
                NetworkEndpoint {
                    membership: &dm,
                    record: &d,
                    registration_pending: false
                },
                50
            )
            .is_err()
        );
        c.port = 0;
        assert!(!allowed(&c, &sm, &s, &dm, &d, false));
        c.port = 8080;
        c.source_id = "intruder".into();
        assert!(!allowed(&c, &sm, &s, &dm, &d, false));
    }
}
