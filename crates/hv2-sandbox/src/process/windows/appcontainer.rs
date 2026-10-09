//! Running a workload in a Windows AppContainer.
//!
//! # What an AppContainer gives us
//!
//! An AppContainer is the kernel's own sandbox for a process: its token is a
//! *low-box* token carrying a container SID and a list of capabilities, and
//! the kernel checks both on every access. One created with no capabilities
//! has no network at all -- no outbound connection, no listening socket, not
//! even loopback -- because the capabilities that would allow each
//! (`internetClient`, `internetClientServer`, `privateNetworkClientServer`)
//! are simply absent. That is the control this module exists for.
//!
//! # What comes with it, asked for or not
//!
//! The same token is checked against every file, registry key and object the
//! workload opens. It can read what Windows lets every packaged application
//! read -- the system directories, mostly -- and its own container folder. It
//! cannot read the user's files, or a program installed outside those places.
//! So a workload given no network on Windows also sees less of the filesystem
//! than the host process does, whatever filesystem policy was asked for. That
//! is more confinement than was asked, never less, and the working directory
//! defaults to the container's own folder so the workload has somewhere it
//! can be.
//!
//! # Why this does not go through `std::process`
//!
//! A process is put in an AppContainer at creation, by an attribute in its
//! startup information. The standard library has no stable way to pass one,
//! so this calls `CreateProcessW` itself: its own pipes, its own command line
//! quoting, its own environment block.

use std::fs::File;
use std::io::{Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::path::PathBuf;
use std::rc::Rc;

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Foundation::{
    CloseHandle, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::{
    GetNamedSecurityInfoW, SetEntriesInAclW, SetNamedSecurityInfoW, ACCESS_MODE, EXPLICIT_ACCESS_W,
    GRANT_ACCESS, NO_MULTIPLE_TRUSTEE, REVOKE_ACCESS, SE_FILE_OBJECT, TRUSTEE_IS_SID,
    TRUSTEE_IS_UNKNOWN, TRUSTEE_W,
};
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeleteAppContainerProfile,
};
use windows_sys::Win32::Security::{FreeSid, PSID, SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES};
use windows_sys::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SUB_CONTAINERS_AND_OBJECTS_INHERIT,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, ResumeThread, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, INFINITE, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
    STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

use super::driver::{Exit, Spawned};
use crate::SandboxCommand;

/// A UTF-16 string with its terminator.
fn wide(text: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    text.as_ref().encode_wide().chain(Some(0)).collect()
}

/// An AppContainer profile made for one run, and removed after it.
pub(super) struct Container {
    name: Vec<u16>,
    sid: PSID,
    /// The folder Windows made for it, which the workload can read and write.
    pub(super) folder: Option<PathBuf>,
}

impl Container {
    /// Create a profile no other run shares, with no capabilities.
    pub(super) fn create() -> std::io::Result<Self> {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        // Letters, digits, dots and hyphens, at most 64 characters.
        let name = format!(
            "hv2.sandbox.{}.{:x}.{}",
            std::process::id(),
            nonce & 0xffff_ffff_ffff,
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let wide_name = wide(&name);
        let display = wide("HyperMachine sandbox");
        let mut sid: PSID = std::ptr::null_mut();
        // SAFETY: the three strings are terminated and outlive the call; no
        // capabilities are passed, so the capability pointer may be null; the
        // SID is returned through a pointer to a local.
        let result = unsafe {
            CreateAppContainerProfile(
                wide_name.as_ptr(),
                display.as_ptr(),
                display.as_ptr(),
                std::ptr::null(),
                0,
                &mut sid,
            )
        };
        if result < 0 || sid.is_null() {
            return Err(std::io::Error::from_raw_os_error(result & 0xffff));
        }
        let folder = std::env::var_os("LOCALAPPDATA")
            .map(|base| PathBuf::from(base).join("Packages").join(&name).join("AC"))
            .filter(|path| path.is_dir());
        Ok(Self {
            name: wide_name,
            sid,
            folder,
        })
    }
}

impl Drop for Container {
    fn drop(&mut self) {
        // SAFETY: the SID was allocated by CreateAppContainerProfile and is
        // freed once; the name is terminated. A profile that cannot be
        // deleted is left behind, which costs an empty folder.
        unsafe {
            FreeSid(self.sid);
            DeleteAppContainerProfile(self.name.as_ptr());
        }
    }
}

/// Read, and run: `FILE_GENERIC_READ | FILE_GENERIC_EXECUTE`.
const READ_ONLY: u32 = 0x0012_0089 | 0x0012_00A0;
/// And write, and delete what is under it:
/// `FILE_GENERIC_WRITE | DELETE | FILE_DELETE_CHILD`.
const READ_WRITE: u32 = READ_ONLY | 0x0012_0116 | 0x0001_0000 | 0x0000_0040;

/// Paths opened to one container, and closed again when this is dropped.
///
/// An AppContainer reaches a file only if the file's access-control list
/// names the container, or every packaged application. So a grant is an
/// entry for this container's SID on the path, inherited by everything under
/// it. The SID is this run's alone, so the entry opens the path to nothing
/// else, and it is taken off again afterwards. A process killed before it
/// can do that leaves entries for a SID that no longer names anything, which
/// grant nothing and cost a line in the list.
///
/// Adding an inherited entry to a large tree rewrites every descriptor under
/// it, which is slow; grant the directory the workload needs, not its parent.
///
/// It borrows the container, so it cannot outlive the SID its entries name:
/// taking them off again after the SID was freed removed nothing, which the
/// first test of this found.
pub(super) struct Grants<'a> {
    container: &'a Container,
    paths: Vec<Vec<u16>>,
}

impl<'a> Grants<'a> {
    /// Open `grants` to `container`. On an error nothing stays opened.
    pub(super) fn give(
        container: &'a Container,
        grants: &crate::PathGrants,
    ) -> std::io::Result<Self> {
        let mut given = Self {
            container,
            paths: Vec::new(),
        };
        let wanted = grants
            .read_only
            .iter()
            .map(|path| (path, READ_ONLY))
            .chain(grants.read_write.iter().map(|path| (path, READ_WRITE)));
        for (path, access) in wanted {
            let path = wide(path);
            entry(&path, container.sid, access, GRANT_ACCESS)?;
            given.paths.push(path);
        }
        Ok(given)
    }
}

impl Drop for Grants<'_> {
    fn drop(&mut self) {
        for path in &self.paths {
            if let Err(e) = entry(path, self.container.sid, 0, REVOKE_ACCESS) {
                tracing::warn!(
                    "a sandbox's access to {} could not be taken back: {e}",
                    String::from_utf16_lossy(&path[..path.len() - 1])
                );
            }
        }
    }
}

/// Add, or with `REVOKE_ACCESS` remove, `sid`'s entry on `path`.
fn entry(path: &[u16], sid: PSID, access: u32, mode: ACCESS_MODE) -> std::io::Result<()> {
    let failed = |code: u32| std::io::Error::from_raw_os_error(code as i32);
    let mut current: *mut ACL = std::ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: the path is terminated; only the DACL is asked for, through
    // out-pointers to locals; the descriptor is freed below.
    let read = unsafe {
        GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut current,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    if read != 0 {
        return Err(failed(read));
    }
    let change = EXPLICIT_ACCESS_W {
        grfAccessPermissions: access,
        grfAccessMode: mode,
        grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_UNKNOWN,
            ptstrName: sid as *mut u16,
        },
    };
    let mut updated: *mut ACL = std::ptr::null_mut();
    // SAFETY: one entry is passed, with the list just read; the new list is
    // returned through a pointer to a local and freed below.
    let merged = unsafe { SetEntriesInAclW(1, &change, current, &mut updated) };
    let result = if merged != 0 {
        Err(failed(merged))
    } else {
        // SAFETY: the path is terminated and the new list is valid until freed.
        let written = unsafe {
            SetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                updated,
                std::ptr::null_mut(),
            )
        };
        if written == 0 {
            Ok(())
        } else {
            Err(failed(written))
        }
    };
    // SAFETY: both were allocated by the calls above and are freed once.
    unsafe {
        if !updated.is_null() {
            LocalFree(updated.cast());
        }
        LocalFree(descriptor);
    }
    result
}

/// An owned handle, closed when dropped.
struct Owned(HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the handle is open and closed exactly once.
        unsafe { CloseHandle(self.0) };
    }
}

/// A pipe: the end the workload inherits and the end this process keeps.
fn pipe(workload_reads: bool) -> std::io::Result<(Owned, Owned)> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 1,
    };
    let (mut read, mut write): (HANDLE, HANDLE) = (std::ptr::null_mut(), std::ptr::null_mut());
    // SAFETY: both out-pointers are to locals and the attributes outlive the call.
    if unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let (theirs, ours) = if workload_reads {
        (Owned(read), Owned(write))
    } else {
        (Owned(write), Owned(read))
    };
    // Only the workload's end is inherited; ours stays in this process.
    // SAFETY: the handle is open.
    if unsafe { SetHandleInformation(ours.0, HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok((theirs, ours))
}

/// One argument, quoted as the Microsoft C runtime parses a command line:
/// backslashes are literal except before a quote, where they are doubled.
fn quote(argument: &str, out: &mut String) {
    if !argument.is_empty() && !argument.contains([' ', '\t', '"']) {
        out.push_str(argument);
        return;
    }
    out.push('"');
    let mut backslashes = 0;
    for c in argument.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                backslashes = 0;
                out.push('"');
                continue;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                backslashes = 0;
            }
        }
        if c != '\\' {
            out.push(c);
        }
    }
    // Before the closing quote, so they do not escape it.
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
}

/// The command line `command` is started with.
pub(super) fn command_line(command: &SandboxCommand) -> String {
    let mut line = String::new();
    quote(&command.program, &mut line);
    for argument in &command.args {
        line.push(' ');
        quote(argument, &mut line);
    }
    line
}

/// The workload's whole environment, as the block `CreateProcessW` takes.
///
/// With one addition the caller did not make. Starting a process in an
/// AppContainer fails with `ERROR_ENVVAR_NOT_FOUND` unless its environment
/// names `LOCALAPPDATA`: Windows rewrites that variable to the container's
/// own folder, and refuses when there is nothing to rewrite. So it is given
/// the host's value when the caller gave none, and what the workload then
/// sees is its container's folder, not the user's.
fn environment_block(command: &SandboxCommand, local_app_data: Option<&str>) -> Vec<u16> {
    let mut block = Vec::new();
    let mut variable = |name: &str, value: &str| {
        block.extend(format!("{name}={value}").encode_utf16());
        block.push(0);
    };
    for (name, value) in &command.env {
        variable(name, value);
    }
    let named = command
        .env
        .keys()
        .any(|name| name.eq_ignore_ascii_case("LOCALAPPDATA"));
    if let (false, Some(value)) = (named, local_app_data) {
        variable("LOCALAPPDATA", value);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

/// The process handle of a started workload, and its primary thread.
pub(super) struct Started {
    pub(super) spawned: Spawned,
    /// For putting the workload in a job before it runs. Owned by `spawned`.
    pub(super) process: HANDLE,
    thread: Owned,
}

impl Started {
    /// Let the workload run. It was created suspended.
    pub(super) fn resume(&self) -> std::io::Result<()> {
        // SAFETY: the thread handle is open and belongs to the workload.
        if unsafe { ResumeThread(self.thread.0) } == u32::MAX {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

/// Start `command` suspended, inside `container`.
pub(super) fn spawn(command: &SandboxCommand, container: &Container) -> std::io::Result<Started> {
    let (stdin_theirs, stdin_ours) = pipe(true)?;
    let (stdout_theirs, stdout_ours) = pipe(false)?;
    let (stderr_theirs, stderr_ours) = pipe(false)?;

    let capabilities = SECURITY_CAPABILITIES {
        AppContainerSid: container.sid,
        Capabilities: std::ptr::null_mut(),
        CapabilityCount: 0,
        Reserved: 0,
    };
    // Exactly these three handles are inherited, whatever else in this
    // process happens to be inheritable.
    let inherited = [stdin_theirs.0, stdout_theirs.0, stderr_theirs.0];

    let mut size = 0usize;
    // SAFETY: a null list with a size out-pointer asks how large it must be.
    unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 2, 0, &mut size) };
    let mut storage = vec![0u8; size];
    let list = storage.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
    // SAFETY: `storage` is the size the call asked for and outlives the list.
    if unsafe { InitializeProcThreadAttributeList(list, 2, 0, &mut size) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    struct List(LPPROC_THREAD_ATTRIBUTE_LIST);
    impl Drop for List {
        fn drop(&mut self) {
            // SAFETY: the list was initialised and is deleted once.
            unsafe { DeleteProcThreadAttributeList(self.0) };
        }
    }
    let list = List(list);
    // SAFETY: each value outlives the list, which is deleted before this
    // function returns, and each size is that of the value passed.
    let updated = unsafe {
        UpdateProcThreadAttribute(
            list.0,
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            &capabilities as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<SECURITY_CAPABILITIES>(),
            std::ptr::null_mut(),
            std::ptr::null(),
        ) != 0
            && UpdateProcThreadAttribute(
                list.0,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                inherited.as_ptr() as *const core::ffi::c_void,
                std::mem::size_of_val(&inherited),
                std::ptr::null_mut(),
                std::ptr::null(),
            ) != 0
    };
    if !updated {
        return Err(std::io::Error::last_os_error());
    }

    // SAFETY: plain data, filled in below.
    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = stdin_theirs.0;
    startup.StartupInfo.hStdOutput = stdout_theirs.0;
    startup.StartupInfo.hStdError = stderr_theirs.0;
    startup.lpAttributeList = list.0;

    let mut line = wide(command_line(command));
    let local_app_data = std::env::var("LOCALAPPDATA").ok();
    let environment = environment_block(command, local_app_data.as_deref());
    // Somewhere the workload can be: the caller's choice, or its own folder.
    // The host process's directory is very likely one it may not open.
    let directory = command
        .working_dir
        .clone()
        .or_else(|| container.folder.clone())
        .map(wide);
    // SAFETY: plain data, written by CreateProcessW.
    let mut information: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: every pointer is to a buffer that outlives the call; the
    // command line is mutable, as the API requires; the environment block is
    // UTF-16 and says so in the flags.
    let created = unsafe {
        CreateProcessW(
            std::ptr::null(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED,
            environment.as_ptr() as *const core::ffi::c_void,
            directory.as_ref().map_or(std::ptr::null(), |d| d.as_ptr()),
            &startup.StartupInfo,
            &mut information,
        )
    };
    if created == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let process = Rc::new(Owned(information.hProcess));
    let thread = Owned(information.hThread);
    // The workload has its ends now; ours are the only other copies.
    drop((stdin_theirs, stdout_theirs, stderr_theirs));

    // SAFETY: each handle is open, owned by nobody else, and handed to a
    // `File` that closes it. `forget` keeps `Owned` from closing it too.
    let file = |handle: Owned| {
        let file = unsafe { File::from_raw_handle(handle.0 as _) };
        std::mem::forget(handle);
        file
    };
    let exit = |process: &Owned| -> std::io::Result<Exit> {
        let mut code = 0u32;
        // SAFETY: the handle is open and the out-pointer is to a local.
        if unsafe { GetExitCodeProcess(process.0, &mut code) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Exit {
            code: Some(code as i32),
            signal: None,
        })
    };
    let polled = Rc::clone(&process);
    let waited = Rc::clone(&process);
    let spawned = Spawned {
        stdin: Some(Box::new(file(stdin_ours)) as Box<dyn Write + Send>),
        stdout: Some(Box::new(file(stdout_ours)) as Box<dyn Read + Send>),
        stderr: Some(Box::new(file(stderr_ours)) as Box<dyn Read + Send>),
        try_wait: Box::new(move || {
            // SAFETY: the handle is open for as long as this closure lives.
            match unsafe { WaitForSingleObject(polled.0, 0) } {
                WAIT_OBJECT_0 => exit(&polled).map(Some),
                WAIT_TIMEOUT => Ok(None),
                _ => Err(std::io::Error::last_os_error()),
            }
        }),
        wait: Box::new(move || {
            // SAFETY: as above.
            match unsafe { WaitForSingleObject(waited.0, INFINITE) } {
                WAIT_OBJECT_0 => exit(&waited),
                _ => Err(std::io::Error::last_os_error()),
            }
        }),
    };
    Ok(Started {
        spawned,
        process: process.0,
        thread,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Quoting follows the C runtime's rules, which are not a shell's: a
    /// backslash is itself unless a quote follows, and an empty argument is
    /// still an argument.
    #[test]
    fn arguments_are_quoted_as_the_c_runtime_reads_them() {
        let line =
            |args: &[&str]| command_line(&SandboxCommand::new("prog").args(args.iter().copied()));
        assert_eq!(line(&["plain", "two words"]), r#"prog plain "two words""#);
        assert_eq!(line(&[""]), r#"prog """#);
        assert_eq!(line(&[r"C:\dir\file"]), r"prog C:\dir\file");
        assert_eq!(line(&[r"C:\a dir\"]), r#"prog "C:\a dir\\""#);
        assert_eq!(line(&[r#"say "hi""#]), r#"prog "say \"hi\"""#);
        assert_eq!(line(&[r#"back\"slash"#]), r#"prog "back\\\"slash""#);
    }

    #[test]
    fn the_environment_block_is_the_whole_environment_and_terminated() {
        let block = |command: &SandboxCommand, local: Option<&str>| {
            String::from_utf16_lossy(&environment_block(command, local))
        };
        assert_eq!(block(&SandboxCommand::new("p"), None), "\0\0");
        assert_eq!(
            block(&SandboxCommand::new("p").env("B", "2").env("A", "1"), None),
            "A=1\0B=2\0\0"
        );
        // The one variable an AppContainer start needs is added when the
        // caller named none, and left alone when the caller did.
        assert_eq!(
            block(&SandboxCommand::new("p").env("A", "1"), Some(r"C:\host")),
            "A=1\0LOCALAPPDATA=C:\\host\0\0"
        );
        assert_eq!(
            block(
                &SandboxCommand::new("p").env("LocalAppData", r"C:\mine"),
                Some(r"C:\host")
            ),
            "LocalAppData=C:\\mine\0\0"
        );
    }
}
