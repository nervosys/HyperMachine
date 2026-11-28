//! The node's syscall shields, in the guests that can apply them.
//!
//! Off unless the node runs with `--shield-guests`: a guest that has been sent
//! a rule list traps calls in every program it starts from then on, which costs
//! each trapped call a round trip to its agent, whatever the rules say. Without
//! the flag nothing here talks to a guest, and `syscallGuests` is empty.
//!
//! A VM sandbox makes its system calls to its own kernel, so the node cannot
//! trap them. The guest agent can, for the programs it starts (see
//! `hv2-guest-agent`'s `agentd/shield.rs`): this module is the node's end. It
//! translates the shields in force for a sandbox into the agent's own rule type
//! and pushes the whole list, and reads back what the agent matched into the
//! node's hit log.
//!
//! # When it pushes
//!
//! - when a sandbox's guest first answers ([`push_now`], called where the VM
//!   becomes ready), so a sandbox never runs a request unshielded because a
//!   sweep had not come round;
//! - whenever a shield is loaded, enforced, monitored, removed or retired by a
//!   keyring reload ([`GuestShields::nudge`]), at once, and on every 5 s tick,
//!   which also covers expiry, a restore, and a sandbox that was in standby.
//!
//! A push is the whole list for that sandbox (a state, not a delta) and is
//! skipped when it is the same list the same VM was last given.
//!
//! # What it tolerates
//!
//! - an agent that predates the operation (an old initrd): noted once, the
//!   sandbox is reported `unsupported`, and it is not asked again until its
//!   list or its VM changes;
//! - a guest that cannot trap calls (not root, old kernel): the same, with the
//!   guest's reason;
//! - a guest that does not answer in time: retried at the next sweep.
//!
//! # Hits
//!
//! Polled on the same sweep, not on `GET /shields/hits`: one place that talks
//! to guests, on a timeout, instead of a request path that fans out to every
//! VM and can hang on a paused one. A hit therefore shows at most ~5 s late.
//! The agent holds 1024 between reads and says how many it lost.
//!
//! # Hook rules
//!
//! A shield's `hooks` rules go the same way, by their own operation
//! (`SetHookShields`) and with their own record of what each guest was last
//! given (`pushed_hooks`), under the same `--shield-guests` flag. Three
//! differences:
//!
//! - A hook rule covers every process in the guest, not only the programs the
//!   agent starts, and is in force as soon as the guest has it.
//! - An empty list is never sent to a guest that was not sent a list before:
//!   it would change nothing, and an agent that predates hook shields would
//!   be marked unsupported for nothing. It is sent to lift rules.
//! - The agent reports counts, not calls: a hit read back says "N matches of
//!   this hook", and enters the log as one entry that adds N to the shield's
//!   tally.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use hv2_agent::{
    is_hook_shields_unavailable, is_syscall_shields_unavailable, is_unsupported_op, AgentError,
    AgentVM,
};
use hv2_guest_agent::{
    GuestArgMatch, GuestArgOp, GuestHit, GuestHookHit, GuestHookRule, GuestSyscallAction,
    GuestSyscallRule,
};
use hv2_shield::registry::Mode;
use hv2_shield::spec::{ArgOp, SyscallAction};
use hv2_shield::{now_secs, Live, Registry};
use serde_json::{json, Value};

use crate::AppState;

/// How long the node waits on one guest, for a push or a read.
const GUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// Guests talked to at once in a sweep.
const CONCURRENCY: usize = 16;

/// What the node knows of one sandbox's guest.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Status {
    /// The agent has this list.
    Applied,
    /// It cannot apply any: an agent from before the operation, or a guest
    /// that cannot trap calls. The reason is for the operator.
    Unsupported(String),
    /// It did not answer; ask again at the next sweep.
    Unreachable,
}

#[derive(Debug, Clone)]
struct Pushed {
    /// Which VM, so a restored or restarted sandbox of the same id is pushed
    /// to again.
    vm: usize,
    /// Of the list pushed; 0 for none.
    list: u64,
    status: Status,
}

/// What is known of every guest this node has pushed to.
#[derive(Default)]
pub(crate) struct GuestShields {
    /// `--shield-guests`.
    enabled: bool,
    pushed: parking_lot::Mutex<HashMap<String, Pushed>>,
    /// The same, for hook rules.
    pushed_hooks: parking_lot::Mutex<HashMap<String, Pushed>>,
    nudge: tokio::sync::Notify,
}

impl GuestShields {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    /// A shield changed: sweep now rather than at the next tick.
    pub(crate) fn nudge(&self) {
        self.nudge.notify_one();
    }

    /// Resolves when a shield has changed since the last time it did.
    pub(crate) async fn nudged(&self) {
        self.nudge.notified().await;
    }

    /// For `GET /shields`: each pushed-to sandbox and what became of it.
    ///
    /// Each entry is the syscall push's status, plus `"hooks"` with the hook
    /// push's when hook rules were pushed to that sandbox. A sandbox that was
    /// only ever pushed hook rules has just `"hooks"`.
    pub(crate) fn status(&self) -> Value {
        let describe = |status: &Status| match status {
            Status::Applied => json!({ "status": "applied" }),
            Status::Unsupported(why) => json!({ "status": "unsupported", "reason": why }),
            Status::Unreachable => json!({ "status": "unreachable" }),
        };
        let pushed = self.pushed.lock();
        let pushed_hooks = self.pushed_hooks.lock();
        let mut out = serde_json::Map::new();
        for (id, p) in pushed.iter() {
            out.insert(id.clone(), describe(&p.status));
        }
        for (id, p) in pushed_hooks.iter() {
            let entry = out.entry(id.clone()).or_insert_with(|| json!({}));
            if let Some(entry) = entry.as_object_mut() {
                entry.insert("hooks".into(), describe(&p.status));
            }
        }
        Value::Object(out)
    }
}

/// The shields in force for `sandbox`, as the guest agent takes them: one rule
/// for each syscall rule, shield by shield in the order the registry yields
/// them (the order `Live::syscall` judges them in), a shield's rules in the
/// order it wrote them, so the first-match semantics are the same ones.
pub(crate) fn translate(registry: &Registry, sandbox: &str, now: u64) -> Vec<GuestSyscallRule> {
    registry
        .in_force(sandbox, now)
        .flat_map(|(entry, mode)| {
            entry
                .spec()
                .syscalls
                .iter()
                .map(move |rule| GuestSyscallRule {
                    shield: entry.spec().name.clone(),
                    digest: entry.digest.clone(),
                    syscall: rule.syscall.clone(),
                    args: rule
                        .args
                        .iter()
                        .map(|a| GuestArgMatch {
                            index: a.index,
                            op: match a.op {
                                ArgOp::Eq => GuestArgOp::Eq,
                                ArgOp::Ne => GuestArgOp::Ne,
                                ArgOp::Lt => GuestArgOp::Lt,
                                ArgOp::Le => GuestArgOp::Le,
                                ArgOp::Gt => GuestArgOp::Gt,
                                ArgOp::Ge => GuestArgOp::Ge,
                                ArgOp::AnyBits => GuestArgOp::AnyBits,
                                ArgOp::AllBits => GuestArgOp::AllBits,
                            },
                            value: a.value,
                            mask: a.mask,
                        })
                        .collect(),
                    action: match rule.action {
                        SyscallAction::Errno { errno } => GuestSyscallAction::Errno { errno },
                        SyscallAction::Kill => GuestSyscallAction::Kill,
                    },
                    enforce: mode == Mode::Enforce,
                    // The agent's own backstop, on the guest's clock: the registry's
                    // expiry (which already caps a shield at its maximum lifetime).
                    expires_at: entry.expires_at,
                })
        })
        .collect()
}

/// The hook rules of the shields in force for `sandbox`, as the guest agent
/// takes them: every `hooks` rule of every shield, shield by shield in the
/// order the registry yields them, a shield's rules in the order it wrote
/// them. A hook rule decides by the first enforcing shield in this order, as
/// the agent's `attribute` and the engine's merge both do.
pub(crate) fn translate_hooks(registry: &Registry, sandbox: &str, now: u64) -> Vec<GuestHookRule> {
    registry
        .in_force(sandbox, now)
        .flat_map(|(entry, mode)| {
            entry.spec().hooks.iter().map(move |rule| GuestHookRule {
                shield: entry.spec().name.clone(),
                digest: entry.digest.clone(),
                hook: rule.hook.clone(),
                errno: rule.errno,
                enforce: mode == Mode::Enforce,
                // The agent's own backstop, on the guest's clock: the registry's
                // expiry, as for syscall rules.
                expires_at: entry.expires_at,
            })
        })
        .collect()
}

fn fingerprint(rules: &[GuestSyscallRule]) -> u64 {
    fingerprint_of(rules)
}

fn fingerprint_hooks(rules: &[GuestHookRule]) -> u64 {
    fingerprint_of(rules)
}

fn fingerprint_of<T: serde::Serialize>(rules: &[T]) -> u64 {
    if rules.is_empty() {
        return 0;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_vec(rules)
        .unwrap_or_default()
        .hash(&mut hasher);
    // Never 0, which means "none".
    hasher.finish() | 1
}

/// How a hit reads in the node's log, as `Live::syscall` writes its own.
pub(crate) fn what(hit: &GuestHit) -> String {
    let a = &hit.args;
    format!(
        "{}({:#x}, {:#x}, {:#x}, {:#x}, {:#x}, {:#x}) [guest pid {}]",
        hit.syscall, a[0], a[1], a[2], a[3], a[4], a[5], hit.pid
    )
}

/// Give a sandbox's guest the shields in force for it, unless it already has
/// exactly them. Never fails the caller: a sandbox the agent cannot shield is
/// reported, not refused (refusing would make a node with one old template
/// unable to start sandboxes whenever any shield exists).
pub(crate) async fn push_now(state: &AppState, id: &str, vm: &Arc<AgentVM>) {
    if !state.shields.guests.enabled {
        return;
    }
    let (rules, hook_rules) = {
        let registry = state.shields.live.registry();
        let now = now_secs();
        (
            translate(&registry, id, now),
            translate_hooks(&registry, id, now),
        )
    };
    sync_one(&state.shields.guests, id, vm, rules).await;
    sync_hooks(&state.shields.guests, id, vm, hook_rules).await;
}

/// Give a sandbox's guest the hook rules in force for it, unless it already
/// has exactly them. As [`sync_one`], except that an empty list is sent only
/// to a guest this VM was sent a list before, to lift it.
async fn sync_hooks(guests: &GuestShields, id: &str, vm: &Arc<AgentVM>, rules: Vec<GuestHookRule>) {
    let key = Arc::as_ptr(vm) as usize;
    let list = fingerprint_hooks(&rules);
    {
        let mut pushed = guests.pushed_hooks.lock();
        let prior = pushed
            .get(id)
            .filter(|p| p.vm == key)
            .map(|p| (p.list, p.status.clone()));
        match prior {
            // Same VM, same list, and the agent either has it or cannot take
            // any: nothing to say. Unreachable is retried.
            Some((was, status)) if was == list && status != Status::Unreachable => return,
            // It cannot take a list, so it has none to lift.
            Some((_, Status::Unsupported(_))) if rules.is_empty() => {
                if let Some(p) = pushed.get_mut(id) {
                    p.list = 0;
                }
                return;
            }
            // Never sent a list: there is nothing for an empty one to lift.
            None if rules.is_empty() => {
                pushed.remove(id);
                return;
            }
            _ => {}
        }
    }
    let status = match vm.set_hook_shields_in_guest(rules, GUEST_TIMEOUT).await {
        Ok(()) => Status::Applied,
        Err(e) if is_unsupported_op(&e) => {
            Status::Unsupported("the guest agent predates hook shields".into())
        }
        Err(e) if is_hook_shields_unavailable(&e) => Status::Unsupported(guest_reason(&e)),
        Err(e) => {
            tracing::debug!("hook shields for {id}: guest not reached: {e}");
            Status::Unreachable
        }
    };
    let mut pushed = guests.pushed_hooks.lock();
    let was = pushed.get(id).map(|p| p.status.clone());
    if let Status::Unsupported(why) = &status {
        if was.as_ref() != Some(&status) {
            tracing::warn!("sandbox {id} is not hook-shielded: {why}");
        }
    }
    pushed.insert(
        id.to_string(),
        Pushed {
            vm: key,
            list,
            status,
        },
    );
}

async fn sync_one(
    guests: &GuestShields,
    id: &str,
    vm: &Arc<AgentVM>,
    rules: Vec<GuestSyscallRule>,
) {
    let key = Arc::as_ptr(vm) as usize;
    let list = fingerprint(&rules);
    {
        let pushed = guests.pushed.lock();
        match pushed.get(id) {
            // Same VM, same list, and the agent either has it or cannot take
            // any: nothing to say. Unreachable is retried.
            Some(p) if p.vm == key && p.list == list && p.status != Status::Unreachable => return,
            // Never pushed: even an empty list is sent, because the first list
            // is what turns trapping on, so a shield loaded later finds the
            // programs started since then trapped.
            _ => {}
        }
    }
    let status = match vm.set_syscall_shields_in_guest(rules, GUEST_TIMEOUT).await {
        Ok(()) => Status::Applied,
        Err(e) if is_unsupported_op(&e) => {
            Status::Unsupported("the guest agent predates syscall shields".into())
        }
        Err(e) if is_syscall_shields_unavailable(&e) => Status::Unsupported(guest_reason(&e)),
        Err(e) => {
            tracing::debug!("syscall shields for {id}: guest not reached: {e}");
            Status::Unreachable
        }
    };
    let mut pushed = guests.pushed.lock();
    let was = pushed.get(id).map(|p| p.status.clone());
    if let Status::Unsupported(why) = &status {
        if was.as_ref() != Some(&status) {
            tracing::warn!("sandbox {id} is not syscall-shielded: {why}");
        }
    }
    pushed.insert(
        id.to_string(),
        Pushed {
            vm: key,
            list,
            status,
        },
    );
}

/// One pass: bring every live guest up to date and read what it matched.
/// Run on the 5 s tick and whenever a shield changes.
pub(crate) async fn sweep(state: &Arc<AppState>) {
    if !state.shields.guests.enabled {
        return;
    }
    let mut targets: Vec<(String, Arc<AgentVM>)> = state
        .sandboxes
        .lock()
        .iter()
        .map(|(id, live)| (id.clone(), Arc::clone(&live.vm)))
        .collect();
    targets.extend(crate::machines::agent_vms());
    // A guest in standby is not asked: it would only time out. It is caught up
    // on the sweep after it wakes.
    targets.retain(|(_, vm)| !vm.in_standby());

    let live_ids: HashSet<&str> = targets.iter().map(|(id, _)| id.as_str()).collect();
    state
        .shields
        .guests
        .pushed
        .lock()
        .retain(|id, _| live_ids.contains(id.as_str()));
    state
        .shields
        .guests
        .pushed_hooks
        .lock()
        .retain(|id, _| live_ids.contains(id.as_str()));

    for chunk in targets.chunks(CONCURRENCY) {
        let mut tasks = tokio::task::JoinSet::new();
        for (id, vm) in chunk {
            let state = Arc::clone(state);
            let (id, vm) = (id.clone(), Arc::clone(vm));
            tasks.spawn(async move {
                let (rules, hook_rules) = {
                    let registry = state.shields.live.registry();
                    let now = now_secs();
                    (
                        translate(&registry, &id, now),
                        translate_hooks(&registry, &id, now),
                    )
                };
                sync_one(&state.shields.guests, &id, &vm, rules).await;
                sync_hooks(&state.shields.guests, &id, &vm, hook_rules).await;
                read_hits(&state, &id, &vm).await;
                read_hook_hits(&state, &id, &vm).await;
            });
        }
        while tasks.join_next().await.is_some() {}
    }
}

/// Take a guest's recorded matches into the node's log, if it has been given
/// shields at all.
async fn read_hits(state: &AppState, id: &str, vm: &Arc<AgentVM>) {
    let applied = state
        .shields
        .guests
        .pushed
        .lock()
        .get(id)
        .is_some_and(|p| p.status == Status::Applied);
    if !applied {
        return;
    }
    let (hits, dropped) = match vm.syscall_shield_hits_in_guest(GUEST_TIMEOUT).await {
        Ok(read) => read,
        Err(e) => {
            tracing::debug!("syscall shield hits for {id}: guest not reached: {e}");
            return;
        }
    };
    record(&state.shields.live, id, &hits, dropped, now_secs());
}

/// The guest is not trusted: every hit is checked against this node's
/// registry by `Live::record_external`, and what it cannot place is counted.
pub(crate) fn record(live: &Live, id: &str, hits: &[GuestHit], dropped: u64, now: u64) {
    for hit in hits {
        live.record_external(id, &hit.digest, &what(hit), hit.enforced, now, 1);
    }
    if dropped > 0 {
        live.record_lost(dropped);
    }
}

/// Take a guest's counted hook matches into the node's log, if it has been
/// given hook rules at all.
async fn read_hook_hits(state: &AppState, id: &str, vm: &Arc<AgentVM>) {
    let applied = state
        .shields
        .guests
        .pushed_hooks
        .lock()
        .get(id)
        .is_some_and(|p| p.status == Status::Applied);
    if !applied {
        return;
    }
    let (hits, dropped) = match vm.hook_shield_hits_in_guest(GUEST_TIMEOUT).await {
        Ok(read) => read,
        Err(e) => {
            tracing::debug!("hook shield hits for {id}: guest not reached: {e}");
            return;
        }
    };
    record_hooks(&state.shields.live, id, &hits, dropped, now_secs());
}

/// As [`record`], for hook hits. A hit stands for `count` matches the guest's
/// kernel counted: one entry in the log, `count` on the shield's tally.
pub(crate) fn record_hooks(live: &Live, id: &str, hits: &[GuestHookHit], dropped: u64, now: u64) {
    for hit in hits {
        live.record_external(
            id,
            &hit.digest,
            &format!("hook {}", hit.hook),
            hit.enforced,
            now,
            hit.count,
        );
    }
    if dropped > 0 {
        live.record_lost(dropped);
    }
}

/// What a guest said when it could not apply rules, without the "Script
/// error: " that wraps every failure an agent reports: it is not a script.
fn guest_reason(error: &AgentError) -> String {
    match error {
        AgentError::Script(message) => message.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hv2_shield::registry::Policy;
    use hv2_shield::{Keyring, SignedShield, SigningKey};

    const SPEC: &str = r#"{"name":"no-unshare","syscalls":[
        {"syscall":"unshare","args":[{"index":0,"op":"anyBits","value":268435456}],
         "action":{"kind":"errno","errno":1}},
        {"syscall":"ptrace","action":{"kind":"kill"}}],
        "scope":{"kind":"sandboxes","ids":["sb"]},"expiresAt":4102444800}"#;

    const HOOKS_A: &str = r#"{"name":"no-mount","hooks":[
        {"hook":"sb_mount","errno":1},{"hook":"socket_create","errno":13}],
        "scope":{"kind":"sandboxes","ids":["sb"]},"expiresAt":4102444800}"#;
    const HOOKS_B: &str = r#"{"name":"no-sockets","hooks":[{"hook":"socket_create","errno":99}],
        "scope":{"kind":"sandboxes","ids":["sb"]},"expiresAt":4102444800}"#;

    /// A node with each of `specs` loaded, in monitor mode, and their digests.
    fn live_with(specs: &[&str]) -> (Live, Vec<String>) {
        let key = SigningKey::generate().unwrap();
        let mut keyring = Keyring::default();
        keyring.trust(key.public_key().clone());
        let live = Live::new(
            Registry::new(Policy {
                min_monitor_secs: 0,
                ..Policy::default()
            }),
            keyring,
        );
        let mut digests = Vec::new();
        for spec in specs {
            let signed = SignedShield::sign(spec, &key).unwrap();
            live.update(|r, k| r.load(&signed, k, &signed.signer, 1))
                .unwrap();
            digests.push(hv2_shield::to_hex(&signed.digest()));
        }
        (live, digests)
    }

    fn live() -> (Live, String) {
        let (live, mut digests) = live_with(&[SPEC]);
        (live, digests.remove(0))
    }

    #[test]
    fn a_shield_in_force_becomes_its_rules_in_order_with_its_mode() {
        let (live, digest) = live();
        let rules = translate(&live.registry(), "sb", 2);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].syscall, "unshare");
        assert_eq!(rules[0].digest, digest);
        assert_eq!(rules[0].shield, "no-unshare");
        assert_eq!(rules[0].args[0].op, GuestArgOp::AnyBits);
        assert_eq!(rules[0].args[0].value, 268_435_456);
        assert_eq!(rules[0].args[0].mask, u64::MAX);
        assert_eq!(rules[0].action, GuestSyscallAction::Errno { errno: 1 });
        assert_eq!(rules[1].action, GuestSyscallAction::Kill);
        assert!(rules.iter().all(|r| !r.enforce), "loaded in monitor mode");
        // The registry's expiry, capped at the lifetime limit from loading,
        // not the far-off one the shield asked for.
        let capped = live.registry().get(&digest).unwrap().expires_at;
        assert!(capped < 4_102_444_800);
        assert!(rules.iter().all(|r| r.expires_at == capped));

        live.update(|r, _| r.set_mode(&digest, Mode::Enforce, "op", 3))
            .unwrap();
        assert!(translate(&live.registry(), "sb", 4)
            .iter()
            .all(|r| r.enforce));

        // Out of scope, and expired: none.
        assert!(translate(&live.registry(), "other", 4).is_empty());
        assert!(translate(&live.registry(), "sb", 4_102_444_800).is_empty());
    }

    #[test]
    fn what_the_guest_translation_decides_is_what_the_node_would_decide() {
        // The same registry judged two ways: in the node (`Live::syscall`) and
        // by the agent's evaluator over the translated list.
        let (live, digest) = live();
        live.update(|r, _| r.set_mode(&digest, Mode::Enforce, "op", 3))
            .unwrap();
        let rules = translate(&live.registry(), "sb", 4);
        for (call, args) in [
            ("unshare", [0x1000_0000u64, 0, 0, 0, 0, 0]),
            ("unshare", [0; 6]),
            ("ptrace", [0; 6]),
            ("bpf", [0; 6]),
        ] {
            let node = live.syscall("sb", call, &args, 4);
            let guest = hv2_guest_agent::syscall_shield::evaluate(&rules, call, &args, 4);
            let guest = guest.verdict.map(|a| match a {
                GuestSyscallAction::Errno { errno } => SyscallAction::Errno { errno },
                GuestSyscallAction::Kill => SyscallAction::Kill,
            });
            assert_eq!(node, guest, "{call} {args:?}");
        }
    }

    #[test]
    fn a_fingerprint_changes_with_the_list_and_is_zero_for_none() {
        let (live, digest) = live();
        assert_eq!(fingerprint(&[]), 0);
        let monitored = translate(&live.registry(), "sb", 2);
        live.update(|r, _| r.set_mode(&digest, Mode::Enforce, "op", 3))
            .unwrap();
        let enforced = translate(&live.registry(), "sb", 4);
        assert_ne!(fingerprint(&monitored), 0);
        assert_ne!(fingerprint(&monitored), fingerprint(&enforced));
        assert_eq!(fingerprint(&enforced), fingerprint(&enforced.clone()));
    }

    #[test]
    fn guest_hits_enter_the_log_checked_against_the_registry() {
        let (live, digest) = live();
        let hit = |digest: &str, enforced| GuestHit {
            at: 0,
            shield: "forged name".into(),
            digest: digest.into(),
            syscall: "unshare".into(),
            args: [0x1000_0000, 0, 0, 0, 0, 0],
            enforced,
            pid: 77,
        };
        record(
            &live,
            "sb",
            &[hit(&digest, true), hit(&"0".repeat(64), true)],
            5,
            9,
        );
        let hits = live.hits();
        assert_eq!(hits.recorded, 1);
        assert_eq!(hits.lost, 6, "5 the guest dropped, 1 it could not place");
        assert_eq!(hits.hits[0].shield, "no-unshare");
        assert_eq!(hits.hits[0].at, 9, "the node's clock");
        assert_eq!(
            hits.hits[0].what,
            "unshare(0x10000000, 0x0, 0x0, 0x0, 0x0, 0x0) [guest pid 77]"
        );
    }

    #[test]
    fn hook_rules_follow_the_registrys_order_with_each_shields_mode_and_expiry() {
        let (live, digests) = live_with(&[SPEC, HOOKS_A, HOOKS_B]);
        let (digest_a, digest_b) = (&digests[1], &digests[2]);
        let rules = translate_hooks(&live.registry(), "sb", 2);
        // The registry's own order, shield by shield; the syscall-only shield
        // contributes nothing, and a shield's rules stay in the order written.
        let mut expected: Vec<(String, &str)> = Vec::new();
        for (entry, _) in live.registry().in_force("sb", 2) {
            let hooks: &[&str] = if entry.digest == *digest_a {
                &["sb_mount", "socket_create"]
            } else if entry.digest == *digest_b {
                &["socket_create"]
            } else {
                &[]
            };
            expected.extend(hooks.iter().map(|h| (entry.digest.clone(), *h)));
        }
        assert_eq!(expected.len(), 3);
        assert_eq!(
            rules
                .iter()
                .map(|r| (r.digest.clone(), r.hook.as_str()))
                .collect::<Vec<_>>(),
            expected
        );
        for rule in &rules {
            let (name, errno) = match (rule.digest == *digest_a, rule.hook.as_str()) {
                (true, "sb_mount") => ("no-mount", 1),
                (true, _) => ("no-mount", 13),
                (false, _) => ("no-sockets", 99),
            };
            assert_eq!((rule.shield.as_str(), rule.errno), (name, errno));
            assert!(!rule.enforce, "loaded in monitor mode");
            // The registry's expiry, capped at the lifetime limit from loading.
            let capped = live.registry().get(&rule.digest).unwrap().expires_at;
            assert!(capped < 4_102_444_800);
            assert_eq!(rule.expires_at, capped);
        }

        live.update(|r, _| r.set_mode(digest_a, Mode::Enforce, "op", 3))
            .unwrap();
        for rule in translate_hooks(&live.registry(), "sb", 4) {
            assert_eq!(rule.enforce, rule.digest == *digest_a, "{rule:?}");
        }

        // Out of scope, and expired: none.
        assert!(translate_hooks(&live.registry(), "other", 4).is_empty());
        assert!(translate_hooks(&live.registry(), "sb", 4_102_444_800).is_empty());
        // A list of syscall rules alone is no hook rules.
        let (syscalls_only, _) = self::live();
        assert!(translate_hooks(&syscalls_only.registry(), "sb", 2).is_empty());
    }

    #[test]
    fn a_hook_list_has_a_fingerprint_that_changes_with_it_and_is_zero_for_none() {
        let (live, digests) = live_with(&[HOOKS_A]);
        assert_eq!(fingerprint_hooks(&[]), 0);
        let monitored = translate_hooks(&live.registry(), "sb", 2);
        live.update(|r, _| r.set_mode(&digests[0], Mode::Enforce, "op", 3))
            .unwrap();
        let enforced = translate_hooks(&live.registry(), "sb", 4);
        assert_ne!(fingerprint_hooks(&monitored), 0);
        assert_ne!(fingerprint_hooks(&monitored), fingerprint_hooks(&enforced));
    }

    #[test]
    fn guest_hook_hits_enter_the_log_with_their_count() {
        let (live, digests) = live_with(&[HOOKS_A]);
        let digest = &digests[0];
        live.update(|r, _| r.set_mode(digest, Mode::Enforce, "op", 3))
            .unwrap();
        let hit = |digest: &str, enforced, count| GuestHookHit {
            at: 0,
            shield: "forged name".into(),
            digest: digest.into(),
            hook: "sb_mount".into(),
            enforced,
            count,
        };
        record_hooks(
            &live,
            "sb",
            &[
                hit(digest, true, 5),
                hit(digest, false, 2),
                hit(&"0".repeat(64), true, 3),
            ],
            4,
            9,
        );
        let hits = live.hits();
        assert_eq!(hits.recorded, 2, "one entry per hit, whatever its count");
        assert_eq!(
            hits.lost, 7,
            "4 the guest dropped, 3 matches it could not place"
        );
        assert_eq!(hits.hits[0].shield, "no-mount", "the registry's name");
        assert_eq!(hits.hits[0].at, 9, "the node's clock");
        assert_eq!(hits.hits[0].what, "hook sb_mount");
        assert!(hits.hits[0].enforced && !hits.hits[1].enforced);
        assert_eq!(
            hits.tally[digest],
            hv2_shield::Tally {
                monitored: 2,
                refused: 5
            }
        );
    }

    #[test]
    fn the_status_lists_hook_pushes_beside_syscall_ones_and_alone() {
        let guests = GuestShields::new(true);
        let pushed = |status| Pushed {
            vm: 1,
            list: 1,
            status,
        };
        guests
            .pushed
            .lock()
            .insert("both".into(), pushed(Status::Applied));
        guests.pushed_hooks.lock().insert(
            "both".into(),
            pushed(Status::Unsupported(
                "the guest agent predates hook shields".into(),
            )),
        );
        guests
            .pushed_hooks
            .lock()
            .insert("hooks-only".into(), pushed(Status::Unreachable));
        guests
            .pushed
            .lock()
            .insert("syscalls-only".into(), pushed(Status::Unreachable));
        assert_eq!(
            guests.status(),
            json!({
                "both": {
                    "status": "applied",
                    "hooks": {
                        "status": "unsupported",
                        "reason": "the guest agent predates hook shields"
                    }
                },
                "hooks-only": { "hooks": { "status": "unreachable" } },
                "syscalls-only": { "status": "unreachable" }
            })
        );
    }

    #[test]
    fn a_guests_reason_is_its_own_words() {
        let said = format!(
            "{} cannot read /sys/kernel/security/lsm",
            hv2_agent::HOOK_SHIELDS_UNAVAILABLE
        );
        let error = AgentError::Script(said.clone());
        assert!(is_hook_shields_unavailable(&error));
        assert_eq!(guest_reason(&error), said);
        assert!(!guest_reason(&error).starts_with("Script error"));
    }
}
