//! Turn an OCI container specification into confinement the kernel enforces.
//!
//! The types in [`runtime`](super::runtime) describe a container thoroughly and
//! run nothing: [`ContainerRuntime::start`](super::runtime::ContainerRuntime::start)
//! returns `NotImplemented`, and there is not one libc call in the whole
//! module. `hv2-sandbox` is the opposite -- a smaller vocabulary, and every
//! word of it backed by a syscall.
//!
//! This is the join. A [`ContainerSpec`] becomes a
//! [`SandboxSpec`] and a [`SandboxCommand`], which `hv2-sandbox` then enforces
//! with namespaces, `pivot_root`, cgroup v2 and rlimits. It is the first path
//! by which an OCI specification in this codebase does anything at all.
//!
//! # Refusing rather than dropping
//!
//! The two vocabularies are not the same size, and that is the whole risk here.
//! A translation that quietly ignored what it could not express would hand back
//! a spec that looks like the caller's and confines less -- seccomp filters
//! gone, a uid switch gone, a read-only path writable -- with nothing to say
//! so. That is worse than refusing, because the caller has no way to find out.
//!
//! So every field is either translated or named in an error. [`Unsupported`]
//! lists what a spec asked for that this host cannot enforce, and the caller
//! decides whether to drop it and retry.
//!
//! # What translates
//!
//! | OCI | sandbox |
//! | --- | --- |
//! | `root.path` with a mount namespace | [`FilesystemPolicy::Isolated`] |
//! | read-only bind mounts | `read_only` roots inside it |
//! | network namespace | [`NetworkPolicy::Denied`] |
//! | PID and IPC namespaces | `isolate_processes` |
//! | `resources.memory.limit` | `memory_bytes` |
//! | `resources.pids.limit` | `max_processes` |
//! | `process.command`, `cwd`, `env` | [`SandboxCommand`] |
//!
//! # What does not, and why
//!
//! Seccomp, uid and gid mappings, user namespaces, masked and read-only paths,
//! CPU shares and quotas, block-I/O weights, cgroup and time namespaces, a
//! terminal, and non-root `process.uid`. `hv2-sandbox` has no mechanism for
//! any of them, and inventing an approximation is how a control comes to be
//! believed in without existing.
//!
//! CPU is the one worth spelling out. OCI's `cpu.quota` and `cpu.period` bound
//! a *share of wall-clock time per period*; the sandbox's `cpu_time` is
//! `RLIMIT_CPU`, a total the process may consume before `SIGKILL`. Mapping one
//! to the other would be arithmetic without meaning, so a spec that sets CPU
//! limits is refused rather than approximated.

use std::collections::BTreeMap;
use std::path::PathBuf;

use hv2_sandbox::{FilesystemPolicy, NetworkPolicy, SandboxCommand, SandboxSpec};

use super::runtime::{ContainerSpec, MountOption, MountType, NamespaceType};

/// Something a specification asked for that `hv2-sandbox` cannot enforce.
///
/// Carries the OCI field name rather than a prose message so a caller can act
/// on it -- strip that field and retry, or report it -- without parsing text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported {
    /// The field, in OCI terms: `linux.seccomp`, `process.uid`.
    pub field: &'static str,
    /// Why there is no equivalent.
    pub reason: &'static str,
}

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.reason)
    }
}

/// Everything in a specification that this host cannot enforce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationError {
    /// Every unsupported field, not just the first.
    ///
    /// All of them, because a caller fixing one at a time learns of the next
    /// only by running again, and a spec with four unsupported fields would
    /// take four round trips to find out it cannot run at all.
    pub unsupported: Vec<Unsupported>,
}

impl std::fmt::Display for TranslationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "this container specification asks for {} control(s) hv2-sandbox cannot enforce: ",
            self.unsupported.len()
        )?;
        for (i, item) in self.unsupported.iter().enumerate() {
            if i > 0 {
                write!(f, "; ")?;
            }
            write!(f, "{item}")?;
        }
        Ok(())
    }
}

impl std::error::Error for TranslationError {}

/// Translate `spec` into confinement `hv2-sandbox` enforces.
///
/// Returns the spec and the command together: an OCI specification describes
/// both, and separating them would let a caller run one workload under
/// another's limits.
///
/// # Errors
///
/// [`TranslationError`] listing every field that has no equivalent. Nothing is
/// silently dropped.
pub fn to_sandbox(spec: &ContainerSpec) -> Result<(SandboxSpec, SandboxCommand), TranslationError> {
    let mut unsupported = Vec::new();
    let mut sandbox = SandboxSpec::default();

    let namespaces: Vec<NamespaceType> = spec
        .linux
        .as_ref()
        .map(|linux| linux.namespaces.iter().map(|ns| ns.ns_type).collect())
        .unwrap_or_default();

    // Joining an existing namespace by path is a different operation from
    // creating one, and the sandbox only creates.
    if let Some(linux) = &spec.linux {
        if linux.namespaces.iter().any(|ns| ns.path.is_some()) {
            unsupported.push(Unsupported {
                field: "linux.namespaces[].path",
                reason: "joining an existing namespace is not supported; the sandbox creates \
                         its own",
            });
        }
    }

    let has = |t: NamespaceType| namespaces.contains(&t);

    // Network. An empty network namespace is exactly NetworkPolicy::Denied.
    sandbox.network = if has(NamespaceType::Network) {
        NetworkPolicy::Denied
    } else {
        NetworkPolicy::Host
    };

    // Processes. The sandbox couples PID and IPC into one control, because it
    // creates both together; asking for one without the other cannot be
    // honoured exactly.
    match (has(NamespaceType::Pid), has(NamespaceType::Ipc)) {
        (true, true) | (false, false) => {
            sandbox.isolate_processes = has(NamespaceType::Pid);
        }
        _ => unsupported.push(Unsupported {
            field: "linux.namespaces",
            reason: "the PID and IPC namespaces are created together; asking for one without \
                     the other cannot be honoured exactly",
        }),
    }

    // Filesystem. A root without a mount namespace is a root nothing applies,
    // which is how a workload comes to run against the host filesystem while
    // its spec says otherwise.
    if has(NamespaceType::Mount) {
        let read_only = read_only_binds(spec, &mut unsupported);
        sandbox.filesystem = FilesystemPolicy::Isolated {
            root: spec.root.path.clone(),
            read_only,
        };
    } else {
        if spec.root.path != std::path::Path::new("/") {
            unsupported.push(Unsupported {
                field: "root.path",
                reason: "a root filesystem without a mount namespace would be ignored; add a \
                         mount namespace or leave the root at /",
            });
        }
        if !spec.mounts.is_empty() {
            unsupported.push(Unsupported {
                field: "mounts",
                reason: "mounts need a mount namespace to happen in",
            });
        }
    }

    // The sandbox has no way to make the root itself read-only; `read_only`
    // names subtrees mounted into it.
    if spec.root.readonly {
        unsupported.push(Unsupported {
            field: "root.readonly",
            reason: "the sandbox mounts a root the caller provides and cannot remount it \
                     read-only; provide a root with nothing writable in it",
        });
    }

    for (present, field, reason) in [
        (
            has(NamespaceType::User),
            "linux.namespaces[user]",
            "user namespaces are not created; uid mapping has no mechanism here",
        ),
        (
            has(NamespaceType::Uts),
            "linux.namespaces[uts]",
            "there is no control for the hostname a workload sees",
        ),
        (
            has(NamespaceType::Cgroup),
            "linux.namespaces[cgroup]",
            "the cgroup a workload is placed in is not hidden from it",
        ),
        (
            has(NamespaceType::Time),
            "linux.namespaces[time]",
            "there is no control for the clock a workload sees",
        ),
    ] {
        if present {
            unsupported.push(Unsupported { field, reason });
        }
    }

    if let Some(linux) = &spec.linux {
        if !linux.uid_mappings.is_empty() {
            unsupported.push(Unsupported {
                field: "linux.uid_mappings",
                reason: "id mapping needs a user namespace, which is not created",
            });
        }
        if !linux.gid_mappings.is_empty() {
            unsupported.push(Unsupported {
                field: "linux.gid_mappings",
                reason: "id mapping needs a user namespace, which is not created",
            });
        }
        if linux.seccomp.is_some() {
            unsupported.push(Unsupported {
                field: "linux.seccomp",
                reason: "no syscall filter is installed; a spec that believed one was would be \
                         running unfiltered",
            });
        }
        if !linux.masked_paths.is_empty() {
            unsupported.push(Unsupported {
                field: "linux.masked_paths",
                reason: "there is no mechanism to mask individual paths",
            });
        }
        if !linux.readonly_paths.is_empty() {
            unsupported.push(Unsupported {
                field: "linux.readonly_paths",
                reason: "paths are made read-only by mounting them read-only into an isolated \
                         root; list them as read-only bind mounts instead",
            });
        }

        if let Some(resources) = &linux.resources {
            if let Some(memory) = &resources.memory {
                // OCI uses -1 for unlimited, and 0 for unset.
                if memory.limit > 0 {
                    sandbox.memory_bytes = Some(memory.limit as u64);
                }
                if memory.swap > 0 {
                    unsupported.push(Unsupported {
                        field: "linux.resources.memory.swap",
                        reason: "only a total memory ceiling is enforced; swap is not bounded \
                                 separately",
                    });
                }
                if memory.kernel > 0 || memory.kernel_tcp > 0 {
                    unsupported.push(Unsupported {
                        field: "linux.resources.memory.kernel",
                        reason: "kernel memory is not bounded separately from the total",
                    });
                }
            }
            if let Some(pids) = &resources.pids {
                if pids.limit > 0 {
                    sandbox.max_processes = Some(pids.limit as u32);
                }
            }
            if resources.cpu.is_some() {
                unsupported.push(Unsupported {
                    field: "linux.resources.cpu",
                    reason: "OCI bounds a share of wall-clock time per period; the sandbox \
                             bounds total CPU time consumed. Neither converts into the other, \
                             so set SandboxSpec::cpu_time directly if that is what you meant",
                });
            }
            if resources.block_io.is_some() {
                unsupported.push(Unsupported {
                    field: "linux.resources.block_io",
                    reason: "block-I/O weights and throttles have no equivalent",
                });
            }
        }
    }

    if spec.process.terminal {
        unsupported.push(Unsupported {
            field: "process.terminal",
            reason: "no pseudo-terminal is allocated; output is captured, not attached",
        });
    }
    if spec.process.uid != 0 || spec.process.gid != 0 {
        unsupported.push(Unsupported {
            field: "process.uid",
            reason: "the workload runs as whatever user the host process is; there is no \
                     mechanism to switch",
        });
    }
    if spec.process.command.is_empty() {
        unsupported.push(Unsupported {
            field: "process.command",
            reason: "a specification with no command names nothing to run",
        });
    }

    if !unsupported.is_empty() {
        return Err(TranslationError { unsupported });
    }

    let mut env = BTreeMap::new();
    for (key, value) in &spec.process.env {
        env.insert(key.clone(), value.clone());
    }

    let command = SandboxCommand {
        program: spec.process.command[0].clone(),
        args: spec.process.command[1..].to_vec(),
        working_dir: Some(spec.process.cwd.clone()),
        env,
        stdin: None,
    };

    Ok((sandbox, command))
}

/// The read-only bind mounts, and complaints about everything else.
///
/// `hv2-sandbox` mounts a host path at the same path inside the root, so a
/// bind mount whose destination differs from its source cannot be expressed.
fn read_only_binds(spec: &ContainerSpec, unsupported: &mut Vec<Unsupported>) -> Vec<PathBuf> {
    let mut read_only = Vec::new();

    for mount in &spec.mounts {
        if mount.mount_type != MountType::Bind {
            unsupported.push(Unsupported {
                field: "mounts[].mount_type",
                reason: "only bind mounts are performed; tmpfs, proc, sysfs, devpts, cgroup and \
                         mqueue are not mounted for the workload",
            });
            continue;
        }
        if !mount.options.contains(&MountOption::ReadOnly) {
            unsupported.push(Unsupported {
                field: "mounts[].options",
                reason: "only read-only bind mounts are performed; a writable bind mount into \
                         an isolated root is not supported",
            });
            continue;
        }
        if mount.source != mount.destination {
            unsupported.push(Unsupported {
                field: "mounts[].destination",
                reason: "a host path is mounted at the same path inside the root, so a \
                         destination that differs from the source cannot be expressed",
            });
            continue;
        }
        read_only.push(mount.source.clone());
    }

    read_only
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::runtime::{
        ContainerProcess, LinuxConfig, MemoryConfig, Mount, NamespaceConfig, PidsConfig,
        ResourceConfig, RootFs,
    };
    use std::collections::HashMap;

    fn namespaces(types: &[NamespaceType]) -> Vec<NamespaceConfig> {
        types
            .iter()
            .map(|t| NamespaceConfig {
                ns_type: *t,
                path: None,
            })
            .collect()
    }

    /// A spec that asks only for things the sandbox can do.
    fn translatable() -> ContainerSpec {
        ContainerSpec {
            root: RootFs {
                path: PathBuf::from("/var/tmp/root"),
                readonly: false,
            },
            process: ContainerProcess {
                command: vec!["/bin/echo".to_string(), "hello".to_string()],
                cwd: PathBuf::from("/"),
                uid: 0,
                gid: 0,
                terminal: false,
                ..Default::default()
            },
            mounts: Vec::new(),
            linux: Some(LinuxConfig {
                namespaces: namespaces(&[
                    NamespaceType::Mount,
                    NamespaceType::Network,
                    NamespaceType::Pid,
                    NamespaceType::Ipc,
                ]),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn errors_of(spec: &ContainerSpec) -> Vec<&'static str> {
        match to_sandbox(spec) {
            Ok(_) => Vec::new(),
            Err(e) => e.unsupported.iter().map(|u| u.field).collect(),
        }
    }

    #[test]
    fn a_translatable_spec_becomes_the_confinement_it_describes() {
        let (sandbox, command) = to_sandbox(&translatable()).expect("should translate");

        assert_eq!(sandbox.network, NetworkPolicy::Denied);
        assert!(sandbox.isolate_processes);
        assert_eq!(
            sandbox.filesystem,
            FilesystemPolicy::Isolated {
                root: PathBuf::from("/var/tmp/root"),
                read_only: Vec::new(),
            }
        );
        assert_eq!(command.program, "/bin/echo");
        assert_eq!(command.args, vec!["hello".to_string()]);
    }

    /// The point of the whole module: what comes out is a spec the sandbox
    /// agrees it can enforce. A translation that produced something
    /// `reconcile` rejects would have moved the failure rather than removed
    /// it.
    #[test]
    fn what_comes_out_is_something_a_full_backend_accepts() {
        let (sandbox, _) = to_sandbox(&translatable()).expect("should translate");

        let everything = hv2_sandbox::Control::ALL
            .into_iter()
            .fold(hv2_sandbox::Controls::none(), |c, control| c.with(control));
        assert_eq!(
            sandbox.missing_from(&everything),
            Vec::new(),
            "the translated spec asks for a control no backend defines"
        );
    }

    #[test]
    fn resource_limits_carry_across() {
        let mut spec = translatable();
        spec.linux.as_mut().unwrap().resources = Some(ResourceConfig {
            cpu: None,
            memory: Some(MemoryConfig {
                limit: 512 * 1024 * 1024,
                ..Default::default()
            }),
            block_io: None,
            pids: Some(PidsConfig { limit: 64 }),
        });

        let (sandbox, _) = to_sandbox(&spec).expect("should translate");
        assert_eq!(sandbox.memory_bytes, Some(512 * 1024 * 1024));
        assert_eq!(sandbox.max_processes, Some(64));
    }

    /// Seccomp is the one that matters most: a spec whose filter was dropped
    /// runs every syscall it meant to forbid, and looks identical from the
    /// outside to one that was applied.
    #[test]
    fn a_seccomp_filter_is_refused_rather_than_dropped() {
        let mut spec = translatable();
        spec.linux.as_mut().unwrap().seccomp = Some(Default::default());

        assert_eq!(errors_of(&spec), vec!["linux.seccomp"]);
    }

    #[test]
    fn a_uid_switch_is_refused_rather_than_ignored() {
        let mut spec = translatable();
        spec.process.uid = 1000;
        spec.process.gid = 1000;

        assert_eq!(errors_of(&spec), vec!["process.uid"]);
    }

    /// OCI bounds a share of wall-clock per period; the sandbox bounds total
    /// CPU consumed. Converting one into the other would be arithmetic
    /// without meaning.
    #[test]
    fn cpu_limits_are_refused_rather_than_approximated() {
        let mut spec = translatable();
        spec.linux.as_mut().unwrap().resources = Some(ResourceConfig {
            cpu: Some(Default::default()),
            memory: None,
            block_io: None,
            pids: None,
        });

        assert_eq!(errors_of(&spec), vec!["linux.resources.cpu"]);
    }

    /// A root with no mount namespace is a root nothing applies, and a
    /// workload that ran that way would see the host filesystem while its
    /// spec said otherwise.
    #[test]
    fn a_root_without_a_mount_namespace_is_refused() {
        let mut spec = translatable();
        spec.linux.as_mut().unwrap().namespaces = namespaces(&[NamespaceType::Network]);

        assert_eq!(errors_of(&spec), vec!["root.path"]);
    }

    #[test]
    fn read_only_bind_mounts_become_read_only_roots() {
        let mut spec = translatable();
        spec.mounts = vec![Mount {
            source: PathBuf::from("/usr"),
            destination: PathBuf::from("/usr"),
            mount_type: MountType::Bind,
            options: vec![MountOption::ReadOnly],
        }];

        let (sandbox, _) = to_sandbox(&spec).expect("should translate");
        match sandbox.filesystem {
            FilesystemPolicy::Isolated { read_only, .. } => {
                assert_eq!(read_only, vec![PathBuf::from("/usr")]);
            }
            other => panic!("expected an isolated filesystem, got {other:?}"),
        }
    }

    #[test]
    fn a_bind_mount_that_relocates_a_path_is_refused() {
        let mut spec = translatable();
        spec.mounts = vec![Mount {
            source: PathBuf::from("/usr"),
            destination: PathBuf::from("/opt/usr"),
            mount_type: MountType::Bind,
            options: vec![MountOption::ReadOnly],
        }];

        assert_eq!(errors_of(&spec), vec!["mounts[].destination"]);
    }

    #[test]
    fn a_tmpfs_mount_is_refused_because_nothing_mounts_one() {
        let mut spec = translatable();
        spec.mounts = vec![Mount {
            source: PathBuf::from("/tmp"),
            destination: PathBuf::from("/tmp"),
            mount_type: MountType::Tmpfs,
            options: vec![MountOption::ReadOnly],
        }];

        assert_eq!(errors_of(&spec), vec!["mounts[].mount_type"]);
    }

    /// A caller fixing one field at a time would learn of the next only by
    /// running again.
    #[test]
    fn every_unsupported_field_is_reported_at_once() {
        let mut spec = translatable();
        spec.process.terminal = true;
        spec.process.uid = 1000;
        let linux = spec.linux.as_mut().unwrap();
        linux.seccomp = Some(Default::default());
        linux.masked_paths = vec![PathBuf::from("/proc/kcore")];

        let fields = errors_of(&spec);
        assert!(fields.contains(&"process.terminal"), "{fields:?}");
        assert!(fields.contains(&"process.uid"), "{fields:?}");
        assert!(fields.contains(&"linux.seccomp"), "{fields:?}");
        assert!(fields.contains(&"linux.masked_paths"), "{fields:?}");
    }

    #[test]
    fn joining_an_existing_namespace_is_refused() {
        let mut spec = translatable();
        spec.linux.as_mut().unwrap().namespaces[0].path = Some(PathBuf::from("/proc/1/ns/mnt"));

        assert_eq!(errors_of(&spec), vec!["linux.namespaces[].path"]);
    }

    /// The sandbox creates the PID and IPC namespaces together, so a spec
    /// asking for one alone cannot be honoured exactly.
    #[test]
    fn a_pid_namespace_without_ipc_is_refused_rather_than_widened() {
        let mut spec = translatable();
        spec.linux.as_mut().unwrap().namespaces =
            namespaces(&[NamespaceType::Mount, NamespaceType::Pid]);

        assert_eq!(errors_of(&spec), vec!["linux.namespaces"]);
    }

    #[test]
    fn the_environment_carries_across_whole() {
        let mut spec = translatable();
        let mut env = HashMap::new();
        env.insert("PATH".to_string(), "/usr/bin".to_string());
        env.insert("LANG".to_string(), "C".to_string());
        spec.process.env = env;

        let (_, command) = to_sandbox(&spec).expect("should translate");
        assert_eq!(
            command.env.get("PATH").map(String::as_str),
            Some("/usr/bin")
        );
        assert_eq!(command.env.get("LANG").map(String::as_str), Some("C"));
    }

    #[test]
    fn the_error_names_every_field_in_its_message() {
        let mut spec = translatable();
        spec.process.terminal = true;
        let err = to_sandbox(&spec).expect_err("should refuse");

        let text = err.to_string();
        assert!(text.contains("process.terminal"), "{text}");
    }
}
