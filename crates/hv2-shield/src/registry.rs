//! A node's shields, and the rules for changing them.
//!
//! - **Proposed**: anyone allowed to may propose a shield, unsigned, for
//!   operators to review. A proposal is not in force and is not measured.
//! - **Loaded** only with a signature from a key in the node's [`Keyring`],
//!   and always in [`Mode::Monitor`] first: the rules are evaluated and what
//!   they would refuse is recorded, but nothing is refused. This is how an
//!   operator sees what a shield would break before it breaks it.
//! - **Enforced** by an operator, at least [`Policy::min_monitor_secs`] after
//!   it was loaded and, if [`Policy::distinct_enforcer`], by someone other than
//!   whoever loaded it.
//! - **Removed** by naming its digest, so the version removed is the version
//!   meant; **expired** by the node at its expiry, at most
//!   [`MAX_LIFETIME_SECS`] after it was loaded; **revoked** when the keyring
//!   no longer vouches for it.
//!
//! One shield of a name is in force at a time; to replace one, remove it and
//! load the new version. Every change to what is in force is measured
//! ([`crate::measure`]).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::measure::{Event, EventKind, MeasuredLog};
use crate::sign::{Action, Keyring, SignedDirective, SignedShield, DIRECTIVE_WINDOW_SECS};
use crate::spec::{ShieldSpec, MAX_LIFETIME_SECS};
use crate::{from_hex, sha256, to_hex, Result, ShieldError};

/// The most proposals waiting at once: whoever may propose cannot fill a
/// node's memory with them.
pub const MAX_PROPOSED: usize = 256;
/// The most shields loaded at once.
pub const MAX_ACTIVE: usize = 256;
/// How many shields no longer in force are kept for reading. The measured
/// log keeps every change regardless.
pub const MAX_RETIRED: usize = 256;
/// Directives applied within [`DIRECTIVE_WINDOW_SECS`] that a node
/// remembers, to refuse them again.
pub const MAX_RECENT_DIRECTIVES: usize = 4096;

/// Whether a loaded shield refuses or only records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Monitor,
    Enforce,
}

/// Where a shield is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    Proposed,
    Monitor,
    Enforce,
    Removed,
    Expired,
    Revoked,
}

impl State {
    /// The mode it is in force in, if it is.
    pub fn mode(self) -> Option<Mode> {
        match self {
            State::Monitor => Some(Mode::Monitor),
            State::Enforce => Some(Mode::Enforce),
            _ => None,
        }
    }
}

/// What a node requires before a shield is enforced.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Policy {
    /// Seconds a shield must be monitored before it may be enforced.
    pub min_monitor_secs: u64,
    /// Whether whoever enforces a shield must be someone other than whoever
    /// loaded it.
    pub distinct_enforcer: bool,
}

/// One shield on a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Entry {
    /// SHA-256 of `spec_json`, hex.
    pub digest: String,
    pub spec_json: String,
    #[serde(skip)]
    pub spec: Option<ShieldSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed: Option<SignedShield>,
    pub state: State,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loaded_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loaded_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enforced_by: Option<String>,
    /// When it stops being in force: its own expiry, or the longest a shield
    /// may be in force from when it was loaded, whichever is sooner.
    pub expires_at: u64,
    /// When its state last changed.
    pub changed_at: u64,
}

impl Entry {
    /// The parsed shield. Every entry has one; it is parsed when the entry is
    /// made or restored.
    pub fn spec(&self) -> &ShieldSpec {
        self.spec
            .as_ref()
            .expect("every entry is parsed when it is made or restored")
    }

    pub fn signer(&self) -> &str {
        self.signed.as_ref().map_or("", |s| s.signer.as_str())
    }

    fn digest_bytes(&self) -> [u8; 32] {
        sha256(self.spec_json.as_bytes())
    }
}

/// A node's shields and their measured log.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    entries: BTreeMap<String, Entry>,
    log: MeasuredLog,
    policy: Policy,
    /// Directives applied, by digest, with when they were issued.
    applied: BTreeMap<String, u64>,
}

/// A registry as kept on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Saved {
    pub entries: Vec<Entry>,
    pub events: Vec<Event>,
    pub register: String,
    /// Directives applied that could still be applied again.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub applied: BTreeMap<String, u64>,
}

fn state_error(why: String) -> ShieldError {
    ShieldError::State(why)
}

impl Registry {
    pub fn new(policy: Policy) -> Self {
        Self {
            policy,
            ..Self::default()
        }
    }

    pub fn policy(&self) -> Policy {
        self.policy
    }

    pub fn log(&self) -> &MeasuredLog {
        &self.log
    }

    /// Every entry, in digest order.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values()
    }

    /// The entry whose digest is `digest` (hex), or that starts with it when
    /// that names one entry alone and is at least 12 digits.
    pub fn get(&self, digest: &str) -> Option<&Entry> {
        let digest = digest.to_ascii_lowercase();
        if let Some(entry) = self.entries.get(&digest) {
            return Some(entry);
        }
        if digest.len() < 12 {
            return None;
        }
        let mut found = self
            .entries
            .range(digest.clone()..)
            .take_while(|(k, _)| k.starts_with(&digest));
        let first = found.next()?;
        found.next().is_none().then_some(first.1)
    }

    fn key_of(&self, digest: &str) -> Result<String> {
        self.get(digest)
            .map(|e| e.digest.clone())
            .ok_or_else(|| ShieldError::NotFound(format!("no shield {digest}")))
    }

    /// The shields in force for the sandbox `id`, and their modes.
    pub fn in_force<'a>(
        &'a self,
        id: &'a str,
        now: u64,
    ) -> impl Iterator<Item = (&'a Entry, Mode)> + 'a {
        self.entries.values().filter_map(move |entry| {
            let mode = entry.state.mode()?;
            (entry.expires_at > now && entry.spec().scope.covers(id)).then_some((entry, mode))
        })
    }

    /// Propose a shield for review. It is not in force.
    ///
    /// # Errors
    ///
    /// If it is not a valid shield, has expired, is already here, or too
    /// many proposals are waiting.
    pub fn propose(&mut self, spec_json: &str, by: &str, now: u64) -> Result<&Entry> {
        let spec = ShieldSpec::parse(spec_json)?;
        if spec.expires_at <= now {
            return Err(ShieldError::Invalid("the shield has expired".into()));
        }
        let digest = to_hex(&sha256(spec_json.as_bytes()));
        if self.entries.contains_key(&digest) {
            return Err(state_error(format!("shield {digest} is already here")));
        }
        let waiting = self
            .entries
            .values()
            .filter(|e| e.state == State::Proposed)
            .count();
        if waiting >= MAX_PROPOSED {
            return Err(state_error(format!(
                "{MAX_PROPOSED} proposals are waiting; remove some first"
            )));
        }
        let entry = Entry {
            digest: digest.clone(),
            spec_json: spec_json.to_string(),
            expires_at: spec.expires_at,
            spec: Some(spec),
            signed: None,
            state: State::Proposed,
            proposed_by: Some(by.to_string()),
            loaded_by: None,
            loaded_at: None,
            enforced_by: None,
            changed_at: now,
        };
        Ok(self.entries.entry(digest).or_insert(entry))
    }

    /// Load a signed shield, in monitor mode.
    ///
    /// # Errors
    ///
    /// If the signature is not one `keyring` trusts, the shield has expired,
    /// it is already in force, another of its name is, or too many are.
    pub fn load(
        &mut self,
        signed: &SignedShield,
        keyring: &Keyring,
        by: &str,
        now: u64,
    ) -> Result<Event> {
        let spec = signed.verify(keyring)?;
        if spec.expires_at <= now {
            return Err(ShieldError::Invalid("the shield has expired".into()));
        }
        let digest = to_hex(&signed.digest());
        let mut active = 0;
        for entry in self.entries.values() {
            if entry.state.mode().is_none() {
                continue;
            }
            active += 1;
            if entry.digest == digest {
                return Err(state_error(format!("shield {digest} is already loaded")));
            }
            if entry.spec().name == spec.name {
                return Err(state_error(format!(
                    "shield {:?} is loaded as {}: remove it first",
                    spec.name, entry.digest
                )));
            }
        }
        if active >= MAX_ACTIVE {
            return Err(state_error(format!("{MAX_ACTIVE} shields are loaded")));
        }
        let proposed_by = self
            .entries
            .get(&digest)
            .and_then(|e| e.proposed_by.clone());
        let expires_at = spec.expires_at.min(now.saturating_add(MAX_LIFETIME_SECS));
        let name = spec.name.clone();
        self.entries.insert(
            digest.clone(),
            Entry {
                digest,
                spec_json: signed.spec.clone(),
                spec: Some(spec),
                signed: Some(signed.clone()),
                state: State::Monitor,
                proposed_by,
                loaded_by: Some(by.to_string()),
                loaded_at: Some(now),
                enforced_by: None,
                expires_at,
                changed_at: now,
            },
        );
        Ok(self.log.extend(
            now,
            EventKind::Load,
            &name,
            &signed.digest(),
            &signed.signer,
            Some(by),
        ))
    }

    /// Move a loaded shield to `mode`.
    ///
    /// # Errors
    ///
    /// If it is not loaded, is in that mode already, or the node's
    /// [`Policy`] does not allow it to be enforced yet, or by `by`.
    pub fn set_mode(&mut self, digest: &str, mode: Mode, by: &str, now: u64) -> Result<Event> {
        let key = self.key_of(digest)?;
        let policy = self.policy;
        let entry = self.entries.get_mut(&key).expect("found above");
        let Some(current) = entry.state.mode() else {
            return Err(state_error(format!("shield {key} is not loaded")));
        };
        if current == mode {
            return Err(state_error(format!("shield {key} is already in that mode")));
        }
        if mode == Mode::Enforce {
            let loaded_at = entry.loaded_at.unwrap_or(now);
            let ready = loaded_at.saturating_add(policy.min_monitor_secs);
            if now < ready {
                return Err(state_error(format!(
                    "shield {key} must be monitored until {ready} before it is enforced"
                )));
            }
            if policy.distinct_enforcer && entry.loaded_by.as_deref() == Some(by) {
                return Err(state_error(
                    "a shield is enforced by someone other than whoever loaded it".into(),
                ));
            }
            entry.state = State::Enforce;
            entry.enforced_by = Some(by.to_string());
        } else {
            entry.state = State::Monitor;
        }
        entry.changed_at = now;
        let kind = match mode {
            Mode::Enforce => EventKind::Enforce,
            Mode::Monitor => EventKind::Monitor,
        };
        let (name, digest, signer) = (
            entry.spec().name.clone(),
            entry.digest_bytes(),
            entry.signer().to_string(),
        );
        Ok(self
            .log
            .extend(now, kind, &name, &digest, &signer, Some(by)))
    }

    /// Apply a signed directive. Whoever signed it is who acts: the
    /// [`Policy`]'s distinct enforcer is a different key from the one that
    /// signed the shield.
    ///
    /// # Errors
    ///
    /// If it does not verify, is not good at `now`, has been applied before,
    /// or what it says cannot be done.
    pub fn apply(
        &mut self,
        signed: &SignedDirective,
        keyring: &Keyring,
        now: u64,
    ) -> Result<Option<Event>> {
        let directive = signed.verify(keyring, now)?;
        let id = to_hex(&signed.digest());
        self.applied
            .retain(|_, issued| issued.saturating_add(DIRECTIVE_WINDOW_SECS) >= now);
        if self.applied.contains_key(&id) {
            return Err(state_error(
                "this directive has been applied already".into(),
            ));
        }
        if self.applied.len() >= MAX_RECENT_DIRECTIVES {
            return Err(state_error(format!(
                "{MAX_RECENT_DIRECTIVES} directives were applied in the last \
                 {DIRECTIVE_WINDOW_SECS} seconds; wait"
            )));
        }
        if !self.entries.contains_key(&directive.shield) {
            return Err(ShieldError::NotFound(format!(
                "no shield {}",
                directive.shield
            )));
        }
        let by = signed.signer.clone();
        let result = match directive.action {
            Action::Enforce => self
                .set_mode(&directive.shield, Mode::Enforce, &by, now)
                .map(Some),
            Action::Monitor => self
                .set_mode(&directive.shield, Mode::Monitor, &by, now)
                .map(Some),
            Action::Remove => self.remove(&directive.shield, &by, now),
        };
        if result.is_ok() {
            self.applied.insert(id, directive.issued_at);
        }
        result
    }

    /// Remove a shield by its digest. A loaded one is measured as unloaded;
    /// a proposal is dropped.
    ///
    /// # Errors
    ///
    /// If there is no such shield, or it is no longer in force.
    pub fn remove(&mut self, digest: &str, by: &str, now: u64) -> Result<Option<Event>> {
        let key = self.key_of(digest)?;
        let entry = self.entries.get_mut(&key).expect("found above");
        match entry.state {
            State::Proposed => {
                self.entries.remove(&key);
                Ok(None)
            }
            State::Monitor | State::Enforce => {
                entry.state = State::Removed;
                entry.changed_at = now;
                let (name, digest, signer) = (
                    entry.spec().name.clone(),
                    entry.digest_bytes(),
                    entry.signer().to_string(),
                );
                let event =
                    self.log
                        .extend(now, EventKind::Unload, &name, &digest, &signer, Some(by));
                self.prune();
                Ok(Some(event))
            }
            _ => Err(state_error(format!("shield {key} is no longer in force"))),
        }
    }

    /// Retire every shield in force that has expired, and drop expired
    /// proposals.
    pub fn expire(&mut self, now: u64) -> Vec<Event> {
        self.entries
            .retain(|_, e| !(e.state == State::Proposed && e.expires_at <= now));
        self.retire(now, EventKind::Expire, |entry| entry.expires_at <= now)
    }

    /// Retire every shield in force that `keyring` no longer vouches for:
    /// its signer is gone or revoked, it is revoked, or its signature does
    /// not verify. A registry restored from disk is checked this way, so an
    /// edited state file cannot put a shield in force.
    pub fn apply_keyring(&mut self, keyring: &Keyring, now: u64) -> Vec<Event> {
        self.retire(now, EventKind::Revoke, |entry| {
            entry
                .signed
                .as_ref()
                .is_none_or(|signed| signed.verify(keyring).is_err())
        })
    }

    fn retire(
        &mut self,
        now: u64,
        kind: EventKind,
        mut done: impl FnMut(&Entry) -> bool,
    ) -> Vec<Event> {
        let mut events = Vec::new();
        for entry in self.entries.values_mut() {
            if entry.state.mode().is_none() || !done(entry) {
                continue;
            }
            entry.state = match kind {
                EventKind::Expire => State::Expired,
                _ => State::Revoked,
            };
            entry.changed_at = now;
            let digest = entry.digest_bytes();
            events.push(self.log.extend(
                now,
                kind,
                &entry.spec().name,
                &digest,
                entry.signer(),
                None,
            ));
        }
        if !events.is_empty() {
            self.prune();
        }
        events
    }

    /// Keep the most recently retired [`MAX_RETIRED`].
    fn prune(&mut self) {
        let mut retired: Vec<(u64, String)> = self
            .entries
            .values()
            .filter(|e| matches!(e.state, State::Removed | State::Expired | State::Revoked))
            .map(|e| (e.changed_at, e.digest.clone()))
            .collect();
        if retired.len() <= MAX_RETIRED {
            return;
        }
        retired.sort();
        for (_, digest) in &retired[..retired.len() - MAX_RETIRED] {
            self.entries.remove(digest);
        }
    }

    /// The registry as kept on disk.
    pub fn save(&self) -> Saved {
        Saved {
            entries: self.entries.values().cloned().collect(),
            events: self.log.events().to_vec(),
            register: to_hex(&self.log.register()),
            applied: self.applied.clone(),
        }
    }

    /// A registry from disk. The log must replay to its register, and every
    /// entry must be the shield its digest names; then the keyring is
    /// applied, so a shield it no longer vouches for is retired, measured.
    ///
    /// # Errors
    ///
    /// If the log does not replay, or an entry is not what it claims.
    pub fn restore(
        saved: Saved,
        policy: Policy,
        keyring: &Keyring,
        now: u64,
    ) -> Result<(Self, Vec<Event>)> {
        let register = from_hex(&saved.register)
            .and_then(|r| <[u8; 32]>::try_from(r).ok())
            .ok_or_else(|| state_error("the saved register is not 32 bytes of hex".into()))?;
        let log = MeasuredLog::restore(saved.events, Some(&register))?;
        // What the log says each shield was last, and when it was loaded:
        // an entry claiming otherwise was edited, and the log is what an
        // attestation vouches for.
        let mut told: BTreeMap<&str, (State, Option<u64>)> = BTreeMap::new();
        for event in log.events() {
            let (state, loaded_at) = told
                .entry(event.digest.as_str())
                .or_insert((State::Proposed, None));
            *state = match event.kind {
                EventKind::Load => {
                    *loaded_at = Some(event.at);
                    State::Monitor
                }
                EventKind::Monitor => State::Monitor,
                EventKind::Enforce => State::Enforce,
                EventKind::Unload => State::Removed,
                EventKind::Expire => State::Expired,
                EventKind::Revoke => State::Revoked,
            };
        }
        let mut entries = BTreeMap::new();
        for mut entry in saved.entries {
            if to_hex(&sha256(entry.spec_json.as_bytes())) != entry.digest {
                return Err(state_error(format!(
                    "saved shield {} is not the shield its digest names",
                    entry.digest
                )));
            }
            if let Some(signed) = &entry.signed {
                if signed.spec != entry.spec_json {
                    return Err(state_error(format!(
                        "saved shield {} carries another shield's signature",
                        entry.digest
                    )));
                }
            }
            let (state, loaded_at) = told
                .get(entry.digest.as_str())
                .copied()
                .unwrap_or((State::Proposed, None));
            if entry.state != state || entry.loaded_at != loaded_at {
                return Err(state_error(format!(
                    "saved shield {} is not in the state its log says",
                    entry.digest
                )));
            }
            if state != State::Proposed && entry.signed.is_none() {
                return Err(state_error(format!(
                    "saved shield {} was loaded and carries no signature",
                    entry.digest
                )));
            }
            entry.spec = Some(ShieldSpec::parse(&entry.spec_json)?);
            entries.insert(entry.digest.clone(), entry);
        }
        let mut registry = Self {
            entries,
            log,
            policy,
            applied: saved.applied,
        };
        let mut events = registry.apply_keyring(keyring, now);
        events.extend(registry.expire(now));
        Ok((registry, events))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sign::SigningKey;

    fn spec(name: &str, expires_at: u64) -> String {
        format!(
            r#"{{"name":"{name}","syscalls":[{{"syscall":"ptrace","action":{{"kind":"errno","errno":1}}}}],"expiresAt":{expires_at}}}"#
        )
    }

    struct Fixture {
        key: SigningKey,
        keyring: Keyring,
    }

    #[test]
    fn a_saved_state_the_log_does_not_tell_is_refused() {
        let f = fixture();
        let mut registry = Registry::new(Policy::default());
        let signed = SignedShield::sign(&spec("a", 100_000), &f.key).unwrap();
        let digest = to_hex(&signed.digest());
        registry.load(&signed, &f.keyring, "op", 1_000).unwrap();
        registry
            .set_mode(&digest, Mode::Enforce, "op2", 1_001)
            .unwrap();
        registry.remove(&digest, "op", 1_002).unwrap();
        let saved = registry.save();
        assert!(Registry::restore(saved.clone(), Policy::default(), &f.keyring, 1_003).is_ok());

        // Brought back into force by an edit.
        let mut edited = saved.clone();
        edited.entries[0].state = State::Enforce;
        assert!(Registry::restore(edited, Policy::default(), &f.keyring, 1_003).is_err());
        // Loaded earlier, to pass a minimum monitoring period.
        let mut edited = saved.clone();
        edited.entries[0].loaded_at = Some(1);
        assert!(Registry::restore(edited, Policy::default(), &f.keyring, 1_003).is_err());
        // A loaded shield with its signature taken away.
        let mut edited = saved;
        edited.entries[0].signed = None;
        assert!(Registry::restore(edited, Policy::default(), &f.keyring, 1_003).is_err());
    }

    fn directive(
        key: &SigningKey,
        action: Action,
        shield: &str,
        issued_at: u64,
    ) -> SignedDirective {
        SignedDirective::sign(
            &crate::sign::Directive {
                action,
                shield: shield.to_string(),
                issued_at,
            },
            key,
        )
        .unwrap()
    }

    #[test]
    fn directives_are_signed_once_used_and_obey_the_two_key_rule() {
        let author = fixture();
        let approver = SigningKey::generate().unwrap();
        let stranger = SigningKey::generate().unwrap();
        let mut keyring = author.keyring.clone();
        keyring.trust(approver.public_key().clone());
        let mut registry = Registry::new(Policy {
            min_monitor_secs: 0,
            distinct_enforcer: true,
        });
        let signed = SignedShield::sign(&spec("a", 100_000), &author.key).unwrap();
        let digest = to_hex(&signed.digest());
        registry
            .load(&signed, &keyring, &signed.signer, 1_000)
            .unwrap();

        // Not a key the node trusts.
        let by_stranger = directive(&stranger, Action::Enforce, &digest, 1_000);
        assert!(registry.apply(&by_stranger, &keyring, 1_000).is_err());
        // The author alone cannot enforce what it wrote.
        let by_author = directive(&author.key, Action::Enforce, &digest, 1_000);
        assert!(registry.apply(&by_author, &keyring, 1_000).is_err());
        // Too old, and from the future.
        let stale = directive(&approver, Action::Enforce, &digest, 1_000);
        assert!(registry
            .apply(&stale, &keyring, 1_000 + DIRECTIVE_WINDOW_SECS + 1)
            .is_err());
        let early = directive(&approver, Action::Enforce, &digest, 1_000 + 61);
        assert!(registry.apply(&early, &keyring, 1_000).is_err());
        // A changed byte is not the directive signed.
        let mut forged = directive(&approver, Action::Enforce, &digest, 1_000);
        forged.directive = forged.directive.replace("enforce", "monitor");
        assert!(registry.apply(&forged, &keyring, 1_000).is_err());

        let enforce = directive(&approver, Action::Enforce, &digest, 1_000);
        let event = registry.apply(&enforce, &keyring, 1_001).unwrap().unwrap();
        assert_eq!(event.kind, EventKind::Enforce);
        assert_eq!(registry.get(&digest).unwrap().state, State::Enforce);
        assert_eq!(
            registry.get(&digest).unwrap().enforced_by.as_deref(),
            Some(approver.public_key().id().as_str())
        );

        let monitor = directive(&approver, Action::Monitor, &digest, 1_002);
        registry.apply(&monitor, &keyring, 1_002).unwrap();
        // Replayed, to put it back in force: refused, even after a restart.
        let saved = registry.save();
        let (mut restored, _) =
            Registry::restore(saved, registry.policy(), &keyring, 1_003).unwrap();
        assert!(restored.apply(&enforce, &keyring, 1_003).is_err());
        assert_eq!(restored.get(&digest).unwrap().state, State::Monitor);

        let remove = directive(&author.key, Action::Remove, &digest, 1_004);
        restored.apply(&remove, &keyring, 1_004).unwrap();
        assert_eq!(restored.get(&digest).unwrap().state, State::Removed);
        // A short digest names nothing.
        assert!(SignedDirective::sign(
            &crate::sign::Directive {
                action: Action::Remove,
                shield: digest[..12].to_string(),
                issued_at: 1,
            },
            &author.key
        )
        .is_err());
    }

    fn fixture() -> Fixture {
        let key = SigningKey::generate().unwrap();
        let mut keyring = Keyring::default();
        keyring.trust(key.public_key().clone());
        Fixture { key, keyring }
    }

    #[test]
    fn a_shield_is_monitored_before_it_is_enforced_and_every_change_is_measured() {
        let f = fixture();
        let mut registry = Registry::new(Policy {
            min_monitor_secs: 60,
            distinct_enforcer: true,
        });
        let json = spec("cve-x", 10_000);
        registry.propose(&json, "agent", 100).unwrap();
        assert_eq!(
            registry.in_force("sb", 100).count(),
            0,
            "a proposal is not in force"
        );
        assert!(registry.log().events().is_empty(), "nor measured");

        let signed = SignedShield::sign(&json, &f.key).unwrap();
        let digest = to_hex(&signed.digest());
        registry.load(&signed, &f.keyring, "alice", 100).unwrap();
        let (entry, mode) = registry.in_force("sb", 100).next().unwrap();
        assert_eq!(mode, Mode::Monitor);
        assert_eq!(entry.proposed_by.as_deref(), Some("agent"));

        // Too soon, then by the same person, then allowed.
        assert!(registry
            .set_mode(&digest, Mode::Enforce, "bob", 159)
            .is_err());
        assert!(registry
            .set_mode(&digest, Mode::Enforce, "alice", 160)
            .is_err());
        registry
            .set_mode(&digest, Mode::Enforce, "bob", 160)
            .unwrap();
        assert!(registry
            .set_mode(&digest, Mode::Enforce, "bob", 161)
            .is_err());
        assert_eq!(
            registry.in_force("sb", 161).next().unwrap().1,
            Mode::Enforce
        );

        // Removed by a prefix of its digest, once.
        registry.remove(&digest[..12], "bob", 200).unwrap();
        assert!(registry.remove(&digest, "bob", 201).is_err());
        assert_eq!(registry.in_force("sb", 201).count(), 0);
        let kinds: Vec<_> = registry.log().events().iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            [EventKind::Load, EventKind::Enforce, EventKind::Unload]
        );
        assert_eq!(registry.log().events()[1].by.as_deref(), Some("bob"));
    }

    #[test]
    fn what_is_not_trusted_or_has_expired_is_not_loaded() {
        let f = fixture();
        let mut registry = Registry::new(Policy::default());
        let other = SigningKey::generate().unwrap();
        let untrusted = SignedShield::sign(&spec("a", 10_000), &other).unwrap();
        assert!(registry.load(&untrusted, &f.keyring, "op", 1).is_err());
        let expired = SignedShield::sign(&spec("a", 50), &f.key).unwrap();
        assert!(registry.load(&expired, &f.keyring, "op", 50).is_err());
        assert!(registry.propose(&spec("a", 50), "op", 50).is_err());
        assert!(registry.log().events().is_empty());
    }

    #[test]
    fn one_shield_of_a_name_is_in_force() {
        let f = fixture();
        let mut registry = Registry::new(Policy::default());
        let first = SignedShield::sign(&spec("a", 10_000), &f.key).unwrap();
        let second = SignedShield::sign(&spec("a", 20_000), &f.key).unwrap();
        registry.load(&first, &f.keyring, "op", 1).unwrap();
        assert!(registry.load(&first, &f.keyring, "op", 2).is_err());
        assert!(registry.load(&second, &f.keyring, "op", 2).is_err());
        registry.remove(&to_hex(&first.digest()), "op", 3).unwrap();
        registry.load(&second, &f.keyring, "op", 4).unwrap();
    }

    #[test]
    fn shields_expire_at_the_sooner_of_their_expiry_and_the_longest_lifetime() {
        let f = fixture();
        let mut registry = Registry::new(Policy::default());
        let short = SignedShield::sign(&spec("short", 500), &f.key).unwrap();
        let long = SignedShield::sign(&spec("long", u64::MAX), &f.key).unwrap();
        registry.load(&short, &f.keyring, "op", 100).unwrap();
        registry.load(&long, &f.keyring, "op", 100).unwrap();
        assert!(registry.expire(499).is_empty());
        assert_eq!(registry.in_force("sb", 499).count(), 2);
        let expired = registry.expire(500);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].shield, "short");
        assert_eq!(registry.in_force("sb", 500).count(), 1);
        let end = 100 + MAX_LIFETIME_SECS;
        assert_eq!(
            registry.in_force("sb", end).count(),
            0,
            "never past its lifetime"
        );
        assert_eq!(registry.expire(end).len(), 1);
        assert_eq!(
            registry.log().events().last().unwrap().kind,
            EventKind::Expire
        );
    }

    #[test]
    fn a_revoked_shield_is_retired_and_a_restored_registry_is_checked() {
        let f = fixture();
        let mut registry = Registry::new(Policy::default());
        let signed = SignedShield::sign(&spec("a", 10_000), &f.key).unwrap();
        registry.load(&signed, &f.keyring, "op", 1).unwrap();
        registry.propose(&spec("b", 10_000), "agent", 1).unwrap();

        // Saved and restored as it was.
        let saved = registry.save();
        let (again, events) =
            Registry::restore(saved.clone(), Policy::default(), &f.keyring, 2).unwrap();
        assert!(events.is_empty());
        assert_eq!(again.in_force("sb", 2).count(), 1);
        assert_eq!(again.entries().count(), 2);

        // A shield edited on disk is not the one its digest names.
        let mut edited = saved.clone();
        edited.entries[0].spec_json = edited.entries[0].spec_json.replace("ptrace", "bpf");
        assert!(Registry::restore(edited, Policy::default(), &f.keyring, 2).is_err());
        // Nor is a history edited.
        let mut rewritten = saved.clone();
        rewritten.events[0].by = Some("mallory".into());
        assert!(Registry::restore(rewritten, Policy::default(), &f.keyring, 2).is_err());
        // An entry edited to carry no signature is refused, not trusted:
        // the log says it was loaded, which takes one.
        let mut unsigned = saved.clone();
        for entry in &mut unsigned.entries {
            if entry.state == State::Monitor {
                entry.signed = None;
            }
        }
        assert!(Registry::restore(unsigned, Policy::default(), &f.keyring, 2).is_err());

        // Revoked while running.
        let mut keyring = f.keyring.clone();
        keyring.revoke_shield(&signed.digest());
        let events = registry.apply_keyring(&keyring, 3);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EventKind::Revoke);
        assert_eq!(registry.in_force("sb", 3).count(), 0);
    }

    #[test]
    fn proposals_and_retired_shields_are_bounded() {
        let f = fixture();
        let mut registry = Registry::new(Policy::default());
        for i in 0..MAX_PROPOSED {
            registry
                .propose(&spec(&format!("p{i}"), 10_000), "agent", 1)
                .unwrap();
        }
        assert!(registry
            .propose(&spec("one-more", 10_000), "agent", 1)
            .is_err());
        let mut registry = Registry::new(Policy::default());
        for i in 0..MAX_RETIRED + 3 {
            let signed = SignedShield::sign(&spec(&format!("r{i}"), 10_000), &f.key).unwrap();
            registry.load(&signed, &f.keyring, "op", i as u64).unwrap();
            registry
                .remove(&to_hex(&signed.digest()), "op", i as u64)
                .unwrap();
        }
        assert_eq!(registry.entries().count(), MAX_RETIRED);
        assert_eq!(registry.log().events().len(), 2 * (MAX_RETIRED + 3));
    }

    #[test]
    fn a_shield_applies_only_where_its_scope_says() {
        let f = fixture();
        let mut registry = Registry::new(Policy::default());
        let json = r#"{"name":"a","scope":{"kind":"sandboxes","ids":["sb-1"]},"network":[{"cidr":"10.0.0.0/8"}],"expiresAt":10000}"#;
        let signed = SignedShield::sign(json, &f.key).unwrap();
        registry.load(&signed, &f.keyring, "op", 1).unwrap();
        assert_eq!(registry.in_force("sb-1", 1).count(), 1);
        assert_eq!(registry.in_force("sb-2", 1).count(), 0);
    }
}
