# Checkpoint archive and reader freeze through the agent's file operations, on KVM

Follow-up to `../agent-ops-kvm/README.md`. The same `tools/check-agent-ops-kvm.py` (extended to seven
cases) runs an owned `hv2-sandboxd` against three guest images and checks, through the API, the three
changes that README listed as "not switched", plus two new agent operations they rest on
(`docs/GUEST_AGENT.md`, "Filesystem operations"):

| Path | Agent operations | Shell fallback (only on `is_unsupported_op`) |
| --- | --- | --- |
| `GET /sandboxes/{id}/checkpoints/{name}/archive` | host builds the gzipped tar by walking with `stat`, `list_dir`, `read_file`, `read_link`; response header `x-hv2-archive-skipped` | the `tar` script in the guest |
| the checkpoint reader's freeze | `list_dir /proc`, then `signal_process` (stop) for every number but 1 | `kill -STOP` through `/bin/sh` |
| environment repository unpack, symlink members | `make_symlink` | the `tar` line |

Three consecutive runs, `run1` to `run3`, each over three images, all pass: 7 of 7 cases each. Each
`runN/<image>/` holds `report.json` and `daemon-log.txt`. The daemon logs which path it took (`debug`,
enabled for `processes`, `checkpoint_files`, `env_vars`, `machines::desktop`): the check requires
`using agent op` and never `falling back to shell` for the new images, and the reverse for the old one.
Run 1 was repeated once after the first attempt, which failed in case g because of a driver bug (it
started a second `sleep 4321` when case a had already left one running; fixed in the driver, no product
code involved). The directory holds only the passing runs.

## Setup

- WSL2 Debian under Windows, `/dev/kvm` (`kvm_amd`, nested under Hyper-V).
- Daemon: Linux release `hv2-sandboxd` built from the working tree on repo commit `5145dfeb` plus the
  uncommitted change under test, sha256 `1d4f510d76e0efb3834ce5c2d91761158537b730431a47247f13350975f25602`.
- Kernel `bzImage`, sha256 `07a76776dd0623e19bc70bfd4caaceb6d82c68ecdb6e1c9e74c4d85b70165297`.
- New static agent `hv2-guest-agentd` (`RUSTFLAGS="-C target-feature=+crt-static"`,
  x86_64-unknown-linux-gnu, `file`: static-pie linked), sha256
  `b54842a91e61b0d4f83b803aca1e66760a5ae483811cc3bc5a18a7c3322af404`.
- Images (`tools/guest-image/build.sh`):
  - `shell-free`: `--no-busybox --init <new agent> --agent <new agent>`, sha256
    `a52ece9f5fa701a7215bd3b99f23e0182ad9877c8b19d4c504e5e711022a0557`. No `/bin/sh`.
  - `busybox`: busybox plus the new agent, sha256
    `c9516420f5e522d345080672fc2b817e0318371d163c48dbf8ba07bc3ad1eb28`.
  - `old-image`: busybox plus the agent from before the file operations (no `list_dir` in it), image
    sha256 `2952bbbe585a1b2e780aa3f3c02ffa722c203065f0341717653bb20dec33cca0`.
- Driver `tools/check-agent-ops-kvm.py` sha256 (in each `report.json`, `input_sha256.driver`):
  `98667e5a3f61178bcf0147b69b3a3f75279e4e02c780039cb97ad7dbd658858f`.
- Daemon flags: `--machine-dir <tmp> --git-remote <loopback repo> --memory-mb 512 --cpu-cores 1
  --capacity 6`, `HV2_KERNEL`/`HV2_INITRD` set to kernel and image.
- The test repository now holds two symlinks: `link-to-readme -> README.md` and `link-to-src -> src`.

Hypervisor lines in each daemon log (counted by the driver, `platform_lines`): 22 `Kvm` and 0 `Tcg`
for `shell-free`, 23 and 0 for `busybox` and `old-image`, in every run.

## Results (identical in all three runs)

| Case | shell-free (new agent) | busybox (new agent) | old-image (old agent) |
| --- | --- | --- | --- |
| a-d. processes, machine processes, checkpoint files, Git repository (as before, now with two symlinks in the repository) | ok | ok | ok |
| e. `.../archive` of `/etc/app`: 200, `application/gzip`, tar holds `config.toml` with bytes `port = 9090\n`, `x-hv2-archive-skipped: 0` | ok (was 400) | ok | ok |
| f. `.../archive` of `/srv/app`: tar has `link-to-readme` as a symlink to `README.md` and `link-to-src` as a symlink to `src`, `README.md` and `src/app.py` byte for byte, `src` a directory (62 members including `.git`), skipped header `0`; the checkpoint `files` listing names both symlinks `symlink` | ok | ok | ok |
| g. freeze: log lines for one reader open | 1 `using agent op (freeze)`, 0 shell | 1, 0 | 0, 1 `falling back to shell (freeze)` |
| g. `/proc/<sleeper>/stat` read from the reader through `.../file` | not observed (no sleeper, below) | state `T` (`sleep 4321`) | state `T` |
| code path in the daemon log | 40 `using agent op`, 0 fallback | 44, 0 | 0, 46 `falling back to shell` |

Notes on g. On `busybox` and `old-image` the live sandbox listed the sleeper in state `S` (case a),
and the same pid read through the checkpoint reader's `/file` route showed `T`: the freeze stopped it.
The old agent froze it through the shell, the new agent through `signal_process`. On `shell-free` the
guest holds only its agent (PID 1) and no other process was started (not attempted: the image's only
binary is the agent itself), so only the code path is shown: the log line says the freeze took the agent
path, and the checkpoint was then read successfully.

Case f on the old agent exercised the shell paths for both the unpack (symlinks made by `tar`) and the
archive; on new agents the unpack used `make_symlink` and the archive `read_link` (the log shows no
fallback; the guest-side tool is not otherwise observed).

## Regression: the existing checks, same daemon and kernel (`regression/`)

| Check | Image | Result | `Kvm` / `Tcg` lines |
| --- | --- | --- | --- |
| `tools/check-guest-processes-kvm.py` | old-image | 5 of 5 cases | 3 / 0 |
| `tools/check-checkpoint-files-kvm.py` | old-image | 7 of 7 | 23 / 0 |
| `tools/check-guest-processes-kvm.py` | busybox, new agent | 5 of 5 | 3 / 0 |
| `tools/check-checkpoint-files-kvm.py` | busybox, new agent | 7 of 7 | 23 / 0 |

## NOT VERIFIED

- Stopped state on the shell-free image: no process other than the agent was started there, so
  `T` was observed only on `busybox` and `old-image`. On shell-free only the `using agent op (freeze)`
  log line and a successful read are shown.
- The freeze was observed for one process (`sleep 4321`). No test of a busy, multi-process or
  write-heavy guest, and no check that a checkpoint read under concurrent guest writes is consistent.
- `x-hv2-archive-skipped` was only seen as `0`. No device node, socket, FIFO, hard link, or symlink with
  a non-UTF-8 target was put in a guest, so the non-zero counts and the 413/504 limits are covered only
  by the unit tests, not on a guest.
- Ownership, modification times and modes in the archive were not compared with `tar`'s output; the check
  asserts names, types, link targets and file bytes only. Archive timing was not measured, and no large
  tree (thousands of files) was tried: the agent path makes one connection per entry.
- The unpack's refusal of a member written through an earlier symlink was not run on a guest.
- A symlink in the repository was checked as a symlink with the right target in the archive and as type
  `symlink` in the listing; its target's resolution inside the guest was not checked.
- Old-agent behaviour was checked on one old image only (agent from before the file operations); an
  agent that has `list_dir` but not `read_link` or `signal_process` (a mixed fleet) was not built.
- The desktop screenshot cleanup, `set_guest_clock`, disk mount and template-build steps are untouched
  and not run here, as in the previous README.
- No timing claim. One vCPU, 512 MiB, nested virtualization.
