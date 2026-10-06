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
        if value.is_empty() || value.len() > 128 || !value.bytes().all(|b|
            b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')) {
            return Err("owner ID must be a bounded opaque ASCII principal");
        }
        Ok(Self(value.to_owned()))
    }
    pub fn as_str(&self) -> &str { &self.0 }
}
impl TryFrom<String> for OwnerId {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> { Self::parse(&value) }
}
impl From<OwnerId> for String {
    fn from(value: OwnerId) -> Self { value.0 }
}

/// Atomic legacy adoption; never transfers an existing owner or reservations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerAdoption { Adopted, AlreadyOwned, OwnerConflict, PortsPresent, SandboxMissing }

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
        assert_eq!(serde_json::from_str::<OwnerId>(&serde_json::to_string(&id).unwrap()).unwrap(), id);
        assert!(OwnerId::parse(&"a".repeat(128)).is_ok());
    }
}
