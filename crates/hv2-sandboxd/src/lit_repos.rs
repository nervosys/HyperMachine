//! An environment's Lit repositories, cloned by the node for a new sandbox.
//!
//! Lit is nervosys' version control system. A repository in an environment
//! names a `lit server` remote and a place in the guest. When a sandbox is
//! created with that environment, this node:
//!
//! 1. refuses the remote unless it is under one its operator allows with
//!    `--lit-remote`, so a team cannot make the node fetch whatever it names;
//! 2. logs in with the repository's account, when it has one, for a session
//!    token, since Lit's tokens expire and so are not stored;
//! 3. runs `lit clone` (the binary `--lit` names) into a directory of its own,
//!    with that token in `LIT_TOKEN` and nothing of the node's in its `HOME`,
//!    and checks out `branch` when one is given;
//! 4. packs the clone, `.lit/` included, into a tar archive that goes into the
//!    guest as one of the environment's files, unpacked into `path` and given
//!    to the template's user before the create answers.
//!
//! The clone is the node's, not the guest's: the guest needs no network and
//! no `lit` of its own. A reboot puts back the same archive, not a new clone.
//!
//! The binary is run, not linked: HyperMachine depends on no Lit code.

use std::path::{Path, PathBuf};
use std::time::Duration;

use hv2_cluster::environments::{GuestFile, Repository};

/// How long one clone may take.
const CLONE_TIMEOUT: Duration = Duration::from_secs(300);
/// The most one cloned repository may be, packed: it is held in memory and
/// written into the guest whole.
pub(crate) const MOST_PACKED: usize = 64 << 20;

/// Whether `remote` is one of `allowed`, or under one: the same scheme, host
/// and port, and a path at or below the allowed one's.
pub(crate) fn allowed(remote: &str, allowed: &[String]) -> bool {
    let Ok(remote) = reqwest::Url::parse(remote) else {
        return false;
    };
    allowed.iter().any(|prefix| {
        let Ok(prefix) = reqwest::Url::parse(prefix) else {
            return false;
        };
        let base = prefix.path().trim_end_matches('/');
        remote.scheme() == prefix.scheme()
            && remote.host_str() == prefix.host_str()
            && remote.port_or_known_default() == prefix.port_or_known_default()
            && (remote.path().trim_end_matches('/') == base
                || remote.path().starts_with(&format!("{base}/")))
    })
}

/// A session token for `repo`'s account, from its remote.
async fn log_in(http: &reqwest::Client, repo: &Repository) -> Result<Option<String>, String> {
    let (Some(username), Some(password)) = (&repo.username, &repo.password) else {
        return Ok(None);
    };
    let url = format!("{}/api/v1/auth/login", repo.remote.trim_end_matches('/'));
    let response = http
        .post(&url)
        .json(&serde_json::json!({"username": username, "password": password}))
        .send()
        .await
        .map_err(|e| format!("{}: logging in: {}", repo.remote, e.without_url()))?;
    let status = response.status();
    let body: serde_json::Value = response.json().await.unwrap_or_default();
    if !status.is_success() {
        let said = body["error"]["message"].as_str().unwrap_or("");
        return Err(format!(
            "{}: logging in as {username}: {status} {said}",
            repo.remote
        ));
    }
    body["token"]
        .as_str()
        .map(|token| Some(token.to_string()))
        .ok_or_else(|| format!("{}: the login answered with no token", repo.remote))
}

/// Run `lit` in `dir` with only what it needs in its environment; what it
/// printed, as Lit's JSON, or why it failed.
async fn lit(
    binary: &Path,
    dir: &Path,
    home: &Path,
    token: Option<&str>,
    args: &[&str],
) -> Result<serde_json::Value, String> {
    let mut command = tokio::process::Command::new(binary);
    command
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    if let Some(token) = token {
        command.env("LIT_TOKEN", token);
    }
    let output = tokio::time::timeout(CLONE_TIMEOUT, command.output())
        .await
        .map_err(|_| format!("lit {} took over {} s", args[0], CLONE_TIMEOUT.as_secs()))?
        .map_err(|e| format!("running {}: {e}", binary.display()))?;
    let answer: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_default();
    if answer["status"] == "ok" {
        Ok(answer)
    } else {
        let said = answer["error"]["message"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| String::from_utf8_lossy(&output.stderr).trim().to_string());
        Err(format!("lit {}: {said}", args[0]))
    }
}

/// Clone `repo` and pack it; the archive, and the commit it holds.
async fn clone_and_pack(
    binary: &Path,
    http: &reqwest::Client,
    repo: &Repository,
    scratch: &Path,
) -> Result<Vec<u8>, String> {
    let token = log_in(http, repo).await?;
    let home = scratch.join("home");
    std::fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    let tree = scratch.join("tree");
    let tree_arg = tree.to_string_lossy().to_string();
    lit(
        binary,
        scratch,
        &home,
        token.as_deref(),
        &["clone", &repo.remote, &tree_arg],
    )
    .await
    .map_err(|e| format!("{}: {e}", repo.remote))?;
    if let Some(branch) = &repo.branch {
        // Lit 1.8.0's clone fetches every branch but makes a local one only
        // of the default: the others are under refs/remotes/origin, and
        // `lit checkout <branch>` does not find them. So the branch's commit
        // is checked out, and a branch of that name made at it.
        let fetched = tree.join(".lit/refs/remotes/origin").join(branch);
        let commit = std::fs::read_to_string(&fetched)
            .map(|text| text.trim().to_string())
            .map_err(|_| format!("{}: the remote has no branch {branch}", repo.remote))?;
        if commit.is_empty() || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("{}: branch {branch} names no commit", repo.remote));
        }
        for step in [
            vec!["checkout", commit.as_str()],
            vec!["checkout", "-b", branch],
        ] {
            lit(binary, &tree, &home, token.as_deref(), &step)
                .await
                .map_err(|e| format!("{}: branch {branch}: {e}", repo.remote))?;
        }
    }
    let packed = tokio::process::Command::new("tar")
        .arg("-C")
        .arg(&tree)
        .args(["-cf", "-", "."])
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(|e| format!("packing {}: {e}", repo.remote))?;
    if !packed.status.success() {
        return Err(format!(
            "packing {}: {}",
            repo.remote,
            String::from_utf8_lossy(&packed.stderr).trim()
        ));
    }
    if packed.stdout.len() > MOST_PACKED {
        return Err(format!(
            "{} is over {} MiB packed; a sandbox's repositories are copied in whole",
            repo.remote,
            MOST_PACKED >> 20
        ));
    }
    Ok(packed.stdout)
}

/// Each of `repos` cloned and packed, as files for the guest that unpack
/// into their places; or why one could not be.
pub(crate) async fn prepare(
    binary: Option<&Path>,
    remotes: &[String],
    http: &reqwest::Client,
    repos: &[Repository],
) -> Result<Vec<GuestFile>, (axum::http::StatusCode, String)> {
    use axum::http::StatusCode;
    if repos.is_empty() {
        return Ok(Vec::new());
    }
    let Some(binary) = binary else {
        return Err((
            StatusCode::BAD_REQUEST,
            "the environment has repositories, and this node has no --lit to clone them with"
                .to_string(),
        ));
    };
    let mut files = Vec::with_capacity(repos.len());
    for (index, repo) in repos.iter().enumerate() {
        if !allowed(&repo.remote, remotes) {
            return Err((
                StatusCode::FORBIDDEN,
                format!(
                    "{}: this node clones only from the Lit remotes its operator allows",
                    repo.remote
                ),
            ));
        }
        let scratch = scratch_dir();
        std::fs::create_dir_all(&scratch)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let packed = clone_and_pack(binary, http, repo, &scratch).await;
        let _ = std::fs::remove_dir_all(&scratch);
        let packed = packed.map_err(|e| (StatusCode::BAD_GATEWAY, e))?;
        files.push(GuestFile {
            path: format!("/tmp/.hm-repository-{index}.tar"),
            content_base64: hv2_guest_agent::b64::encode(&packed),
            unpack_into: Some(repo.path.clone()),
        });
    }
    Ok(files)
}

fn scratch_dir() -> PathBuf {
    std::env::temp_dir().join(format!("hm-lit-{}", uuid::Uuid::new_v4().simple()))
}

/// A shell word that is `text`, whatever is in it.
pub(crate) fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The guest's command to unpack `archive` into `into`, give the tree to
/// whoever owns the archive (the template's user, who it was written as),
/// and remove the archive.
pub(crate) fn unpack_command(archive: &str, into: &str) -> String {
    let (archive, into) = (quoted(archive), quoted(into));
    format!(
        "set -e; owner=$(stat -c %u:%g {archive}); mkdir -p {into}; \
         tar -xf {archive} -C {into}; chown -R \"$owner\" {into}; rm -f {archive}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remote_is_cloned_only_from_under_one_the_operator_allows() {
        let allowed_remotes = vec![
            "https://lit.example/team".to_string(),
            "http://127.0.0.1:3917".to_string(),
        ];
        for remote in [
            "https://lit.example/team",
            "https://lit.example/team/",
            "https://lit.example/team/app",
            "https://lit.example:443/team/app",
            "http://127.0.0.1:3917",
            "http://127.0.0.1:3917/any/repo",
        ] {
            assert!(allowed(remote, &allowed_remotes), "{remote}");
        }
        for remote in [
            "https://lit.example/teammate",
            "https://lit.example/other",
            "http://lit.example/team",
            "https://lit.example.evil/team",
            "https://evil.example/team",
            "http://127.0.0.1:3918",
            "http://169.254.169.254/team",
            "not a url",
        ] {
            assert!(!allowed(remote, &allowed_remotes), "{remote}");
        }
        assert!(!allowed("https://lit.example/team", &[]));
    }

    #[test]
    fn the_unpack_command_quotes_what_it_is_given() {
        let command = unpack_command("/tmp/.hm-repository-0.tar", "/srv/it's here");
        assert!(command.contains(r"'/srv/it'\''s here'"), "{command}");
        assert!(command.starts_with("set -e;"));
        assert_eq!(quoted("$(x)"), "'$(x)'");
    }
}
