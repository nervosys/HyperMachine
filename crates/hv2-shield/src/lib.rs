//! Shields: protections applied to sandboxes that are already running.
//!
//! A shield is a short list of rules, each of which refuses one thing a
//! workload might do: a system call, or a call with particular arguments
//! ([`SyscallRule`]), or a connection to an address, port or name
//! ([`NetworkRule`]). It is meant for the time between a vulnerability being
//! known and a fix being deployed: the code path that exposes it is closed in
//! every sandbox it applies to, and nothing has to be restarted.
//!
//! This crate is the part every enforcement backend shares, and decides
//! nothing about how a rule is enforced:
//!
//! - [`spec`]: what a shield says, and the checks every shield must pass.
//! - [`sign`]: ML-DSA-87 (FIPS 204) signatures over a shield's exact bytes,
//!   checked against a [`Keyring`] of trusted signers, with revocation.
//! - [`registry`]: a node's shields and their life: a shield is proposed,
//!   loaded only with a trusted signature, runs in monitor mode (refusals
//!   are recorded, not made) until an operator enforces it, and is removed
//!   by naming its digest, or expires: every shield has an expiry, at most
//!   [`spec::MAX_LIFETIME_SECS`] after it is loaded.
//! - [`measure`]: every load, mode change, removal and expiry extends a
//!   hash register and is appended to an event log; a signed attestation of
//!   the register over a verifier's nonce lets a remote party check which
//!   shields a node has run, by replaying the log.
//!
//! The attestation key is the node's own, held in its state directory: the
//! evidence is as trustworthy as the host. It is not rooted in a TPM.

pub mod live;
pub mod measure;
pub mod registry;
pub mod sign;
pub mod spec;

pub use live::{Hit, HitSnapshot, Live, Tally};
pub use measure::{verify_attestation, Attestation, Event, EventKind, MeasuredLog};
pub use registry::{Entry, Mode, Registry, State};
pub use sign::{
    Action, Directive, Keyring, KeyringFile, PublicKey, SignedDirective, SignedShield, SigningKey,
};
pub use spec::{
    ArgMatch, ArgOp, Connection, HookRule, NetworkRule, PortRange, Protocol, Scope, ShieldSpec,
    SyscallAction, SyscallRule,
};

/// Why a shield, a signature or an operation was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShieldError {
    #[error("invalid shield: {0}")]
    Invalid(String),
    #[error("signature: {0}")]
    Signature(String),
    #[error("{0}")]
    State(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("key: {0}")]
    Key(String),
}

pub type Result<T> = std::result::Result<T, ShieldError>;

/// SHA-256 of `data`.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    use ic_core::traits::Digest;
    ic_hash::Sha256::digest(data)
}

/// Lowercase hex of `bytes`.
pub fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 15) as usize] as char);
    }
    out
}

/// The bytes of lowercase or uppercase hex, or `None` if it is not hex.
pub fn from_hex(text: &str) -> Option<Vec<u8>> {
    let text = text.as_bytes();
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let nibble = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    text.chunks(2)
        .map(|pair| Some(nibble(pair[0])? << 4 | nibble(pair[1])?))
        .collect()
}

/// Seconds since the Unix epoch.
/// 32 bytes from the operating system's randomness: a verifier's nonce.
///
/// # Errors
///
/// If there is no randomness.
pub fn fresh_nonce() -> Result<[u8; 32]> {
    let mut nonce = [0u8; 32];
    let mut rng = ic_drbg::Rng::from_os().map_err(|_| ShieldError::Key("no randomness".into()))?;
    rng.fill(&mut nonce[..])
        .map_err(|_| ShieldError::Key("no randomness".into()))?;
    Ok(nonce)
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_goes_both_ways_and_refuses_what_is_not_hex() {
        let bytes = [0u8, 1, 0x7f, 0x80, 0xff];
        assert_eq!(to_hex(&bytes), "00017f80ff");
        assert_eq!(from_hex("00017F80ff").unwrap(), bytes);
        for bad in ["0", "zz", "0g", "\u{e9}a"] {
            assert!(from_hex(bad).is_none(), "{bad}");
        }
    }
}
