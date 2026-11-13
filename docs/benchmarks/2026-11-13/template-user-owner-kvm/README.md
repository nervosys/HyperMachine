# Ownership by a non-root template user, and batch failure paths, on KVM

Closes two gaps listed NOT VERIFIED in `../agent-ops-batch-kvm/README.md` and `../build-noshell-kvm/README.md`:

1. Every image so far ran as root, so the `@template` owner on `make_dir`, `write_file` (owner), `make_symlink`
   and the recursive `set_attr` of an environment repository's unpack (`env_vars.rs`, `unpack_by_agent`, now
   one batch), and the owner given by a COPY (`builds.rs`, `copy_by_agent`), was only unit-tested.
2. Failure paths through a real batch: a checkpoint archive that must skip things, a COPY upload that must be
   refused (member through a symlink), a COPY upload with a hard link.

Drivers (new): `tools/check-template-user-owner-kvm.py` (part 1), `tools/check-agent-batch-failures-kvm.py`
(part 2). Both run an owned `hv2-sandboxd` (`--guest-kit` an empty directory, so nothing is pulled; part 1 also
`--git-remote` to a loopback HTTP repository) whose base template is the image under test, with
`RUST_LOG=info,hv2_sandboxd::env_vars=debug,hv2_sandboxd::builds=debug,hv2_sandboxd::checkpoint_files=debug`.

## Part 1: template user `alice` (1000:1000)

The driver builds a template (`fromTemplate: base`) from the steps COPY `/etc/passwd`, COPY `/etc/group`
(both hold `alice:x:1000:1000`), `USER alice`, COPY of a directory tree owned `alice`, COPY of a file owned
`alice`, COPY of a file with no owner, `WORKDIR /srv/work/sub`. It then creates a sandbox from that template with an
environment holding a file (`/etc/app/config.toml`) and a Git repository cloned into `/srv/app` (10 tracked
paths: nested directories, a dot file, an empty file, a 100 KiB file, a 0755 script, three symlinks: one to a
file, one to a directory, one `../README.md`), takes a checkpoint and lists it recursively through
`GET .../checkpoints/one/files` (which reports type, mode, uid, gid), and compares against the repository.

Per image, three consecutive runs (`run1`..`run3`, each `<image>/report.json` + `daemon-log.txt`), all
`success: true`:

| Observation | busybox + new (batch) agent | shell-free + new agent | old image (shell path) |
| --- | --- | --- | --- |
| path taken (log, `/srv/app` unpack) | `using agent op` 1, shell 0, "no batch operation" 0 | same | `falling back to shell (the guest's agent predates file operations)` 1, agent op 0, "no batch operation" 7 |
| entries under `/srv/app` (`.git` included) | 88 (48 files, 37 dirs, 3 symlinks), **all 88 are 1000:1000** | same | same: **all 88 are 1000:1000** |
| `/srv/app` itself | 1000:1000 | 1000:1000 | 1000:1000 |
| tracked file modes | 644, `bin/run.sh` 755, as in the repository; bytes equal for all 7 regular files | same | same |
| directory modes outside `.git` | 755 | 755 | 755 |
| symlinks (`link-to-readme`, `link-to-src`, `src/link-up`) | symlink entries, 1000:1000 (listed mode 777) | same | same |
| `.git` | dirs 755 (33), files 644 (27) / 755 (14), all 1000:1000 | same | same |
| no archive left in `/tmp` | yes | yes | yes |
| `/etc/app` (made for the environment file) and `config.toml` | both 1000:1000, bytes right | same | same |
| COPY `/opt/tree` owned `alice` (dir, 750 subdir, 755 script, 640 file, file symlink, dir symlink) | every entry and the tree's own directory **1000:1000**, links included, modes kept (777 on links) | same | uid **1000, gid 0** on every entry (the shell's `chown -h alice` keeps the group) |
| COPY file owned `alice` (`/srv/work/owned.txt`) | 1000:1000 | 1000:1000 | 1000:**0** |
| COPY with no owner after `USER alice` (`plain.txt`) | 1000:1000 (it defaults to the current user, `builds.rs` `copy`) | 1000:1000 | 1000:**0** |
| `/srv/work` (made as a parent by the first COPY, root, before `WORKDIR`) / `WORKDIR sub` as alice | 0:0 / 1000:1000 | same | 0:0 / uid 1000, gid **0** |
| COPY path (log) | agent op 5, shell 0 | same | agent op 0, shell 5 |

The one agent-versus-shell difference found: **group ownership of COPY results.** The agent sets the owner's
primary group as well (documented in `../build-noshell-kvm/README.md`: "the agent sets the owner's primary group
too (`chown user` kept the group)"); the old image's shell `chown -h alice` leaves gid 0. The repository
unpack gives 1000:1000 on all three images (the shell path reads the archive's owner with `stat -c %u:%g` and
`chown -R`s to it). A command executed in these sandboxes (`busybox id` via exec) reported `uid=0(root)`: exec
does not run as the template user here; only file ownership was exercised, this is recorded, not a claim.

Flake seen once (not in the three counted runs): the first old-image attempt (kept as
`restore-timeout-old-image-run0/`) failed creating the sandbox: `unpacking the repository into /srv/app: making it:
Timeout: the guest agent did not answer request 1 in time`; the daemon log shows the VM restored from the
template's snapshot, 1 vCPU task running 60 s with only 47 exits, then stopped. The next three old-image runs, the
same image and daemon, did not repeat it. Not investigated; the same guest on busybox/shell-free never did it
in 7 runs.

## Part 2: failure paths (`batch-failures/`, busybox + new agent, one run; shell-free for b and c)

| Case | busybox + new agent | shell-free + new agent |
| --- | --- | --- |
| a. archive of `/data`: 300 small files, a FIFO, a character device, a symlink with a non-UTF-8 target (2 bytes 0xFF 0xFE, among the small files so the batch prefetch reads it), a mode-000 file, a good file symlink and directory symlink | **pass**: `x-hv2-archive-skipped: 3` (FIFO, device, bad link); tar has 307 members: every other entry, all 300 small files byte-exact, link targets right, mode-000 file archived with mode 0 and its bytes (root reads it), 755 script; daemon log for the request: `using agent op (archive)` 1, shell 0, "no batch operation" 0 | not run: no shell to build the tree |
| b. COPY upload `esc -> /etc` then `esc/passwd` (and `esc -> sub` then `esc/file.txt`) | **pass**: build `error`, message `the COPY's files holds "opt/tree/esc/passwd" through a symlink it made earlier` (relative: `.../esc/file.txt ...`); control (same link, no member under it) builds, `esc` listed as symlink, nothing in `/etc` | pass, same messages and control |
| b'. the same pair in a repository | Not constructible through the API: Git refuses, recorded in the report: `git add esc/passwd` -> `fatal: pathspec 'esc/passwd' is beyond a symbolic link`; `update-index --cacheinfo` without `--replace` -> `appears as both a file and as a directory`; with `--replace` it succeeds by **removing** the symlink (index then holds only `esc/passwd`) | same |
| c. COPY upload with a hard link (`hl/a.txt`, `hl/b.txt` -> `hl/a.txt`) | **pass after the fix** (before it: FAIL, see "Defect"): shell path as designed (log `falling back to shell`, reason names the hard link), build `ready`, both files hold the bytes, busybox `stat` through exec: `289 2 /opt/hl/a.txt`, `289 2 /opt/hl/b.txt` (same inode, link count 2) | pass, unchanged: build `error`: `this COPY needs the shell ("opt/hl/b.txt" is not a file, directory or symlink (a hard link or special file)): this guest's image has no /bin/sh, ...` |

Case a also asserts, after the second fix, that a file the guest lists but cannot read is counted and left out:
`x-hv2-archive-skipped` and the archive of `/proc/1` answer **200**, skipped **24**, 281 members; `/proc/1/task`
**200**, skipped **12**, 136 members (before the fix both were 502 on `clear_refs`, kept in
`batch-failures-before-fix/`). `/sys/kernel` (recorded only): 200, 4,754 members, skipped 0. No socket was created
(busybox here has no working `nc -lU`), so the 3 skipped in the main tree are a FIFO, a device and the non-UTF-8
link. The public API still gives no way to make a path *vanish* mid-walk (a checkpoint is a frozen restored
guest), so that branch stays unit-tested only.

### Defect (found on the first run, FIXED in the working tree and re-verified below): a COPY upload containing a hard link failed on a shell image

Evidence (`batch-failures/busybox/report.json`, `builds.hl-build`): `tar: can't create hardlink 'opt/hl/b.txt' to
'hl/a.txt'`, script exited 1. Cause: `repack()` (`crates/hv2-sandboxd/src/builds.rs`) rewrites each member's
path to its final place (`opt/hl/b.txt`) but passes a hard link's *linkname* through verbatim (`hl/a.txt`, its
place in the upload), and the shell script then extracts with `tar -xf ... -C /`, which resolves a hard link target
relative to the extraction root, where `hl/a.txt` does not exist. It works only when the upload's layout already
equals the destination (e.g. `src=.`, `dest=/`). Not caused by the batch work: the `append_link(.., &link)` line is from `bf02d746`. The fix (coordinator's, in
`builds.rs` `repack`) runs a hard link's linkname through the same placement as member paths and refuses a hard
link to a file outside the COPY (`... is a hard link to ..., which this COPY leaves out`). Together with the
`build_archive` change (a file the guest lists but cannot read, `AgentError::Script` on `read_file`, is counted in
`x-hv2-archive-skipped` and left out; transport errors still fail), the result is the "after fix" column above.
The refusal message for a hard link to a file outside the COPY was not provoked on a guest (unit test only).

## After the fixes

- Tree: HEAD `dddb40ae` plus the uncommitted `builds.rs` and `checkpoint_files.rs` changes. WSL rustc 1.100.0,
  `RUSTFLAGS="-D warnings"`: `cargo clippy --all-targets -p hv2-sandboxd` exit 0; `cargo test -p hv2-sandboxd`
  exit 0, `test result: ok. 227 passed; 0 failed; 2 ignored`.
- New release daemon sha256 `c2fd5511376587940793062432effe8f68283ed18af59239600f496038d60485` (the sha256 of
  the old one, `8aee91d7a98230a59471565a8bc1f8ae232623a0d73162516b24a54981987e07`, is for the runs in `run1`..`run3`,
  `batch-failures-before-fix/`, `restore-timeout-old-image-run0/`).
- `batch-failures/` (new daemon; driver sha256 `bc8f627a6ef13cf2e8022190f70b9e87e7c72dffd241ed4ec8287364b791ce08`):
  busybox `success: true` (a, b, c; 16 Kvm lines, 0 Tcg), shell-free `success: true` (b, c; 8 Kvm, 0 Tcg).
- `regression-after-fix/{busybox,shell-free,old-image}/`: `tools/check-template-user-owner-kvm.py` once per
  image with the new daemon, all `success: true`, 60 Kvm lines and 0 Tcg each, same observations as the table in
  Part 1 (all 88 `/srv/app` entries 1000:1000 on all three; COPY trees 1000:1000 on busybox and shell-free, uid
  1000 gid 0 on the old image; path-taken log counts unchanged).

## Setup and hashes

- WSL2 Debian, `/dev/kvm` (`kvm_amd`, nested under Hyper-V). Every daemon log: `Detected hypervisor platform: Kvm`
  lines, `Tcg` 0 (part 1: Kvm 60 per run, 60 in each of the 9 counted logs; the old-image failed attempt 3;
  part 2: busybox 13, shell-free 8).
- Daemon: release `hv2-sandboxd`, `/var/tmp/hm-wsl-release/release/hv2-sandboxd`, sha256
  `8aee91d7a98230a59471565a8bc1f8ae232623a0d73162516b24a54981987e07` (the hash `../agent-ops-batch-kvm/README.md`
  gives; not rebuilt; tree at HEAD `dddb40ae` had only test-only changes since).
- Kernel `bzImage` sha256 `07a76776dd0623e19bc70bfd4caaceb6d82c68ecdb6e1c9e74c4d85b70165297`.
- Images: busybox + new agent `busybox.cpio.gz` `85d929acc80d0f0eadfae26e7187f92bcac31c3acf2802a6d46f99dffd3ab485`;
  shell-free `noshell-tools.cpio.gz` `f0e5ac68e1ae5e88ed2c3a14f281a7fbf87168a29e86834d07d19ce44695adbc`;
  old image `initrd.cpio.gz` `2952bbbe585a1b2e780aa3f3c02ffa722c203065f0341717653bb20dec33cca0`.
- Drivers: `tools/check-template-user-owner-kvm.py` sha256
  `02040c212febbb204b7a2a8155b99284ffed78156d5751ef87b2ff6339404ad2`; `tools/check-agent-batch-failures-kvm.py`
  `bc8f627a6ef13cf2e8022190f70b9e87e7c72dffd241ed4ec8287364b791ce08` (the earlier revision `5b6d28b2...` produced
  `batch-failures-before-fix/`); each `report.json`'s `input_sha256` has its driver and daemon hashes.
- Layout: `run{1,2,3}/{busybox,shell-free,old-image}/`, `batch-failures/`, `batch-failures-before-fix/`, `regression-after-fix/`,
  `restore-timeout-old-image-run0/` (a driver run that failed on the flake above; an earlier exploratory run
  of the busybox and shell-free images with a slightly earlier driver revision was discarded).
- `batch-failures-before-fix/busybox` has `success: false` (case c, the hard-link defect); `batch-failures/` is the run after the fixes.

## NOT VERIFIED

- **Exec as the template user** (`busybox id` ran as root); only file
  ownership was checked.
- Group ownership with an owner whose primary group differs from its uid, and `--chown user:group` (goes to the
  shell by design), not tried; `alice` is 1000:1000.
- A vanished file during an archive walk, and a plain permission-denied file (root reads everything): not
  reachable through the public API on a frozen checkpoint; unit-tested only (the unreadable `/proc` files are covered). A unix socket in the archive was not
  tested (no way to create one on this busybox).
- A repository whose tar writes through an earlier symlink: shown unconstructible via Git; the refusal itself
  is verified through COPY, which shares `members_of`, not through the repository route.
- The old-image snapshot-restore timeout seen once: cause unknown, one occurrence in 4 old-image attempts.
- No timing claim anywhere.
