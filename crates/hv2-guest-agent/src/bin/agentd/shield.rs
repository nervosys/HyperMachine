//! Syscall shields for the programs this agent starts.
//!
//! The node's shields refuse system calls in process sandboxes with a seccomp
//! user-notify filter and a supervisor thread (`hv2_sandbox::seccomp_notify`).
//! A VM sandbox's guest makes its calls to its own kernel, which the host does
//! not see; this module gives the in-guest agent the same engine, so a rule
//! the operator loads reaches the programs in a guest too.
//!
//! # Off until the host turns it on
//!
//! Trapping costs every trapped call a round trip to this agent, so nothing
//! is trapped until the host first sends `SetSyscallShields` (an empty list
//! will do). A node does that only when it runs with `--shield-guests`; on
//! any other node a guest pays nothing. Programs started before that first
//! list are not shielded later: a filter cannot be added to a running process
//! from outside it.
//!
//! # How a program is shielded
//!
//! [`attach`] is called on the `Command` of every `Exec` and `Start`, before
//! the user switch is registered. Once the host has turned shields on, it:
//!
//! 1. builds a filter that traps every call in the engine's trappable list
//!    (less the "hot" ones no rule names, see [`HOT`]);
//! 2. starts a supervisor thread in this agent, on one end of a socket pair,
//!    BEFORE the spawn (the child's own `setuid` is a trapped call and `spawn`
//!    does not return until `exec`), and detaches it: it answers until the
//!    kernel says no process holds the filter any more, so a daemon the
//!    program leaves behind stays covered;
//! 3. registers a `pre_exec` closure that installs the filter on the child.
//!
//! The filter is installed while the child is still root, with
//! `CAP_SYS_ADMIN` standing in for no-new-privileges. NNP is NOT set:
//! guest workloads use `sudo` and setuid binaries, and NNP would silently turn
//! the set-id bit off for everything under the filter. The switch to the
//! requested user follows in a later `pre_exec` closure (this agent does the
//! switch by hand, with `setgroups`/`setgid`/`setuid`, not `Command::uid`).
//!
//! Why the order matters: std applies `Command::uid/gid/groups` BEFORE it runs
//! `pre_exec` closures (in `do_exec`: stdio dup2, then setgroups/setgid/setuid,
//! then chroot/cwd/pgroup, then signal-mask reset, then the closures in
//! registration order, then exec). A closure therefore could not install a
//! filter as root if the switch were `Command::uid`. Because this agent does
//! the switch inside its own closure, registration order is what decides, and
//! `attach` is called before the switch closure is registered.
//!
//! # The child's own calls
//!
//! `setuid`, `setgid`, `setresuid`, `setresgid` are in the trappable list, so
//! the child's privilege drop is trapped by its own filter. The gate allows
//! everything until the first `execve`/`execveat` notification, which is the
//! child's own initial exec: it is allowed too and arms the gate. Until then
//! the only code running under the filter is this agent's own `pre_exec`
//! closures. From the first exec on, every call is judged by the current rules.
//! An initial exec of the host-requested program is never itself a guest act,
//! so it is neither refused nor recorded.
//!
//! # Where this is not safe to rely on
//!
//! - An agent that is not root cannot install without NNP: no shields, and
//!   `SetSyscallShields` says so. Programs run unshielded; the host is told.
//! - A guest process that is root can kill this agent, and with it the
//!   supervisor: calls trapped by a filter whose listener is gone fail with
//!   ENOSYS. Shields stop a compromised unprivileged process and a root one
//!   that only uses the calls a rule names; they are not a barrier against
//!   guest root (the shield module of the host says the same of the guest's
//!   kernel).
//! - 32-bit and x32 syscall entry is killed by the filter (as in process
//!   sandboxes): the engine's filter must not let those numbers past the list.

use std::collections::VecDeque;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once, OnceLock, RwLock};

use hv2_guest_agent::syscall_shield::{evaluate, validate_rules};
use hv2_guest_agent::{GuestHit, GuestSyscallAction, GuestSyscallRule, OpResult, HIT_RING};
use hv2_sandbox::seccomp_notify as notify;
use notify::{GateVerdict, SyscallGate, TrappedCall};

/// What `SetSyscallShields` answers, up to the reason, when this guest cannot
/// shield: the host matches on it.
pub const UNAVAILABLE: &str = "syscall shields unavailable:";

/// Calls made often enough that trapping them would slow a workload that
/// uses them for no reason: trapped only if a rule in force names them when
/// the program starts. A rule about one of these loaded later reaches only
/// programs started after it. (`execve` is hot-ish but always trapped: the
/// gate arms on it.)
const HOT: &[&str] = &["io_uring_enter", "splice", "tee", "vmsplice"];

/// The rules and the hits, in one place a test can make its own copy of.
struct Shared {
    /// Set by the first rule list the host sends, valid or empty; until then
    /// programs are started untrapped.
    enabled: AtomicBool,
    rules: RwLock<Arc<Vec<GuestSyscallRule>>>,
    hits: Mutex<Ring>,
}

impl Shared {
    fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            rules: RwLock::new(Arc::new(Vec::new())),
            hits: Mutex::new(Ring::with_capacity(HIT_RING)),
        }
    }

    fn rules(&self) -> Arc<Vec<GuestSyscallRule>> {
        Arc::clone(&self.rules.read().unwrap_or_else(|e| e.into_inner()))
    }

    fn replace(&self, rules: Vec<GuestSyscallRule>) {
        *self.rules.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(rules);
    }

    fn record(&self, hit: GuestHit) {
        self.hits
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(hit);
    }

    fn drain(&self) -> (Vec<GuestHit>, u64) {
        self.hits.lock().unwrap_or_else(|e| e.into_inner()).drain()
    }

    /// What to do with a trapped call, recording what matched.
    fn decide(&self, syscall: &str, args: &[u64; 6], pid: u32) -> GateVerdict {
        let rules = self.rules();
        if rules.is_empty() {
            return GateVerdict::Allow;
        }
        let now = unix_now();
        let evaluation = evaluate(&rules, syscall, args, now);
        for (shield, digest, enforced) in evaluation.matched {
            self.record(GuestHit {
                at: now,
                shield,
                digest,
                syscall: syscall.to_string(),
                args: *args,
                enforced,
                pid,
            });
        }
        match evaluation.verdict {
            Some(GuestSyscallAction::Errno { errno }) => GateVerdict::Deny { errno },
            Some(GuestSyscallAction::Kill) => GateVerdict::Kill,
            None => GateVerdict::Allow,
        }
    }
}

fn shared() -> &'static Arc<Shared> {
    static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
    SHARED.get_or_init(|| Arc::new(Shared::new()))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The hits not yet read: bounded, with a count of what did not fit.
///
/// When full, a new hit is dropped and counted -- except an enforced one,
/// which takes the place of the oldest monitored hit if there is one: a flood
/// of "would have refused" must not hide a refusal.
struct Ring {
    items: VecDeque<GuestHit>,
    dropped: u64,
    capacity: usize,
    /// Monitored hits in `items`, so a full ring of refusals is not scanned.
    monitored: usize,
}

impl Ring {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            items: VecDeque::new(),
            dropped: 0,
            capacity,
            monitored: 0,
        }
    }

    fn push(&mut self, hit: GuestHit) {
        if self.items.len() >= self.capacity {
            let victim = (hit.enforced && self.monitored > 0)
                .then(|| self.items.iter().position(|h| !h.enforced))
                .flatten();
            let Some(victim) = victim else {
                self.dropped = self.dropped.saturating_add(1);
                return;
            };
            self.items.remove(victim);
            self.monitored -= 1;
            self.dropped = self.dropped.saturating_add(1);
        }
        if !hit.enforced {
            self.monitored += 1;
        }
        self.items.push_back(hit);
    }

    fn drain(&mut self) -> (Vec<GuestHit>, u64) {
        self.monitored = 0;
        (
            self.items.drain(..).collect(),
            std::mem::take(&mut self.dropped),
        )
    }
}

/// The gate for one started program and everything it starts.
struct ChildGate {
    shared: Arc<Shared>,
    /// False until the child's first `execve`: until then the only code
    /// under the filter is this agent's own `pre_exec` closures.
    armed: AtomicBool,
}

impl SyscallGate for ChildGate {
    fn decide(&self, call: &TrappedCall<'_>) -> GateVerdict {
        if !self.armed.load(Ordering::Acquire) {
            if matches!(call.syscall, "execve" | "execveat") {
                self.armed.store(true, Ordering::Release);
            }
            return GateVerdict::Allow;
        }
        self.shared.decide(call.syscall, &call.args, call.pid)
    }
}

/// Whether this guest can shield, decided once.
fn support() -> &'static Result<(), String> {
    static SUPPORT: OnceLock<Result<(), String>> = OnceLock::new();
    SUPPORT.get_or_init(|| {
        if std::env::var("HV2_SHIELDS").is_ok_and(|v| v == "off") {
            return Err("turned off by HV2_SHIELDS=off in the agent's environment".into());
        }
        // SAFETY: geteuid takes no arguments.
        if unsafe { libc::geteuid() } != 0 {
            return Err(
                "the agent is not root, and installing a filter without no-new-privileges \
                 (which would break sudo and setuid programs) needs CAP_SYS_ADMIN"
                    .into(),
            );
        }
        notify::probe_with(false)
    })
}

/// Calls to leave untrapped, from `HV2_SHIELD_SKIP=connect,socket`: an
/// operator's way to take a hot call out of a guest that makes it constantly.
fn skipped() -> &'static Vec<String> {
    static SKIP: OnceLock<Vec<String>> = OnceLock::new();
    SKIP.get_or_init(|| {
        std::env::var("HV2_SHIELD_SKIP")
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// The calls a program started now has trapped.
fn trapped_names(rules: &[GuestSyscallRule]) -> Vec<String> {
    let skip = skipped();
    notify::trappable()
        .filter(|name| {
            // The gate arms on execve: it is always trapped.
            let always = matches!(*name, "execve" | "execveat");
            always
                || (!skip.iter().any(|s| s == *name)
                    && (!HOT.contains(name) || rules.iter().any(|r| r.syscall == *name)))
        })
        .map(str::to_string)
        .collect()
}

/// Held by the caller across `spawn`, then dropped: the parent's copy of the
/// child's end of the listener channel. Dropping it lets the supervisor see a
/// child that never sent a listener (the spawn failed first).
pub struct Attached {
    _child_end: OwnedFd,
}

/// Shield the program `command` will start. Call after the command is
/// otherwise configured but BEFORE registering the closure that drops to
/// another user. `Ok(None)`: this guest cannot shield (the host was or will
/// be told by `SetSyscallShields`); the program runs unshielded.
///
/// # Errors
///
/// The reason the program must not be started: shields are supported here, yet
/// the filter or its supervisor could not be set up. Failing closed: starting
/// the program without them would be a silent hole.
pub fn attach(command: &mut Command) -> Result<Option<Attached>, String> {
    attach_with(command, shared())
}

fn attach_with(command: &mut Command, shared: &Arc<Shared>) -> Result<Option<Attached>, String> {
    if !shared.enabled.load(Ordering::Acquire) {
        return Ok(None);
    }
    if let Err(why) = support() {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| eprintln!("hv2-guest-agentd: {UNAVAILABLE} {why}"));
        return Ok(None);
    }
    let names = trapped_names(&shared.rules());
    let filter = notify::Filter::new(&names).map_err(|e| format!("syscall shield filter: {e}"))?;
    let (ours, theirs) =
        notify::channel().map_err(|e| format!("syscall shield socket pair: {e}"))?;
    let gate = Arc::new(ChildGate {
        shared: Arc::clone(shared),
        armed: AtomicBool::new(false),
    });
    // Before the spawn: see the module docs.
    notify::Supervisor::start(ours, gate)
        .map_err(|e| format!("syscall shield supervisor: {e}"))?
        .detach();
    let fd = theirs.as_raw_fd();
    // SAFETY: the closure runs in the child between fork and exec and calls
    // only `install`, which makes two system calls and allocates nothing; the
    // filter was built before the fork.
    unsafe {
        command.pre_exec(move || notify::install(&filter, fd));
    }
    Ok(Some(Attached { _child_end: theirs }))
}

/// `Operation::SetSyscallShields`.
pub fn set_rules(rules: Vec<GuestSyscallRule>) -> OpResult {
    set_rules_in(shared(), rules)
}

fn set_rules_in(shared: &Shared, rules: Vec<GuestSyscallRule>) -> OpResult {
    if let Err(message) = validate_rules(&rules) {
        return OpResult::Failed { message };
    }
    if let Err(why) = support() {
        return OpResult::Failed {
            message: format!("{UNAVAILABLE} {why}"),
        };
    }
    let trappable: Vec<&str> = notify::trappable().collect();
    if let Some(rule) = rules
        .iter()
        .find(|r| !trappable.contains(&r.syscall.as_str()))
    {
        return OpResult::Failed {
            message: format!(
                "rule of shield {:?} names {:?}, which this agent cannot trap; nothing applied",
                rule.shield, rule.syscall
            ),
        };
    }
    shared.replace(rules);
    shared.enabled.store(true, Ordering::Release);
    OpResult::Acknowledged
}

/// `Operation::SyscallShieldHits`.
pub fn drain_hits() -> OpResult {
    let (hits, dropped) = shared().drain();
    OpResult::SyscallShieldHits { hits, dropped }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hv2_guest_agent::GuestSyscallAction;

    fn rule(digest: &str, syscall: &str, enforce: bool) -> GuestSyscallRule {
        GuestSyscallRule {
            shield: format!("s-{digest}"),
            digest: digest.into(),
            syscall: syscall.into(),
            args: Vec::new(),
            action: GuestSyscallAction::Errno { errno: 1 },
            enforce,
            expires_at: 0,
        }
    }

    fn hit(enforced: bool, n: u64) -> GuestHit {
        GuestHit {
            at: n,
            shield: "s".into(),
            digest: "d".into(),
            syscall: "ptrace".into(),
            args: [n; 6],
            enforced,
            pid: 1,
        }
    }

    #[test]
    fn the_ring_is_bounded_counts_what_it_drops_and_drains_once() {
        let mut ring = Ring::with_capacity(3);
        for n in 0..5 {
            ring.push(hit(false, n));
        }
        let (hits, dropped) = ring.drain();
        assert_eq!(hits.iter().map(|h| h.at).collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!(dropped, 2);
        assert_eq!(ring.drain(), (Vec::new(), 0), "drained once");
    }

    #[test]
    fn a_refusal_displaces_a_monitored_hit_but_never_a_refusal() {
        let mut ring = Ring::with_capacity(2);
        ring.push(hit(false, 0));
        ring.push(hit(true, 1));
        ring.push(hit(true, 2)); // evicts the monitored one
        ring.push(hit(true, 3)); // nothing monitored left: dropped
        ring.push(hit(false, 4)); // dropped
        let (hits, dropped) = ring.drain();
        assert_eq!(hits.iter().map(|h| h.at).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(dropped, 3);
    }

    #[test]
    fn the_gate_allows_until_the_first_exec_and_judges_after() {
        let shared = Arc::new(Shared::new());
        shared.replace(vec![rule("a", "setuid", true), rule("b", "ptrace", true)]);
        let gate = ChildGate {
            shared: Arc::clone(&shared),
            armed: AtomicBool::new(false),
        };
        let call = |syscall| TrappedCall {
            syscall,
            args: [0; 6],
            pid: 7,
        };
        // The agent's own privilege drop, though a rule names setuid.
        assert_eq!(gate.decide(&call("setuid")), GateVerdict::Allow);
        // The initial exec: allowed, recorded as nothing, and it arms.
        assert_eq!(gate.decide(&call("execve")), GateVerdict::Allow);
        assert_eq!(shared.drain().0.len(), 0);
        // Now the guest's own calls are judged.
        assert_eq!(gate.decide(&call("ptrace")), GateVerdict::Deny { errno: 1 });
        assert_eq!(gate.decide(&call("setuid")), GateVerdict::Deny { errno: 1 });
        assert_eq!(gate.decide(&call("bpf")), GateVerdict::Allow);
        let (hits, dropped) = shared.drain();
        assert_eq!(dropped, 0);
        assert_eq!(
            hits.iter()
                .map(|h| (h.syscall.as_str(), h.enforced, h.pid))
                .collect::<Vec<_>>(),
            [("ptrace", true, 7), ("setuid", true, 7)]
        );
    }

    #[test]
    fn a_monitored_shield_records_and_allows() {
        let shared = Shared::new();
        shared.replace(vec![rule("a", "ptrace", false)]);
        assert_eq!(shared.decide("ptrace", &[0; 6], 3), GateVerdict::Allow);
        let (hits, _) = shared.drain();
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].enforced);
    }

    #[test]
    fn hot_calls_are_trapped_only_when_a_rule_names_them() {
        let names = trapped_names(&[]);
        assert!(names.iter().any(|n| n == "execve"), "the gate arms on it");
        assert!(names.iter().any(|n| n == "ptrace"));
        assert!(!names.iter().any(|n| n == "io_uring_enter"));
        let names = trapped_names(&[rule("a", "io_uring_enter", true)]);
        assert!(names.iter().any(|n| n == "io_uring_enter"));
        // Whatever it names, the filter must build.
        assert!(notify::Filter::new(&names).is_ok());
    }

    #[test]
    fn a_rule_list_that_does_not_validate_keeps_the_previous_one() {
        let shared = Shared::new();
        shared.replace(vec![rule("a", "ptrace", true)]);
        let mut bad = rule("b", "ptrace", true);
        bad.action = GuestSyscallAction::Errno { errno: 0 };
        assert!(matches!(
            set_rules_in(&shared, vec![bad]),
            OpResult::Failed { .. }
        ));
        assert_eq!(shared.rules().len(), 1);
        assert_eq!(shared.rules()[0].digest, "a");
    }

    /// Root only (it installs a filter without no-new-privileges and drops to
    /// another user): a shielded program runs, switches user AFTER the filter
    /// is installed, and the filter does not stop it.
    #[test]
    fn a_shielded_child_still_drops_privileges_and_runs() {
        // SAFETY: geteuid takes no arguments.
        if unsafe { libc::geteuid() } != 0 || support().is_err() {
            eprintln!("skipping: needs root and a kernel with seccomp user-notify");
            return;
        }
        let shared = Arc::new(Shared::new());
        // Off until the host sends a list.
        let mut untouched = Command::new("/bin/true");
        assert!(attach_with(&mut untouched, &shared).unwrap().is_none());
        assert!(matches!(
            set_rules_in(&shared, vec![rule("a", "personality", true)]),
            OpResult::Acknowledged
        ));
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "test \"$(id -u)\" = 65534"]);
        let attached = attach_with(&mut command, &shared).unwrap();
        assert!(attached.is_some());
        // SAFETY: only async-signal-safe calls, as in the agent.
        unsafe {
            command.pre_exec(|| {
                if libc::setgid(65534) < 0 || libc::setuid(65534) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let status = command.status();
        drop(attached);
        assert!(status.unwrap().success());
    }

    /// Root only, as above: after the drop, the child's own calls are judged.
    /// A shell refused `execve` cannot run another program.
    #[test]
    fn a_shielded_child_is_refused_what_a_rule_names_after_the_drop() {
        // SAFETY: geteuid takes no arguments.
        if unsafe { libc::geteuid() } != 0 || support().is_err() {
            eprintln!("skipping: needs root and a kernel with seccomp user-notify");
            return;
        }
        let run = |rules: Vec<GuestSyscallRule>| {
            let shared = Arc::new(Shared::new());
            assert!(matches!(
                set_rules_in(&shared, rules),
                OpResult::Acknowledged
            ));
            let mut command = Command::new("/bin/sh");
            command.args(["-c", "/bin/true && /bin/true"]);
            let attached = attach_with(&mut command, &shared).unwrap();
            assert!(attached.is_some());
            // SAFETY: only async-signal-safe calls, as in the agent.
            unsafe {
                command.pre_exec(|| {
                    if libc::setgid(65534) < 0 || libc::setuid(65534) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            let status = command.status().unwrap();
            drop(attached);
            (status.success(), shared.drain().0)
        };
        let (ran, hits) = run(Vec::new());
        assert!(ran, "nothing in force: the shell runs its programs");
        assert!(hits.is_empty());
        let (ran, hits) = run(vec![rule("a", "execve", true)]);
        assert!(!ran, "the shell's own execve is refused");
        assert!(hits.iter().any(|h| h.syscall == "execve" && h.enforced));
        let (ran, hits) = run(vec![rule("a", "execve", false)]);
        assert!(ran, "monitored: recorded, not refused");
        assert!(hits.iter().any(|h| h.syscall == "execve" && !h.enforced));
    }
}
