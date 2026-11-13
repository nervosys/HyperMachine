//! E2B's `envVars`: environment variables for every command a sandbox runs.
//!
//! They go where a template's own `ENV` already lives -- the guest's
//! [`TEMPLATE_DEFAULTS_PATH`], which the guest agent reads at every `Exec`
//! and `Start` -- layered over the template's variables and under a
//! command's own. Kept in the guest rather than on the host, so a pause, a
//! resume on another node, a fork and a snapshot all carry them with the
//! rest of the guest's memory, and nothing on the host can drift from it.
//!
//! Write-only, as in E2B: no response returns them, so a secret passed here
//! is readable inside its sandbox and nowhere else.
//!
//! One copy is kept on the host, in this process's memory only ([`keep`]):
//! a guest that reboots ([`crate::reboot`]) starts again from its template,
//! whose defaults do not have them, and they are put back from here. Never
//! written to disk, never in a record, and gone when the sandbox ends -- so a
//! sandbox resumed on another node, or after this daemon restarts, reboots
//! without them.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::Duration;

use hv2_agent::AgentVM;
use hv2_cluster::environments::GuestFile;
use hv2_guest_agent::{TemplateDefaults, TEMPLATE_DEFAULTS_PATH};

/// How long the guest has to read or write its defaults.
const AGENT_TIMEOUT: Duration = Duration::from_secs(10);
/// At most this many variables.
pub(crate) const MAX_VARS: usize = 1024;
/// At most this many bytes of names and values together: what the guest
/// reads at every command start, so it is kept small.
pub(crate) const MAX_BYTES: usize = 128 * 1024;

/// What each sandbox was created with, for its reboots.
static KEPT: Mutex<Option<HashMap<String, BTreeMap<String, String>>>> = Mutex::new(None);
/// The files of the environment each sandbox was created with, for its
/// reboots, as the variables are kept.
static FILES: Mutex<Option<HashMap<String, Vec<GuestFile>>>> = Mutex::new(None);

fn files_map<T>(f: impl FnOnce(&mut HashMap<String, Vec<GuestFile>>) -> T) -> T {
    let mut guard = FILES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(guard.get_or_insert_with(HashMap::new))
}

/// Keep `files` for `sandbox_id`'s reboots. Nothing is kept for none.
pub(crate) fn keep_files(sandbox_id: &str, files: &[GuestFile]) {
    if !files.is_empty() {
        files_map(|m| m.insert(sandbox_id.to_string(), files.to_vec()));
    }
}

/// The files `sandbox_id` was created with, as far as this process knows.
pub(crate) fn kept_files(sandbox_id: &str) -> Vec<GuestFile> {
    files_map(|m| m.get(sandbox_id).cloned().unwrap_or_default())
}

/// Write an environment's `files` into `vm`'s guest, owned by the
/// template's user, so its commands can read them.
pub(crate) async fn write_files(vm: &AgentVM, files: &[GuestFile]) -> Result<(), String> {
    for file in files {
        let bytes = file.bytes()?;
        // A cloned repository goes in through the agent's file operations,
        // with no archive written to the guest and no `tar`, `chown` or shell
        // in it. Only an agent that predates them, or an archive the agent
        // cannot reproduce (a hard link or special file), uses the shell.
        if let Some(into) = &file.unpack_into {
            match unpack_by_agent(vm, &bytes, into).await {
                Ok(()) => {
                    tracing::debug!("unpacking a repository into {into}: using agent op");
                    continue;
                }
                Err(Unpack::Shell(why)) => {
                    tracing::debug!(
                        "unpacking a repository into {into}: falling back to shell ({why})"
                    );
                }
                Err(Unpack::Failed(e)) => return Err(e),
            }
        }
        vm.write_file_in_guest_as(
            &file.path,
            bytes,
            Some(hv2_guest_agent::TEMPLATE_USER),
            AGENT_TIMEOUT,
        )
        .await
        .map_err(|e| format!("writing the environment's {}: {e}", file.path))?;
        // A cloned repository: unpacked into its place, then removed.
        if let Some(into) = &file.unpack_into {
            let line = crate::lit_repos::unpack_command(&file.path, into);
            let done = vm
                .exec_in_guest("/bin/sh", &["-c".to_string(), line], AGENT_TIMEOUT * 6)
                .await
                .map_err(|e| {
                    if crate::guest_setup::guest_has_no_shell(&e.to_string()) {
                        format!(
                            "unpacking the repository into {into}: {}",
                            crate::guest_setup::no_shell_message(false, &e.to_string())
                        )
                    } else {
                        format!("unpacking the repository into {into}: {e}")
                    }
                })?;
            if done.exit_code != Some(0) {
                return Err(format!(
                    "unpacking the repository into {into}: {}",
                    done.stderr.trim()
                ));
            }
        }
    }
    Ok(())
}

/// One entry of a repository's tar, with its path relative to the tree's
/// root (empty for the root itself).
#[derive(Debug, PartialEq, Eq)]
enum Member {
    Dir {
        rel: String,
        mode: u32,
    },
    File {
        rel: String,
        mode: u32,
        data: Vec<u8>,
    },
    /// Created verbatim with the agent's `make_symlink`, never followed.
    Symlink {
        rel: String,
        target: String,
    },
}

/// What stopped the agent from unpacking a tar.
#[derive(Debug)]
enum Unpack {
    /// Not through the agent: the shell may do it. Says why.
    Shell(String),
    Failed(String),
}

/// A tar's members, or why the agent's file operations cannot reproduce it:
/// they have no hard link or device. Nothing is skipped silently.
///
/// A symlink is created verbatim, whatever it points at: a repository's
/// links are its own business inside its own guest. What must not happen is
/// a *later member being written through one*: the agent's `write_file` and
/// `make_dir` follow symlinks in a path's parents (as `open(2)` does), so a
/// tar holding `a -> /etc` and then `a/passwd` would write outside the tree.
/// A member whose path is, or lies under, an earlier symlink member is
/// therefore refused outright (what `tar -x` would also not be asked to do
/// for a real repository, whose tree has no such path).
fn members(tar_bytes: &[u8]) -> Result<Vec<Member>, Unpack> {
    let bad = |e: std::io::Error| Unpack::Failed(format!("reading the repository's archive: {e}"));
    let mut archive = tar::Archive::new(tar_bytes);
    let mut out = Vec::new();
    let mut links: Vec<String> = Vec::new();
    for entry in archive.entries().map_err(bad)? {
        let mut entry = entry.map_err(bad)?;
        let kind = entry.header().entry_type();
        let mode = entry.header().mode().map_err(bad)? & 0o7777;
        let raw = entry.path_bytes().into_owned();
        let path = String::from_utf8(raw).map_err(|_| {
            Unpack::Failed("the repository's archive holds a name that is not UTF-8".into())
        })?;
        let mut parts = Vec::new();
        for part in path.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    return Err(Unpack::Failed(format!(
                        "the repository's archive holds {path:?}, which leaves its tree"
                    )))
                }
                p => parts.push(p),
            }
        }
        let rel = parts.join("/");
        if links
            .iter()
            .any(|link| rel == *link || rel.starts_with(&format!("{link}/")))
            && !kind.is_pax_global_extensions()
            && !kind.is_pax_local_extensions()
        {
            return Err(Unpack::Failed(format!(
                "the repository's archive holds {path:?} through a symlink it made earlier"
            )));
        }
        if kind.is_symlink() {
            if rel.is_empty() {
                return Err(Unpack::Failed(
                    "the repository's archive makes its own root a symlink".into(),
                ));
            }
            let target = entry
                .link_name_bytes()
                .map(|t| t.into_owned())
                .and_then(|t| String::from_utf8(t).ok())
                .filter(|t| !t.is_empty())
                .ok_or_else(|| {
                    Unpack::Failed(format!(
                        "the repository's archive holds symlink {path:?} with no usable \
                         (UTF-8) target"
                    ))
                })?;
            links.push(rel.clone());
            out.push(Member::Symlink { rel, target });
        } else if kind.is_dir() {
            out.push(Member::Dir { rel, mode });
        } else if kind.is_file() {
            // Whatever `chown` would have left: it clears set-user-ID and
            // set-group-ID on a file.
            let mut data = Vec::with_capacity(entry.size() as usize);
            std::io::Read::read_to_end(&mut entry, &mut data).map_err(bad)?;
            out.push(Member::File {
                rel,
                mode: mode & 0o1777,
                data,
            });
        } else if kind.is_pax_global_extensions() || kind.is_pax_local_extensions() {
            // Metadata for the entry after it, which the reader applied.
        } else {
            return Err(Unpack::Shell(format!(
                "{path:?} is not a file, directory or symlink (a hard link or special file)"
            )));
        }
    }
    Ok(out)
}

/// `into` and `rel` joined, as a guest path.
fn join_guest(into: &str, rel: &str) -> String {
    let base = into.trim_end_matches('/');
    if rel.is_empty() {
        if base.is_empty() {
            "/".into()
        } else {
            base.to_string()
        }
    } else {
        format!("{base}/{rel}")
    }
}

/// Unpack a repository's tar into `into` through the agent, every file and
/// directory owned by the template's user, as `unpack_command` leaves them:
/// modes from the archive, modification times not kept.
async fn unpack_by_agent(vm: &AgentVM, tar_bytes: &[u8], into: &str) -> Result<(), Unpack> {
    let owner = Some(hv2_guest_agent::TEMPLATE_USER);
    let wait = AGENT_TIMEOUT * 6;
    let fail = |what: &str, e: hv2_agent::AgentError| {
        Unpack::Failed(format!("unpacking the repository into {into}: {what}: {e}"))
    };
    // Read the whole archive first, so one the agent cannot reproduce is
    // found before anything is written.
    let members = members(tar_bytes)?;
    match vm.make_dir_in_guest(into, true, owner, None, wait).await {
        Ok(()) => {}
        Err(e) if hv2_agent::is_unsupported_op(&e) => {
            return Err(Unpack::Shell(
                "the guest's agent predates file operations".into(),
            ))
        }
        Err(e) => return Err(fail("making it", e)),
    }
    for member in &members {
        match member {
            Member::Dir { rel, mode } => {
                let path = join_guest(into, rel);
                vm.make_dir_in_guest(&path, true, owner, Some(*mode), wait)
                    .await
                    .map_err(|e| fail(&path, e))?;
                // One that was there already keeps its mode from
                // `make_dir`, which `tar` would have changed.
                vm.set_attr_in_guest(&path, None, Some(*mode), false, wait)
                    .await
                    .map_err(|e| fail(&path, e))?;
            }
            Member::File { rel, mode, data } => {
                let path = join_guest(into, rel);
                vm.write_file_in_guest_as(&path, data.clone(), owner, wait)
                    .await
                    .map_err(|e| fail(&path, e))?;
                vm.set_attr_in_guest(&path, None, Some(*mode), false, wait)
                    .await
                    .map_err(|e| fail(&path, e))?;
            }
            Member::Symlink { rel, target } => {
                let path = join_guest(into, rel);
                match vm.make_symlink_in_guest(target, &path, owner, wait).await {
                    Ok(()) => {}
                    // An agent between versions: the shell redoes the whole
                    // tree, overwriting what was written so far.
                    Err(e) if hv2_agent::is_unsupported_op(&e) => {
                        return Err(Unpack::Shell(
                            "the guest's agent predates the symlink operation".into(),
                        ))
                    }
                    Err(e) => return Err(fail(&path, e)),
                }
            }
        }
    }
    // `chown -R` over the whole of `into`, what was in it before included.
    vm.set_attr_in_guest(into, owner, None, true, wait)
        .await
        .map_err(|e| fail("giving it to the template's user", e))?;
    Ok(())
}

fn kept_map<T>(f: impl FnOnce(&mut HashMap<String, BTreeMap<String, String>>) -> T) -> T {
    let mut guard = KEPT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(guard.get_or_insert_with(HashMap::new))
}

/// Keep `env` for `sandbox_id`'s reboots. Nothing is kept for none.
pub(crate) fn keep(sandbox_id: &str, env: &BTreeMap<String, String>) {
    if !env.is_empty() {
        kept_map(|m| m.insert(sandbox_id.to_string(), env.clone()));
    }
}

/// What `sandbox_id` was created with, as far as this process knows.
pub(crate) fn kept(sandbox_id: &str) -> BTreeMap<String, String> {
    kept_map(|m| m.get(sandbox_id).cloned().unwrap_or_default())
}

/// A fork has its source's variables in its memory; its reboots need them too.
pub(crate) fn inherit(source: &str, fork: &str) {
    let env = kept(source);
    keep(fork, &env);
    keep_files(fork, &kept_files(source));
}

/// The sandbox ended.
pub(crate) fn forget(sandbox_id: &str) {
    kept_map(|m| m.remove(sandbox_id));
    files_map(|m| m.remove(sandbox_id));
}

/// Refuse what cannot be an environment variable, or is too much of one.
pub(crate) fn validate(env: &BTreeMap<String, String>) -> Result<(), String> {
    if env.len() > MAX_VARS {
        return Err(format!(
            "envVars has {} variables; at most {MAX_VARS}",
            env.len()
        ));
    }
    let mut bytes = 0usize;
    for (name, value) in env {
        if name.is_empty() || name.contains('=') || name.contains('\0') {
            return Err(format!(
                "envVars: {name:?} is not a variable name (empty, or holds '=' or NUL)"
            ));
        }
        if value.contains('\0') {
            return Err(format!("envVars: the value of {name} holds a NUL byte"));
        }
        bytes += name.len() + value.len();
    }
    if bytes > MAX_BYTES {
        return Err(format!("envVars is {bytes} bytes; at most {MAX_BYTES}"));
    }
    Ok(())
}

/// The defaults a guest should hold: `current`'s, with `env` over its
/// variables. `current` is `None` for a template not built by steps, which
/// has no defaults file.
pub(crate) fn merged(
    current: Option<&[u8]>,
    env: &BTreeMap<String, String>,
) -> Result<Vec<u8>, String> {
    let mut defaults: TemplateDefaults = match current {
        Some(bytes) => serde_json::from_slice(bytes)
            .map_err(|e| format!("the guest's {TEMPLATE_DEFAULTS_PATH} is not readable: {e}"))?,
        None => TemplateDefaults::default(),
    };
    defaults
        .env
        .extend(env.iter().map(|(k, v)| (k.clone(), v.clone())));
    serde_json::to_vec(&defaults).map_err(|e| e.to_string())
}

/// Whether a guest's read failure is its file not existing, which is the one
/// failure that means "no defaults" rather than "could not tell".
///
/// Anything else fails the create: taking an unreadable file for an absent
/// one would overwrite a template's `ENV`, `WORKDIR` and `USER` with nothing.
fn is_absent(error: &str) -> bool {
    error.contains("(os error 2)")
}

/// Give `vm`'s guest `env`, over its template's variables.
pub(crate) async fn apply(vm: &AgentVM, env: &BTreeMap<String, String>) -> Result<(), String> {
    if env.is_empty() {
        return Ok(());
    }
    let current = match vm
        .read_file_in_guest(TEMPLATE_DEFAULTS_PATH, 1 << 20, AGENT_TIMEOUT)
        .await
    {
        Ok(bytes) => Some(bytes),
        Err(e) if is_absent(&e.to_string()) => None,
        Err(e) => return Err(format!("reading the template's defaults: {e}")),
    };
    let bytes = merged(current.as_deref(), env)?;
    vm.write_file_in_guest(TEMPLATE_DEFAULTS_PATH, bytes, AGENT_TIMEOUT)
        .await
        .map_err(|e| format!("setting envVars: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn tar_of(build: impl FnOnce(&mut tar::Builder<Vec<u8>>)) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        build(&mut builder);
        builder.into_inner().unwrap()
    }

    fn header(kind: tar::EntryType, mode: u32, size: u64) -> tar::Header {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(kind);
        header.set_mode(mode);
        header.set_size(size);
        header.set_cksum();
        header
    }

    #[test]
    fn a_repositorys_tar_is_read_as_directories_and_files() {
        let bytes = tar_of(|b| {
            b.append_data(
                &mut header(tar::EntryType::Directory, 0o755, 0),
                "./",
                &[][..],
            )
            .unwrap();
            b.append_data(
                &mut header(tar::EntryType::Directory, 0o700, 0),
                "./.lit",
                &[][..],
            )
            .unwrap();
            b.append_data(
                &mut header(tar::EntryType::Regular, 0o4755, 5),
                "./.lit/HEAD",
                &b"hello"[..],
            )
            .unwrap();
        });
        let got = members(&bytes).unwrap();
        assert_eq!(
            got,
            vec![
                Member::Dir {
                    rel: String::new(),
                    mode: 0o755
                },
                Member::Dir {
                    rel: ".lit".into(),
                    mode: 0o700
                },
                // Set-user-ID is dropped, as `chown` drops it.
                Member::File {
                    rel: ".lit/HEAD".into(),
                    mode: 0o755,
                    data: b"hello".to_vec()
                },
            ]
        );
    }

    #[test]
    fn a_symlink_in_the_tar_is_kept_verbatim_even_if_absolute_or_dotted() {
        let bytes = tar_of(|b| {
            let mut h = header(tar::EntryType::Symlink, 0o777, 0);
            b.append_link(&mut h, "./link", "../../etc/x").unwrap();
            let mut h = header(tar::EntryType::Symlink, 0o777, 0);
            b.append_link(&mut h, "abs", "/etc").unwrap();
        });
        assert_eq!(
            members(&bytes).unwrap(),
            vec![
                Member::Symlink {
                    rel: "link".into(),
                    target: "../../etc/x".into()
                },
                Member::Symlink {
                    rel: "abs".into(),
                    target: "/etc".into()
                },
            ]
        );
    }

    #[test]
    fn a_member_written_through_an_earlier_symlink_is_refused() {
        // `a -> /etc` then `a/passwd` would make the agent's write follow the
        // link out of the tree.
        for later in ["a/passwd", "a", "./a/b/c"] {
            let bytes = tar_of(|b| {
                let mut h = header(tar::EntryType::Symlink, 0o777, 0);
                b.append_link(&mut h, "a", "/etc").unwrap();
                b.append_data(
                    &mut header(tar::EntryType::Regular, 0o644, 1),
                    later,
                    &b"x"[..],
                )
                .unwrap();
            });
            assert!(matches!(members(&bytes), Err(Unpack::Failed(_))), "{later}");
        }
        // A sibling that merely shares a prefix is fine.
        let bytes = tar_of(|b| {
            let mut h = header(tar::EntryType::Symlink, 0o777, 0);
            b.append_link(&mut h, "a", "/etc").unwrap();
            b.append_data(
                &mut header(tar::EntryType::Regular, 0o644, 1),
                "ab",
                &b"x"[..],
            )
            .unwrap();
        });
        assert_eq!(members(&bytes).unwrap().len(), 2);
    }

    #[test]
    fn a_hard_link_is_still_left_to_the_shell_and_never_dropped() {
        let bytes = tar_of(|b| {
            let mut h = header(tar::EntryType::Link, 0o644, 0);
            b.append_link(&mut h, "./hard", "target").unwrap();
        });
        assert!(matches!(members(&bytes), Err(Unpack::Shell(_))));
    }

    #[test]
    fn a_name_that_leaves_the_tree_is_refused() {
        let mut bytes = tar_of(|b| {
            b.append_data(
                &mut header(tar::EntryType::Regular, 0o644, 1),
                "x",
                &b"y"[..],
            )
            .unwrap();
        });
        // `append_data` refuses `..`, so write it into the header's name field.
        bytes[..2].copy_from_slice(b"..");
        bytes[2] = b'/';
        bytes[3] = b'x';
        bytes[4] = 0;
        let mut header = tar::Header::new_gnu();
        header.as_mut_bytes().copy_from_slice(&bytes[..512]);
        header.set_cksum();
        bytes[..512].copy_from_slice(header.as_bytes());
        assert!(matches!(members(&bytes), Err(Unpack::Failed(_))));
    }

    #[test]
    fn guest_paths_join_without_doubled_slashes() {
        assert_eq!(join_guest("/srv/app", ""), "/srv/app");
        assert_eq!(join_guest("/srv/app/", "a/b"), "/srv/app/a/b");
        assert_eq!(join_guest("/", "a"), "/a");
        assert_eq!(join_guest("/", ""), "/");
    }

    #[test]
    fn sandbox_variables_go_over_the_templates_and_keep_the_rest() {
        let template = TemplateDefaults {
            env: env(&[("PATH", "/usr/bin"), ("MODE", "template")]),
            cwd: Some("/app".into()),
            user: Some("node".into()),
        };
        let bytes = serde_json::to_vec(&template).unwrap();
        let out = merged(Some(&bytes), &env(&[("MODE", "sandbox"), ("TOKEN", "t")])).unwrap();
        let out: TemplateDefaults = serde_json::from_slice(&out).unwrap();
        assert_eq!(
            out.env,
            env(&[("MODE", "sandbox"), ("PATH", "/usr/bin"), ("TOKEN", "t")])
        );
        assert_eq!(out.cwd.as_deref(), Some("/app"));
        assert_eq!(out.user.as_deref(), Some("node"));
    }

    #[test]
    fn a_template_without_defaults_gets_only_the_sandboxs() {
        let out = merged(None, &env(&[("A", "1")])).unwrap();
        let out: TemplateDefaults = serde_json::from_slice(&out).unwrap();
        assert_eq!(
            out,
            TemplateDefaults {
                env: env(&[("A", "1")]),
                ..TemplateDefaults::default()
            }
        );
    }

    #[test]
    fn an_unreadable_defaults_file_is_an_error_not_an_empty_one() {
        assert!(merged(Some(b"not json"), &env(&[("A", "1")])).is_err());
        assert!(is_absent(
            "guest refused: reading /etc/hv2/defaults.json: No such file or directory (os error 2)"
        ));
        assert!(!is_absent("guest did not answer within 10s"));
        assert!(!is_absent(
            "reading /etc/hv2/defaults.json: Permission denied (os error 13)"
        ));
    }

    #[test]
    fn names_and_sizes_are_checked() {
        assert!(validate(&env(&[("GOOD_NAME", "any value = fine")])).is_ok());
        assert!(validate(&env(&[("", "x")])).is_err());
        assert!(validate(&env(&[("A=B", "x")])).is_err());
        assert!(validate(&env(&[("A\0", "x")])).is_err());
        assert!(validate(&env(&[("A", "x\0y")])).is_err());

        let many: BTreeMap<String, String> = (0..=MAX_VARS)
            .map(|i| (format!("V{i}"), String::new()))
            .collect();
        assert!(validate(&many).is_err());
        let big = env(&[("BIG", &"x".repeat(MAX_BYTES))]);
        assert!(validate(&big).is_err());
    }
}
