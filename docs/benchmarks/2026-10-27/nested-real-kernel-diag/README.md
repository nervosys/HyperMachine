# A real kernel in a sandbox's nested VM: one diagnostic run (2026-10-27)

Follow-up to the 2026-10-24 "tried, and did not work" note in
[docker-kvm-sandbox-kvm](../../2026-10-24/docker-kvm-sandbox-kvm/README.md).
`nested_diag.py` (here, run as root in WSL2; it is a one-off driver and not a
check) started `hv2-sandboxd` with the Docker+KVM kernel and tools image,
created one sandbox, and ran the static `linux_boot_probe` inside it against a
6.x bzImage and a busybox initramfs, 256 MiB, `HV2_SETTLE_SECS=60`, with
`RUST_LOG=info`. One run.

What was seen (`probe.out`, `daemon.log`):

- The nested VM was created (`Provisioned VM ... on the Kvm backend`) and the
  kernel started. **Creating the in-kernel PIT did not fail**: there is no
  `Failed to create PIT` warning in the output. `kvm.rs` only warns when it
  fails, so this was worth checking.
- In 60 s of wall time the probe counted **22,031 userspace exits** (about 367 a
  second), and the nested console reached **14.2 s of guest time** at
  `kprobes: kprobe jump-optimization is enabled`, then printed nothing more
  until the probe stopped it. The guest clock advanced, so it was not frozen.
- `tsc: Unable to calibrate against PIT` at 0.8 s of guest time, and
  `PCI: Fatal: No config space access function found` at 14.2 s (the probe's
  legacy PC set has no PCI host, so the second line is not a stall).
- The same kernel, probe and initramfs reach userspace in 0.6 s of wall time on
  the WSL2 host directly (2026-10-24).

What it does and does not show: the nested guest runs far slower than the same
guest on the host (roughly 0.24 s of guest time per second of wall time here, 0.13
in the earlier 270 s run), mostly in guest mode and not in userspace exits. The
PIT-creation theory is ruled out. **Guesses, not tested:** first-touch NPT faults
on the guest's memory being expensive through two levels of nesting under
Hyper-V; a delay loop with a poor `lpj` after the failed TSC calibration. Neither
is shown by this run.

NOT VERIFIED: why it is slow; whether it would ever reach userspace given
enough time (the longest wait was 270 s); a smaller memory size, a prefaulted
guest memory, or `lpj=` on the kernel command line as ways to tell the two guesses
apart; an Intel host.

## Three variants, one sandbox (same day)

`nested_diag2.py` ran the probe three times in one sandbox, 60 s each,
`variants/*.out`:

| Variant | Guest time the console reached | Userspace exits | Reached userspace |
|---|---|---|---|
| 256 MiB, baseline again | 4.9 s | 15,042 | no |
| 96 MiB | 7.0 s | 15,296 | no |
| 256 MiB, `lpj=4000000 tsc=reliable` | 6.2 s | 15,976 | no |

- None of the three was clearly faster, and none reached userspace. The
  guest-time-per-wall-second ratio was about 0.08 to 0.12 in all three.
- The baseline itself differs a lot between runs: 14.2 s of guest time above,
  4.9 s here. One run each, so the spread is as large as the differences
  between variants, and **no conclusion about either guess follows**. A smaller
  guest and a fixed `lpj` did not visibly help, which is weak evidence
  against both guesses and not a refutation of either.
- Userspace exits stayed near 15,000 a minute in each variant.

NOT VERIFIED: still the cause. Next things that would tell more: the same
three variants on the WSL2 host directly (one level of nesting, for the
exit-cost baseline), `perf kvm` or `trace-cmd` in the sandbox, a longer wait
than 270 s, a run with host load measured.
