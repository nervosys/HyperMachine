//! Stable raw-port reservations. Socket publication and API authorization are separate.
use crate::store::{Result, StoreError};
use serde::{Deserialize, Serialize};

pub const MAX_PORTS_PER_SANDBOX: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortProtocol {
    Tcp,
    Udp,
    Both,
}
impl PortProtocol {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
            Self::Both => "both",
        }
    }
}

/// A bounded operator-configured allocation window; existing reservations retain their port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicPortRange {
    first: u16,
    last: u16,
}
impl PublicPortRange {
    /// # Errors
    /// Rejects port zero, reversed ranges and windows larger than 4096 ports.
    pub fn new(first: u16, last: u16) -> Result<Self> {
        if first == 0 || last < first || u32::from(last) - u32::from(first) >= 4096 {
            return Err(StoreError(
                "public port range must contain 1–4096 nonzero ports".into(),
            ));
        }
        Ok(Self { first, last })
    }
    pub fn first(self) -> u16 {
        self.first
    }
    pub fn last(self) -> u16 {
        self.last
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortAllocation {
    pub(crate) sandbox_id: String,
    pub(crate) machine_port: u16,
    pub(crate) public_port: u16,
    pub(crate) owner_id: String,
    pub(crate) protocol: PortProtocol,
}
impl PortAllocation {
    pub fn sandbox_id(&self) -> &str {
        &self.sandbox_id
    }
    pub fn machine_port(&self) -> u16 {
        self.machine_port
    }
    pub fn public_port(&self) -> u16 {
        self.public_port
    }
    /// Opaque principal identifier, never a plaintext credential.
    pub fn owner_id(&self) -> &str {
        &self.owner_id
    }
    pub fn protocol(&self) -> PortProtocol {
        self.protocol
    }
    pub(crate) fn validate(&self) -> Result<()> {
        validate_request(&self.sandbox_id, self.machine_port, &self.owner_id)?;
        if self.public_port == 0 {
            return Err(StoreError("invalid public port record".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortClaim {
    Allocated(PortAllocation),
    OwnerConflict,
    SandboxMissing,
    LimitReached,
    PoolExhausted,
}

pub(crate) fn validate_request(sandbox: &str, machine_port: u16, owner: &str) -> Result<()> {
    let valid = |s: &str| {
        !s.is_empty()
            && s.len() <= 128
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    if machine_port == 0 || !valid(sandbox) || !valid(owner) {
        return Err(StoreError(
            "nonzero machine port and bounded opaque sandbox/owner IDs required".into(),
        ));
    }
    Ok(())
}

/// An owner-authorized store operation. Authorization and data access are atomic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnedPortAccess<T> {
    Granted(T),
    OwnerConflict,
    SandboxMissing,
}
