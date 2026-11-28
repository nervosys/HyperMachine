//! The engine: maps, programs and links, through `bpf(2)`.
//!
//! No libbpf: the commands used are `BPF_MAP_CREATE`, the `BPF_MAP_*_ELEM`
//! family, `BPF_PROG_LOAD` and `BPF_LINK_CREATE` (with
//! `BPF_RAW_TRACEPOINT_OPEN` as the fallback on a kernel that has no LSM link
//! type), each with an attribute buffer laid out as in `linux/bpf.h`.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::MetadataExt;

use crate::insn::{self, Insn};
use crate::rules::{self, Merged};
use crate::{btf, HookCount, HookRule, Scope, HOOKS};

// `enum bpf_cmd`.
const BPF_MAP_CREATE: u32 = 0;
const BPF_MAP_LOOKUP_ELEM: u32 = 1;
const BPF_MAP_UPDATE_ELEM: u32 = 2;
const BPF_MAP_DELETE_ELEM: u32 = 3;
const BPF_PROG_LOAD: u32 = 5;
const BPF_RAW_TRACEPOINT_OPEN: u32 = 17;
const BPF_LINK_CREATE: u32 = 28;

const BPF_MAP_TYPE_HASH: u32 = 1;
const BPF_PROG_TYPE_LSM: u32 = 29;
/// `enum bpf_attach_type`: `BPF_LSM_MAC`.
const BPF_LSM_MAC: u32 = 27;
/// `BPF_ANY`: create the element or replace it.
const BPF_ANY: u64 = 0;

const VMLINUX_BTF: &str = "/sys/kernel/btf/vmlinux";
const LSM_LIST: &str = "/sys/kernel/security/lsm";
/// How much of the verifier's log is kept in an error.
const LOG_SIZE: usize = 64 * 1024;
const LOG_TAIL: usize = 2048;
/// Processes that can be exempt at once.
const MAX_EXEMPT: u32 = 1024;
/// Distinct `(hook, scope)` keys: one rule each, at most `MAX_RULES` of them.
const MAX_KEYS: u32 = rules::MAX_RULES as u32;

/// An attribute buffer for `bpf(2)`, zeroed, 8-byte aligned as the kernel's
/// `union bpf_attr` is. Fields are written at the offsets of `linux/bpf.h`.
#[repr(C, align(8))]
struct Attr([u8; 128]);

impl Attr {
    fn new() -> Self {
        Attr([0; 128])
    }

    fn u32(&mut self, off: usize, v: u32) -> &mut Self {
        self.0[off..off + 4].copy_from_slice(&v.to_ne_bytes());
        self
    }

    fn u64(&mut self, off: usize, v: u64) -> &mut Self {
        self.0[off..off + 8].copy_from_slice(&v.to_ne_bytes());
        self
    }
}

fn sys(cmd: u32, attr: &mut Attr) -> io::Result<libc::c_long> {
    // SAFETY: `attr` is a live, zero-initialised buffer of the length passed,
    // 8-byte aligned. Every pointer written into it by the callers refers to a
    // buffer that outlives this call, and the kernel reads or writes only
    // within the sizes the attribute gives.
    let r = unsafe {
        libc::syscall(
            libc::SYS_bpf,
            libc::c_long::from(cmd),
            attr.0.as_mut_ptr(),
            attr.0.len(),
        )
    };
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(r)
    }
}

fn fd_of(r: libc::c_long) -> OwnedFd {
    // SAFETY: `r` is a file descriptor the kernel has just returned from a
    // successful bpf(2) call, and nothing else owns it.
    unsafe { OwnedFd::from_raw_fd(r as RawFd) }
}

/// A `bpf(2)` failure, with a hint when the cause is probably privilege.
fn describe(what: &str, e: &io::Error) -> String {
    match e.raw_os_error() {
        Some(libc::EPERM) | Some(libc::EACCES) => format!(
            "{what}: {e} (loading BPF LSM programs needs root, or CAP_BPF with CAP_SYS_ADMIN)"
        ),
        _ => format!("{what}: {e}"),
    }
}

fn map_create(name: &str, key: usize, value: usize, entries: u32) -> Result<OwnedFd, String> {
    let mut a = Attr::new();
    a.u32(0, BPF_MAP_TYPE_HASH)
        .u32(4, key as u32)
        .u32(8, value as u32)
        .u32(12, entries);
    let n = name.as_bytes();
    a.0[28..28 + n.len().min(15)].copy_from_slice(&n[..n.len().min(15)]);
    sys(BPF_MAP_CREATE, &mut a)
        .map(fd_of)
        .map_err(|e| describe(&format!("creating map {name}"), &e))
}

fn map_update(map: &OwnedFd, key: &[u8], value: &[u8]) -> io::Result<()> {
    let mut a = Attr::new();
    a.u32(0, map.as_raw_fd() as u32)
        .u64(8, key.as_ptr() as u64)
        .u64(16, value.as_ptr() as u64)
        .u64(24, BPF_ANY);
    sys(BPF_MAP_UPDATE_ELEM, &mut a).map(drop)
}

fn map_lookup(map: &OwnedFd, key: &[u8], value: &mut [u8]) -> io::Result<()> {
    let mut a = Attr::new();
    a.u32(0, map.as_raw_fd() as u32)
        .u64(8, key.as_ptr() as u64)
        .u64(16, value.as_mut_ptr() as u64);
    sys(BPF_MAP_LOOKUP_ELEM, &mut a).map(drop)
}

fn map_delete(map: &OwnedFd, key: &[u8]) -> io::Result<()> {
    let mut a = Attr::new();
    a.u32(0, map.as_raw_fd() as u32).u64(8, key.as_ptr() as u64);
    sys(BPF_MAP_DELETE_ELEM, &mut a).map(drop)
}

fn prog_load(insns: &[Insn], btf_id: u32, name: &str) -> Result<OwnedFd, String> {
    let license = CString::new("GPL").expect("no NUL in a literal");
    let mut log = vec![0u8; LOG_SIZE];
    let mut attempt = |with_log: bool| {
        let mut a = Attr::new();
        a.u32(0, BPF_PROG_TYPE_LSM)
            .u32(4, insns.len() as u32)
            .u64(8, insns.as_ptr() as u64)
            .u64(16, license.as_ptr() as u64);
        if with_log {
            a.u32(24, 1)
                .u32(28, LOG_SIZE as u32)
                .u64(32, log.as_mut_ptr() as u64);
        }
        let n = name.as_bytes();
        a.0[48..48 + n.len().min(15)].copy_from_slice(&n[..n.len().min(15)]);
        a.u32(68, BPF_LSM_MAC).u32(108, btf_id);
        sys(BPF_PROG_LOAD, &mut a)
    };
    match attempt(false) {
        Ok(fd) => Ok(fd_of(fd)),
        Err(first) => {
            // Again, with the verifier talking, to say why.
            let second = attempt(true);
            if let Ok(fd) = second {
                return Ok(fd_of(fd));
            }
            let end = log.iter().position(|&c| c == 0).unwrap_or(log.len());
            let text = String::from_utf8_lossy(&log[..end]);
            let text = text.trim_end();
            let tail_start = text.len().saturating_sub(LOG_TAIL);
            let tail = text
                .char_indices()
                .map(|(i, _)| i)
                .find(|&i| i >= tail_start)
                .map_or("", |i| &text[i..]);
            Err(describe(
                &format!("loading the program for {name} (BTF id {btf_id})"),
                &first,
            ) + &if tail.is_empty() {
                String::new()
            } else {
                format!("; verifier log (tail): {tail}")
            })
        }
    }
}

fn link_create(prog: &OwnedFd) -> Result<OwnedFd, String> {
    let mut a = Attr::new();
    a.u32(0, prog.as_raw_fd() as u32)
        .u32(4, 0)
        .u32(8, BPF_LSM_MAC);
    match sys(BPF_LINK_CREATE, &mut a) {
        Ok(fd) => Ok(fd_of(fd)),
        Err(first) if first.raw_os_error() == Some(libc::EINVAL) => {
            // A kernel from before LSM links: a raw tracepoint with no name.
            let mut b = Attr::new();
            b.u64(0, 0).u32(8, prog.as_raw_fd() as u32);
            sys(BPF_RAW_TRACEPOINT_OPEN, &mut b)
                .map(fd_of)
                .map_err(|e| describe("attaching the program", &e))
        }
        // x86 attaches by patching the 5-byte no-op the function tracer puts
        // at each function's entry; a kernel built without it has none, and
        // the patch is refused as busy.
        Err(e) if e.raw_os_error() == Some(libc::EBUSY) => Err(format!(
            "{}; is the kernel built with CONFIG_FUNCTION_TRACER and \
             CONFIG_DYNAMIC_FTRACE_WITH_DIRECT_CALLS? A BPF LSM program attaches \
             through the no-op the function tracer leaves at each function's entry",
            describe("attaching the program", &e)
        )),
        Err(e) => Err(describe("attaching the program", &e)),
    }
}

/// The type id in the kernel's BTF of each hook's `bpf_lsm_<hook>`, by index
/// in [`HOOKS`]; `None` where this kernel has no such function.
fn lsm_ids() -> Result<Vec<Option<u32>>, String> {
    let blob = std::fs::read(VMLINUX_BTF)
        .map_err(|e| format!("reading the kernel's BTF ({VMLINUX_BTF}): {e}"))?;
    let funcs = btf::funcs_with_prefix(&blob, "bpf_lsm_")?;
    Ok(HOOKS
        .iter()
        .map(|h| funcs.get(&format!("bpf_lsm_{h}")).copied())
        .collect())
}

/// The `HOOKS` this kernel actually has (`bpf_lsm_<hook>` is in its BTF).
pub fn available_hooks() -> Result<Vec<&'static str>, String> {
    Ok(HOOKS
        .iter()
        .zip(lsm_ids()?)
        .filter(|(_, id)| id.is_some())
        .map(|(h, _)| *h)
        .collect())
}

/// Whether this host can load and attach a BPF LSM program now: `bpf` is an
/// active LSM, the kernel has BTF for itself, and the caller may load
/// programs. It is done for real: the engine's program is loaded and attached
/// to one hook (with no rules, so it judges nothing), then dropped.
pub fn probe() -> Result<(), String> {
    let lsms = std::fs::read_to_string(LSM_LIST).map_err(|e| {
        format!("cannot read {LSM_LIST} ({e}); is securityfs mounted? BPF LSM needs `bpf` in the lsm= boot list")
    })?;
    if !lsms.trim().split(',').any(|l| l == "bpf") {
        return Err(format!(
            "the active LSMs are {:?}, which lack `bpf`; boot with lsm=...,bpf",
            lsms.trim()
        ));
    }
    let hooks = available_hooks()?;
    let hook = hooks
        .iter()
        .find(|h| **h == "quotactl")
        .or_else(|| hooks.first())
        .ok_or("the kernel's BTF has none of the hooks a shield may name")?;
    let mut engine = HookEngine::new()?;
    engine.set(&[HookRule {
        hook: (*hook).to_string(),
        scope: Scope::Everywhere,
        errno: 1,
        enforce: false,
    }])
}

/// One hook's program, and the link that keeps it attached.
struct Attached {
    // Field order is drop order: the link goes first, which detaches.
    _link: OwnedFd,
    _prog: OwnedFd,
}

/// The engine: two maps, and one attached program for each hook that has a
/// rule. Dropping it detaches everything.
pub struct HookEngine {
    exempt: OwnedFd,
    rules: OwnedFd,
    ids: Vec<Option<u32>>,
    /// `(st_dev, st_ino)` of the pid namespace tgids are given in.
    pidns: (u64, u64),
    attached: BTreeMap<usize, Attached>,
    /// What the rules map holds: `(hook index, scope)` to `(errno, enforce)`.
    current: Merged,
}

impl std::fmt::Debug for HookEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookEngine")
            .field(
                "hooks",
                &self.attached.keys().map(|i| HOOKS[*i]).collect::<Vec<_>>(),
            )
            .field("rules", &self.current.len())
            .finish()
    }
}

fn rule_key(index: usize, scope: Scope) -> [u8; insn::RULE_KEY] {
    let cgroup = match scope {
        Scope::Cgroup(id) => id,
        Scope::Everywhere => 0,
    };
    let mut k = [0u8; insn::RULE_KEY];
    k[..8].copy_from_slice(&cgroup.to_ne_bytes());
    k[8..12].copy_from_slice(&(index as u32).to_ne_bytes());
    k
}

fn rule_value(errno: i32, enforce: bool, monitored: u64, refused: u64) -> [u8; insn::RULE_VALUE] {
    let mut v = [0u8; insn::RULE_VALUE];
    v[..4].copy_from_slice(&(errno as u32).to_ne_bytes());
    v[4..8].copy_from_slice(&u32::from(enforce).to_ne_bytes());
    v[8..16].copy_from_slice(&monitored.to_ne_bytes());
    v[16..24].copy_from_slice(&refused.to_ne_bytes());
    v
}

fn counters(v: &[u8; insn::RULE_VALUE]) -> (u64, u64) {
    let m = u64::from_ne_bytes(v[8..16].try_into().expect("8 bytes"));
    let r = u64::from_ne_bytes(v[16..24].try_into().expect("8 bytes"));
    (m, r)
}

impl HookEngine {
    /// Creates the maps; attaches nothing.
    pub fn new() -> Result<Self, String> {
        let ids = lsm_ids()?;
        let ns = std::fs::metadata("/proc/self/ns/pid")
            .map_err(|e| format!("reading this process's pid namespace: {e}"))?;
        Ok(HookEngine {
            exempt: map_create("shield_exempt", insn::EXEMPT_KEY, 1, MAX_EXEMPT)?,
            rules: map_create("shield_rules", insn::RULE_KEY, insn::RULE_VALUE, MAX_KEYS)?,
            ids,
            pidns: (ns.dev(), ns.ino()),
            attached: BTreeMap::new(),
            current: Merged::new(),
        })
    }

    /// A process that is never judged, whatever rules say (the caller itself,
    /// so that it can still run what it shields). `tgid` is the process's id
    /// as seen from the pid namespace the engine was made in. Exemptions last until
    /// [`HookEngine::unexempt`] or the engine is dropped: a tgid is reused
    /// after its process exits, so a caller that exempts short-lived processes
    /// must remove them. At most 1024 at a time.
    pub fn exempt(&self, tgid: u32) -> Result<(), String> {
        map_update(&self.exempt, &tgid.to_ne_bytes(), &[1])
            .map_err(|e| describe(&format!("exempting {tgid}"), &e))
    }

    /// Takes back [`HookEngine::exempt`]; a tgid that was not exempt is fine.
    pub fn unexempt(&self, tgid: u32) -> Result<(), String> {
        match map_delete(&self.exempt, &tgid.to_ne_bytes()) {
            Err(e) if e.raw_os_error() != Some(libc::ENOENT) => {
                Err(describe(&format!("removing the exemption of {tgid}"), &e))
            }
            _ => Ok(()),
        }
    }

    /// The hooks that have a program attached now, in [`HOOKS`] order.
    pub fn attached_hooks(&self) -> Vec<&'static str> {
        self.attached.keys().map(|i| HOOKS[*i]).collect()
    }

    fn attach(&self, index: usize) -> Result<Attached, String> {
        let id = self.ids[index].ok_or_else(|| format!("no BPF LSM hook {}", HOOKS[index]))?;
        let program = insn::lsm_program(
            self.exempt.as_raw_fd(),
            self.rules.as_raw_fd(),
            index as u32,
            self.pidns,
        );
        let prog = prog_load(&program, id, "hv2_shield")?;
        let link = link_create(&prog)?;
        Ok(Attached {
            _link: link,
            _prog: prog,
        })
    }

    /// Sets the whole desired state, not a change to it.
    ///
    /// A program is attached for every hook the rules name, and the program of
    /// a hook no rule names any more is detached. The rules are checked first
    /// and an invalid list changes nothing: a hook must be one of
    /// [`available_hooks`], an errno must be in `1..=4095`, a cgroup id must
    /// not be 0, and there may be at most 4096 rules. Rules for the same
    /// `(hook, scope)` are merged: the key enforces if any of them enforces,
    /// with the errno of the first enforcing rule in order, and otherwise
    /// monitors.
    ///
    /// The counters of a `(hook, scope)` that stays are kept, and those of one
    /// that goes are deleted. A key whose errno or mode changes keeps its
    /// counters too, but an operation counted in the instant between the
    /// counters being read and written back is lost from them.
    ///
    /// Programs are attached before the rules reach them, so a new hook never
    /// judges by half a rule list.
    pub fn set(&mut self, rules: &[HookRule]) -> Result<(), String> {
        let available: Vec<&str> = HOOKS
            .iter()
            .zip(&self.ids)
            .filter(|(_, id)| id.is_some())
            .map(|(h, _)| *h)
            .collect();
        let merged = rules::merge(rules, &available)?;
        let wanted: BTreeSet<usize> = merged.keys().map(|k| k.0).collect();

        // Load and attach what is new before anything is changed: a failure
        // here drops what was made and leaves the engine as it was.
        let mut fresh = Vec::new();
        for &index in &wanted {
            if !self.attached.contains_key(&index) {
                fresh.push((index, self.attach(index)?));
            }
        }
        self.attached.extend(fresh);

        for (&(index, scope), &(errno, enforce)) in &merged {
            let key = rule_key(index, scope);
            let value = match self.current.get(&(index, scope)) {
                Some(&same) if same == (errno, enforce) => continue,
                Some(_) => {
                    let mut old = [0u8; insn::RULE_VALUE];
                    map_lookup(&self.rules, &key, &mut old)
                        .map_err(|e| describe("reading a rule's counters", &e))?;
                    let (m, r) = counters(&old);
                    rule_value(errno, enforce, m, r)
                }
                None => rule_value(errno, enforce, 0, 0),
            };
            map_update(&self.rules, &key, &value)
                .map_err(|e| describe(&format!("writing a rule for {}", HOOKS[index]), &e))?;
            self.current.insert((index, scope), (errno, enforce));
        }
        let gone: Vec<(usize, Scope)> = self
            .current
            .keys()
            .filter(|k| !merged.contains_key(k))
            .copied()
            .collect();
        for (index, scope) in gone {
            match map_delete(&self.rules, &rule_key(index, scope)) {
                Ok(()) => {}
                Err(e) if e.raw_os_error() == Some(libc::ENOENT) => {}
                Err(e) => return Err(describe("deleting a rule", &e)),
            }
            self.current.remove(&(index, scope));
        }
        self.attached.retain(|index, _| wanted.contains(index));
        Ok(())
    }

    /// Cumulative counters of every `(hook, scope)` currently set, in hook
    /// order (the order of [`HOOKS`]), then scope.
    pub fn counts(&self) -> Result<Vec<HookCount>, String> {
        let mut out = Vec::with_capacity(self.current.len());
        for &(index, scope) in self.current.keys() {
            let mut v = [0u8; insn::RULE_VALUE];
            map_lookup(&self.rules, &rule_key(index, scope), &mut v)
                .map_err(|e| describe("reading a rule's counters", &e))?;
            let (monitored, refused) = counters(&v);
            out.push(HookCount {
                hook: HOOKS[index].to_string(),
                scope,
                monitored,
                refused,
            });
        }
        Ok(out)
    }
}
