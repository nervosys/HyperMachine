//! Confining e2fsprogs when it reads a filesystem the guest wrote.
//!
//! `debugfs`, `e2fsck` and `resize2fs` parse an ext4 image whose every byte a
//! guest chose, in a C program that has had memory-safety and path bugs
//! (`debugfs rdump` has written outside its destination before). They run as
//! the node's own user, so a crafted image is a way to write anywhere the node
//! can, or to read what the node can.
//!
//! Every such tool is therefore started through [`confine`], which in the
//! forked child, before `exec`:
//!
//! * installs a **Landlock** ruleset (no privilege and no user namespace
//!   needed): the only writable places are the scratch directory and the one
//!   image, the only readable places are the image, the tool's own binaries
//!   and libraries and a few `/etc` files, and nothing else on the host is
//!   reachable -- not `/proc` (the node's environment), not its keys, not
//!   other machines' disks;
//! * sets `no_new_privs`, so `exec` gains nothing from a setuid binary;
//! * sets `RLIMIT_FSIZE` (no single file bigger than the image or dump may
//!   be), `RLIMIT_AS`, `RLIMIT_CPU` and `RLIMIT_CORE`.
//!
//! The ruleset is built in the parent, so a host that cannot confine (a
//! kernel without Landlock, or one with it disabled) is found out *before* a
//! process is started, and the operation is refused with `--allow-unconfined-e2fs-tools`
//! named as the way to accept the risk. [`output`] adds a wall-clock timeout.
//!
//! Not done: the tools keep the node's network namespace. They open no
//! sockets, and a user namespace is not available to every node (Ubuntu's
//! AppArmor refuses it), where Landlock is.

use std::ffi::OsString;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static ALLOW_UNCONFINED: AtomicBool = AtomicBool::new(false);

/// `--allow-unconfined-e2fs-tools`: run the tools unconfined where the host
/// cannot confine them.
pub(crate) fn allow_unconfined(on: bool) {
    ALLOW_UNCONFINED.store(on, Ordering::Relaxed);
}

const SYS_LANDLOCK_CREATE_RULESET: libc::c_long = 444;
const SYS_LANDLOCK_ADD_RULE: libc::c_long = 445;
const SYS_LANDLOCK_RESTRICT_SELF: libc::c_long = 446;
const LANDLOCK_CREATE_RULESET_VERSION: libc::c_ulong = 1;
const LANDLOCK_RULE_PATH_BENEATH: libc::c_int = 1;

const EXECUTE: u64 = 1 << 0;
const WRITE_FILE: u64 = 1 << 1;
const READ_FILE: u64 = 1 << 2;
const READ_DIR: u64 = 1 << 3;
const REFER: u64 = 1 << 13;
const TRUNCATE: u64 = 1 << 14;
/// Every filesystem right of ABI 1 (bits 0 to 12).
const ABI1: u64 = (1 << 13) - 1;

#[repr(C)]
struct RulesetAttr {
    handled_access_fs: u64,
}

#[repr(C, packed)]
struct PathBeneath {
    allowed_access: u64,
    parent_fd: i32,
}

/// What a tool may touch.
#[derive(Debug, Default, Clone)]
pub(crate) struct Jail {
    /// Directories it may create, write and remove in: the scratch dump.
    pub writable_dirs: Vec<PathBuf>,
    /// Files it may read.
    pub read_files: Vec<PathBuf>,
    /// Files it may read and write: the image `e2fsck` or `resize2fs` fixes.
    pub write_files: Vec<PathBuf>,
    /// The most any one file it writes may hold (`RLIMIT_FSIZE`).
    pub max_file_bytes: u64,
    /// Its address space (`RLIMIT_AS`).
    pub max_memory_bytes: u64,
    /// CPU seconds (`RLIMIT_CPU`), a backstop to the wall-clock timeout.
    pub cpu_secs: u64,
}

fn landlock_abi() -> Result<u64, String> {
    // SAFETY: a version query takes no pointers.
    let v = unsafe {
        libc::syscall(
            SYS_LANDLOCK_CREATE_RULESET,
            std::ptr::null::<RulesetAttr>(),
            0usize,
            LANDLOCK_CREATE_RULESET_VERSION,
        )
    };
    if v < 1 {
        let e = std::io::Error::last_os_error();
        return Err(format!(
            "Landlock is not available on this kernel ({e}); it needs Linux 5.13 or later \
             built with it and enabled in the boot `lsm=` list"
        ));
    }
    Ok(v as u64)
}

fn add_path(ruleset: i32, path: &Path, access: u64, must_exist: bool) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_PATH | libc::O_CLOEXEC)
        .open(path)
    {
        Ok(f) => f,
        Err(e) if !must_exist && e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    // A rule on a plain file may carry only the rights that apply to files.
    let is_dir = file.metadata().map(|m| m.is_dir()).unwrap_or(false);
    let access = if is_dir {
        access
    } else {
        access & (EXECUTE | WRITE_FILE | READ_FILE | TRUNCATE)
    };
    let rule = PathBeneath {
        allowed_access: access,
        parent_fd: file.as_raw_fd(),
    };
    // SAFETY: `rule` is a live, correctly laid out landlock_path_beneath_attr.
    let rc = unsafe {
        libc::syscall(
            SYS_LANDLOCK_ADD_RULE,
            ruleset,
            LANDLOCK_RULE_PATH_BENEATH,
            &rule as *const PathBeneath,
            0u32,
        )
    };
    if rc != 0 {
        return Err(format!(
            "Landlock refused a rule for {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// Where `tool` resolves on `PATH`, if it does.
fn which(tool: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(tool))
        .find(|p| p.is_file())
}

/// Build the ruleset for `jail`; the descriptor, to be applied in the child.
fn ruleset(tool: &str, jail: &Jail) -> Result<OwnedFd, String> {
    let abi = landlock_abi()?;
    let mut handled = ABI1;
    if abi >= 2 {
        handled |= REFER;
    }
    if abi >= 3 {
        handled |= TRUNCATE;
    }
    let attr = RulesetAttr {
        handled_access_fs: handled,
    };
    // SAFETY: `attr` is a live landlock_ruleset_attr and the size is its own.
    let fd = unsafe {
        libc::syscall(
            SYS_LANDLOCK_CREATE_RULESET,
            &attr as *const RulesetAttr,
            std::mem::size_of::<RulesetAttr>(),
            0u32,
        )
    };
    if fd < 0 {
        return Err(format!(
            "Landlock would not make a ruleset: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: a fresh descriptor this call owns.
    let owned = unsafe { OwnedFd::from_raw_fd(fd as i32) };
    let raw = owned.as_raw_fd();
    let truncate = if abi >= 3 { TRUNCATE } else { 0 };

    // Everything below the system's own directories: to run and to load.
    for dir in [
        "/usr", "/bin", "/sbin", "/lib", "/lib32", "/lib64", "/libx32", "/nix",
    ] {
        add_path(raw, Path::new(dir), EXECUTE | READ_FILE | READ_DIR, false)?;
    }
    if let Some(bin) = which(tool).and_then(|p| p.parent().map(Path::to_path_buf)) {
        add_path(raw, &bin, EXECUTE | READ_FILE | READ_DIR, false)?;
    }
    // The few configuration files the tools and the loader read.
    for file in [
        "/etc/ld.so.cache",
        "/etc/ld.so.preload",
        "/etc/ld.so.conf",
        "/etc/mke2fs.conf",
        "/etc/e2fsck.conf",
        "/etc/localtime",
    ] {
        add_path(raw, Path::new(file), READ_FILE, false)?;
    }
    add_path(
        raw,
        Path::new("/etc/ld.so.conf.d"),
        READ_FILE | READ_DIR,
        false,
    )?;
    for dev in ["/dev/null", "/dev/zero", "/dev/urandom", "/dev/random"] {
        add_path(raw, Path::new(dev), READ_FILE | WRITE_FILE, false)?;
    }
    for file in &jail.read_files {
        add_path(raw, file, READ_FILE, true)?;
    }
    for file in &jail.write_files {
        add_path(raw, file, READ_FILE | WRITE_FILE | truncate, true)?;
    }
    for dir in &jail.writable_dirs {
        add_path(raw, dir, handled, true)?;
    }
    Ok(owned)
}

/// Make `command` run confined to `jail`; the tool is `command`'s program.
///
/// An error means this host cannot confine it, and nothing was changed. With
/// `--allow-unconfined-e2fs-tools` the command is then left to run as it is
/// (with a warning) -- the only case in which this returns `Ok` without a
/// jail.
pub(crate) fn confine(command: &mut Command, jail: &Jail) -> Result<(), String> {
    let tool = command.get_program().to_string_lossy().into_owned();
    match ruleset(&tool, jail) {
        Ok(fd) => {
            apply(command, fd, jail);
            Ok(())
        }
        Err(why) if ALLOW_UNCONFINED.load(Ordering::Relaxed) => {
            tracing::warn!(
                "running {tool} UNCONFINED on a guest-written filesystem \
                 (--allow-unconfined-e2fs-tools): {why}"
            );
            Ok(())
        }
        Err(why) => Err(format!(
            "refusing to run {tool} on a guest-written filesystem without confinement: \
             {why}. Pass --allow-unconfined-e2fs-tools to accept running it unconfined"
        )),
    }
}

/// Allow reading the file at `path` under `ruleset`, with nothing but system
/// calls (it runs between fork and exec). A file that is not there is skipped.
///
/// # Safety
/// `ruleset` must be a Landlock ruleset descriptor.
unsafe fn allow_read_raw(ruleset: i32, path: &std::ffi::CStr) {
    let fd = libc::open(path.as_ptr(), libc::O_PATH | libc::O_CLOEXEC);
    if fd < 0 {
        return;
    }
    let rule = PathBeneath {
        allowed_access: READ_FILE,
        parent_fd: fd,
    };
    libc::syscall(
        SYS_LANDLOCK_ADD_RULE,
        ruleset,
        LANDLOCK_RULE_PATH_BENEATH,
        &rule as *const PathBeneath,
        0u32,
    );
    libc::close(fd);
}

fn apply(command: &mut Command, fd: OwnedFd, jail: &Jail) {
    let fsize = jail.max_file_bytes;
    let memory = jail.max_memory_bytes;
    let cpu = jail.cpu_secs;
    // SAFETY: only async-signal-safe system calls between fork and exec.
    unsafe {
        command.pre_exec(move || {
            let limit = |resource, value: u64| {
                let l = libc::rlimit {
                    rlim_cur: value as libc::rlim_t,
                    rlim_max: value as libc::rlim_t,
                };
                libc::setrlimit(resource, &l)
            };
            limit(libc::RLIMIT_CORE as _, 0);
            if fsize > 0 && limit(libc::RLIMIT_FSIZE as _, fsize) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if memory > 0 && limit(libc::RLIMIT_AS as _, memory) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if cpu > 0 && limit(libc::RLIMIT_CPU as _, cpu) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            // The mount table, which resize2fs reads to be sure the image is
            // not mounted. Its path names this very process, so the rule is
            // made here, after the fork, and not by the parent.
            for path in [c"/proc/self/mounts", c"/proc/self/mountinfo"] {
                allow_read_raw(fd.as_raw_fd(), path);
            }
            if libc::syscall(SYS_LANDLOCK_RESTRICT_SELF, fd.as_raw_fd(), 0u32) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

/// A confined `tool image` command for an e2fsprogs tool: `args` first, the
/// image last; `scratch` the one directory it may write in, and `writable`
/// whether it may change the image.
pub(crate) fn tool(
    tool: &str,
    args: &[OsString],
    image: &Path,
    writable: bool,
    scratch: Option<&Path>,
    max_file_bytes: u64,
) -> Result<Command, String> {
    let mut jail = Jail {
        max_file_bytes,
        max_memory_bytes: 8 << 30,
        cpu_secs: 3600,
        ..Jail::default()
    };
    if writable {
        jail.write_files.push(image.to_path_buf());
    } else {
        jail.read_files.push(image.to_path_buf());
    }
    if let Some(dir) = scratch {
        jail.writable_dirs.push(dir.to_path_buf());
    }
    let mut command = Command::new(tool);
    command.args(args).stdin(Stdio::null());
    confine(&mut command, &jail)?;
    Ok(command)
}

/// Run `command` to its end, or kill it at `timeout`; its output.
pub(crate) fn output(command: &mut Command, timeout: Duration) -> Result<Output, String> {
    const CAP: u64 = 1 << 30;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "running {} (is e2fsprogs installed?): {e}",
                command.get_program().to_string_lossy()
            )
        })?;
    let drain = |stream: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(stream) = stream {
                let mut stream = stream;
                let _ = (&mut stream).take(CAP).read_to_end(&mut bytes);
                let _ = std::io::copy(&mut stream, &mut std::io::sink());
            }
            bytes
        })
    };
    let out = drain(child.stdout.take().map(|s| Box::new(s) as _));
    let err = drain(child.stderr.take().map(|s| Box::new(s) as _));
    let began = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if began.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out.join();
                let _ = err.join();
                return Err(format!("took over {timeout:?}; stopped"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(e.to_string()),
        }
    };
    Ok(Output {
        status,
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str, jail: &Jail) -> Result<Output, String> {
        let mut c = Command::new("sh");
        c.arg("-c").arg(script);
        confine(&mut c, jail)?;
        output(&mut c, Duration::from_secs(20))
    }

    /// The confinement is real: a child that may write only in its scratch
    /// directory and read only its image can do that, and cannot write
    /// anywhere else, nor read what is not its own.
    #[test]
    fn a_confined_child_writes_only_in_its_scratch_directory() {
        let scratch = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let image = tempfile::NamedTempFile::new().unwrap();
        let secret = outside.path().join("secret");
        std::fs::write(&secret, "hunter2").unwrap();
        let jail = Jail {
            writable_dirs: vec![scratch.path().to_path_buf()],
            read_files: vec![image.path().to_path_buf()],
            max_file_bytes: 1 << 20,
            max_memory_bytes: 1 << 30,
            cpu_secs: 60,
            ..Jail::default()
        };
        if landlock_abi().is_err() {
            // No Landlock: confinement must be refused, not skipped.
            let e = sh("true", &jail).unwrap_err();
            assert!(e.contains("--allow-unconfined-e2fs-tools"), "{e}");
            return;
        }
        let script = format!(
            "echo ok > '{inside}/in' && echo inside-written; \
             echo x > '{outside}/escaped' 2>/dev/null && echo OUTSIDE-WRITTEN; \
             cat '{secret}' 2>/dev/null && echo SECRET-READ; \
             cat '{image}' >/dev/null && echo image-read; \
             ls /proc/self/environ >/dev/null 2>&1; cat /proc/self/environ >/dev/null 2>&1 \
               && echo PROC-READ; \
             true",
            inside = scratch.path().display(),
            outside = outside.path().display(),
            secret = secret.display(),
            image = image.path().display(),
        );
        let out = sh(&script, &jail).unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(said.contains("inside-written"), "{said}");
        assert!(said.contains("image-read"), "{said}");
        assert!(!said.contains("OUTSIDE-WRITTEN"), "{said}");
        assert!(!said.contains("SECRET-READ"), "{said}");
        assert!(!said.contains("PROC-READ"), "{said}");
        assert!(!outside.path().join("escaped").exists());
        assert!(scratch.path().join("in").exists());
    }

    /// The file-size limit is in force in the child: a file it writes stops
    /// at the limit.
    #[test]
    fn a_confined_child_cannot_write_a_file_past_its_size_limit() {
        let scratch = tempfile::tempdir().unwrap();
        let jail = Jail {
            writable_dirs: vec![scratch.path().to_path_buf()],
            max_file_bytes: 4096,
            max_memory_bytes: 1 << 30,
            cpu_secs: 60,
            ..Jail::default()
        };
        if landlock_abi().is_err() {
            return;
        }
        let script = format!(
            "head -c 100000 /dev/zero > '{}/big'; true",
            scratch.path().display()
        );
        sh(&script, &jail).unwrap();
        let size = std::fs::metadata(scratch.path().join("big")).unwrap().len();
        assert!(size <= 4096, "wrote {size} bytes past a 4096 limit");
    }

    #[test]
    fn a_child_that_overruns_its_timeout_is_killed() {
        let mut c = Command::new("sleep");
        c.arg("30");
        let began = Instant::now();
        let e = output(&mut c, Duration::from_millis(200)).unwrap_err();
        assert!(e.contains("stopped"), "{e}");
        assert!(began.elapsed() < Duration::from_secs(10));
    }

    /// With no way to confine and no flag, nothing runs.
    #[test]
    fn without_confinement_and_without_the_flag_the_tool_is_refused() {
        let jail = Jail {
            // A path that is not there makes the ruleset fail the same way an
            // unsupported kernel does.
            read_files: vec![PathBuf::from("/nonexistent-hm-image")],
            ..Jail::default()
        };
        let mut c = Command::new("true");
        let e = confine(&mut c, &jail).unwrap_err();
        assert!(e.contains("--allow-unconfined-e2fs-tools"), "{e}");
    }
}
