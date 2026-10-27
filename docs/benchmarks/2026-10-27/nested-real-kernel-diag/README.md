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

## A baseline on the host, and a 240 s run that tested a prediction

**On the WSL2 host directly** (no nesting; `host-direct/*.out`, run by a shell
loop that is not kept): all three variants reached the busybox userspace in
about 1.4 s of guest time, after about **37,000** userspace exits, in under
10 s of wall time (`lpj` and the 96 MiB guest made no difference there either).

**The prediction.** If the nested guest were slow only because each exit costs
about 4 ms (60 s for about 15,000 exits), it would need those same 37,000 exits
and so reach userspace at about 150 s. `nested_diag3.py` ran the baseline with a
240 s window (`long/long256.out`). Result: **243 s of wall time, 16,962 exits,
35.9 s of guest time, no userspace** (the console ended at 35.9 s, at the
`perf` PMU lines). The prediction **failed**: the exit count barely moved
between 60 s (about 15,000) and 243 s (16,962), so the guest is not limited by
how fast exits are handled.

What this does and does not show: the nested guest is **not exit-bound**; it
makes few userspace exits and little progress, and its clock advances at
roughly 0.15 guest-seconds per wall-second. **Guess, not tested:** it spends
most of the time halted, waiting for timer interrupts that arrive slowly, since
a halt that the in-kernel irqchip handles is not counted as a userspace exit.
That would fit the failed PIT calibration too, but no run here measured it.

NOT VERIFIED: why the guest is slow; any fix. A test of the guess: count halts in
the L1 kernel (`kvm` tracepoints or `/sys/kernel/debug/kvm/` statistics) during
a nested boot, or give the L2 guest a lapic timer by another route.

## The sandbox kernel's own KVM counters during a nested boot

`nested_diag5.py`, one 60 s run (`kvm-stats/`): at 20 s and 50 s into the nested boot
the sandbox kernel's per-VM debugfs counters for the probe's VM
(`/sys/kernel/debug/kvm/<pid>-<fd>/`; they exist only while the VM is alive, so a first
attempt that read them after the probe had stopped got zeros and is not counted):

| Counter | 20 s | 50 s |
|---|---|---|
| exits (all, in the sandbox kernel) | 31,152 | 86,444 |
| `halt_exits`, `halt_wakeup` | 0, 0 | 0, 0 |
| `irq_exits` (external interrupts hitting the guest) | 5,320 | 30,394 |
| `io_exits` (port I/O, sent to userspace) | 7,695 | 16,539 |
| `irq_injections` (interrupts delivered into the guest) | 0 | 10,763 |
| `pf_fixed` (nested page faults fixed) | 18,096 | 18,442 |

What this refutes, for this one run: **the guest is not halted** (zero halt exits, so
the "waiting for slow timer ticks" guess in the section above is wrong); **it is not
bound by first-touch memory faults** (`pf_fixed` is flat after the first 20 s, so
the guest's memory was touched early and cheaply). What it shows: the guest is
running; the sandbox kernel took about 830 external-interrupt exits a second from the
layer below it, delivered about 360 interrupts a second into the guest, and handled
about 290 port-I/O exits a second. **Nothing here says where the time goes.** A guess,
not tested: the sandbox's own vCPU is getting a small share of a core from the layer
below, so the guest is slow because its host is slow. A cheap test would be a
busy loop's wall time inside the sandbox against the same loop on the WSL2 host.

NOT VERIFIED: the cause, and any fix.
