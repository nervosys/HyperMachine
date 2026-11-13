# Node-initiated guest commands through the agent's file operations, on KVM

`tools/check-agent-ops-kvm.py` runs an owned `hv2-sandboxd` (a template, a `--machine-dir`, and one
`--git-remote` for a repository served over loopback HTTP) and checks, through the API, the four
node paths that used to run a shell command in the guest and now call the guest agent's filesystem
operations first (`docs/GUEST_AGENT.md`, "Filesystem operations"):

| Path | Agent operations | Shell fallback (only on `is_unsupported_op`) |
| --- | --- | --- |
| `GET /sandboxes/{id}/processes`, `GET /machines/{name}/processes` | `list_dir /proc`, `read_file` of `/proc/uptime` and each `/proc/<pid>/{stat,cmdline,status}` | the old `/bin/sh -c` listing |
| environment repository unpack (`write_files`) | host-side tar parse, `make_dir`, `write_file_as`, `set_attr` | `tar`/`chown`/`rm` line |
| `GET /sandboxes/{id}/checkpoints/{name}/files`, `/file` | `stat`, `list_dir`, `read_file` | `LIST` and `SIZE` scripts |
| desktop screenshot cleanup | `remove` | `/bin/rm -f` |

Not switched: checkpoint `.../archive` (the agent has no readlink, so a tar built on the host would
lose symlinks; it stays on the shell and answers 400 on a no-shell guest) and the checkpoint reader's
freeze (`kill -STOP`; the agent can signal only pids it started; on a no-shell guest the freeze is
skipped and the checkpoint is read unfrozen).

Three consecutive runs, `run1` to `run3`, each over three images, all pass: 5 of 5 cases each.
Each directory holds `report.json` and `daemon-log.txt`. The daemon logs which path it took
(`debug`, enabled for the four modules): the check requires `using agent op` and never
`falling back to shell` for the new images, and the reverse for the old one.

## Setup

- WSL2 Debian under Windows, `/dev/kvm` (`kvm_amd`, nested under Hyper-V). Every run's daemon log
  has 19 lines `Detected hypervisor platform: Kvm` and 0 saying `Tcg` (counted by the driver,
  `platform_lines` in `report.json`).
- Daemon: Linux release `hv2-sandboxd` built from the working tree on repo commit `54c0e291`
  plus this change, sha256 `24082e1b9d6b2a8fec1a0eb9048a16663aa7d32298634ad0fae4b1a6ff138e4c`.
- Kernel `bzImage`, sha256 `07a76776dd0623e19bc70bfd4caaceb6d82c68ecdb6e1c9e74c4d85b70165297`.
- New static agent `hv2-guest-agentd` (`-C target-feature=+crt-static`, x86_64-unknown-linux-gnu,
  `file`: static-pie linked), sha256 `1d9418929b4e9b337f2f6870c40a02dbd538b7689f6553f9c80f95a3f5e29e25`.
- Images (`tools/guest-image/build.sh`):
  - `shell-free`: `--no-busybox --init <new agent> --agent <new agent>`, sha256
    `96ebae2c5b29733a3a3a73e5df98924dad60cfa5be4ffc0507ee8208668cb7d0`. No `/bin/sh`.
  - `busybox`: busybox plus the new agent, sha256
    `70a5b03d5d2f95e831338c881273c882daae45a547b3301340d905303f4c6c98`.
  - `old-image`: busybox plus the agent from before the file operations (agent sha256
    `6be0e5cd4f6404d6f9dad5c9c27f0c0b6dfb25c1bcd9ecaad880dfeedd70e6ee`, no `list_dir` in it), image
    sha256 `2952bbbe585a1b2e780aa3f3c02ffa722c203065f0341717653bb20dec33cca0`. This is not
    `tools.cpio.gz` as first planned: that 187 MB Debian rootfs does not boot as a sandbox
    template (its PID 1 exits with status 127 and the guest reboots); it only serves machines.
- Driver `tools/check-agent-ops-kvm.py` sha256 is in each `report.json` (`input_sha256.driver`,
  `27cd0c9794b26456092084172571ec882e88d95308eaa9ec25f036a431aec29d`).
- Daemon flags: `--machine-dir <tmp> --git-remote <loopback repo> --memory-mb 512 --cpu-cores 1
  --capacity 6`, `HV2_KERNEL`/`HV2_INITRD` set to kernel and image.

## Results (identical in all three runs)

| Case | shell-free (new agent) | busybox (new agent) | old-image (old agent) |
| --- | --- | --- | --- |
| a. sandbox processes: PID 1 with ppid 0, every field present, the agent's command line, PIDs sorted, kernel threads only with `?all=true` (and exactly the difference) | ok; PID 1 is the agent as `/init` | ok; plus a `setsid sleep 4321` found with `args == "sleep 4321"`, state `S` | ok (same, via shell) |
| b. machine processes the same way | ok (the code before this change listed through `/bin/sh`; that failure was not run here) | ok | ok |
| c. an environment file `/etc/app/config.toml` listed (type, size 12, mode 644) and read back through the checkpoint; `/` lists `etc`; 404 for a missing path and file, 400 for a file listed, a directory read, a relative path | ok | ok | ok |
| d. an environment Git repository at `/srv/app`: `README.md`/`src`/`.git` listed, dot files after the rest, modes 644/755, every tracked file byte for byte, one owner for the tree, no `hm-repository` archive left in `/tmp` | ok | ok | ok |
| e. `.../archive` of `/etc/app` | 400 naming the missing `/bin/sh` | 200, tar holds `config.toml` | 200 |
| code path in the daemon log | 17 `using agent op`, 0 fallback | 18, 0 | 0, 20 `falling back to shell` |

Process, checkpoint-listing and repository paths on the shell-free image now work; before this
change the process listing and the reader's freeze both ran `/bin/sh`, and the unpack ran `tar`.

## Regression: the existing checks, same daemon and kernel (`regression/`)

| Check | Image | Result | `Kvm` / `Tcg` lines |
| --- | --- | --- | --- |
| `tools/check-guest-processes-kvm.py` | old-image (shell path) | 5 of 5 cases | 3 / 0 |
| `tools/check-checkpoint-files-kvm.py` | old-image (shell path) | 7 of 7 cases | 23 / 0 |
| `tools/check-guest-processes-kvm.py` | busybox, new agent (agent path) | 5 of 5 | 3 / 0 |
| `tools/check-checkpoint-files-kvm.py` | busybox, new agent (agent path) | 7 of 7 | 23 / 0 |

The Rust sweep: `cargo clippy -p hv2-sandboxd --all-targets -D warnings` and
`cargo test -p hv2-sandboxd` pass on Windows (136 tests) and on WSL, rustc 1.100.0 (201 tests).

## NOT VERIFIED

- The desktop screenshot cleanup (`remove_in_guest`) is not run on a guest; it needs the desktop
  stack (`tools/check-machine-desktop-kvm.py` needs `--firmware` and a desktop image, absent here).
  Only its compile and the unchanged `remove` operation's own tests cover it.
- Ownership by a non-root template user: every image here runs as root, so "one owner for the whole
  tree" held as uid 0 for both the agent path and the shell path. The agent path's
  `@template` owner on `make_dir`, `write_file_as` and the final recursive `set_attr` is checked by
  the agent's own tests, not by this check against a template with a `USER`.
- A repository that holds a symlink, hard link or special file takes the shell path by design
  (unit-tested: it is not silently dropped); on a no-shell guest it fails with the no-shell message.
  Not run on a guest. File modification times are not kept by the agent path (the shell path kept
  them); not checked for any consumer.
- Large repositories: the agent path makes one connection per file (and per attribute change), the
  shell path one tar. No timing was taken, and a `.git` of thousands of objects is not tried.
- A checkpoint read on a no-shell guest is not frozen; the guest there runs only what its agent
  started, and the check does not stress concurrent writes.
- Archive on a no-shell guest is a 400, not a working download.
- `set_guest_clock`, disk mount and template-build RUN/USER/COPY/WORKDIR still need a shell and
  were not touched.
- No timing claim. One vCPU, 512 MiB, nested virtualization.
