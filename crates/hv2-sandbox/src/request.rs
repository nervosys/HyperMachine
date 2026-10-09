//! A sandboxed run, as one JSON document.
//!
//! [`SandboxCommand`] and [`SandboxSpec`] are Rust values. A caller in another
//! language, or on the other side of a pipe, needs the same thing as data: what
//! to run, and what it may touch. [`Request`] is that, versioned, so a
//! document written for one release means the same thing to the next.
//!
//! ```json
//! {
//!   "version": 1,
//!   "command": ["python3", "-c", "print(6 * 7)"],
//!   "env": { "PATH": "/usr/bin" },
//!   "limits": { "memoryBytes": 268435456, "timeoutMs": 10000 },
//!   "network": { "egress": "deny" },
//!   "filesystem": { "readOnly": ["/usr"], "readWrite": ["/work"] }
//! }
//! ```
//!
//! # What the defaults are, and why
//!
//! A request that says nothing about the network gets none, and one that
//! does not say `bestEffort` is refused where this host cannot enforce what
//! it asks. A document is the easiest place to leave something out, so what
//! is left out is the careful choice, as it is for a [`SandboxSpec`] built in
//! Rust.
//!
//! The environment is exactly `env`: nothing is inherited from the process
//! that runs the request.
//!
//! A field this version does not know is an error, not something skipped.
//! A caller who wrote `"netwrok"` meant something by it, and running without
//! it would be the quiet downgrade this crate exists to refuse.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

use crate::{
    FilesystemPolicy, NetworkPolicy, PathGrants, SandboxCommand, SandboxError, SandboxSpec,
};

/// The request format this build reads.
pub const VERSION: u32 = 1;

/// One sandboxed run: a command and what contains it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    /// The format's version. Must be [`VERSION`].
    pub version: u32,
    /// The program, then its arguments. Run directly, never through a shell.
    pub command: Vec<String>,
    /// The workload's whole environment.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Where it starts, or the backend's choice.
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
    /// Text written to its standard input.
    #[serde(default)]
    pub stdin: Option<String>,
    /// What it may consume.
    #[serde(default)]
    pub limits: Limits,
    /// What it may reach on the network.
    #[serde(default)]
    pub network: Network,
    /// What it may reach on the filesystem.
    #[serde(default)]
    pub filesystem: Filesystem,
    /// Hide the host's processes from it.
    #[serde(default)]
    pub isolate_processes: bool,
    /// Bar it from gaining privileges.
    #[serde(default)]
    pub no_new_privileges: bool,
    /// Run with whatever of this the host can enforce, and say what was
    /// dropped, where the default is to refuse.
    #[serde(default)]
    pub best_effort: bool,
}

/// Ceilings on what a workload consumes. Each is unlimited when absent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Limits {
    /// Memory, in bytes.
    #[serde(default)]
    pub memory_bytes: Option<u64>,
    /// Processes and threads.
    #[serde(default)]
    pub max_processes: Option<u32>,
    /// CPU time, in milliseconds.
    #[serde(default)]
    pub cpu_time_ms: Option<u64>,
    /// Wall-clock time before it is killed, in milliseconds.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// A workload's network.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Network {
    /// Outbound traffic: none unless this says otherwise.
    #[serde(default)]
    pub egress: Egress,
}

/// Whether a workload may make outbound connections.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Egress {
    /// No network at all.
    #[default]
    Deny,
    /// The host's network, unrestricted.
    Host,
}

/// A workload's filesystem.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Filesystem {
    /// A directory that becomes its root, hiding the host's. Absent, it sees
    /// what its containment lets it see of the host.
    #[serde(default)]
    pub root: Option<PathBuf>,
    /// Paths it may read. See [`PathGrants`].
    #[serde(default)]
    pub read_only: Vec<PathBuf>,
    /// Paths it may read and write.
    #[serde(default)]
    pub read_write: Vec<PathBuf>,
    /// Whether those two lists are all of the caller's filesystem it
    /// reaches. Without this they only open paths its containment would
    /// hide; with it, what is not listed is closed.
    #[serde(default)]
    pub confine: bool,
}

impl Request {
    /// Read a request.
    ///
    /// # Errors
    ///
    /// [`SandboxError::InvalidSpec`] for text that is not a request: not
    /// JSON, a field this version does not know, a missing command, or a
    /// version this build does not read.
    pub fn from_json(text: &str) -> Result<Self, SandboxError> {
        // The version first, by itself, so a document from a later release is
        // told it is from a later release and not that its new fields are
        // unknown.
        #[derive(Deserialize)]
        struct Versioned {
            version: Option<u32>,
        }
        let invalid = |e: serde_json::Error| SandboxError::InvalidSpec(format!("request: {e}"));
        match serde_json::from_str::<Versioned>(text)
            .map_err(invalid)?
            .version
        {
            Some(VERSION) => {}
            Some(other) => {
                return Err(SandboxError::InvalidSpec(format!(
                    "request version {other} is not one this build reads (it reads {VERSION})"
                )))
            }
            None => {
                return Err(SandboxError::InvalidSpec(
                    "request has no version".to_string(),
                ))
            }
        }
        let request: Self = serde_json::from_str(text).map_err(invalid)?;
        if request.command.first().is_none_or(|p| p.trim().is_empty()) {
            return Err(SandboxError::InvalidSpec(
                "request has no command to run".to_string(),
            ));
        }
        Ok(request)
    }

    /// The command and the spec this request asks for.
    #[must_use]
    pub fn into_parts(self) -> (SandboxCommand, SandboxSpec) {
        let mut words = self.command.into_iter();
        let mut command = SandboxCommand::new(words.next().unwrap_or_default()).args(words);
        command.env = self.env;
        command.working_dir = self.working_dir;
        command.stdin = self.stdin.map(String::into_bytes);

        // Inside a root of its own, a read-only path is one of its mounts;
        // otherwise both lists are grants.
        let (filesystem, read_only) = match self.filesystem.root {
            Some(root) => (
                FilesystemPolicy::Isolated {
                    root,
                    read_only: self.filesystem.read_only,
                },
                Vec::new(),
            ),
            None => (FilesystemPolicy::Host, self.filesystem.read_only),
        };
        let spec = SandboxSpec {
            memory_bytes: self.limits.memory_bytes,
            max_processes: self.limits.max_processes,
            cpu_time: self.limits.cpu_time_ms.map(Duration::from_millis),
            wall_clock: self.limits.timeout_ms.map(Duration::from_millis),
            network: match self.network.egress {
                Egress::Deny => NetworkPolicy::Denied,
                Egress::Host => NetworkPolicy::Host,
            },
            filesystem,
            grants: PathGrants {
                read_only,
                read_write: self.filesystem.read_write,
            },
            confine_paths: self.filesystem.confine,
            isolate_processes: self.isolate_processes,
            no_new_privileges: self.no_new_privileges,
            best_effort: self.best_effort,
        };
        (command, spec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(text: &str) -> String {
        match Request::from_json(text) {
            Err(SandboxError::InvalidSpec(why)) => why,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    /// The least a request can say, and what it then means: no network, a
    /// refusal where the host cannot enforce that, and an empty environment.
    #[test]
    fn the_least_a_request_can_say_is_the_careful_choice() {
        let request = Request::from_json(r#"{"version":1,"command":["true"]}"#).unwrap();
        let (command, spec) = request.into_parts();
        assert_eq!(command.program, "true");
        assert!(command.args.is_empty() && command.env.is_empty());
        assert!(command.stdin.is_none() && command.working_dir.is_none());
        assert_eq!(spec.network, NetworkPolicy::Denied);
        assert!(!spec.best_effort);
        assert_eq!(spec.filesystem, FilesystemPolicy::Host);
        assert!(spec.grants.is_empty());
        assert!(spec.memory_bytes.is_none() && spec.wall_clock.is_none());
    }

    #[test]
    fn every_field_arrives_where_it_belongs() {
        let request = Request::from_json(
            r#"{
              "version": 1,
              "command": ["python3", "-c", "print(1)"],
              "env": {"PATH": "/usr/bin", "LANG": "C"},
              "workingDir": "/work",
              "stdin": "input",
              "limits": {"memoryBytes": 1024, "maxProcesses": 8, "cpuTimeMs": 1500, "timeoutMs": 2500},
              "network": {"egress": "host"},
              "filesystem": {"readOnly": ["/usr"], "readWrite": ["/work"]},
              "isolateProcesses": true,
              "noNewPrivileges": true,
              "bestEffort": true
            }"#,
        )
        .unwrap();
        let (command, spec) = request.into_parts();
        assert_eq!(command.program, "python3");
        assert_eq!(command.args, ["-c", "print(1)"]);
        assert_eq!(command.env.len(), 2);
        assert_eq!(command.working_dir, Some(PathBuf::from("/work")));
        assert_eq!(command.stdin.as_deref(), Some(&b"input"[..]));
        assert_eq!(spec.memory_bytes, Some(1024));
        assert_eq!(spec.max_processes, Some(8));
        assert_eq!(spec.cpu_time, Some(Duration::from_millis(1500)));
        assert_eq!(spec.wall_clock, Some(Duration::from_millis(2500)));
        assert_eq!(spec.network, NetworkPolicy::Host);
        assert_eq!(spec.filesystem, FilesystemPolicy::Host);
        assert_eq!(spec.grants.read_only, [PathBuf::from("/usr")]);
        assert_eq!(spec.grants.read_write, [PathBuf::from("/work")]);
        assert!(spec.isolate_processes && spec.no_new_privileges && spec.best_effort);
    }

    /// With a root, read-only paths are its mounts and not grants.
    #[test]
    fn a_root_makes_read_only_paths_its_mounts() {
        let request = Request::from_json(
            r#"{"version":1,"command":["sh"],
                "filesystem":{"root":"/srv/root","readOnly":["/usr"]}}"#,
        )
        .unwrap();
        let (_, spec) = request.into_parts();
        assert_eq!(
            spec.filesystem,
            FilesystemPolicy::Isolated {
                root: PathBuf::from("/srv/root"),
                read_only: vec![PathBuf::from("/usr")],
            }
        );
        assert!(spec.grants.is_empty());
    }

    /// The published schema and this reader name the same fields. A document
    /// using every one of them, at every level, is read; so a field added to
    /// one and not the other fails here and not in a caller's hands.
    #[test]
    fn the_published_schema_names_the_fields_this_reads() {
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../docs/schemas/sandbox-request-v1.schema.json"
        ))
        .unwrap();
        let names = |object: &serde_json::Value| -> Vec<String> {
            let mut names: Vec<String> = object["properties"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect();
            names.sort();
            names
        };
        assert_eq!(
            names(&schema),
            [
                "bestEffort",
                "command",
                "env",
                "filesystem",
                "isolateProcesses",
                "limits",
                "network",
                "noNewPrivileges",
                "stdin",
                "version",
                "workingDir"
            ]
        );
        let properties = &schema["properties"];
        assert_eq!(
            names(&properties["limits"]),
            ["cpuTimeMs", "maxProcesses", "memoryBytes", "timeoutMs"]
        );
        assert_eq!(names(&properties["network"]), ["egress"]);
        assert_eq!(
            names(&properties["filesystem"]),
            ["confine", "readOnly", "readWrite", "root"]
        );
        assert_eq!(properties["version"]["const"], VERSION);
        // Every one of those names, in one document this reads.
        Request::from_json(
            r#"{"version":1,"command":["x"],"env":{},"workingDir":"/w","stdin":"",
                "limits":{"memoryBytes":1,"maxProcesses":1,"cpuTimeMs":1,"timeoutMs":1},
                "network":{"egress":"deny"},
                "filesystem":{"root":"/r","readOnly":[],"readWrite":[],"confine":false},
                "isolateProcesses":false,"noNewPrivileges":false,"bestEffort":false}"#,
        )
        .unwrap();
    }

    /// What is not a request is refused, each with what was wrong.
    #[test]
    fn what_is_not_a_request_is_refused_with_the_reason() {
        assert!(refusal("not json").contains("request:"));
        assert!(refusal(r#"{"command":["true"]}"#).contains("no version"));
        let later = refusal(r#"{"version":2,"command":["true"],"somethingNew":1}"#);
        assert!(
            later.contains("version 2") && later.contains("reads 1"),
            "{later}"
        );
        assert!(refusal(r#"{"version":1}"#).contains("command"));
        assert!(refusal(r#"{"version":1,"command":[]}"#).contains("no command"));
        assert!(refusal(r#"{"version":1,"command":["  "]}"#).contains("no command"));
        // A misspelt field is not skipped, at any depth.
        assert!(refusal(r#"{"version":1,"command":["true"],"netwrok":{}}"#).contains("netwrok"));
        assert!(
            refusal(r#"{"version":1,"command":["true"],"limits":{"memoryMb":1}}"#)
                .contains("memoryMb")
        );
        assert!(
            refusal(r#"{"version":1,"command":["true"],"network":{"egress":"allow"}}"#)
                .contains("allow")
        );
    }
}
