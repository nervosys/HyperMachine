//! Windows confinement, built on job objects and AppContainers.
//!
//! A job object bounds what a workload consumes. An AppContainer
//! ([`appcontainer`]) bounds what it can reach, and is what a workload asked to
//! run with no network is started in.
//!
//! # What a job object gives us, and what it does not
//!
//! A job object is a kernel container for a set of processes with limits the
//! kernel enforces: committed memory, active process count, and total CPU
//! time. Terminating the job kills every process in it at once, so a workload
//! that spawned children cannot outlive its sandbox.
//!
//! It is not a container. A job object does not isolate the network, does not
//! give the workload a different filesystem, does not hide the rest of the
//! process table, and does not stop a process gaining privileges. The first
//! is what the AppContainer is for; the other three
//! are reported as unavailable, with a reason, so a caller asking for them is
//! refused here rather than being quietly handed a process with none of them.
//! A caller that needs them on Windows needs the microVM sandbox.
//!
//! # The assignment race, and why the child starts suspended
//!
//! A process must be *in* the job before it runs, or it has a window in which
//! to allocate past the memory limit or spawn a process that escapes the set.
//! Assigning after `spawn` returns leaves exactly that window. So the child is
//! created suspended, assigned to the job, and only then resumed — which is
//! why this file walks the thread table to find the main thread. There is no
//! way to resume a process through `std::process`, and a sandbox with a
//! start-up hole in it is not one.

use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicUIRestrictions,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_BASIC_LIMIT_INFORMATION, JOBOBJECT_BASIC_UI_RESTRICTIONS,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_JOB_MEMORY, JOB_OBJECT_LIMIT_JOB_TIME, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    OpenThread, ResumeThread, CREATE_SUSPENDED, THREAD_SUSPEND_RESUME,
};

use crate::{
    Control, Controls, FilesystemPolicy, NetworkPolicy, RunIo, SandboxCommand, SandboxError,
    SandboxOutput, SandboxSpec,
};

use super::driver;

mod appcontainer;

/// What Windows job objects enforce.
///
/// Probed the same way as everywhere else — a job object can be unavailable,
/// most often because this process is already inside one that forbids nested
/// jobs on older Windows.
pub(super) fn probe() -> Controls {
    let mut controls = Controls::none();
    // An AppContainer with no capabilities has no network. Probed by making
    // one: profiles are refused in some sessions, and saying so here beats
    // finding out on the first workload.
    // The same container confines a workload to the paths it was granted,
    // beyond what Windows lets every packaged application read.
    controls = match appcontainer::Container::create(false) {
        Ok(_) => controls
            .with(Control::NetworkIsolation)
            .with(Control::PathConfinement)
            .with(Control::PathDenial),
        Err(e) => {
            let why = format!("an AppContainer could not be created here: {e}");
            controls
                .without(Control::NetworkIsolation, why.clone())
                .without(Control::PathConfinement, why.clone())
                .without(Control::PathDenial, why)
        }
    };
    let mut controls = controls
        .without(
            Control::FilesystemIsolation,
            "a job object does not change the filesystem view; use the microVM sandbox",
        )
        .without(
            Control::ProcessIsolation,
            "a job object bounds a process set but does not hide the rest of the process table",
        )
        .without(
            Control::NoNewPrivileges,
            "Windows has no no-new-privileges bit; a restricted token would be a different \
             mechanism with different semantics",
        );

    // Try to create and configure a job. If that fails, this host enforces
    // nothing through this backend, and saying so beats discovering it on the
    // first workload.
    match Job::create() {
        Ok(job) => {
            controls = match job.restrict_ui() {
                Ok(()) => controls.with(Control::UiIsolation),
                Err(e) => controls.without(
                    Control::UiIsolation,
                    format!("a job object's user-interface restrictions could not be set: {e}"),
                ),
            };
            let limits = JobLimits {
                memory_bytes: Some(64 * 1024 * 1024),
                max_processes: Some(8),
                cpu_time: Some(std::time::Duration::from_secs(1)),
            };
            match job.apply(&limits) {
                Ok(()) => {
                    controls = controls
                        .with(Control::Memory)
                        .with(Control::ProcessCount)
                        .with(Control::CpuTime)
                        // Enforced by this crate rather than by the kernel: the
                        // driver kills the whole job when the deadline passes,
                        // which is a real kill, not a request.
                        .with(Control::WallClock);
                }
                Err(e) => {
                    let reason = format!("job object limits could not be set: {e}");
                    for control in [Control::Memory, Control::ProcessCount, Control::CpuTime] {
                        controls = controls.clone().without(control, reason.clone());
                    }
                    controls = controls.with(Control::WallClock);
                }
            }
        }
        Err(e) => {
            let reason = format!("job objects are unavailable: {e}");
            for control in [
                Control::Memory,
                Control::ProcessCount,
                Control::CpuTime,
                Control::WallClock,
                Control::UiIsolation,
            ] {
                controls = controls.clone().without(control, reason.clone());
            }
        }
    }

    controls
}

/// Every `JOB_OBJECT_UILIMIT_*` bit: handles, reading and writing the
/// clipboard, system parameters, display settings, global atoms, desktops,
/// and exiting Windows.
const UI_LIMIT_ALL: u32 = 0xFF;

/// Limits a job object can carry.
struct JobLimits {
    memory_bytes: Option<u64>,
    max_processes: Option<u32>,
    cpu_time: Option<std::time::Duration>,
}

/// An owned job object handle.
struct Job(HANDLE);

impl Job {
    fn create() -> std::io::Result<Self> {
        // SAFETY: a null name and null attributes create an unnamed job owned
        // by this process, which is what the arguments say.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self(handle))
    }

    /// Apply `limits` to the job.
    fn apply(&self, limits: &JobLimits) -> std::io::Result<()> {
        // SAFETY: the struct is plain data and is fully initialised below.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        let mut basic = JOBOBJECT_BASIC_LIMIT_INFORMATION {
            // Kill everything still in the job when the last handle closes, so
            // a panic on the host side cannot leave a workload running.
            LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            ..unsafe { std::mem::zeroed() }
        };

        if let Some(max) = limits.max_processes {
            basic.LimitFlags |= JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
            basic.ActiveProcessLimit = max;
        }
        if let Some(cpu) = limits.cpu_time {
            basic.LimitFlags |= JOB_OBJECT_LIMIT_JOB_TIME;
            // PerJobUserTimeLimit is in 100-nanosecond units.
            basic.PerJobUserTimeLimit = (cpu.as_nanos() / 100).min(i64::MAX as u128) as i64;
        }

        info.BasicLimitInformation = basic;
        if let Some(bytes) = limits.memory_bytes {
            info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
            info.JobMemoryLimit = bytes as usize;
        }

        // SAFETY: `info` outlives the call and its size is passed exactly.
        let ok: BOOL = unsafe {
            SetInformationJobObject(
                self.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    /// Keep everything in the job from the desktop it runs on.
    ///
    /// Every restriction a job has, together: the clipboard both ways, the
    /// windows and other user-interface handles of processes outside the
    /// job, the system's parameters and the display's settings, the global
    /// atom table, making or switching desktops, and logging off or shutting
    /// down.
    fn restrict_ui(&self) -> std::io::Result<()> {
        let restrictions = JOBOBJECT_BASIC_UI_RESTRICTIONS {
            UIRestrictionsClass: UI_LIMIT_ALL,
        };
        // SAFETY: `restrictions` outlives the call and its size is passed
        // exactly.
        let ok: BOOL = unsafe {
            SetInformationJobObject(
                self.0,
                JobObjectBasicUIRestrictions,
                &restrictions as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>() as u32,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    /// Put `process` in the job.
    fn assign(&self, process: HANDLE) -> std::io::Result<()> {
        // SAFETY: both handles are open and owned by this process.
        if unsafe { AssignProcessToJobObject(self.0, process) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    /// Kill every process in the job.
    fn terminate(&self) {
        // SAFETY: the handle is open; terminating a job with nothing in it is
        // not an error.
        unsafe { TerminateJobObject(self.0, 1) };
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // KILL_ON_JOB_CLOSE means this also stops anything still running, so a
        // dropped handle cannot leave a workload behind.
        // SAFETY: the handle was created by this type and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

/// Run `command` under `spec`.
pub(super) fn run(
    command: &SandboxCommand,
    spec: &SandboxSpec,
    io: &RunIo,
) -> Result<SandboxOutput, SandboxError> {
    if let FilesystemPolicy::Isolated { .. } = spec.filesystem {
        return Err(SandboxError::InvalidSpec(
            "this backend cannot isolate the filesystem on Windows".to_string(),
        ));
    }
    // No network means an AppContainer. Where one cannot be made, the spec
    // was refused before it got here -- unless the caller said best effort,
    // and then the workload runs with the host's network and the control is
    // reported as dropped, which the probe already told them.
    // Confined to its granted paths means a container too, and one that
    // keeps the network when the network was not what was asked to go.
    // A path is closed to a container, so a denied path means one too: with
    // the network kept, and with everything else of the user's closed as it
    // is to any container. More than was asked, never less.
    let container = if spec.network == NetworkPolicy::Denied
        || spec.confine_paths
        || !spec.grants.denied.is_empty()
    {
        match appcontainer::Container::create(spec.network == NetworkPolicy::Host) {
            Ok(container) => Some(container),
            Err(_) if spec.best_effort => None,
            Err(e) => {
                return Err(SandboxError::ConfinementFailed {
                    control: if spec.network == NetworkPolicy::Denied {
                        Control::NetworkIsolation
                    } else if spec.confine_paths {
                        Control::PathConfinement
                    } else {
                        Control::PathDenial
                    },
                    source: e,
                })
            }
        }
    } else {
        None
    };

    let job = Job::create().map_err(|e| SandboxError::Spawn {
        program: command.program.clone(),
        source: e,
    })?;
    job.apply(&JobLimits {
        memory_bytes: spec.memory_bytes,
        max_processes: spec.max_processes,
        cpu_time: spec.cpu_time,
    })
    .map_err(|e| SandboxError::ConfinementFailed {
        control: Control::Memory,
        source: e,
    })?;
    if spec.isolate_ui {
        job.restrict_ui()
            .map_err(|e| SandboxError::ConfinementFailed {
                control: Control::UiIsolation,
                source: e,
            })?;
    }

    if let Some(container) = container {
        return run_contained(command, spec, io, job, container);
    }

    let mut builder = Command::new(&command.program);
    builder
        .args(&command.args)
        .env_clear()
        .envs(&command.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Suspended, so the process is in the job before it executes an
        // instruction. Assigning afterwards leaves a window in which it can
        // allocate past the limit or spawn a process that escapes the set.
        .creation_flags(CREATE_SUSPENDED);
    if let Some(dir) = &command.working_dir {
        builder.current_dir(dir);
    }

    let child = builder.spawn().map_err(|e| SandboxError::Spawn {
        program: command.program.clone(),
        source: e,
    })?;

    let process_handle = child.as_raw_handle() as HANDLE;
    let pid = child.id();

    if let Err(e) = job.assign(process_handle) {
        // The child exists and is suspended with no limits on it. Kill it
        // rather than resume it: a workload outside its sandbox is worse than
        // one that never started.
        job.terminate();
        let _ = kill_suspended(pid);
        return Err(SandboxError::ConfinementFailed {
            control: Control::ProcessCount,
            source: e,
        });
    }

    if let Err(e) = resume_main_thread(pid) {
        job.terminate();
        return Err(SandboxError::Runtime(format!(
            "the workload was confined but could not be started: {e}"
        )));
    }

    driver::wait_with_deadline(child, command.stdin.as_deref(), spec.wall_clock, io, || {
        // Terminate the job, not the process: a workload that spawned children
        // would otherwise leave them running past its own deadline.
        job.terminate();
    })
}

/// Run `command` in an AppContainer with no capabilities, inside `job`.
fn run_contained(
    command: &SandboxCommand,
    spec: &SandboxSpec,
    io: &RunIo,
    job: Job,
    container: appcontainer::Container,
) -> Result<SandboxOutput, SandboxError> {
    // The paths it was granted, opened to this container and no other, for
    // as long as it runs.
    let granted = appcontainer::Grants::give(&container, &spec.grants).map_err(|e| {
        SandboxError::ConfinementFailed {
            control: if spec.grants.denied.is_empty() {
                Control::NetworkIsolation
            } else {
                Control::PathDenial
            },
            source: e,
        }
    })?;
    let started = appcontainer::spawn(command, &container).map_err(|e| SandboxError::Spawn {
        program: command.program.clone(),
        source: e,
    })?;
    // In the job before it runs, for the reason this file opens with. It is
    // suspended, so killing the job is all that undoing it takes.
    if let Err(e) = job.assign(started.process) {
        job.terminate();
        return Err(SandboxError::ConfinementFailed {
            control: Control::ProcessCount,
            source: e,
        });
    }
    if let Err(e) = started.resume() {
        job.terminate();
        return Err(SandboxError::Runtime(format!(
            "the workload was confined but could not be started: {e}"
        )));
    }
    let output = driver::wait_with_deadline(
        started.spawned,
        command.stdin.as_deref(),
        spec.wall_clock,
        io,
        || job.terminate(),
    );
    // After the workload, and in this order: what was opened to it is closed
    // while its SID still names something, then the profile goes, with the
    // folder Windows made for it.
    drop(granted);
    drop(container);
    output
}

/// Kill a process we created suspended and then decided not to run.
fn kill_suspended(pid: u32) -> std::io::Result<()> {
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    // SAFETY: the pid is one we just created, so it is valid; a failed open is
    // reported rather than used.
    let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
    if handle.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: the handle was opened with PROCESS_TERMINATE and is closed below.
    unsafe {
        TerminateProcess(handle, 1);
        CloseHandle(handle);
    }
    Ok(())
}

/// Resume the initial thread of a process created with `CREATE_SUSPENDED`.
///
/// `std::process` gives no way to reach the thread it created, so the thread
/// table is walked to find the one belonging to this process. A freshly
/// created suspended process has exactly one.
fn resume_main_thread(pid: u32) -> std::io::Result<()> {
    // SAFETY: a thread snapshot takes no process handle and is closed below.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }

    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;

    let mut result = Err(std::io::Error::other(
        "no thread found for the suspended workload",
    ));

    // SAFETY: `entry` is sized as the API requires and the snapshot is valid.
    let mut ok = unsafe { Thread32First(snapshot, &mut entry) };
    while ok != 0 {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: the thread id came from the snapshot; the handle is
            // closed immediately after use.
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if thread.is_null() {
                result = Err(std::io::Error::last_os_error());
            } else {
                let previous = unsafe { ResumeThread(thread) };
                unsafe { CloseHandle(thread) };
                result = if previous == u32::MAX {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                };
            }
            break;
        }
        // SAFETY: as above.
        ok = unsafe { Thread32Next(snapshot, &mut entry) };
    }

    // SAFETY: the snapshot handle is open and closed exactly once.
    unsafe { CloseHandle(snapshot) };
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProcessSandbox, Sandbox};
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// A system program, with the environment one needs to start.
    fn system(program: &str) -> SandboxCommand {
        SandboxCommand::new(format!(r"C:\Windows\System32\{program}"))
            .env("SystemRoot", r"C:\Windows")
            .env("PATH", r"C:\Windows\System32")
    }

    /// No network, and nothing else asked for.
    fn no_network() -> SandboxSpec {
        SandboxSpec {
            network: NetworkPolicy::Denied,
            wall_clock: Some(Duration::from_secs(30)),
            ..SandboxSpec::unconfined()
        }
    }

    /// An HTTP server on loopback that counts the connections it accepts.
    fn listener() -> (u16, Arc<AtomicUsize>) {
        let socket = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = socket.local_addr().expect("address").port();
        let accepted = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&accepted);
        std::thread::spawn(move || {
            for stream in socket.incoming() {
                let Ok(mut stream) = stream else { break };
                count.fetch_add(1, Ordering::SeqCst);
                let mut request = [0u8; 1024];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nreached",
                );
            }
        });
        (port, accepted)
    }

    /// The control this backend claims for a workload with no network is one
    /// the kernel enforces: the same request that reaches a listener on this
    /// machine with the host's network does not reach it without, and the
    /// listener never sees a connection.
    #[test]
    fn a_workload_with_no_network_cannot_reach_even_loopback() {
        let sandbox = ProcessSandbox::new();
        if !sandbox.controls().enforces(Control::NetworkIsolation) {
            eprintln!(
                "skipping: {:?}",
                sandbox.controls().reason(Control::NetworkIsolation)
            );
            return;
        }
        let (port, accepted) = listener();
        let fetch = system("curl.exe").args([
            "-s".to_string(),
            "-m".to_string(),
            "5".to_string(),
            format!("http://127.0.0.1:{port}/"),
        ]);

        // With the host's network it gets through, so what follows is the
        // sandbox's doing and not a broken request.
        let open = sandbox
            .run(&fetch, &SandboxSpec::unconfined())
            .expect("run with the host's network");
        assert_eq!(open.exit_code, Some(0), "{open:?}");
        assert_eq!(open.stdout, b"reached");
        assert_eq!(accepted.load(Ordering::SeqCst), 1);

        let closed = sandbox
            .run(&fetch, &no_network())
            .expect("run with no network");
        assert_ne!(closed.exit_code, Some(0), "{closed:?}");
        assert!(closed.stdout.is_empty(), "{closed:?}");
        assert!(closed.unenforced.is_empty(), "{closed:?}");
        assert_eq!(accepted.load(Ordering::SeqCst), 1, "it connected");
    }

    /// A contained workload is still a workload: its output, its exit code
    /// and its standard input arrive as they do outside a container.
    #[test]
    fn a_contained_workload_keeps_its_streams_and_exit_code() {
        let sandbox = ProcessSandbox::new();
        if !sandbox.controls().enforces(Control::NetworkIsolation) {
            return;
        }
        let command = system("cmd.exe").args(["/c", "echo out& echo err 1>&2& exit 7"]);
        let output = sandbox.run(&command, &no_network()).expect("run");
        assert_eq!(output.exit_code, Some(7), "{output:?}");
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "out");
        assert_eq!(String::from_utf8_lossy(&output.stderr).trim(), "err");

        let mut echo = system("findstr.exe").args(["x"]);
        echo.stdin = Some(b"axb\r\nnone\r\n".to_vec());
        let output = sandbox.run(&echo, &no_network()).expect("run");
        assert_eq!(output.exit_code, Some(0), "{output:?}");
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "axb");
    }

    /// The deadline and the job's limits hold for a contained workload too.
    #[test]
    fn a_contained_workload_is_killed_at_its_deadline() {
        let sandbox = ProcessSandbox::new();
        if !sandbox.controls().enforces(Control::NetworkIsolation) {
            return;
        }
        let spec = SandboxSpec {
            wall_clock: Some(Duration::from_millis(500)),
            ..no_network()
        };
        let started = std::time::Instant::now();
        let output = sandbox
            // A loop that never ends by itself. Not `ping`, the usual way to
            // wait: with no network it cannot reach even its own driver.
            .run(
                &system("cmd.exe").args(["/c", "for /l %i in () do @rem"]),
                &spec,
            )
            .expect("run");
        assert_eq!(output.killed_by, Some(Control::WallClock), "{output:?}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    /// A granted path is opened to the workload for the run -- to read, or to
    /// read and write, as asked -- and closed again afterwards: the entries
    /// that opened it are gone from its access-control list.
    #[test]
    fn a_granted_path_is_opened_for_the_run_and_closed_after() {
        let sandbox = ProcessSandbox::new();
        if !sandbox.controls().enforces(Control::NetworkIsolation) {
            return;
        }
        let base = std::env::temp_dir().join(format!("hv2-sandbox-grants-{}", std::process::id()));
        let (readable, writable) = (base.join("readable"), base.join("writable"));
        std::fs::create_dir_all(&readable).expect("make");
        std::fs::create_dir_all(&writable).expect("make");
        std::fs::write(readable.join("note.txt"), "granted").expect("write");
        let shell = |line: String| system("cmd.exe").args(["/c".to_string(), line]);
        let text =
            |output: &SandboxOutput| String::from_utf8_lossy(&output.stdout).trim().to_string();

        // Without a grant, neither can be touched.
        let closed = sandbox
            .run(
                &shell(format!("type {}", readable.join("note.txt").display())),
                &no_network(),
            )
            .expect("run");
        assert_ne!(closed.exit_code, Some(0), "{closed:?}");

        let spec = SandboxSpec {
            grants: crate::PathGrants {
                read_only: vec![readable.clone()],
                read_write: vec![writable.clone()],
                denied: Vec::new(),
            },
            ..no_network()
        };
        let read = sandbox
            .run(
                &shell(format!("type {}", readable.join("note.txt").display())),
                &spec,
            )
            .expect("run");
        assert_eq!(text(&read), "granted", "{read:?}");
        // Read-only means it: nothing is written there.
        let refused = sandbox
            .run(
                &shell(format!("echo x> {}", readable.join("made.txt").display())),
                &spec,
            )
            .expect("run");
        assert_ne!(refused.exit_code, Some(0), "{refused:?}");
        assert!(!readable.join("made.txt").exists());
        // Read-write: a file made inside is on the host afterwards.
        let wrote = sandbox
            .run(
                &shell(format!(
                    "echo made> {0}& type {0}",
                    writable.join("made.txt").display()
                )),
                &spec,
            )
            .expect("run");
        assert_eq!(text(&wrote), "made", "{wrote:?}");
        assert_eq!(
            std::fs::read_to_string(writable.join("made.txt"))
                .expect("read")
                .trim(),
            "made"
        );

        // Afterwards the container is named on neither: an AppContainer's
        // SID begins S-1-15-2, and `icacls` prints one it cannot resolve.
        for path in [&readable, &writable] {
            let listed = sandbox
                .run(
                    &system("icacls.exe").args([path.display().to_string()]),
                    &SandboxSpec::unconfined(),
                )
                .expect("run");
            let listed = text(&listed);
            assert!(
                !listed.is_empty() && !listed.contains("S-1-15-2"),
                "{listed}"
            );
        }

        // A path that cannot be granted refuses the run before it starts.
        for bad in [std::path::PathBuf::from("relative"), base.join("absent")] {
            let spec = SandboxSpec {
                grants: crate::PathGrants {
                    read_only: vec![bad],
                    read_write: Vec::new(),
                    denied: Vec::new(),
                },
                ..no_network()
            };
            let refused = sandbox.run(&shell("echo ran".to_string()), &spec);
            assert!(
                matches!(refused, Err(SandboxError::InvalidSpec(_))),
                "{refused:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Under this variable, [`ui_helper`] is the workload: it reads a system
    /// parameter and sets it to the value it read, which changes nothing and
    /// is still a change the job may refuse.
    const UI_HELPER: &str = "HV2_SANDBOX_UI_HELPER";

    #[link(name = "user32")]
    extern "system" {
        fn SystemParametersInfoW(
            action: u32,
            param: u32,
            value: *mut core::ffi::c_void,
            update: u32,
        ) -> i32;
        fn OpenClipboard(owner: *mut core::ffi::c_void) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(format: u32) -> *mut core::ffi::c_void;
        fn EmptyClipboard() -> i32;
        fn SetClipboardData(format: u32, data: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalAlloc(flags: u32, bytes: usize) -> *mut core::ffi::c_void;
    }

    #[test]
    fn ui_helper() {
        let Ok(mode) = std::env::var(UI_HELPER) else {
            return;
        };
        const SPI_GETMOUSESPEED: u32 = 0x0070;
        const SPI_SETMOUSESPEED: u32 = 0x0071;
        let mut speed: i32 = 0;
        // SAFETY: the get writes one int through the pointer; the set takes
        // the value itself in the pointer argument, as documented.
        let read =
            unsafe { SystemParametersInfoW(SPI_GETMOUSESPEED, 0, (&raw mut speed).cast(), 0) };
        assert_ne!(read, 0, "the mouse speed could not be read");
        let set = unsafe { SystemParametersInfoW(SPI_SETMOUSESPEED, 0, speed as usize as _, 0) };
        println!("set={}", if set != 0 { "allowed" } else { "refused" });

        if mode == "clipboard" {
            // SAFETY: the clipboard is opened, asked for text, emptied, given
            // an empty string, and closed. The string is movable zeroed
            // memory, which is what the clipboard takes; it owns it once set.
            // Only reached on a clipboard the caller said may be lost.
            unsafe {
                const CF_UNICODETEXT: u32 = 13;
                const GMEM_MOVEABLE_ZEROED: u32 = 0x0002 | 0x0040;
                let opened = OpenClipboard(std::ptr::null_mut()) != 0;
                let text = opened && !GetClipboardData(CF_UNICODETEXT).is_null();
                // Emptying is what a write begins with, and is reported apart
                // from it: the first run of this under CI showed a restricted
                // program refused the read and allowed to empty.
                let emptied = opened && EmptyClipboard() != 0;
                let wrote = opened
                    && !SetClipboardData(CF_UNICODETEXT, GlobalAlloc(GMEM_MOVEABLE_ZEROED, 4))
                        .is_null();
                if opened {
                    CloseClipboard();
                }
                println!("read={}", if text { "allowed" } else { "refused" });
                println!("write={}", if wrote { "allowed" } else { "refused" });
                println!("empty={}", if emptied { "allowed" } else { "refused" });
            }
        }
    }

    /// The same program is allowed to change a system parameter outside the
    /// restrictions and refused inside them. Where the clipboard may be lost
    /// -- under CI, never on a desk -- reading and writing it are tried too.
    #[test]
    fn a_ui_isolated_workload_cannot_change_the_desktop_it_runs_on() {
        let sandbox = ProcessSandbox::new();
        assert!(sandbox.controls().enforces(Control::UiIsolation));
        let clipboard = std::env::var_os("CI").is_some();
        if clipboard {
            // Something for a read to find.
            let filled = sandbox
                .run(
                    &system("cmd.exe").args(["/c", "echo hv2-sandbox| clip"]),
                    &SandboxSpec::unconfined(),
                )
                .expect("run");
            assert_eq!(filled.exit_code, Some(0), "{filled:?}");
        }
        let exe = std::env::current_exe().expect("this test binary's own path");
        let helper = SandboxCommand::new(exe.to_string_lossy())
            .args([
                "--exact",
                "process::windows::tests::ui_helper",
                "--nocapture",
            ])
            .env("SystemRoot", r"C:\Windows")
            .env("PATH", r"C:\Windows\System32")
            .env(
                UI_HELPER,
                if clipboard { "clipboard" } else { "parameters" },
            );
        let said = |spec: &SandboxSpec| {
            let output = sandbox.run(&helper, spec).expect("run");
            let text = String::from_utf8_lossy(&output.stdout).to_string();
            let lines: Vec<String> = text
                .lines()
                .filter(|line| {
                    ["set=", "read=", "write=", "empty="]
                        .iter()
                        .any(|key| line.starts_with(key))
                })
                .map(str::to_string)
                .collect();
            assert!(!lines.is_empty(), "{output:?}");
            lines
        };

        let isolated = SandboxSpec {
            isolate_ui: true,
            wall_clock: Some(Duration::from_secs(30)),
            ..SandboxSpec::unconfined()
        };
        assert_eq!(
            isolated.required(),
            vec![Control::WallClock, Control::UiIsolation]
        );
        // Inside first, so the clipboard still holds its text when the read
        // is refused.
        let inside = said(&isolated);
        if clipboard {
            // Whatever the run inside did to it, the one outside has text to
            // find.
            let refilled = sandbox
                .run(
                    &system("cmd.exe").args(["/c", "echo hv2-sandbox| clip"]),
                    &SandboxSpec::unconfined(),
                )
                .expect("run");
            assert_eq!(refilled.exit_code, Some(0), "{refilled:?}");
        }
        let outside = said(&SandboxSpec {
            isolate_ui: false,
            ..isolated
        });
        assert_eq!(inside[0], "set=refused", "{inside:?}");
        assert_eq!(outside[0], "set=allowed", "{outside:?}");
        if clipboard {
            assert_eq!(
                inside[1..3],
                ["read=refused", "write=refused"],
                "{inside:?}"
            );
            // Not asserted, reported: emptying the clipboard is not writing
            // to it, and the restriction may let it through.
            println!("clipboard: inside the restrictions, {}", inside[3]);
            // A runner with no usable clipboard refuses outside as well, and
            // then the refusal inside shows nothing. Say which it was, where
            // the log will be read, instead of failing on the runner or
            // passing as if the clipboard had been tried.
            if outside[1..3] == ["read=allowed", "write=allowed"] {
                println!("clipboard: refused inside the restrictions and allowed outside them");
            } else {
                println!(
                    "clipboard: NOT VERIFIED, unusable outside the restrictions too: {outside:?}"
                );
            }
        }
    }

    /// A denied path under a grant is closed, the rest of the grant is open,
    /// and afterwards the path inherits its access-control list again and
    /// names no container. An access-denied entry for the container's SID was
    /// tried first, and failed the same assertions: the file was read.
    #[test]
    fn a_denied_path_is_carved_out_of_a_grant() {
        let sandbox = ProcessSandbox::new();
        assert!(sandbox.controls().enforces(Control::PathDenial));
        let base = std::env::temp_dir().join(format!("hv2-sandbox-deny-{}", std::process::id()));
        let private = base.join("private");
        std::fs::create_dir_all(&private).expect("make");
        std::fs::write(base.join("notes.txt"), "open").expect("write");
        std::fs::write(private.join("key.txt"), "private").expect("write");
        let read = |path: std::path::PathBuf| {
            system("cmd.exe").args(["/c".to_string(), format!("type {}", path.display())])
        };
        let text = |output: &SandboxOutput| String::from_utf8_lossy(&output.stdout).to_string();
        let listed = |path: &std::path::Path| {
            let output = sandbox
                .run(
                    &system("icacls.exe").args([path.display().to_string()]),
                    &SandboxSpec::unconfined(),
                )
                .expect("run");
            text(&output)
        };
        // Whether its entries are inherited is the host's doing: under a
        // temporary directory with nothing to hand down, as on CI's runner,
        // they are the creator's defaults and its own. Either way the list
        // must come back as it is here.
        let before = listed(&private);
        assert!(
            before.contains("Successfully processed 1 files"),
            "the path's list could not be read: {before}"
        );

        let granted = SandboxSpec {
            grants: crate::PathGrants {
                read_only: Vec::new(),
                read_write: vec![base.clone()],
                denied: Vec::new(),
            },
            wall_clock: Some(Duration::from_secs(30)),
            ..SandboxSpec::unconfined()
        };
        // Without the denial there is no container, and the file is read.
        let reached = sandbox
            .run(&read(private.join("key.txt")), &granted)
            .expect("run");
        assert_eq!(text(&reached), "private", "{reached:?}");
        // In a container with the grant above it, it is read too: so its
        // being closed below is the denial and not the container.
        let contained = SandboxSpec {
            confine_paths: true,
            ..granted.clone()
        };
        let reached = sandbox
            .run(&read(private.join("key.txt")), &contained)
            .expect("run");
        assert_eq!(text(&reached), "private", "{reached:?}");
        // What a grant above it, made and taken back, leaves on the path.
        let after_a_grant = listed(&private);

        // A denial by itself is what brings the container.
        let spec = SandboxSpec {
            grants: crate::PathGrants {
                denied: vec![private.clone()],
                ..granted.grants.clone()
            },
            ..granted
        };
        assert_eq!(
            spec.required(),
            vec![Control::WallClock, Control::PathDenial]
        );
        let open = sandbox
            .run(&read(base.join("notes.txt")), &spec)
            .expect("run");
        assert_eq!(text(&open), "open", "{open:?}");
        let closed = sandbox
            .run(&read(private.join("key.txt")), &spec)
            .expect("run");
        assert_ne!(closed.exit_code, Some(0), "{closed:?}");
        assert!(closed.stdout.is_empty(), "{closed:?}");
        let write = system("cmd.exe").args([
            "/c".to_string(),
            format!("echo x> {}", private.join("made.txt").display()),
        ]);
        let refused = sandbox.run(&write, &spec).expect("run");
        assert_ne!(refused.exit_code, Some(0), "{refused:?}");
        assert!(!private.join("made.txt").exists());

        // A path with a list of its own that lets every container in, which
        // the caller may not rewrite, is not skipped: the run is refused.
        // Where the caller may rewrite it, as an administrator, it is closed.
        let hosts = std::path::PathBuf::from(r"C:\Windows\System32\drivers\etc");
        let system = SandboxSpec {
            grants: crate::PathGrants {
                denied: vec![hosts.clone()],
                ..crate::PathGrants::default()
            },
            wall_clock: Some(Duration::from_secs(30)),
            ..SandboxSpec::unconfined()
        };
        match sandbox.run(&read(hosts.join("hosts")), &system) {
            Err(SandboxError::ConfinementFailed {
                control: Control::PathDenial,
                ..
            }) => {}
            Ok(output) => {
                assert_ne!(output.exit_code, Some(0), "{output:?}");
                assert!(output.stdout.is_empty(), "{output:?}");
            }
            other => panic!("{other:?}"),
        }

        // Afterwards the denial has left nothing a grant alone does not:
        // the same list, entry for entry.
        assert_eq!(listed(&private), after_a_grant, "before: {before}");
        // And that is who it was before anything ran, with the access each
        // had. Only the mark that says an entry is inherited is set aside:
        // on CI's runner the entries began as the path's own, and came back
        // from a run marked as its parent's.
        let unmarked = |list: &str| list.replace("(I)", "");
        assert_eq!(unmarked(&after_a_grant), unmarked(&before));
        let after = listed(&base);
        assert!(!after.is_empty() && !after.contains("S-1-15-"), "{after}");
        assert_eq!(
            std::fs::read_to_string(private.join("key.txt")).expect("read"),
            "private"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Confinement asked for by itself: the network stays, and the user's
    /// files go all the same, short of the ones granted.
    #[test]
    fn a_workload_confined_to_its_grants_cannot_read_the_users_other_files() {
        let sandbox = ProcessSandbox::new();
        if !sandbox.controls().enforces(Control::PathConfinement) {
            return;
        }
        let base = std::env::temp_dir().join(format!("hv2-sandbox-confine-{}", std::process::id()));
        let granted = base.join("granted");
        std::fs::create_dir_all(&granted).expect("make");
        std::fs::write(granted.join("note.txt"), "granted").expect("write");
        std::fs::write(base.join("secret.txt"), "user-only").expect("write");
        let read = |path: std::path::PathBuf| {
            system("cmd.exe").args(["/c".to_string(), format!("type {}", path.display())])
        };

        let spec = SandboxSpec {
            grants: crate::PathGrants {
                read_only: vec![granted.clone()],
                read_write: Vec::new(),
                denied: Vec::new(),
            },
            confine_paths: true,
            wall_clock: Some(Duration::from_secs(30)),
            ..SandboxSpec::unconfined()
        };
        assert_eq!(spec.network, NetworkPolicy::Host);
        assert_eq!(
            spec.required(),
            vec![Control::WallClock, Control::PathConfinement]
        );

        let inside = sandbox
            .run(&read(granted.join("note.txt")), &spec)
            .expect("run");
        assert_eq!(
            String::from_utf8_lossy(&inside.stdout),
            "granted",
            "{inside:?}"
        );
        let hidden = sandbox
            .run(&read(base.join("secret.txt")), &spec)
            .expect("run");
        assert_ne!(hidden.exit_code, Some(0), "{hidden:?}");
        assert!(hidden.stdout.is_empty(), "{hidden:?}");

        // The same spec without the flag is not contained at all, and reads it.
        let open = SandboxSpec {
            confine_paths: false,
            ..spec
        };
        let seen = sandbox
            .run(&read(base.join("secret.txt")), &open)
            .expect("run");
        assert_eq!(
            String::from_utf8_lossy(&seen.stdout),
            "user-only",
            "{seen:?}"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The workload cannot read a file the user can, and can write in the
    /// folder it is started in. More confinement than a spec with no
    /// filesystem policy asked for, and documented as coming with the
    /// container.
    #[test]
    fn a_contained_workload_sees_less_of_the_filesystem() {
        let sandbox = ProcessSandbox::new();
        if !sandbox.controls().enforces(Control::NetworkIsolation) {
            return;
        }
        let secret =
            std::env::temp_dir().join(format!("hv2-sandbox-secret-{}", std::process::id()));
        std::fs::write(&secret, "user-only").expect("write");
        let read = system("cmd.exe").args(["/c".to_string(), format!("type {}", secret.display())]);
        let outside = sandbox.run(&read, &SandboxSpec::unconfined()).expect("run");
        assert_eq!(String::from_utf8_lossy(&outside.stdout), "user-only");
        let inside = sandbox.run(&read, &no_network()).expect("run");
        assert_ne!(inside.exit_code, Some(0), "{inside:?}");
        assert!(inside.stdout.is_empty(), "{inside:?}");

        // What it is told is its local application data is its container's
        // folder, not the user's.
        let told = system("cmd.exe").args(["/c", "echo %LOCALAPPDATA%"]);
        let told = sandbox.run(&told, &no_network()).expect("run");
        let folder = String::from_utf8_lossy(&told.stdout).trim().to_string();
        assert!(
            folder.contains("Packages") && folder.contains("hv2.sandbox."),
            "{folder}"
        );

        let write = system("cmd.exe").args(["/c", "echo mine> note.txt& type note.txt"]);
        let wrote = sandbox.run(&write, &no_network()).expect("run");
        assert_eq!(
            String::from_utf8_lossy(&wrote.stdout).trim(),
            "mine",
            "{wrote:?}"
        );
        let _ = std::fs::remove_file(&secret);
    }
}
