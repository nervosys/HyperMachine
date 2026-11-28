//! The engine against a real kernel: programs are loaded, attached, and a
//! child process (this binary, re-executed on the test named `child`) is
//! refused or not. Needs root and a kernel with BPF LSM; a host without them
//! prints a line starting `skipping:` and passes, and CI treats that line as
//! a failure.
#![cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use hv2_shield_bpf::{available_hooks, cgroup_id, probe, HookEngine, HookRule, Scope, HOOKS};

const MODE: &str = "HV2_SHIELD_BPF_CHILD";
const CGROUP: &str = "HV2_SHIELD_BPF_CGROUP";
/// The error `ptrace_traceme` rules refuse with: nothing else produces it.
const EOPNOTSUPP: i32 = 95;

fn skip() -> bool {
    match probe() {
        Ok(()) => false,
        Err(why) => {
            eprintln!("skipping: {why}");
            true
        }
    }
}

/// The child half. Run on its own by the parent, with `MODE` set; run as an
/// ordinary test (no `MODE`) it does nothing.
///
/// Modes: `socket` joins the cgroup in `CGROUP` and makes a TCP socket;
/// `traceme` waits for its stdin to close, then calls `PTRACE_TRACEME`.
/// Either way it prints `RESULT <mode> <errno or 0>`.
#[test]
fn child() {
    let Ok(mode) = std::env::var(MODE) else {
        return;
    };
    let errno = match mode.as_str() {
        "socket" => {
            let dir = std::env::var(CGROUP).expect("a cgroup to join");
            std::fs::write(Path::new(&dir).join("cgroup.procs"), "0").expect("joining the cgroup");
            // SAFETY: socket(2) with constant arguments; the result is checked.
            let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
            if fd < 0 {
                std::io::Error::last_os_error().raw_os_error().unwrap()
            } else {
                // SAFETY: `fd` was just returned by socket(2) and is owned here.
                unsafe { libc::close(fd) };
                0
            }
        }
        "traceme" => {
            let mut sink = Vec::new();
            std::io::stdin().read_to_end(&mut sink).unwrap();
            // SAFETY: ptrace(PTRACE_TRACEME) takes no pointers; the result is checked.
            let r = unsafe { libc::ptrace(libc::PTRACE_TRACEME, 0, 0, 0) };
            let errno = if r < 0 {
                std::io::Error::last_os_error().raw_os_error().unwrap()
            } else {
                0
            };
            println!("RESULT traceme {errno}");
            std::io::stdout().flush().unwrap();
            if errno == 0 {
                // This is a traced thread of a test harness, which the parent
                // could never reap once it exits. Exec instead: the thread
                // becomes the whole process, stops at the exec for its
                // tracer, and the parent kills it.
                use std::os::unix::process::CommandExt;
                let e = Command::new("/bin/true").exec();
                panic!("exec: {e}");
            }
            return;
        }
        other => panic!("unknown mode {other}"),
    };
    println!("RESULT {mode} {errno}");
}

fn child_command(mode: &str, cgroup: Option<&Path>) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", "child", "--nocapture", "--test-threads=1", "-q"])
        .env(MODE, mode)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cgroup {
        c.env(CGROUP, dir);
    }
    c
}

fn result_of(mode: &str, out: &std::process::Output) -> i32 {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let prefix = format!("RESULT {mode} ");
    stdout
        .lines()
        .find_map(|l| l.strip_prefix(&prefix))
        .unwrap_or_else(|| {
            panic!(
                "child printed no result: {stdout} / {}",
                String::from_utf8_lossy(&out.stderr)
            )
        })
        .trim()
        .parse()
        .unwrap()
}

/// A cgroup of this test's own, removed when the guard goes, however the test ends.
struct TestCgroup {
    path: PathBuf,
}

impl TestCgroup {
    fn new() -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let path = PathBuf::from(format!(
            "/sys/fs/cgroup/hv2-shield-bpf-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&path).expect("making a cgroup");
        TestCgroup { path }
    }

    fn id(&self) -> u64 {
        cgroup_id(&self.path).unwrap()
    }

    /// What a child in this cgroup gets from `socket(2)`: 0 or an errno.
    fn socket(&self) -> i32 {
        let out = child_command("socket", Some(&self.path)).output().unwrap();
        result_of("socket", &out)
    }
}

impl Drop for TestCgroup {
    fn drop(&mut self) {
        // A cgroup can be removed only when its processes are gone and reaped.
        for _ in 0..50 {
            if std::fs::remove_dir(&self.path).is_ok() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        eprintln!("could not remove {}", self.path.display());
    }
}

fn rule(hook: &str, scope: Scope, errno: i32, enforce: bool) -> HookRule {
    HookRule {
        hook: hook.into(),
        scope,
        errno,
        enforce,
    }
}

/// This process can make a socket: it is not in the cgroup being judged.
fn own_socket_works() {
    // SAFETY: socket(2) with constant arguments; the result is checked.
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
    assert!(fd >= 0, "the test process was refused a socket");
    // SAFETY: `fd` was just returned by socket(2) and is owned here.
    unsafe { libc::close(fd) };
}

#[test]
fn a_cgroup_is_refused_counted_and_released() {
    if skip() {
        return;
    }
    let cg = TestCgroup::new();
    let id = cg.id();
    let mut engine = HookEngine::new().unwrap();
    assert_eq!(cg.socket(), 0, "no rules yet");
    assert!(engine.counts().unwrap().is_empty());

    engine
        .set(&[rule("socket_create", Scope::Cgroup(id), 1, true)])
        .unwrap();
    assert_eq!(engine.attached_hooks(), ["socket_create"]);
    assert_eq!(cg.socket(), libc::EPERM, "enforced");
    own_socket_works();
    let counts = engine.counts().unwrap();
    assert_eq!(counts.len(), 1);
    assert_eq!(counts[0].hook, "socket_create");
    assert_eq!(counts[0].scope, Scope::Cgroup(id));
    assert!(counts[0].refused >= 1, "{counts:?}");
    assert_eq!(counts[0].monitored, 0);
    let refused = counts[0].refused;

    // Another errno, same key: the counters stay.
    engine
        .set(&[rule("socket_create", Scope::Cgroup(id), libc::EACCES, true)])
        .unwrap();
    assert_eq!(cg.socket(), libc::EACCES);
    let counts = engine.counts().unwrap();
    assert!(counts[0].refused > refused, "{counts:?}");

    // A rule for a different cgroup does not judge this one.
    engine
        .set(&[rule("socket_create", Scope::Cgroup(id + 1), 1, true)])
        .unwrap();
    assert_eq!(cg.socket(), 0, "someone else's rule");

    // Monitor: counted, not refused; the refusals already counted are kept.
    engine
        .set(&[rule("socket_create", Scope::Cgroup(id), 1, true)])
        .unwrap();
    assert_eq!(cg.socket(), libc::EPERM);
    let before = engine.counts().unwrap()[0].refused;
    engine
        .set(&[rule("socket_create", Scope::Cgroup(id), 1, false)])
        .unwrap();
    assert_eq!(cg.socket(), 0, "monitoring refuses nothing");
    let counts = engine.counts().unwrap();
    assert!(counts[0].monitored >= 1, "{counts:?}");
    assert_eq!(counts[0].refused, before, "{counts:?}");

    // Several rules for one key: any enforcing one enforces, with its errno.
    engine
        .set(&[
            rule("socket_create", Scope::Cgroup(id), 1, false),
            rule("socket_create", Scope::Cgroup(id), libc::EACCES, true),
            rule("socket_create", Scope::Cgroup(id), libc::EPERM, true),
        ])
        .unwrap();
    assert_eq!(cg.socket(), libc::EACCES);

    // Nothing: released, counters gone, hook detached.
    engine.set(&[]).unwrap();
    assert_eq!(cg.socket(), 0, "released");
    assert!(engine.counts().unwrap().is_empty());
    assert!(
        engine.attached_hooks().is_empty(),
        "the hook is still attached"
    );
}

#[test]
fn a_rule_naming_a_hook_no_more_detaches_it() {
    if skip() {
        return;
    }
    let cg = TestCgroup::new();
    let id = cg.id();
    let mut engine = HookEngine::new().unwrap();
    engine
        .set(&[
            rule("socket_create", Scope::Cgroup(id), 1, true),
            rule("quotactl", Scope::Cgroup(id), 1, true),
        ])
        .unwrap();
    assert_eq!(engine.attached_hooks(), ["socket_create", "quotactl"]);
    assert_eq!(cg.socket(), libc::EPERM);
    engine
        .set(&[rule("quotactl", Scope::Cgroup(id), 1, true)])
        .unwrap();
    assert_eq!(engine.attached_hooks(), ["quotactl"]);
    assert_eq!(cg.socket(), 0, "socket_create is no longer judged");
}

#[test]
fn an_invalid_set_changes_nothing() {
    if skip() {
        return;
    }
    let cg = TestCgroup::new();
    let id = cg.id();
    let mut engine = HookEngine::new().unwrap();
    let good = rule("socket_create", Scope::Cgroup(id), 1, true);
    engine.set(std::slice::from_ref(&good)).unwrap();
    assert_eq!(cg.socket(), libc::EPERM);
    let mut before = engine.counts().unwrap();
    assert_eq!(before[0].refused, 1);

    let other = rule("quotactl", Scope::Cgroup(id), 1, true);
    let too_many = vec![other.clone(); 4097];
    for bad in [
        vec![
            other.clone(),
            rule("no_such_hook", Scope::Everywhere, 1, true),
        ],
        vec![other.clone(), rule("syslog", Scope::Everywhere, 0, true)],
        vec![other.clone(), rule("syslog", Scope::Everywhere, 4096, true)],
        too_many,
    ] {
        assert!(engine.set(&bad).is_err());
        assert_eq!(engine.counts().unwrap(), before, "counters or keys changed");
        assert_eq!(engine.attached_hooks(), ["socket_create"]);
        assert_eq!(cg.socket(), libc::EPERM, "the old rules still hold");
        // That refusal was counted, so the baseline moves with it.
        before = engine.counts().unwrap();
    }
}

#[test]
fn exempt_can_be_taken_back_and_available_hooks_are_named_ones() {
    if skip() {
        return;
    }
    let engine = HookEngine::new().unwrap();
    engine.exempt(std::process::id()).unwrap();
    engine.unexempt(std::process::id()).unwrap();
    engine.unexempt(std::process::id()).unwrap();
    let available = available_hooks().unwrap();
    assert!(available.contains(&"socket_create"));
    assert!(available.contains(&"ptrace_traceme"));
    assert!(available.iter().all(|h| HOOKS.contains(h)));
}

/// A child waiting for its stdin to close before it calls `PTRACE_TRACEME`.
fn waiting_child() -> Child {
    child_command("traceme", None)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap()
}

fn release(mut child: Child) -> i32 {
    drop(child.stdin.take());
    // The answer is the first line that has one; a child that succeeded is
    // stopped as a tracee, never closing its output, so read lines.
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let mut answer = None;
    for line in stdout.lines() {
        if let Some(n) = line.unwrap().strip_prefix("RESULT traceme ") {
            answer = Some(n.trim().parse().unwrap());
            break;
        }
    }
    let answer = answer.expect("the child never answered");
    let pid = child.id() as libc::pid_t;
    let mut status = 0;
    if answer == 0 {
        // It is a tracee now and stops at its exec; killing it before that
        // would leave a traced thread nobody can reap.
        // SAFETY: waitpid on this test's own child, with a valid status pointer.
        let r = unsafe { libc::waitpid(pid, &mut status, libc::__WALL) };
        assert_eq!(r, pid);
        assert!(libc::WIFSTOPPED(status), "status {status:#x}");
        child.kill().unwrap();
        // SAFETY: as above.
        let r = unsafe { libc::waitpid(pid, &mut status, libc::__WALL) };
        assert_eq!(r, pid);
    } else {
        child.wait().unwrap();
    }
    answer
}

#[test]
fn everywhere_judges_all_but_the_exempt() {
    if skip() {
        return;
    }
    let mut engine = HookEngine::new().unwrap();
    // Both children exist and wait; only then is the rule set, and it is
    // taken off as soon as both have answered: it judges the whole host.
    let exempt_child = waiting_child();
    let judged_child = waiting_child();
    engine.exempt(exempt_child.id()).unwrap();
    engine
        .set(&[rule("ptrace_traceme", Scope::Everywhere, EOPNOTSUPP, true)])
        .unwrap();
    let exempt_result = release(exempt_child);
    let judged_result = release(judged_child);
    let counts = engine.counts().unwrap();
    engine.set(&[]).unwrap();

    assert_eq!(exempt_result, 0, "the exempt process was judged");
    assert_eq!(judged_result, EOPNOTSUPP, "the other process was not");
    assert_eq!(counts.len(), 1);
    assert_eq!(counts[0].scope, Scope::Everywhere);
    assert_eq!(counts[0].refused, 1, "{counts:?}");
    assert_eq!(counts[0].monitored, 0);
}
