//! Owner-bound browser grants. Credentials are authenticated separately.
//! Empty grants retain a revision tombstone; stores must enforce atomic CAS.
use crate::{model::SandboxRecord, ownership::OwnerId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_WEB_GRANTS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawGrant")]
pub struct WebGrant {
    subject: String,
    expires_at: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGrant { subject: String, expires_at: i64 }
impl TryFrom<RawGrant> for WebGrant {
    type Error = &'static str;
    fn try_from(raw: RawGrant) -> Result<Self, Self::Error> {
        if raw.subject.is_empty() || raw.subject.len() > 128
            || !raw.subject.bytes().all(|b| b.is_ascii_alphanumeric() || b"._@+-".contains(&b))
            || raw.expires_at <= 0 {
            return Err("invalid browser grant subject or expiry");
        }
        Ok(Self { subject: raw.subject, expires_at: raw.expires_at })
    }
}
impl WebGrant {
    pub fn new(subject: &str, expires_at: i64) -> Result<Self, &'static str> {
        Self::try_from(RawGrant { subject: subject.into(), expires_at })
    }
    pub fn subject(&self) -> &str { &self.subject }
    pub fn expires_at(&self) -> i64 { self.expires_at }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawState")]
pub struct WebSharingState {
    owner_id: OwnerId,
    sandbox_id: String,
    started_at_ms: u64,
    revision: String,
    grants: Vec<WebGrant>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawState {
    owner_id: OwnerId,
    sandbox_id: String,
    started_at_ms: u64,
    revision: String,
    grants: Vec<WebGrant>,
}
impl TryFrom<RawState> for WebSharingState {
    type Error = &'static str;
    fn try_from(mut raw: RawState) -> Result<Self, Self::Error> {
        if raw.sandbox_id.is_empty() || raw.sandbox_id.len() > 128
            || !raw.sandbox_id.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
            || !uuid::Uuid::parse_str(&raw.revision).is_ok_and(|id| id.get_version_num() == 4 && id.to_string() == raw.revision)
            || raw.grants.len() > MAX_WEB_GRANTS {
            return Err("invalid browser sharing state");
        }
        let subjects: BTreeSet<_> = raw.grants.iter().map(|g| g.subject.as_str()).collect();
        if subjects.len() != raw.grants.len() { return Err("duplicate browser grant subject"); }
        raw.grants.sort_by(|a,b| a.subject.cmp(&b.subject));
        Ok(Self { owner_id: raw.owner_id, sandbox_id: raw.sandbox_id,
            started_at_ms: raw.started_at_ms, revision: raw.revision, grants: raw.grants })
    }
}
impl WebSharingState {
    /// The caller must supply the authoritative record, never a client claim.
    pub fn new(record: &SandboxRecord, grants: Vec<WebGrant>) -> Result<Self, &'static str> {
        Self::with_revision(record, grants, &uuid::Uuid::new_v4().to_string())
    }
    /// Caller-supplied canonical revision makes exact API retries idempotent.
    pub fn with_revision(record: &SandboxRecord, grants: Vec<WebGrant>, revision: &str) -> Result<Self, &'static str> {
        Self::try_from(RawState {
            owner_id: record.owner_id.clone().ok_or("ownerless sandbox cannot be shared")?,
            sandbox_id: record.sandbox_id.clone(), started_at_ms: record.started_at_ms,
            revision: revision.into(), grants,
        })
    }
    pub fn owner_id(&self) -> &OwnerId { &self.owner_id }
    pub fn sandbox_id(&self) -> &str { &self.sandbox_id }
    pub fn revision(&self) -> &str { &self.revision }
    pub fn grants(&self) -> &[WebGrant] { &self.grants }
    /// Node placement is not guest incarnation; pause/resume may move a guest.
    pub fn matches_record(&self, record: &SandboxRecord) -> bool {
        record.owner_id.as_ref() == Some(&self.owner_id)
            && record.sandbox_id == self.sandbox_id && record.started_at_ms == self.started_at_ms
    }
    pub fn allows(&self, record: &SandboxRecord, subject: &str, now: i64) -> bool {
        now >= 0 && self.matches_record(record)
            && self.grants.iter().any(|g| g.subject == subject && now < g.expires_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(grants: serde_json::Value) -> serde_json::Value {
        serde_json::json!({"owner_id":"owner", "sandbox_id":"guest", "started_at_ms":9007199254740993u64,
            "revision":"12345678-1234-4234-8234-123456789abc", "grants":grants})
    }
    #[test]
    fn grant_validation_and_unknown_fields_fail_closed() {
        for subject in ["", "*", "a/b", "a b", "é"] { assert!(WebGrant::new(subject, 1).is_err()); }
        assert!(WebGrant::new(&"a".repeat(129), 1).is_err());
        for expiry in [0, -1] { assert!(WebGrant::new("alice", expiry).is_err()); }
        assert!(serde_json::from_value::<WebGrant>(serde_json::json!({"subject":"alice","expires_at":1,"admin":true})).is_err());
    }
    #[test]
    fn tombstone_roundtrip_preserves_exact_incarnation_and_revision() {
        let value = state(serde_json::json!([]));
        let parsed: WebSharingState = serde_json::from_value(value.clone()).unwrap();
        assert!(parsed.grants().is_empty());
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
    }
    #[test]
    fn duplicate_oversized_and_malformed_states_are_rejected() {
        let grant = serde_json::json!({"subject":"alice","expires_at":100});
        assert!(serde_json::from_value::<WebSharingState>(state(serde_json::json!([grant,grant]))).is_err());
        let many: Vec<_> = (0..257).map(|i| serde_json::json!({"subject":format!("user{i}"),"expires_at":100})).collect();
        assert!(serde_json::from_value::<WebSharingState>(state(serde_json::json!(many))).is_err());
        for revision in ["bad", "12345678-1234-1234-8234-123456789abc", "12345678-1234-4234-8234-123456789ABC"] {
            let mut value = state(serde_json::json!([])); value["revision"] = revision.into();
            assert!(serde_json::from_value::<WebSharingState>(value).is_err());
        }
        let mut value = state(serde_json::json!([])); value["unexpected"] = true.into();
        assert!(serde_json::from_value::<WebSharingState>(value).is_err());
    }
    #[test]
    fn authorization_binds_owner_incarnation_subject_and_expiry() {
        let mut record: SandboxRecord = serde_json::from_value(serde_json::json!({
            "sandbox_id":"guest", "owner_id":"owner", "node_id":"node-a", "template_id":"base",
            "started_at_ms":9007199254740993u64, "end_at_ms":9007199254741993u64,
            "cpu_count":1,"memory_mb":128,"envd_version":"test","descriptor":{}
        })).unwrap();
        let sharing = WebSharingState::new(&record, vec![WebGrant::new("alice", 100).unwrap()]).unwrap();
        assert!(sharing.allows(&record, "alice", 99));
        for now in [-1, 100, 101] { assert!(!sharing.allows(&record, "alice", now)); }
        assert!(!sharing.allows(&record, "bob", 99));
        record.node_id = "node-b".into();
        record.paused = true;
        assert!(sharing.allows(&record, "alice", 99));
        record.started_at_ms += 1;
        assert!(!sharing.allows(&record, "alice", 99));
        record.started_at_ms -= 1;
        record.owner_id = Some(OwnerId::parse("other").unwrap());
        assert!(!sharing.allows(&record, "alice", 99));
        record.owner_id = None;
        assert!(!sharing.allows(&record, "alice", 99));
        assert!(WebSharingState::new(&record, vec![]).is_err());
        record.owner_id = Some(OwnerId::parse("owner").unwrap());
        record.sandbox_id = "replacement".into();
        assert!(!sharing.allows(&record, "alice", 99));
        let revoked = WebSharingState::new(&record, vec![]).unwrap();
        assert!(!revoked.allows(&record, "alice", 99));
        assert_ne!(sharing.revision(), revoked.revision());
    }
    #[test]
    fn grant_order_is_canonical() {
        let value = state(serde_json::json!([{"subject":"z","expires_at":2},{"subject":"a","expires_at":1}]));
        let parsed: WebSharingState = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.grants()[0].subject(), "a");
    }
}

/// Outcome of an owner-authorized atomic sharing update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharingChange { Applied, RevisionConflict, OwnerConflict, SandboxMissing, RecordChanged }
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SharingAccess<T> { Granted(T), OwnerConflict, SandboxMissing }
