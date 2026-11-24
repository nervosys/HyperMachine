//! The measured log: what shields a node has run, and how a remote party
//! checks it.
//!
//! Every change to what is in force is an [`Event`], appended to the log and
//! folded into a register: `register = SHA-256(register || SHA-256(event))`,
//! from 32 zero bytes, where an event is hashed as its JSON. The register is
//! a summary no one can reach with a different history, so a verifier who
//! has the log replays it to the register an attestation signs.
//!
//! An [`Attestation`] is the node's ML-DSA-87 signature over a verifier's
//! nonce, the register, how many events it covers, and the time. The nonce
//! is the verifier's, so the evidence cannot have been made before it asked.

use serde::{Deserialize, Serialize};

use crate::sign::{PublicKey, SigningKey};
use crate::{from_hex, sha256, to_hex, Result, ShieldError};

/// FIPS 204 context for an attestation's signature.
pub const ATTESTATION_CONTEXT: &[u8] = b"hypermachine shield attestation v1";
/// The longest nonce a verifier may send.
pub const MAX_NONCE: usize = 64;
/// The shortest: less than this is not a challenge.
pub const MIN_NONCE: usize = 16;

/// What changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EventKind {
    /// Loaded, in monitor mode.
    Load,
    /// Moved to enforcing.
    Enforce,
    /// Moved back to monitoring.
    Monitor,
    /// Removed by an operator.
    Unload,
    /// Removed because it expired.
    Expire,
    /// Removed because its signer or the shield itself was revoked.
    Revoke,
}

/// One change, as it is measured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Event {
    /// From 1, with no gaps.
    pub seq: u64,
    /// Unix seconds.
    pub at: u64,
    pub kind: EventKind,
    pub shield: String,
    /// SHA-256 of the shield's JSON, hex.
    pub digest: String,
    /// The signer's key ID.
    pub signer: String,
    /// Who made the change; none for one the node made itself (expiry).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
}

impl Event {
    fn measurement(&self) -> [u8; 32] {
        // Serialising a struct of strings and integers cannot fail.
        sha256(&serde_json::to_vec(self).unwrap_or_default())
    }
}

/// The log and its register.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MeasuredLog {
    register: [u8; 32],
    events: Vec<Event>,
}

impl MeasuredLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a change, and return the event recorded.
    pub fn extend(
        &mut self,
        at: u64,
        kind: EventKind,
        shield: &str,
        digest: &[u8; 32],
        signer: &str,
        by: Option<&str>,
    ) -> Event {
        let event = Event {
            seq: self.events.len() as u64 + 1,
            at,
            kind,
            shield: shield.to_string(),
            digest: to_hex(digest),
            signer: signer.to_string(),
            by: by.map(str::to_string),
        };
        self.register = fold(&self.register, &event);
        self.events.push(event.clone());
        event
    }

    pub fn register(&self) -> [u8; 32] {
        self.register
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// A log from its events, as kept on disk: replayed, and refused if the
    /// sequence has a gap or `expected` (the register kept beside it) differs.
    ///
    /// # Errors
    ///
    /// If the events are out of sequence, or do not replay to `expected`.
    pub fn restore(events: Vec<Event>, expected: Option<&[u8; 32]>) -> Result<Self> {
        let register = replay(&events)?;
        if let Some(expected) = expected {
            if &register != expected {
                return Err(ShieldError::State(
                    "the shield event log does not replay to its register".into(),
                ));
            }
        }
        Ok(Self { register, events })
    }

    /// Sign the register over `nonce`.
    ///
    /// # Errors
    ///
    /// If the nonce is not 16 to 64 bytes, or signing fails.
    pub fn attest(&self, key: &SigningKey, nonce: &[u8], at: u64) -> Result<Attestation> {
        check_nonce(nonce)?;
        let count = self.events.len() as u64;
        let message = attested(nonce, &self.register, count, at);
        let signature = key.sign(&message, ATTESTATION_CONTEXT)?;
        Ok(Attestation {
            node_key: key.public_key().id(),
            nonce: to_hex(nonce),
            register: to_hex(&self.register),
            events: count,
            at,
            signature: to_hex(&signature),
        })
    }
}

fn fold(register: &[u8; 32], event: &Event) -> [u8; 32] {
    let mut data = [0u8; 64];
    data[..32].copy_from_slice(register);
    data[32..].copy_from_slice(&event.measurement());
    sha256(&data)
}

/// The register `events` produce, if they are in sequence.
///
/// # Errors
///
/// If an event's `seq` is not the one after the last.
pub fn replay(events: &[Event]) -> Result<[u8; 32]> {
    let mut register = [0u8; 32];
    for (index, event) in events.iter().enumerate() {
        if event.seq != index as u64 + 1 {
            return Err(ShieldError::State(format!(
                "event {} is out of sequence (expected {})",
                event.seq,
                index + 1
            )));
        }
        register = fold(&register, event);
    }
    Ok(register)
}

fn check_nonce(nonce: &[u8]) -> Result<()> {
    if (MIN_NONCE..=MAX_NONCE).contains(&nonce.len()) {
        Ok(())
    } else {
        Err(ShieldError::Invalid(format!(
            "a nonce is {MIN_NONCE} to {MAX_NONCE} bytes"
        )))
    }
}

fn attested(nonce: &[u8], register: &[u8; 32], events: u64, at: u64) -> Vec<u8> {
    let mut message = Vec::with_capacity(1 + nonce.len() + 32 + 16);
    // The nonce's length first, so no two (nonce, rest) pairs share bytes.
    message.push(nonce.len() as u8);
    message.extend_from_slice(nonce);
    message.extend_from_slice(register);
    message.extend_from_slice(&events.to_be_bytes());
    message.extend_from_slice(&at.to_be_bytes());
    message
}

/// A node's signed statement of its register, over a verifier's nonce.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Attestation {
    /// The attestation key's ID.
    pub node_key: String,
    pub nonce: String,
    pub register: String,
    /// How many events the register covers.
    pub events: u64,
    pub at: u64,
    pub signature: String,
}

/// Check `attestation` against the node's key, the nonce the verifier sent,
/// and the events the node gave, and return them as verified: the shields
/// that were loaded, changed and removed, in order.
///
/// # Errors
///
/// If the signature, the nonce, or the replay of `events` does not match.
pub fn verify_attestation<'a>(
    attestation: &Attestation,
    node_key: &PublicKey,
    nonce: &[u8],
    events: &'a [Event],
) -> Result<&'a [Event]> {
    let fail = |why: &str| Err(ShieldError::Signature(why.to_string()));
    check_nonce(nonce)?;
    if attestation.node_key != node_key.id() {
        return fail("the attestation is by another key");
    }
    if from_hex(&attestation.nonce).as_deref() != Some(nonce) {
        return fail("the attestation is not over this nonce");
    }
    let Some(register) = from_hex(&attestation.register).and_then(|r| <[u8; 32]>::try_from(r).ok())
    else {
        return fail("the register is not 32 bytes of hex");
    };
    let Some(signature) = from_hex(&attestation.signature) else {
        return fail("the signature is not hex");
    };
    let message = attested(nonce, &register, attestation.events, attestation.at);
    if !node_key.verify(&message, ATTESTATION_CONTEXT, &signature) {
        return fail("the signature does not verify");
    }
    let Ok(count) = usize::try_from(attestation.events) else {
        return fail("more events than this verifier can hold");
    };
    let Some(covered) = events.get(..count) else {
        return fail("fewer events were given than the register covers");
    };
    if replay(covered)? != register {
        return fail("the events do not replay to the register");
    }
    Ok(covered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log() -> MeasuredLog {
        let mut log = MeasuredLog::new();
        log.extend(1, EventKind::Load, "a", &[1; 32], "k", Some("alice"));
        log.extend(2, EventKind::Enforce, "a", &[1; 32], "k", Some("bob"));
        log.extend(3, EventKind::Expire, "a", &[1; 32], "k", None);
        log
    }

    #[test]
    fn an_attestation_verifies_with_its_events_and_nothing_else() {
        let key = SigningKey::generate().unwrap();
        let log = log();
        let nonce = [7u8; 32];
        let evidence = log.attest(&key, &nonce, 10).unwrap();
        let events = log.events();
        let verified = verify_attestation(&evidence, key.public_key(), &nonce, events).unwrap();
        assert_eq!(verified.len(), 3);

        // Another nonce: evidence made before the verifier asked.
        assert!(verify_attestation(&evidence, key.public_key(), &[8u8; 32], events).is_err());
        // Another key.
        let other = SigningKey::generate().unwrap();
        assert!(verify_attestation(&evidence, other.public_key(), &nonce, events).is_err());
        // A history rewritten: an event dropped, changed, or reordered.
        assert!(verify_attestation(&evidence, key.public_key(), &nonce, &events[..2]).is_err());
        let mut changed = events.to_vec();
        changed[1].by = Some("mallory".into());
        assert!(verify_attestation(&evidence, key.public_key(), &nonce, &changed).is_err());
        let mut swapped = events.to_vec();
        swapped.swap(0, 1);
        assert!(verify_attestation(&evidence, key.public_key(), &nonce, &swapped).is_err());
        // The register or the count changed in the evidence.
        let mut forged = evidence.clone();
        forged.events = 2;
        assert!(verify_attestation(&forged, key.public_key(), &nonce, events).is_err());
        // Events after those the evidence covers are not vouched for.
        let mut later = log.clone();
        later.extend(4, EventKind::Load, "b", &[2; 32], "k", Some("alice"));
        let verified =
            verify_attestation(&evidence, key.public_key(), &nonce, later.events()).unwrap();
        assert_eq!(verified.len(), 3);
        // Nonces too short or too long are refused.
        assert!(log.attest(&key, &[0; 15], 1).is_err());
        assert!(log.attest(&key, &[0; 65], 1).is_err());
    }

    #[test]
    fn a_log_is_restored_only_as_it_was() {
        let log = log();
        let events = log.events().to_vec();
        let again = MeasuredLog::restore(events.clone(), Some(&log.register())).unwrap();
        assert_eq!(again, log);
        assert!(
            MeasuredLog::restore(events[1..].to_vec(), None).is_err(),
            "a gap"
        );
        assert!(MeasuredLog::restore(events[..2].to_vec(), Some(&log.register())).is_err());
        assert_eq!(
            MeasuredLog::restore(Vec::new(), None).unwrap().register(),
            [0; 32]
        );
    }
}
