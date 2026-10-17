# A private Git remote, cloned with the node's credential, on KVM (2026-10-17)

**What this checks:** that a node given `--git-credential PREFIX=ENV` clones a
private Git remote for an environment, sends the credential only where it was
meant, and leaves it out of the guest, the clone and its own error messages.
Before this, a Git repository in an environment could only be public.

## Method

`tools/check-git-private-kvm.py` serves one repository over plain HTTP on
loopback under `/private/` (every request must carry
`Authorization: Basic base64(bot:<token>)`, else 401) and under `/public/`
(open). The server records every request's `Authorization` header. It runs an
owned `hv2-sandboxd` on real KVM with both remotes allowed and the token in
the daemon's environment.

## Result

All 5 cases passed:

| Case | Observed |
|---|---|
| Private remote cloned with the node's credential | tree and `.git` in the guest; all 5 requests to `/private/` carried exactly the expected header |
| Token nowhere in the guest | not in any file under `/srv /root /tmp /etc /home`, not in any `/proc/*/environ`, no `extraHeader` in `.git/config`; a control (a file holding the token) is found by the same search |
| Remote outside the prefix | 5 requests to `/public/`, none with an `Authorization` header |
| Wrong token | 502 before anything boots, no sandbox left, token not in the message |
| No credential configured | 502, the same |

Unit tests (5, in `git_repos.rs`) cover prefix scoping (including a look-alike
host), the `user:token` shape, the credential not appearing in `Debug`, and
the scrubbing of error text. Clippy and fmt are clean.

The first three runs (before the search control was added) passed all 5
cases; only the final run's report is kept here as `report.json`.

## Not verified

- A real GitHub, GitLab or other host: the server here is a Python one doing
  Basic auth. SSH remotes are not supported (http and https only).
- That the Git version on the node sends `GIT_CONFIG_COUNT` config (needs Git
  2.31 or later): the check ran with the WSL Debian's own Git.
- The token is in the node process's environment, so anyone who can read
  `/proc/<pid>/environ` as the node's user, or root, can read it. One token
  per node and prefix: not per team, and not rotated without a restart.
- Windows and macOS nodes: the code is shared, but the check ran on Linux.
