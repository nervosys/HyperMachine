//! Validated names and immutable ownership identities for shared reservations.
//! Store transactions and API creation wiring are required before enforcement.
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

/// Case-sensitive CLI-compatible name; never silently normalized.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SandboxName(String);

impl SandboxName {
    /// Validate 1-64 ASCII letters, digits, hyphens, underscores or dots.
    ///
    /// # Errors
    /// Reject empty names, dot segments, non-ASCII and unsupported characters.
    pub fn parse(value: &str) -> Result<Self, String> {
        if value.is_empty()
            || value.len() > 64
            || matches!(value, "." | "..")
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        {
            return Err("sandbox names require 1-64 ASCII letters, digits, - _ or .; dot segments are refused".into());
        }
        Ok(Self(value.into()))
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for SandboxName {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        Self::parse(&value)
    }
}
impl From<SandboxName> for String {
    fn from(value: SandboxName) -> Self {
        value.0
    }
}

/// Pending ownership has no automatic expiry: a failed create may still exist.
/// The token is an operation identity, never an API authentication credential.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawReservation")]
pub struct NameReservation {
    name: SandboxName,
    token: String,
    sandbox_id: Option<String>,
}
impl std::fmt::Debug for NameReservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NameReservation")
            .field("name", &self.name)
            .field("sandbox_id", &self.sandbox_id)
            .finish_non_exhaustive()
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawReservation {
    name: SandboxName,
    token: String,
    sandbox_id: Option<String>,
}
fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
}
impl TryFrom<RawReservation> for NameReservation {
    type Error = String;
    fn try_from(raw: RawReservation) -> Result<Self, String> {
        let token =
            uuid::Uuid::parse_str(&raw.token).map_err(|_| "invalid name reservation token")?;
        if token.get_version_num() != 4
            || token.to_string() != raw.token
            || raw.sandbox_id.as_ref().is_some_and(|id| !valid_id(id))
        {
            return Err("invalid name reservation identity".into());
        }
        Ok(Self {
            name: raw.name,
            token: raw.token,
            sandbox_id: raw.sandbox_id,
        })
    }
}
impl NameReservation {
    #[must_use]
    pub fn pending(name: SandboxName) -> Self {
        Self {
            name,
            token: uuid::Uuid::new_v4().to_string(),
            sandbox_id: None,
        }
    }
    #[must_use]
    pub fn name(&self) -> &SandboxName {
        &self.name
    }
    /// Internal ownership identity. Do not include it in public inventory.
    #[must_use]
    pub fn operation_token(&self) -> &str {
        &self.token
    }
    #[must_use]
    pub fn sandbox_id(&self) -> Option<&str> {
        self.sandbox_id.as_deref()
    }
    /// Bind once, or replay the same binding. The store must additionally check
    /// that the sandbox exists and perform the transition atomically.
    ///
    /// # Errors
    /// Refuse wrong ownership, invalid IDs and transfer to another sandbox.
    pub fn bind(&mut self, token: &str, sandbox_id: &str) -> Result<(), String> {
        if !bool::from(self.token.as_bytes().ct_eq(token.as_bytes()))
            || !valid_id(sandbox_id)
            || self
                .sandbox_id
                .as_ref()
                .is_some_and(|old| old != sandbox_id)
        {
            return Err("name reservation ownership or target mismatch".into());
        }
        self.sandbox_id = Some(sandbox_id.into());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn name_grammar_matches_cli_and_keeps_case() {
        for name in ["a", "Guest-1_test.vm", &"a".repeat(64)] {
            let parsed = SandboxName::parse(name).unwrap();
            assert_eq!(parsed.as_str(), name);
            assert_eq!(
                serde_json::from_str::<SandboxName>(&serde_json::to_string(&parsed).unwrap())
                    .unwrap(),
                parsed
            );
        }
        for name in ["", ".", "..", "a/b", "a b", "é", &"a".repeat(65)] {
            assert!(SandboxName::parse(name).is_err());
        }
        assert_ne!(
            SandboxName::parse("VM").unwrap(),
            SandboxName::parse("vm").unwrap()
        );
    }
    #[test]
    fn ownership_survives_round_trip_and_never_transfers() {
        let mut reservation = NameReservation::pending(SandboxName::parse("guest").unwrap());
        let token = reservation.operation_token().to_owned();
        assert!(!format!("{reservation:?}").contains(&token));
        assert!(reservation.bind("wrong", "vm-1").is_err());
        assert!(reservation.bind(&token, "../bad").is_err());
        assert_eq!(reservation.sandbox_id(), None);
        reservation.bind(&token, "vm-1").unwrap();
        reservation.bind(&token, "vm-1").unwrap();
        assert!(reservation.bind(&token, "vm-2").is_err());
        let mut restored: NameReservation =
            serde_json::from_str(&serde_json::to_string(&reservation).unwrap()).unwrap();
        assert_eq!(restored, reservation);
        assert!(restored.bind("wrong", "vm-1").is_err());
        assert_eq!(restored.sandbox_id(), Some("vm-1"));
        let mut value = serde_json::to_value(&reservation).unwrap();
        value["token"] = "bad".into();
        assert!(serde_json::from_value::<NameReservation>(value).is_err());
        let mut value = serde_json::to_value(&reservation).unwrap();
        value["sandbox_id"] = "../bad".into();
        assert!(serde_json::from_value::<NameReservation>(value).is_err());
    }
}
