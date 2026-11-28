//! Hook shields over the whole guest.
//!
//! A shield's `hooks` rules refuse a kernel operation at a Linux BPF-LSM hook
//! (`hv2_shield_bpf`). Unlike the syscall shields in `shield.rs`, which trap
//! calls only in the programs this agent starts, a hook is asked about every
//! process in the guest: `Scope::Everywhere` covers processes the agent never
//! started (a login shell on the console, a container runtime, a daemon that
//! was running before the agent), and it is in force from the moment the rule
//! is set. The one process it never judges is this agent, so the agent keeps
//! talking to the host even under a rule on `socket_create`.
//!
//! # Off until the host turns it on
//!
//! Nothing is loaded into the kernel until the host sends a non-empty list
//! with `SetHookShields`: an empty list sent to an agent that has no engine
//! creates nothing, so a guest with no hook shields pays nothing. The engine
//! is created once and kept; programs are attached only while a rule names
//! their hook.
//!
//! # Hits are counts
//!
//! The kernel counts matches per hook, and this agent reads the counters
//! when the host asks for hits and before it replaces the rules. The
//! difference since the last read is credited to the shields that named the
//! hook by `hv2_guest_agent::hook_shield::attribute`. Rules are replaced only
//! after the counters are read, so a match is credited to the shield that was
//! in force when it happened. A hit therefore says "N matches", not when each
//! happened or which process.
//!
//! # Expiry
//!
//! Each rule may carry the guest-clock second it expires at. A thread sleeps
//! until the earliest and, if the list has not been replaced meanwhile,
//! applies the list again without the expired rules, so a host that stops
//! talking cannot leave a hook rule in force past its expiry.
//!
//! # Where this is not safe to rely on
//!
//! - It needs a kernel with `bpf` in its active LSMs (the `lsm=` boot
//!   parameter), BTF, and a root agent; otherwise `SetHookShields` answers
//!   `hook shields unavailable:` and nothing is applied. The stock guest
//!   kernel configuration does not have them.
//! - A guest process that is root can kill this agent, and the programs are
//!   detached with it: the engine's links live and die with this process.
//! - A hook is a coarse tool: the operation is refused for everything in the
//!   guest, so a rule on `sb_mount` or `socket_create` also refuses it to the
//!   guest's own init and services.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use hv2_guest_agent::hook_shield::{attribute, validate_hook_rules};
use hv2_guest_agent::{GuestHookHit, GuestHookRule, OpResult, HOOK_HIT_LIMIT};
use hv2_shield_bpf::{HookEngine, HookRule, Scope, HOOKS};

/// What `SetHookShields` answers, up to the reason, when this guest cannot
/// shield hooks: the host matches on it.
pub const UNAVAILABLE: &str = "hook shields unavailable:";

/// How often a waiting expiry thread looks at whether it was superseded.
const EXPIRY_POLL: Duration = Duration::from_secs(10);

/// Everything the agent knows about hook shields, in one place a test can make
/// its own copy of.
struct Hooks {
    /// Created by the first non-empty list, then kept.
    engine: Option<HookEngine>,
    /// The rules the engine has, in the order the host sent them.
    rules: Vec<GuestHookRule>,
    /// The kernel's cumulative `(monitored, refused)` per hook at the last
    /// read.
    seen: BTreeMap<String, (u64, u64)>,
    /// Hits not yet read: bounded, oldest first.
    hits: Vec<GuestHookHit>,
    /// Hits that did not fit since the last read.
    dropped: u64,
    /// Bumped on every change of `rules`, so an expiry thread knows it is
    /// stale.
    generation: u64,
}

impl Hooks {
    fn new() -> Self {
        Self {
            engine: None,
            rules: Vec::new(),
            seen: BTreeMap::new(),
            hits: Vec::new(),
            dropped: 0,
            generation: 0,
        }
    }

    /// Keep a hit, within [`HOOK_HIT_LIMIT`]. When full, a refusal takes the
    /// place of the oldest monitored hit if there is one: a flood of "would
    /// have refused" must not hide a refusal. Anything else is dropped and
    /// counted.
    fn push(&mut self, hit: GuestHookHit) {
        if self.hits.len() >= HOOK_HIT_LIMIT {
            let victim = hit
                .enforced
                .then(|| self.hits.iter().position(|h| !h.enforced))
                .flatten();
            let Some(victim) = victim else {
                self.dropped = self.dropped.saturating_add(1);
                return;
            };
            self.hits.remove(victim);
            self.dropped = self.dropped.saturating_add(1);
        }
        self.hits.push(hit);
    }

    /// Read the kernel's counters and credit what is new to the rules the
    /// engine has now. Call before changing the rules.
    fn collect(&mut self, now: u64) -> Result<(), String> {
        let Some(engine) = &self.engine else {
            return Ok(());
        };
        let mut totals: BTreeMap<String, (u64, u64)> = BTreeMap::new();
        for count in engine.counts()? {
            let entry = totals.entry(count.hook).or_default();
            entry.0 = entry.0.saturating_add(count.monitored);
            entry.1 = entry.1.saturating_add(count.refused);
        }
        // What the engine has is what was in force, whatever the clock says
        // now: an expired rule is dropped when the expiry thread runs, not
        // when the second passes, and matches until then are its.
        let in_force: Vec<GuestHookRule> = self
            .rules
            .iter()
            .map(|r| GuestHookRule {
                expires_at: 0,
                ..r.clone()
            })
            .collect();
        for (hook, (monitored, refused)) in &totals {
            let (was_monitored, was_refused) = self.seen.get(hook).copied().unwrap_or((0, 0));
            // A counter that went down was reset: all of it is new.
            let new_monitored = delta(*monitored, was_monitored);
            let new_refused = delta(*refused, was_refused);
            for hit in attribute(&in_force, hook, new_monitored, new_refused, now) {
                self.push(hit);
            }
        }
        self.seen = totals;
        Ok(())
    }
}

fn delta(now: u64, before: u64) -> u64 {
    if now >= before {
        now - before
    } else {
        now
    }
}

fn lock(shared: &Mutex<Hooks>) -> MutexGuard<'_, Hooks> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

fn shared() -> &'static Arc<Mutex<Hooks>> {
    static SHARED: OnceLock<Arc<Mutex<Hooks>>> = OnceLock::new();
    SHARED.get_or_init(|| Arc::new(Mutex::new(Hooks::new())))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `Operation::SetHookShields`.
pub fn set_rules(rules: Vec<GuestHookRule>) -> OpResult {
    set_rules_in(shared(), rules)
}

fn set_rules_in(shared: &Arc<Mutex<Hooks>>, rules: Vec<GuestHookRule>) -> OpResult {
    if let Err(message) = validate_hook_rules(&rules) {
        return OpResult::Failed { message };
    }
    if let Some(rule) = rules.iter().find(|r| !HOOKS.contains(&r.hook.as_str())) {
        return OpResult::Failed {
            message: format!(
                "rule of shield {:?} names hook {:?}, which this agent does not offer; \
                 nothing applied",
                rule.shield, rule.hook
            ),
        };
    }
    let mut hooks = lock(shared);
    if rules.is_empty() && hooks.engine.is_none() {
        return OpResult::Acknowledged;
    }
    if hooks.engine.is_none() {
        match create_engine() {
            Ok(engine) => hooks.engine = Some(engine),
            Err(why) => {
                return OpResult::Failed {
                    message: format!("{UNAVAILABLE} {why}"),
                }
            }
        }
    }
    let now = unix_now();
    // Credit what the old rules matched before they are replaced. If the
    // counters cannot be read the change still goes ahead: lifting or
    // changing a rule must not depend on its bookkeeping.
    if let Err(why) = hooks.collect(now) {
        eprintln!("hv2-guest-agentd: reading hook shield counters: {why}");
    }
    match apply(&mut hooks, shared, rules, now) {
        Ok(()) => OpResult::Acknowledged,
        Err(why) => OpResult::Failed {
            message: format!("{UNAVAILABLE} {why}"),
        },
    }
}

/// The engine, ready: the kernel can load programs, and this agent is never
/// judged by them.
fn create_engine() -> Result<HookEngine, String> {
    mount_securityfs();
    hv2_shield_bpf::probe()?;
    let engine = HookEngine::new()?;
    engine.exempt(std::process::id())?;
    Ok(engine)
}

/// The probe reads the active LSMs from securityfs, which a minimal guest init
/// may not have mounted. Mounting it is harmless if it is already there (the
/// second mount fails with EBUSY and is ignored); any other failure is left for
/// the probe to report.
fn mount_securityfs() {
    const TARGET: &std::ffi::CStr = c"/sys/kernel/security";
    const FS: &std::ffi::CStr = c"securityfs";
    if std::path::Path::new("/sys/kernel/security/lsm").exists() {
        return;
    }
    // SAFETY: every pointer is a NUL-terminated constant, and no data is passed.
    unsafe {
        libc::mount(
            FS.as_ptr(),
            TARGET.as_ptr(),
            FS.as_ptr(),
            0,
            std::ptr::null(),
        );
    }
}

/// Make `rules`, less the ones expired at `now`, the engine's. On error
/// nothing changed. `hooks.engine` must be set.
fn apply(
    hooks: &mut Hooks,
    shared: &Arc<Mutex<Hooks>>,
    rules: Vec<GuestHookRule>,
    now: u64,
) -> Result<(), String> {
    let rules: Vec<GuestHookRule> = rules
        .into_iter()
        .filter(|r| r.expires_at == 0 || r.expires_at > now)
        .collect();
    let engine_rules: Vec<HookRule> = rules
        .iter()
        .map(|r| HookRule {
            hook: r.hook.clone(),
            scope: Scope::Everywhere,
            errno: r.errno,
            enforce: r.enforce,
        })
        .collect();
    let Some(engine) = hooks.engine.as_mut() else {
        return Err("no engine".into());
    };
    engine.set(&engine_rules)?;
    // The engine deletes the counters of a hook no rule names any more.
    hooks
        .seen
        .retain(|hook, _| rules.iter().any(|r| r.hook == *hook));
    hooks.rules = rules;
    hooks.generation = hooks.generation.wrapping_add(1);
    arm_expiry(hooks, shared);
    Ok(())
}

/// Start the thread that drops rules at the earliest expiry among the
/// current ones, unless the list is replaced first.
fn arm_expiry(hooks: &Hooks, shared: &Arc<Mutex<Hooks>>) {
    let Some(at) = hooks
        .rules
        .iter()
        .map(|r| r.expires_at)
        .filter(|&t| t != 0)
        .min()
    else {
        return;
    };
    let generation = hooks.generation;
    let shared = Arc::clone(shared);
    let spawned = std::thread::Builder::new()
        .name("hook-shield-expiry".into())
        .spawn(move || expire(&shared, generation, at));
    if let Err(e) = spawned {
        eprintln!("hv2-guest-agentd: hook shield expiry thread: {e}");
    }
}

fn expire(shared: &Arc<Mutex<Hooks>>, generation: u64, at: u64) {
    loop {
        let now = unix_now();
        if now < at {
            std::thread::sleep(Duration::from_secs(at - now).min(EXPIRY_POLL));
            if lock(shared).generation != generation {
                return;
            }
            continue;
        }
        let mut hooks = lock(shared);
        if hooks.generation != generation {
            return;
        }
        if let Err(why) = hooks.collect(now) {
            eprintln!("hv2-guest-agentd: reading hook shield counters: {why}");
        }
        let rules = hooks.rules.clone();
        match apply(&mut hooks, shared, rules, now) {
            Ok(()) => return,
            Err(why) => {
                eprintln!("hv2-guest-agentd: dropping expired hook shields: {why}");
                drop(hooks);
                std::thread::sleep(Duration::from_secs(5));
            }
        }
    }
}

/// `Operation::HookShieldHits`.
pub fn drain_hits() -> OpResult {
    drain_hits_in(shared())
}

fn drain_hits_in(shared: &Mutex<Hooks>) -> OpResult {
    let mut hooks = lock(shared);
    if let Err(why) = hooks.collect(unix_now()) {
        return OpResult::Failed {
            message: format!("reading hook shield counters: {why}"),
        };
    }
    let hooks = &mut *hooks;
    OpResult::HookShieldHits {
        hits: std::mem::take(&mut hooks.hits),
        dropped: std::mem::take(&mut hooks.dropped),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    const CHILD: &str = "HV2_HOOK_SHIELD_CHILD";

    fn rule(digest: &str, hook: &str, enforce: bool) -> GuestHookRule {
        GuestHookRule {
            shield: format!("s-{digest}"),
            digest: digest.into(),
            hook: hook.into(),
            errno: 1,
            enforce,
            expires_at: 0,
        }
    }

    fn hit(enforced: bool, n: u64) -> GuestHookHit {
        GuestHookHit {
            at: n,
            shield: "s".into(),
            digest: "d".into(),
            hook: "sb_mount".into(),
            enforced,
            count: 1,
        }
    }

    #[test]
    fn a_list_that_does_not_validate_keeps_the_previous_one() {
        let shared = Arc::new(Mutex::new(Hooks::new()));
        lock(&shared).rules = vec![rule("a", "sb_mount", true)];
        let mut bad = rule("b", "sb_mount", true);
        bad.errno = 0;
        assert!(matches!(
            set_rules_in(&shared, vec![bad]),
            OpResult::Failed { .. }
        ));
        // A hook this agent does not offer is refused whole, too.
        let result = set_rules_in(
            &shared,
            vec![rule("c", "sb_mount", true), rule("d", "no_such_hook", true)],
        );
        assert!(
            matches!(&result, OpResult::Failed { message } if message.contains("no_such_hook"))
        );
        let hooks = lock(&shared);
        assert_eq!(hooks.rules.len(), 1);
        assert_eq!(hooks.rules[0].digest, "a");
        assert!(hooks.engine.is_none());
    }

    #[test]
    fn an_empty_list_creates_no_engine() {
        let shared = Arc::new(Mutex::new(Hooks::new()));
        assert!(matches!(
            set_rules_in(&shared, Vec::new()),
            OpResult::Acknowledged
        ));
        assert!(lock(&shared).engine.is_none());
        assert!(matches!(
            drain_hits_in(&shared),
            OpResult::HookShieldHits { hits, dropped: 0 } if hits.is_empty()
        ));
    }

    #[test]
    fn the_hit_list_is_bounded_counts_what_it_drops_and_a_refusal_displaces_a_monitored_hit() {
        let mut hooks = Hooks::new();
        for n in 0..HOOK_HIT_LIMIT as u64 {
            hooks.push(hit(false, n));
        }
        hooks.push(hit(false, 5000)); // full: dropped
        assert_eq!((hooks.hits.len(), hooks.dropped), (HOOK_HIT_LIMIT, 1));
        hooks.push(hit(true, 6000)); // evicts the oldest monitored one
        assert_eq!((hooks.hits.len(), hooks.dropped), (HOOK_HIT_LIMIT, 2));
        assert_eq!(hooks.hits[0].at, 1);
        assert_eq!(hooks.hits.last().map(|h| h.at), Some(6000));
    }

    #[test]
    fn a_counter_that_went_down_was_reset() {
        assert_eq!(delta(10, 4), 6);
        assert_eq!(delta(4, 4), 0);
        assert_eq!(delta(3, 9), 3);
    }

    /// Not a test: the workload the root test below runs, as this same
    /// binary. Nothing else on a machine calls `PTRACE_TRACEME`.
    #[test]
    fn hook_child() {
        if std::env::var_os(CHILD).is_none() {
            return;
        }
        // The call is made in a forked, single-threaded process that this one
        // reaps. Made here, on a harness thread that is not the leader, a
        // successful PTRACE_TRACEME leaves that thread a zombie only its
        // tracer can reap, and the parent's wait for the leader never returns.
        // SAFETY: the child calls only ptrace and _exit before it ends.
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork: {}", std::io::Error::last_os_error());
        if pid == 0 {
            // SAFETY: PTRACE_TRACEME takes no pointer; the other arguments are
            // ignored by the kernel for it.
            let result = unsafe { libc::ptrace(libc::PTRACE_TRACEME, 0_i32, 0_usize, 0_usize) };
            // SAFETY: __errno_location returns this thread's errno slot.
            let errno = if result >= 0 {
                0
            } else {
                unsafe { *libc::__errno_location() }
            };
            // SAFETY: _exit ends the child without running the parent's handlers.
            unsafe { libc::_exit(errno & 0xff) };
        }
        let mut status = 0;
        // SAFETY: `status` is a valid place for the kernel to write.
        let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
        assert_eq!(waited, pid, "waitpid: {}", std::io::Error::last_os_error());
        assert!(libc::WIFEXITED(status), "child status {status:#x}");
        let errno = libc::WEXITSTATUS(status);
        println!("traceme {} {}", errno == 0, errno);
    }

    /// What the child printed: whether `PTRACE_TRACEME` worked, and its errno.
    fn traceme() -> (bool, i32) {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "hooks::tests::hook_child",
                "--nocapture",
                "--test-threads=1",
                "-q",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let line = stdout
            .lines()
            .find(|l| l.starts_with("traceme "))
            .unwrap_or_else(|| panic!("no answer from the child: {stdout}"));
        let mut words = line.split(' ').skip(1);
        let ok = words.next() == Some("true");
        let errno = words.next().and_then(|e| e.parse().ok()).unwrap_or(-1);
        (ok, errno)
    }

    /// Root only, and a kernel with BPF LSM: a hook rule refuses the
    /// operation in a process the agent did not start with a filter, counts
    /// it for the shield, and lifting the list lets it through.
    #[test]
    fn an_enforced_hook_rule_refuses_a_child_and_reports_the_count() {
        // SAFETY: geteuid takes no arguments.
        if unsafe { libc::geteuid() } != 0 {
            eprintln!("skipping: needs root");
            return;
        }
        if let Err(why) = hv2_shield_bpf::probe() {
            eprintln!("skipping: {why}");
            return;
        }
        let shared = Arc::new(Mutex::new(Hooks::new()));
        let result = set_rules_in(&shared, vec![rule("a", "ptrace_traceme", true)]);
        assert!(matches!(result, OpResult::Acknowledged), "{result:?}");
        assert_eq!(traceme(), (false, libc::EPERM));
        let OpResult::HookShieldHits { hits, dropped } = drain_hits_in(&shared) else {
            panic!("not a hits answer");
        };
        assert_eq!(dropped, 0);
        let refused: u64 = hits
            .iter()
            .filter(|h| h.digest == "a" && h.enforced && h.hook == "ptrace_traceme")
            .map(|h| h.count)
            .sum();
        assert!(refused >= 1, "{hits:?}");
        // Read once: nothing new.
        assert!(matches!(
            drain_hits_in(&shared),
            OpResult::HookShieldHits { hits, .. } if hits.is_empty()
        ));

        let result = set_rules_in(&shared, Vec::new());
        assert!(matches!(result, OpResult::Acknowledged), "{result:?}");
        assert_eq!(traceme(), (true, 0));
    }
}
