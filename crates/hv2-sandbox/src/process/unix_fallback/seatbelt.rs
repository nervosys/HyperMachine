//! Containment on macOS, through the system's own sandbox.
//!
//! # What this is built on, and what that costs
//!
//! macOS has one sandbox a program can put another program in: the kernel's
//! sandbox extension, driven by a profile and applied by `sandbox-exec`.
//! Apple has marked the interface deprecated since 10.8 and has not said what
//! a third party should use instead. It is also what Apple's own tools, and
//! every other project that contains a process on macOS, are built on, and it
//! has kept working across releases. Using it was a decision taken with that
//! known: this crate claims what the profile was seen to enforce on the
//! systems its tests ran on, and probes before it claims anything.
//!
//! # What a profile does
//!
//! A profile is a list of rules, and the last rule that matches an operation
//! decides it. Each one written here starts from "allow everything", so a
//! workload asked for nothing extra is not disturbed, and then takes away:
//!
//! - **the network**, all of it, loopback and local sockets included;
//! - **the filesystem outside what was granted**, when the workload is to be
//!   confined to its grants;
//! - **each denied path**, last, so it wins over a grant above it.
//!
//! A sandboxed process cannot take its sandbox off, and everything it starts
//! inherits it.
//!
//! # Confined, on macOS
//!
//! "Only the granted paths" cannot be meant literally: a program that cannot
//! read the system's libraries does not start. So a confined workload reads
//! its grants and the places the system and installed software live
//! ([`SYSTEM_READABLE`]), and writes only to its read-write grants. That is
//! the same shape as an AppContainer on Windows, and the reason path
//! confinement is a control apart from filesystem isolation.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{NetworkPolicy, SandboxError, SandboxSpec};

/// The program that applies a profile and then runs another.
pub(super) const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// Where a confined workload may read besides its grants: the system, and
/// software installed on it. Without these nothing starts.
const SYSTEM_READABLE: &[&str] = &[
    "/System",
    "/usr",
    "/bin",
    "/sbin",
    "/Library",
    "/Applications",
    "/opt",
    "/dev",
    "/private/etc",
    "/private/var/db",
];

/// Single paths a confined workload may read: the root, and the links at it
/// that lead into `/private`.
const SYSTEM_READABLE_EXACT: &[&str] = &["/", "/etc", "/tmp", "/var", "/private"];

/// Devices a confined workload may still write to. They hold nothing.
const SYSTEM_WRITABLE_EXACT: &[&str] = &["/dev/null", "/dev/zero", "/dev/tty", "/dev/dtracehelper"];

/// Whether `spec` asks for anything a profile provides.
pub(super) fn wanted(spec: &SandboxSpec) -> bool {
    spec.network == NetworkPolicy::Denied || spec.confine_paths || !spec.grants.denied.is_empty()
}

/// A path as a profile names it: resolved, because the sandbox matches the
/// real path and `/tmp` is a link to `/private/tmp`; and quoted.
fn quoted(path: &Path) -> Result<String, SandboxError> {
    let invalid = |why: String| SandboxError::InvalidSpec(why);
    let real: PathBuf = std::fs::canonicalize(path).map_err(|e| {
        invalid(format!(
            "the path {} cannot be resolved: {e}",
            path.display()
        ))
    })?;
    let text = real.to_str().ok_or_else(|| {
        invalid(format!(
            "the path {} is not UTF-8, which a sandbox profile cannot name",
            real.display()
        ))
    })?;
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        if character == '"' || character == '\\' {
            out.push('\\');
        }
        out.push(character);
    }
    out.push('"');
    Ok(out)
}

/// The profile that gives `spec` its network isolation, path confinement and
/// denied paths.
pub(super) fn profile(spec: &SandboxSpec) -> Result<String, SandboxError> {
    let mut text = String::from("(version 1)\n(allow default)\n");
    if spec.network == NetworkPolicy::Denied {
        text.push_str("(deny network*)\n");
    }
    if spec.confine_paths {
        // Reading a file's contents and writing anything, everywhere; then
        // back what the system needs and what was granted. Looking a path up
        // is left alone, since nothing can be opened without it.
        text.push_str("(deny file-read-data (regex #\"^/\"))\n");
        text.push_str("(deny file-write* (regex #\"^/\"))\n");
        text.push_str("(allow file-read-data");
        for path in SYSTEM_READABLE_EXACT {
            let _ = write!(text, " (literal \"{path}\")");
        }
        for path in SYSTEM_READABLE {
            let _ = write!(text, " (subpath \"{path}\")");
        }
        text.push_str(")\n(allow file-write*");
        for path in SYSTEM_WRITABLE_EXACT {
            let _ = write!(text, " (literal \"{path}\")");
        }
        text.push_str(")\n");
        for path in spec.grants.read_only.iter().chain(&spec.grants.read_write) {
            let path = quoted(path)?;
            let _ = writeln!(
                text,
                "(allow file-read-data (literal {path}) (subpath {path}))"
            );
        }
        for path in &spec.grants.read_write {
            let path = quoted(path)?;
            let _ = writeln!(
                text,
                "(allow file-write* (literal {path}) (subpath {path}))"
            );
        }
    }
    // Last, so that a denial wins over anything above it.
    for path in &spec.grants.denied {
        let path = quoted(path)?;
        let _ = writeln!(
            text,
            "(deny file-read* file-write* (literal {path}) (subpath {path}))"
        );
    }
    Ok(text)
}

/// Whether this host applies a profile using every rule [`profile`] writes,
/// and still starts a program under it.
///
/// A process already in a sandbox may not apply another, and then
/// `sandbox-exec` fails here as it would for a workload.
pub(super) fn probe() -> Result<(), String> {
    let spec = SandboxSpec {
        network: NetworkPolicy::Denied,
        confine_paths: true,
        grants: crate::PathGrants {
            read_only: vec![PathBuf::from("/usr")],
            read_write: vec![std::env::temp_dir()],
            denied: vec![PathBuf::from("/usr/share")],
        },
        ..SandboxSpec::default()
    };
    let profile = profile(&spec).map_err(|e| e.to_string())?;
    let output = Command::new(SANDBOX_EXEC)
        .arg("-p")
        .arg(&profile)
        .arg("/usr/bin/true")
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("{SANDBOX_EXEC} could not be run: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "{SANDBOX_EXEC} could not apply a profile here ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

/// Where `program` is, searched for as the workload's own `PATH` would find
/// it.
///
/// `sandbox-exec` starts the program, so a program that is not there would
/// otherwise come back as `sandbox-exec`'s exit code and not as a failure to
/// start, which is what it is everywhere else.
pub(super) fn resolve(program: &str, path: Option<&str>) -> std::io::Result<PathBuf> {
    let missing = || std::io::Error::from(std::io::ErrorKind::NotFound);
    if program.contains('/') {
        let candidate = PathBuf::from(program);
        // A relative one is the workload's working directory's to resolve.
        if candidate.is_absolute() && !candidate.exists() {
            return Err(missing());
        }
        return Ok(candidate);
    }
    path.unwrap_or("/usr/bin:/bin")
        .split(':')
        .filter(|directory| !directory.is_empty())
        .map(|directory| Path::new(directory).join(program))
        .find(|candidate| candidate.is_file())
        .ok_or_else(missing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Control, PathGrants, ProcessSandbox, Sandbox, SandboxCommand};
    use std::time::Duration;

    /// A sandbox that enforces `control` here, or a failed test. Not a skip:
    /// a macOS host that cannot apply a profile is one these tests have
    /// nothing to say about, and passing there would say they had.
    fn sandbox_enforcing(control: Control) -> ProcessSandbox {
        let sandbox = ProcessSandbox::new();
        assert!(
            sandbox.controls().enforces(control),
            "{control} is not enforced here: {:?}",
            sandbox.controls().reason(control)
        );
        sandbox
    }

    /// What `script` printed, run by bash under `spec`.
    fn said(sandbox: &ProcessSandbox, script: &str, spec: &SandboxSpec) -> String {
        let command = SandboxCommand::new("/bin/bash")
            .args(["-c", script])
            .env("PATH", "/usr/bin:/bin");
        let output = sandbox.run(&command, spec).expect("run");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// A scratch directory under the temporary one, by its real path.
    fn scratch(label: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "hv2-sandbox-seatbelt-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("a scratch directory");
        std::fs::canonicalize(&base).expect("its real path")
    }

    fn host() -> SandboxSpec {
        SandboxSpec {
            network: NetworkPolicy::Host,
            wall_clock: Some(Duration::from_secs(30)),
            ..SandboxSpec::default()
        }
    }

    #[test]
    fn a_profile_says_what_the_spec_asked_and_quotes_what_it_names() {
        let here = scratch("profile");
        let odd = here.join("a \"quoted\" name");
        std::fs::create_dir_all(&odd).expect("a directory with quotes in its name");

        let nothing = profile(&host()).expect("a profile");
        assert_eq!(nothing, "(version 1)\n(allow default)\n");
        assert!(!wanted(&host()));

        let spec = SandboxSpec {
            network: NetworkPolicy::Denied,
            confine_paths: true,
            grants: PathGrants {
                read_only: vec![PathBuf::from("/usr")],
                read_write: vec![here.clone()],
                denied: vec![odd.clone()],
            },
            ..SandboxSpec::default()
        };
        assert!(wanted(&spec));
        let text = profile(&spec).expect("a profile");
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines.contains(&"(deny network*)"), "{text}");
        let escaped = format!(
            "{}/a \\\"quoted\\\" name",
            here.to_str().expect("a UTF-8 path")
        );
        // The denial is the last rule, so it wins over the grant above it.
        assert_eq!(
            *lines.last().expect("rules"),
            format!(
                "(deny file-read* file-write* (literal \"{escaped}\") (subpath \"{escaped}\"))"
            ),
            "{text}"
        );
        let write = format!(
            "(allow file-write* (literal \"{0}\") (subpath \"{0}\"))",
            here.display()
        );
        assert!(lines.contains(&write.as_str()), "{text}");
        assert!(
            !text.contains("(allow file-write* (literal \"/usr\")"),
            "a read-only grant was made writable: {text}"
        );
        let _ = std::fs::remove_dir_all(&here);
    }

    #[test]
    fn a_program_that_is_not_there_is_a_failure_to_start() {
        assert_eq!(
            resolve("sh", Some("/nowhere:/bin")).expect("found"),
            PathBuf::from("/bin/sh")
        );
        assert!(resolve("no-such-program-anywhere", Some("/usr/bin:/bin")).is_err());
        assert!(resolve("/no/such/program", None).is_err());

        let sandbox = sandbox_enforcing(Control::NetworkIsolation);
        let refused = sandbox.run(
            &SandboxCommand::new("/no/such/program"),
            &SandboxSpec {
                network: NetworkPolicy::Denied,
                ..host()
            },
        );
        assert!(
            matches!(&refused, Err(SandboxError::Spawn { program, .. }) if program == "/no/such/program"),
            "{refused:?}"
        );
    }

    #[test]
    fn a_workload_with_no_network_cannot_reach_even_loopback() {
        let sandbox = sandbox_enforcing(Control::NetworkIsolation);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("address").port();
        listener.set_nonblocking(true).expect("nonblocking");
        let script = format!(
            "if (echo > /dev/tcp/127.0.0.1/{port}) 2>/dev/null; then echo OPEN; else echo CLOSED; fi"
        );

        // With the host's network it connects: otherwise CLOSED below would
        // be a listener nobody could reach, and show nothing.
        assert_eq!(said(&sandbox, &script, &host()), "OPEN");
        assert!(listener.accept().is_ok(), "the listener saw no connection");

        let denied = SandboxSpec {
            network: NetworkPolicy::Denied,
            ..host()
        };
        assert_eq!(said(&sandbox, &script, &denied), "CLOSED");
        assert!(
            listener.accept().is_err(),
            "a workload with no network reached the listener"
        );
    }

    #[test]
    fn a_denied_path_is_closed_and_its_neighbour_is_not() {
        let sandbox = sandbox_enforcing(Control::PathDenial);
        let base = scratch("deny");
        std::fs::create_dir_all(base.join("private")).expect("a directory to deny");
        std::fs::write(base.join("private/key"), "private\n").expect("a file under it");
        std::fs::write(base.join("notes"), "open\n").expect("a file beside it");
        let script = format!(
            "cat '{0}/notes'; \
             if cat '{0}/private/key'; then echo READ; else echo CLOSED; fi; \
             if echo x > '{0}/private/made'; then echo WROTE; else echo CLOSED; fi",
            base.display()
        );

        // Without the denial it reads and writes there.
        assert_eq!(
            said(&sandbox, &script, &host()),
            "open\nprivate\nREAD\nWROTE"
        );
        std::fs::remove_file(base.join("private/made")).expect("the file it made");

        let spec = SandboxSpec {
            grants: PathGrants {
                denied: vec![base.join("private")],
                ..PathGrants::default()
            },
            ..host()
        };
        assert_eq!(
            spec.required(),
            vec![Control::WallClock, Control::PathDenial]
        );
        assert_eq!(said(&sandbox, &script, &spec), "open\nCLOSED\nCLOSED");
        assert!(!base.join("private/made").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_workload_confined_to_its_grants_reaches_them_and_not_the_users_other_files() {
        let sandbox = sandbox_enforcing(Control::PathConfinement);
        let base = scratch("confine");
        for directory in ["readable", "readable/private", "writable"] {
            std::fs::create_dir_all(base.join(directory)).expect("a directory");
        }
        std::fs::write(base.join("readable/note"), "granted\n").expect("a file to read");
        std::fs::write(base.join("readable/private/key"), "private\n").expect("a file to deny");
        std::fs::write(base.join("secret"), "user-only\n").expect("a file not granted");
        let script = format!(
            "cat '{0}/readable/note'; \
             if cat '{0}/secret'; then echo VISIBLE; else echo HIDDEN; fi; \
             if cat '{0}/readable/private/key'; then echo READ; else echo CLOSED; fi; \
             if echo x > '{0}/readable/made'; then echo WROTE; else echo REFUSED; fi; \
             if echo y > '{0}/writable/made'; then echo WROTE; else echo REFUSED; fi",
            base.display()
        );

        let spec = SandboxSpec {
            grants: PathGrants {
                read_only: vec![base.join("readable")],
                read_write: vec![base.join("writable")],
                // Under a grant, and closed all the same.
                denied: vec![base.join("readable/private")],
            },
            confine_paths: true,
            ..host()
        };
        assert_eq!(
            said(&sandbox, &script, &spec),
            "granted\nHIDDEN\nCLOSED\nREFUSED\nWROTE"
        );
        assert!(!base.join("readable/made").exists());
        assert_eq!(
            std::fs::read_to_string(base.join("writable/made")).expect("the file it made"),
            "y\n"
        );

        // The same grants without the flag take nothing away.
        let open = SandboxSpec {
            confine_paths: false,
            grants: PathGrants {
                denied: Vec::new(),
                ..spec.grants.clone()
            },
            ..spec
        };
        assert_eq!(
            said(&sandbox, &script, &open),
            "granted\nuser-only\nVISIBLE\nprivate\nREAD\nWROTE\nWROTE"
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
