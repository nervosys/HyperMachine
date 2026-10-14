//! An environment's Git repositories, cloned by the node for a new sandbox.
//!
//! A repository of kind `git` names a Git remote over `http` or `https`. When
//! a sandbox is created with that environment, this node:
//!
//! 1. refuses the remote unless it is under one its operator allows with
//!    `--git-remote`, as Lit remotes are with `--lit-remote`;
//! 2. runs `git clone` with [`clone_arguments`]: only `http` and `https`, no
//!    redirects (one could lead off the allowed host), no hooks, no
//!    submodules and no tags beyond the branch's;
//! 3. runs it with no system or global Git configuration, no terminal to
//!    prompt on and nothing of the node's in its `HOME`;
//! 4. packs the clone, `.git` and all, into the same archive a Lit clone is,
//!    which goes into the guest and is unpacked into `path` before the
//!    create answers.
//!
//! Git remotes take no credentials: an environment's repository of this kind
//! cannot hold a password, so the node's store does not hold one for it.
//! `git` is run, not linked, and must be on the node's `PATH`.

use std::path::Path;
use std::time::Duration;

use hv2_cluster::environments::Repository;

/// How long one clone may take.
const CLONE_TIMEOUT: Duration = Duration::from_secs(300);

/// The `git` arguments that clone `remote` into `tree`: checked out at
/// `branch` when one is given, from that branch alone.
pub(crate) fn clone_arguments(branch: Option<&str>, remote: &str, tree: &str) -> Vec<String> {
    let mut args: Vec<String> = [
        // Only the two web protocols, whatever the host's config says.
        "-c",
        "protocol.allow=never",
        "-c",
        "protocol.http.allow=always",
        "-c",
        "protocol.https.allow=always",
        // An answer that sends the clone elsewhere is not followed.
        "-c",
        "http.followRedirects=false",
        // Nothing the remote could set runs on the node.
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "submodule.recurse=false",
        "-c",
        "transfer.fsckObjects=true",
        "clone",
        "--quiet",
        "--no-tags",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    if let Some(branch) = branch {
        args.extend(["--branch".into(), branch.into(), "--single-branch".into()]);
    }
    args.push(remote.into());
    args.push(tree.into());
    args
}

/// Clone `repo` into a directory of `scratch`'s and pack it.
pub(crate) async fn clone_and_pack(repo: &Repository, scratch: &Path) -> Result<Vec<u8>, String> {
    let home = scratch.join("home");
    std::fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    let tree = scratch.join("tree");
    let args = clone_arguments(
        repo.branch.as_deref(),
        &repo.remote,
        &tree.to_string_lossy(),
    );
    let mut command = tokio::process::Command::new("git");
    command
        .args(&args)
        .current_dir(scratch)
        .env_clear()
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(CLONE_TIMEOUT, command.output())
        .await
        .map_err(|_| {
            format!(
                "{}: git clone took over {} s",
                repo.remote,
                CLONE_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| format!("running git: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "{}: git clone: {}",
            repo.remote,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    crate::lit_repos::pack(&tree, &repo.remote).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clone_is_held_to_the_web_protocols_and_no_redirects() {
        let args = clone_arguments(None, "https://git.example/team/app", "/tmp/t").join(" ");
        for wanted in [
            "protocol.allow=never",
            "protocol.http.allow=always",
            "protocol.https.allow=always",
            "http.followRedirects=false",
            "core.hooksPath=/dev/null",
            "submodule.recurse=false",
        ] {
            assert!(args.contains(wanted), "{wanted}: {args}");
        }
        assert!(!args.contains("--recurse-submodules"), "{args}");
        assert!(!args.contains("--branch"), "{args}");
        assert!(
            args.ends_with("https://git.example/team/app /tmp/t"),
            "{args}"
        );
    }

    #[test]
    fn a_branch_is_cloned_alone() {
        let args = clone_arguments(Some("feature"), "https://git.example/app", "/tmp/t");
        let at = args.iter().position(|a| a == "--branch").unwrap();
        assert_eq!(args[at + 1], "feature");
        assert!(args.contains(&"--single-branch".to_string()));
    }
}
