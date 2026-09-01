//! Windows confinement, built on job objects.
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
//! process table, and does not stop a process gaining privileges. Those four
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

use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_BASIC_LIMIT_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_JOB_MEMORY, JOB_OBJECT_LIMIT_JOB_TIME, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    OpenThread, ResumeThread, CREATE_SUSPENDED, THREAD_SUSPEND_RESUME,
};

use crate::{
    Control, Controls, FilesystemPolicy, NetworkPolicy, SandboxCommand, SandboxError,
    SandboxOutput, SandboxSpec,
};

use super::driver;

/// What Windows job objects enforce.
///
/// Probed the same way as everywhere else — a job object can be unavailable,
/// most often because this process is already inside one that forbids nested
/// jobs on older Windows.
pub(super) fn probe() -> Controls {
    let mut controls = Controls::none()
        .without(
            Control::NetworkIsolation,
            "a job object does not isolate the network; use the microVM sandbox",
        )
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
            ] {
                controls = controls.clone().without(control, reason.clone());
            }
        }
    }

    controls
}

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
) -> Result<SandboxOutput, SandboxError> {
    if let FilesystemPolicy::Isolated { .. } = spec.filesystem {
        return Err(SandboxError::InvalidSpec(
            "this backend cannot isolate the filesystem on Windows".to_string(),
        ));
    }
    debug_assert!(
        spec.network == NetworkPolicy::Host || spec.best_effort,
        "reconcile should have refused a network-isolated spec before reaching this backend"
    );

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

    driver::wait_with_deadline(child, command.stdin.as_deref(), spec.wall_clock, || {
        // Terminate the job, not the process: a workload that spawned children
        // would otherwise leave them running past its own deadline.
        job.terminate();
    })
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
