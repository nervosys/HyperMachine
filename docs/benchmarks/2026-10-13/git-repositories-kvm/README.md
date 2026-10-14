# Git repositories in an environment

**What this checks:** a repository of kind `git` in an environment, cloned
by the node into a sandbox's guest. Before this, a Git remote was not cloned
at all, and an environment could hold only Lit repositories. boat clones Git
repositories; see the [comparison](../../../BOAT_PARITY.md) and
[the environments guide](../../../ENVIRONMENTS.md#repositories).

## Method

`tools/check-git-repositories-kvm.py` makes a Git repository with `main` and
`feature` branches, and copies its history into a dumb-HTTP repository served
on loopback. A second path under the same server answers every request with
a redirect to the first. The owned `hv2-sandboxd` is given two `--git-remote`
prefixes: the repository's, and the redirecting one's. The guest image is
busybox with the guest agent.

1. **Described:** an environment holds the repository twice, once on `main`
   and once on `feature`, with no credentials in the description.
2. **In the sandbox:** `/srv/app` has `main`'s files and `.git/`, and
   `/srv/feature` has `feature`'s, with `FEATURE.md` only there. Everything
   is owned by the user commands run as, and no archive is left in `/tmp`.
3. **Refused, each before anything boots, and no sandbox left:**
   - a `username` and `password` on a Git repository (400, at the
     environment);
   - a remote not under an allowed prefix (403);
   - a remote under an allowed prefix whose answer redirects outside it (502),
     because `http.followRedirects=false` stops `git` following it.

## Result

All three cases passed on 2026-10-13, on Linux 6.18.33 under WSL2 with
nested KVM, with git 2.47.3 on the node. From [`report.json`](report.json):

| | |
|---|---|
| `main` in the sandbox | `on main`, `print('main')` first lines |
| `feature` in the sandbox | `only on feature` in `FEATURE.md`, and `main`'s `README.md` |
| Credentials on a Git repository | 400, refused at the environment |
| Remote outside the allowed list | 403, no sandbox left |
| Remote redirecting out of the list | 502 (git: not followed), no sandbox left |

Unit tests cover the clone arguments: only `http`/`https`, no redirects, no
hooks, no submodules, and a branch cloned alone.

## What this does not show

- **Private remotes.** No credential is ever given to a Git clone, so only
  public repositories work. A credential the node can use without keeping an
  admin password in the store is not designed.
- **A remote over SSH or `git://`.** Refused by the protocol settings; not
  run here.
- **Large repositories.** Each clone is held whole in memory, up to 64 MiB
  packed, as a Lit clone is.
- **Submodules and LFS** are not fetched; a clone does not recurse.
- **The node's `git` being an old one.** `GIT_CONFIG_GLOBAL` needs git 2.32
  or later; not checked on older builds.
