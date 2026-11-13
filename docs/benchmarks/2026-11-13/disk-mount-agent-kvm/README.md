# Disk mounting through the guest agent's `mount_block`, on KVM

Follow-up to `../agent-ops-archive-kvm/README.md`. Mounting a disk was the last node-initiated guest step
that needed a shell. `disks::mount_swapped` and `disks::mount` (`crates/hv2-sandboxd/src/disks.rs`) ran
`/bin/busybox sh -c` (poll `/sys/block/vda/size`, `sync`, drop caches, `mkdir -p`, `mount -t ext4`). They now
call the new additive agent operation `mount_block` (`docs/GUEST_AGENT.md`, "Filesystem operations"; protocol
version still 3) and keep the shell script only when the host maps the reply to `is_unsupported_op`.

| Path | Agent operation | Shell fallback (only on `is_unsupported_op`) |
| --- | --- | --- |
| `disks::mount` (cold boot with the disk behind `/dev/vda`) | `mount_block {device:/dev/vda, target, fstype:ext4}` | `mkdir -p` and `mount` through `/bin/busybox sh` |
| `disks::mount_swapped` (restored from a template, disk swapped in; `--disk-slot`) | `mount_block` with `expect_sectors` (poll the size, 10 ms, about 2 s) and `drop_caches` (`sync`, then `1` into `/proc/sys/vm/drop_caches`) | the polling shell script |

`tools/check-disk-mount-kvm.py` (new) runs two owned daemons from one binary, one with `--disk-slot` and one
without, against one image, with `RUST_LOG=info,hv2_sandboxd::disks=debug`. On each it creates a 64 MiB disk,
creates a sandbox with it mounted at `/data`, checks `/proc/mounts` says `/dev/vda /data ext4`, checks
`/sys/block/vda/size` is 131072 sectors, writes a 4 MiB random file with `conv=fsync`, ends the sandbox,
attaches the disk to a second sandbox and requires the same SHA-256. With the slot, on images with a shell,
it also requires the guest kernel to have logged a size change on `vda` (a restored guest, not a cold boot),
and on the cold-boot daemon requires none. It then counts the daemon log's `mounting the disk at ...` and
`mounting the swapped disk at ... : using agent op / falling back to shell` lines: exactly 2 of the expected
kind for each path, 0 of the other.

Three consecutive runs, `run1` to `run3`, each over three images, all pass. Each `runN/<image>/` holds
`report.json` and `daemon-log.txt` (both daemons' logs).

## Setup

- WSL2 Debian under Windows, `/dev/kvm` (`kvm_amd`, nested under Hyper-V).
- Daemon: Linux release `hv2-sandboxd` built from the working tree on repo commit `01b8cbbf` plus the
  uncommitted change under test, sha256 `2722f1bcf0b3ac099076249521e87d77bad6ebb1626524a0a3f123d0ae83ea56`.
- Kernel `bzImage`, sha256 `07a76776dd0623e19bc70bfd4caaceb6d82c68ecdb6e1c9e74c4d85b70165297`.
- New static agent `hv2-guest-agentd` (`RUSTFLAGS="-C target-feature=+crt-static"`, x86_64-unknown-linux-gnu,
  `file`: static-pie linked), sha256 `873be8420692813dd4a6c77eccda405f332ec15843bc236eb1080f638f6be49a`.
- Images (`tools/guest-image/build.sh`):
  - `busybox`: busybox plus the new agent, sha256
    `12577fb386d9649fbea4961478c4279812ab594e27fbad028e37c5342b837bd9`. Agent path expected.
  - `old-image`: busybox plus the agent from before the file operations (no `mount_block`), sha256
    `2952bbbe585a1b2e780aa3f3c02ffa722c203065f0341717653bb20dec33cca0`. Shell fallback expected.
  - `shell-free`: `--no-busybox --init <new agent> --agent <new agent>` plus five copies of busybox installed as
    `/bin/cat`, `/bin/dd`, `/bin/sha256sum`, `/bin/grep`, `/bin/ls` (`--extra busybox:NAME`), sha256
    `acb882968a8659ddcb332d28dd7004d21e65078e42dd6ae6fb8c219c19de36ec`. There is no `/bin/sh` and no
    `/bin/busybox`, so the old fallback script cannot succeed there; the check drives the guest by exec `argv`
    (no shell). (A plain shell-free image, sha256 `bf2c22d1...` was also built but has no program to read the
    disk back with and was not used.)
- Driver `tools/check-disk-mount-kvm.py` sha256 `1a2379fdda0091473309e6b1470e14acecfb4cec3596caa68bdcbbcdbc1a739c`
  (also in each `report.json`, `input_sha256.driver`).
- Daemon flags: `--memory-mb 512 --cpu-cores 1 --capacity 8 --disk-dir <tmp>` (+ `--disk-slot` on the second
  daemon), `HV2_KERNEL`/`HV2_INITRD` set to kernel and image.

Hypervisor lines in each run's daemon log: 6 `Kvm` and 0 `Tcg` (two daemons, three lines each), in all nine runs.

## Results (identical in all three runs)

| Case | busybox (new agent) | old-image (old agent) | shell-free (new agent, argv exec) |
| --- | --- | --- | --- |
| a. cold boot with a disk: ext4 at `/data`, 131072 sectors, 4 MiB file by SHA-256, second sandbox reads the same | ok | ok | ok |
| b. restored from the template and given the disk (`--disk-slot`): ext4, real size, same round trip; kernel logged a resize (shell images only) | ok | ok | ok (resize line not checked, see below) |
| `mounting the disk at`: agent / shell | 2 / 0 | 0 / 2 | 2 / 0 |
| `mounting the swapped disk at`: agent / shell | 2 / 0 | 0 / 2 | 2 / 0 |

Before this change a sandbox with a disk could not be created on the shell-free image at all (the mount ran
`/bin/busybox sh`); that was not run here, only inferred from the script.

## Regression: existing disk checks, same daemon and kernel (`regression/`)

| Check | Image | Result |
| --- | --- | --- |
| `tools/check-disk-slot-kvm.py` | busybox, new agent | exit 0, every case ok (a-f and timings) |
| `tools/check-disk-slot-kvm.py` | old-image | exit 0, every case ok |
| `tools/check-block-disk-kvm.py`, runs 1-3 | busybox, new agent | steps 1-3 ok (disk created, mount, write, exclusive attach), then FAILS at the snapshot/checkpoint refusal; log: 1 agent mount line, 0 shell |
| `tools/check-block-disk-kvm.py`, runs 1-3 | old-image | the same failure at the same step; log: 0 agent, 1 shell mount line |

`check-block-disk-kvm.py` fails identically on the old image, so the failure is not from this change: the
check still asserts that checkpointing a sandbox that holds a disk is refused (409/404), but the daemon
answers 201 (checkpointing a disk sandbox has since become supported; `check-disk-checkpoint-kvm.py` covers
it). The `regression/block-disk-*-run*` directories hold its `report.json` (with the error) and daemon
log. The check was then corrected to assert only the snapshot refusal and rerun once per image with the
same daemon and kernel (`regression/block-disk-fixed-{busybox-new-agent,old-image}`): `result: pass`,
all 8 steps, 6 `Kvm` / 0 `Tcg` lines each. That rerun used the check's default log level, so its logs
do not show which mount path was taken. `check-disk-slot-kvm.py` hard-codes `RUST_LOG=info`, so its
logs do not show which path was taken; only its pass is claimed. Kvm / Tcg lines in the block-disk logs:
3 / 0 each.

## Rust sweep

- Windows (rustc 1.98.1): `cargo fmt --check` clean; `cargo clippy --all-targets -p hv2-guest-agent -p hv2-agent
  -p hv2-sandboxd -- -D warnings` clean; `cargo test` for the same three crates, all 15 `test result` lines ok
  (largest binaries: 535 passed; 145 passed, 2 ignored).
- WSL (rustc 1.100.0, `RUSTFLAGS="-D warnings"`): clippy clean; tests pass, including the Linux-only agent
  tests (210 passed, 2 ignored in one binary; 29 passed in the agent binary).

## NOT VERIFIED

- The refusal paths of `mount_block` (bad device, relative target, `..`, `/`) are covered by unit tests only,
  not sent to a guest over the wire. The agent's own mount is exercised only by the cases above.
- `mount_block`'s `DeviceSizeNotSeen` answer and the host's mapping to "the guest never saw its disk change
  size" were not provoked on a guest (the unit test covers the frame round trip and the host mapping to
  `Ok(false)`).
- On `shell-free` the guest kernel's resize log line was not read (no `dmesg`), so that the swap path really
  restored rather than cold-booted is inferred from the daemon log's `mount_swapped` line, not from the
  guest. The size was read as 131072 sectors, not the placeholder's 2048.
- That the drop-caches step is what makes the swapped mount read the real superblock was not tested by
  removing it.
- Only ext4, only `/dev/vda`, a whole disk (no partition) and a single 64 MiB disk were mounted.
- Old-agent behaviour was checked on one old image only.
- Mount failures (a corrupt or unformatted disk) were not tried on a guest; the kernel's error text path is
  unexercised.
- No timing claim. One vCPU, 512 MiB, nested virtualization.
