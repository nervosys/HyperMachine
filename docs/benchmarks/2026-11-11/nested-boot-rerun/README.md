# Nested-guest boot re-run on the current build (2026-11-11)

Re-run of [the 2026-10-27 diagnostic](../../2026-10-27/nested-real-kernel-diag/README.md),
on today's `master` (after the WHPX I/O-APIC interrupt-delivery fix, unrelated to this
KVM-only path, and the `advertise_hypervisor` fix from 2026-10-27 itself). Purpose:
keep a current baseline (guest time reached in 60 s, userspace exits), per
`docs/BOAT_COMPLETION_PLAN.md` section 2's "re-run" item, which had not been done
since the fixtures were lost.

## Setup

WSL2 (6.18.33.2-microsoft-standard-WSL2), `/dev/kvm` present (`kvm_amd` reloaded
with `modprobe` -- it does not survive a `wsl --shutdown` or a fresh WSL session;
needed again before any KVM run).

Rebuilt from scratch, since the prior fixtures were gone:

- Outer (sandbox) kernel: `tools/guest-image/build-kernel.sh --docker --kvm`, same
  6.6.52 source as before -> `/var/tmp/hm-docker/bzImage` (14 MiB).
- Outer tools image: `tools/guest-image/build-tools-image.sh` (debootstrap +
  apt, Ubuntu 24.04 noble, the same package list as the script's default) ->
  `/var/tmp/hm-docker/tools.cpio.gz` (179 MiB, 654 MiB rootfs).
- `hv2-sandboxd` and the `linux_boot_probe` example, release build, in WSL
  (`cargo build --release -p hv2-sandboxd`, `--example linux_boot_probe -p hv2-core`;
  needed `protoc` installed first -- `apt-get install protobuf-compiler` --
  and a login shell for `cargo` to be on `PATH` under WSL root).
- Inner (nested-guest) kernel and initramfs: reused the plain (no `--docker
  --kvm`) fixtures already built 2026-11-10 for the WHPX boot-test work
  (`/var/tmp/hm-kernel-build/bzImage`, `initrd.cpio.gz`) -- the same kind of
  fixture the probe used in the 2026-10-27 run.

Driver: a new script (not committed; a one-off, like `nested_diag*.py` before
it), adapted from `nested_diag8.py` -- starts `hv2-sandboxd`, creates a `base`
sandbox, uploads the probe binary and the inner kernel/initramfs into it over
`exec` (base64 in 36000-byte chunks -- a larger chunk size, tried first, failed
with a non-JSON response from the exec endpoint; the original script's chunk
size is a real limit, not just caution), then runs the probe inside the
sandbox: `HV2_MEMORY_MIB=256 HV2_SETTLE_SECS=60`, cmdline `console=ttyS0,115200
earlyprintk=serial,ttyS0,115200 panic=1`, same as the 2026-10-27 "fixed256"
variant.

One false start: the first attempt ran with the `Tcg` backend instead of `Kvm`
(the outer VM took a single exception and stopped in under a second, 0 exits) --
`/dev/kvm` had been dropped by an earlier `wsl --shutdown` recovery (see below)
and was not reloaded before that run. Reloading `kvm_amd` and re-running fixed it;
`daemon.log` confirms `Detected hypervisor platform: Kvm` this time, and the
sandbox's own `uname -r` (6.6.52) matches the outer kernel just built.

One other recovery mid-session: WSL itself went unresponsive (`wsl.exe -- echo
hello` returned exit 1 with no output) while the earlier `hv2-sandboxd` build was
running -- the same "WSL goes read-only mid-run" failure mode noted before. A
`wsl.exe --shutdown` and restart recovered it; the half-finished build was
re-run, and the already-built kernel and tools-image fixtures on disk survived
the restart intact.

## Result

| | 2026-10-27 baseline (`fixed256`, after the `advertise_hypervisor` fix) | 2026-11-11 (this run) |
|---|---|---|
| Wall time | 60 s | 60.4 s |
| Guest time reached | 17.7 s (the xstate lines) | 19.3 s (the xstate lines) |
| Userspace exits (probe's own count) | 15,643 | 15,641 |
| Reached userspace | no | no |
| Hypervisor detected by the guest | KVM, kvm-clock, real TSC frequency | KVM, kvm-clock, real TSC frequency (`tsc: Detected 4391.680 MHz processor`) |

Read plainly: this matches the 2026-10-27 baseline closely -- guest time and
exit count are both within the run-to-run spread already documented there (that
README itself shows baselines from 4.9 s to 17.7 s of guest time across single
runs). **The nested-guest slowness is unchanged on the current build.** No
regression, no improvement, from whatever changed in `master` since
2026-10-27 (which did not touch this code path -- the WHPX I/O-APIC fix is
WHPX/HVF-only and this run is entirely KVM-in-KVM).

Full console output: [`probe.out`](probe.out) (`probe.err` was empty). The
daemon's own log (`daemon.log`, confirming `Detected hypervisor platform: Kvm`
and the false start on `Tcg` before `/dev/kvm` was reloaded) is not committed
-- `*.log` is gitignored, same as the 2026-10-27 directory before it.

NOT VERIFIED (same as before, still open): why the nested guest is slow, any
fix, whether it would ever reach userspace given much longer than 60 s (the
2026-10-27 README's own 243 s run did not either), a run on an Intel host, a
run on a non-Hyper-V-nested host.
