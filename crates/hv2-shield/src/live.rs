//! Shields as the enforcers see them: a node's [`Registry`] behind a lock,
//! asked once for each trapped call or connection, and a log of what was
//! refused or would have been.
//!
//! A shield in [`Mode::Monitor`] refuses nothing here: its matches are
//! recorded as hits that were not enforced, which is how an operator sees
//! what it would break. Only a shield in [`Mode::Enforce`] refuses.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Mutex, RwLock, RwLockReadGuard};

use serde::Serialize;

use crate::registry::{Mode, Registry};
use crate::sign::Keyring;
use crate::spec::{Connection, SyscallAction};

/// How many of the first hits are kept.
pub const HIT_HEAD: usize = 256;
/// How many of the most recent hits are kept, after the first.
pub const HIT_TAIL: usize = 1024;

/// One match of a shield's rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    /// From 1, never reused: a gap in a read-out is a hit let go.
    pub seq: u64,
    pub at: u64,
    pub shield: String,
    pub digest: String,
    pub sandbox: String,
    /// What matched, such as `ptrace(16, 1234, 0, 0, 0, 0)` or
    /// `tcp 203.0.113.9:443 evil.example`.
    pub what: String,
    /// Whether it was refused (enforced) or only recorded (monitored).
    pub enforced: bool,
}

/// Hits per shield.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tally {
    /// Matches while monitored: what enforcing it would have refused.
    pub monitored: u64,
    pub refused: u64,
}

#[derive(Debug, Default)]
struct HitLog {
    head: Vec<Hit>,
    tail: VecDeque<Hit>,
    recorded: u64,
    /// Matches an external enforcer (a guest agent) saw and could not keep.
    lost: u64,
    tally: BTreeMap<String, Tally>,
}

/// The hit log as read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HitSnapshot {
    pub hits: Vec<Hit>,
    pub recorded: u64,
    pub dropped: u64,
    /// Matches a guest agent reported it lost, to a full ring or to a
    /// refused report, before they reached this log. Not in `recorded`.
    pub lost: u64,
    /// By shield digest.
    pub tally: BTreeMap<String, Tally>,
}

/// A node's shields, shared by everything that enforces them.
#[derive(Debug, Default)]
pub struct Live {
    registry: RwLock<Registry>,
    keyring: RwLock<Keyring>,
    hits: Mutex<HitLog>,
}

impl Live {
    pub fn new(registry: Registry, keyring: Keyring) -> Self {
        Self {
            registry: RwLock::new(registry),
            keyring: RwLock::new(keyring),
            hits: Mutex::default(),
        }
    }

    /// The registry, to read.
    pub fn registry(&self) -> RwLockReadGuard<'_, Registry> {
        self.registry.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Change the registry, with the keyring to check signatures against.
    pub fn update<R>(&self, change: impl FnOnce(&mut Registry, &Keyring) -> R) -> R {
        let keyring = self.keyring.read().unwrap_or_else(|e| e.into_inner());
        let mut registry = self.registry.write().unwrap_or_else(|e| e.into_inner());
        change(&mut registry, &keyring)
    }

    /// Replace the keyring, and retire what it no longer vouches for.
    pub fn set_keyring(&self, keyring: Keyring, now: u64) -> Vec<crate::Event> {
        // Keyring, then registry: the order `update` takes them in.
        let mut current = self.keyring.write().unwrap_or_else(|e| e.into_inner());
        let mut registry = self.registry.write().unwrap_or_else(|e| e.into_inner());
        let events = registry.apply_keyring(&keyring, now);
        *current = keyring;
        events
    }

    /// Whether any shield is in force: callers with nothing to ask can skip.
    pub fn is_empty(&self) -> bool {
        self.registry().entries().all(|e| e.state.mode().is_none())
    }

    /// What to do with a trapped call to `syscall` from `sandbox`: the action
    /// of the first enforced shield whose rule matches, or none. Matches of
    /// monitored shields are recorded and refuse nothing.
    pub fn syscall(
        &self,
        sandbox: &str,
        syscall: &str,
        args: &[u64; 6],
        now: u64,
    ) -> Option<SyscallAction> {
        let registry = self.registry();
        let mut verdict = None;
        let mut hits = Vec::new();
        for (entry, mode) in registry.in_force(sandbox, now) {
            let Some(action) = entry.spec().syscall_verdict(syscall, args) else {
                continue;
            };
            let enforced = mode == Mode::Enforce && verdict.is_none();
            hits.push((entry.spec().name.clone(), entry.digest.clone(), enforced));
            if enforced {
                verdict = Some(action);
            }
        }
        drop(registry);
        if !hits.is_empty() {
            let what = format!(
                "{syscall}({:#x}, {:#x}, {:#x}, {:#x}, {:#x}, {:#x})",
                args[0], args[1], args[2], args[3], args[4], args[5]
            );
            self.record(sandbox, &what, hits, now);
        }
        verdict
    }

    /// Whether a shield in force for `sandbox` has a rule about names, so
    /// that connections are worth reading for the name they send.
    pub fn names_matter(&self, sandbox: &str, now: u64) -> bool {
        self.registry()
            .in_force(sandbox, now)
            .any(|(entry, _)| entry.spec().network.iter().any(|rule| rule.name.is_some()))
    }

    /// Whether a connection from `sandbox` is refused: the name of the first
    /// enforced shield that refuses it. Monitored matches are recorded.
    pub fn connection(
        &self,
        sandbox: &str,
        connection: &Connection<'_>,
        now: u64,
    ) -> Option<String> {
        let registry = self.registry();
        let mut refused = None;
        let mut hits = Vec::new();
        for (entry, mode) in registry.in_force(sandbox, now) {
            if !entry.spec().refuses(connection) {
                continue;
            }
            let enforced = mode == Mode::Enforce && refused.is_none();
            hits.push((entry.spec().name.clone(), entry.digest.clone(), enforced));
            if enforced {
                refused = Some(entry.spec().name.clone());
            }
        }
        drop(registry);
        if !hits.is_empty() {
            let protocol = match connection.protocol {
                crate::Protocol::Tcp => "tcp",
                crate::Protocol::Udp => "udp",
                crate::Protocol::Any => "dns",
            };
            let mut what = protocol.to_string();
            if let Some(address) = connection.address {
                what.push_str(&format!(
                    " {}",
                    std::net::SocketAddr::new(address, connection.port)
                ));
            }
            if let Some(name) = connection.name {
                what.push(' ');
                what.push_str(name);
            }
            self.record(sandbox, &what, hits, now);
        }
        refused
    }

    fn record(&self, sandbox: &str, what: &str, hits: Vec<(String, String, bool)>, now: u64) {
        let mut log = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        for (shield, digest, enforced) in hits {
            let tally = log.tally.entry(digest.clone()).or_default();
            if enforced {
                tally.refused = tally.refused.saturating_add(1);
            } else {
                tally.monitored = tally.monitored.saturating_add(1);
            }
            log.recorded += 1;
            let hit = Hit {
                seq: log.recorded,
                at: now,
                shield,
                digest,
                sandbox: sandbox.to_string(),
                what: what.to_string(),
                enforced,
            };
            if log.head.len() < HIT_HEAD {
                log.head.push(hit);
            } else {
                if log.tail.len() == HIT_TAIL {
                    log.tail.pop_front();
                }
                log.tail.push_back(hit);
            }
        }
    }

    /// Record a match that an enforcer outside this process saw: a guest
    /// agent applying this node's rules. `what` is how the call reads, as in
    /// [`Hit::what`]. The shield is named by `digest`, and its name taken from
    /// the registry: the reporter is not trusted to name anything.
    ///
    /// Returns whether it was recorded. It is not when no shield in force
    /// here, for `sandbox`, has that digest: the report is counted in
    /// [`HitSnapshot::lost`] instead. A report of a refusal by a shield this
    /// node has in monitor mode is recorded as monitored: the node's own mode
    /// decides, not the reporter.
    pub fn record_external(
        &self,
        sandbox: &str,
        digest: &str,
        what: &str,
        enforced: bool,
        at: u64,
    ) -> bool {
        /// What a hit's text may be, so a guest cannot fill the log.
        const WHAT_LIMIT: usize = 256;
        let shield = {
            let registry = self.registry();
            let found = registry
                .in_force(sandbox, at)
                .find(|(entry, _)| entry.digest == digest)
                .map(|(entry, mode)| {
                    (
                        entry.spec().name.clone(),
                        entry.digest.clone(),
                        enforced && mode == Mode::Enforce,
                    )
                });
            found
        };
        let Some((name, digest, enforced)) = shield else {
            self.record_lost(1);
            return false;
        };
        let what: String = what.chars().take(WHAT_LIMIT).collect();
        self.record(sandbox, &what, vec![(name, digest, enforced)], at);
        true
    }

    /// Count matches an external enforcer could not report.
    pub fn record_lost(&self, count: u64) {
        let mut log = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        log.lost = log.lost.saturating_add(count);
    }

    /// The hits, and the tally by shield.
    pub fn hits(&self) -> HitSnapshot {
        let log = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        let hits: Vec<Hit> = log.head.iter().chain(log.tail.iter()).cloned().collect();
        HitSnapshot {
            dropped: log.recorded - hits.len() as u64,
            recorded: log.recorded,
            lost: log.lost,
            hits,
            tally: log.tally.clone(),
        }
    }
}

/// A [`hv2_sandbox::SyscallGate`] that asks a node's shields, for one sandbox.
#[cfg(feature = "sandbox")]
pub struct SandboxGate {
    live: std::sync::Arc<Live>,
    sandbox: String,
}

#[cfg(feature = "sandbox")]
impl SandboxGate {
    pub fn new(live: std::sync::Arc<Live>, sandbox: impl Into<String>) -> Self {
        Self {
            live,
            sandbox: sandbox.into(),
        }
    }

    /// A [`hv2_sandbox::SyscallShield`] trapping every call this host can, so
    /// a shield loaded while the workload runs finds its call trapped.
    pub fn shield(
        live: std::sync::Arc<Live>,
        sandbox: impl Into<String>,
    ) -> hv2_sandbox::SyscallShield {
        hv2_sandbox::SyscallShield {
            syscalls: hv2_sandbox::trappable_syscalls()
                .into_iter()
                .map(str::to_string)
                .collect(),
            gate: std::sync::Arc::new(Self::new(live, sandbox)),
        }
    }
}

#[cfg(feature = "sandbox")]
impl hv2_sandbox::SyscallGate for SandboxGate {
    fn decide(&self, call: &hv2_sandbox::TrappedCall<'_>) -> hv2_sandbox::GateVerdict {
        match self
            .live
            .syscall(&self.sandbox, call.syscall, &call.args, crate::now_secs())
        {
            Some(SyscallAction::Errno { errno }) => hv2_sandbox::GateVerdict::Deny { errno },
            Some(SyscallAction::Kill) => hv2_sandbox::GateVerdict::Kill,
            None => hv2_sandbox::GateVerdict::Allow,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Policy;
    use crate::sign::{SignedShield, SigningKey};
    use crate::spec::Protocol;
    use crate::to_hex;

    fn live_with(json: &str) -> (Live, String) {
        let key = SigningKey::generate().unwrap();
        let mut keyring = Keyring::default();
        keyring.trust(key.public_key().clone());
        let live = Live::new(Registry::new(Policy::default()), keyring);
        let signed = SignedShield::sign(json, &key).unwrap();
        live.update(|registry, keyring| registry.load(&signed, keyring, "op", 1))
            .unwrap();
        (live, to_hex(&signed.digest()))
    }

    #[test]
    fn a_monitored_shield_records_and_an_enforced_one_refuses() {
        let (live, digest) = live_with(
            r#"{"name":"no-ptrace","syscalls":[{"syscall":"ptrace","action":{"kind":"errno","errno":1}}],"expiresAt":10000}"#,
        );
        assert_eq!(live.syscall("sb", "ptrace", &[0; 6], 2), None, "monitored");
        assert_eq!(live.syscall("sb", "bpf", &[0; 6], 2), None, "no rule");
        live.update(|r, _| r.set_mode(&digest, Mode::Enforce, "op", 3))
            .unwrap();
        assert_eq!(
            live.syscall("sb", "ptrace", &[0; 6], 4),
            Some(SyscallAction::Errno { errno: 1 })
        );
        let hits = live.hits();
        assert_eq!(hits.recorded, 2);
        assert!(!hits.hits[0].enforced);
        assert!(hits.hits[1].enforced);
        assert!(hits.hits[1].what.starts_with("ptrace(0x0"));
        assert_eq!(
            hits.tally[&digest],
            Tally {
                monitored: 1,
                refused: 1
            }
        );
        // Expired: refuses nothing.
        assert_eq!(live.syscall("sb", "ptrace", &[0; 6], 10_000), None);
    }

    #[test]
    fn connections_are_refused_by_enforced_network_rules() {
        let (live, digest) = live_with(
            r#"{"name":"block","network":[{"name":"*.evil.example"}],"expiresAt":10000}"#,
        );
        let conn = Connection {
            address: Some("192.0.2.1".parse().unwrap()),
            port: 443,
            name: Some("c2.evil.example"),
            protocol: Protocol::Tcp,
        };
        assert_eq!(live.connection("sb", &conn, 2), None);
        live.update(|r, _| r.set_mode(&digest, Mode::Enforce, "op", 3))
            .unwrap();
        assert_eq!(live.connection("sb", &conn, 4).as_deref(), Some("block"));
        let hits = live.hits();
        assert_eq!(hits.hits[1].what, "tcp 192.0.2.1:443 c2.evil.example");
        assert!(!live.is_empty());
    }

    #[test]
    fn the_hit_log_keeps_its_first_and_latest() {
        let (live, _) = live_with(
            r#"{"name":"a","syscalls":[{"syscall":"ptrace","action":{"kind":"kill"}}],"expiresAt":10000}"#,
        );
        for _ in 0..HIT_HEAD + HIT_TAIL + 10 {
            live.syscall("sb", "ptrace", &[0; 6], 2);
        }
        let hits = live.hits();
        assert_eq!(hits.hits.len(), HIT_HEAD + HIT_TAIL);
        assert_eq!(hits.dropped, 10);
        assert_eq!(hits.hits[0].seq, 1);
        assert_eq!(hits.hits.last().unwrap().seq, hits.recorded);
    }

    #[test]
    fn a_guests_hits_join_the_log_under_the_registrys_names_and_unknown_ones_are_lost() {
        let (live, digest) = live_with(
            r#"{"name":"no-ptrace","syscalls":[{"syscall":"ptrace","action":{"kind":"errno","errno":1}}],"scope":{"kind":"sandboxes","ids":["sb"]},"expiresAt":10000}"#,
        );
        assert!(live.record_external("sb", &digest, "ptrace(0x0)", false, 5));
        // Claimed as a refusal while the node has it monitored: monitored.
        assert!(live.record_external("sb", &digest, "ptrace(0x1)", true, 5));
        live.update(|r, _| r.set_mode(&digest, Mode::Enforce, "op", 5))
            .unwrap();
        assert!(live.record_external("sb", &digest, &"x".repeat(10_000), true, 6));
        // Expired: no longer in force, so not placed.
        assert!(!live.record_external("sb", &digest, "ptrace(0x0)", true, 10_000));
        // Another sandbox's, a digest nobody loaded: refused and counted.
        assert!(!live.record_external("other", &digest, "ptrace(0x0)", true, 7));
        assert!(!live.record_external("sb", &"0".repeat(64), "ptrace(0x0)", true, 7));
        live.record_lost(10);
        let hits = live.hits();
        assert_eq!(hits.recorded, 3);
        assert_eq!(hits.lost, 13);
        assert_eq!(hits.hits[0].shield, "no-ptrace", "the registry's name");
        assert_eq!(hits.hits[0].sandbox, "sb");
        assert!(!hits.hits[0].enforced && !hits.hits[1].enforced && hits.hits[2].enforced);
        assert_eq!(hits.hits[2].what.chars().count(), 256);
        assert_eq!(
            hits.tally[&digest],
            Tally {
                monitored: 2,
                refused: 1
            }
        );
    }

    #[test]
    fn a_new_keyring_retires_what_it_no_longer_vouches_for() {
        let (live, _) = live_with(
            r#"{"name":"a","syscalls":[{"syscall":"ptrace","action":{"kind":"kill"}}],"expiresAt":10000}"#,
        );
        let events = live.set_keyring(Keyring::default(), 5);
        assert_eq!(events.len(), 1);
        assert!(live.is_empty());
    }
}
