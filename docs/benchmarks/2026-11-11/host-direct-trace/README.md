# A host-direct baseline for the per-exit latency trace (2026-11-11)

Closes the "NOT VERIFIED: ... a non-nested host's equivalent per-exit-reason
latency for comparison" item from
[the nested kvm_exit trace README](../nested-kvm-trace/README.md).

**What this is, exactly, and what it is not.** This runs the same
`linux_boot_probe`, the same inner kernel and initramfs, and the same
`trace-cmd record -e kvm:*` method directly in WSL2 -- against WSL2's own
kernel's `/dev/kvm`, with no `hv2-sandboxd` and no sandboxed VM in between.
That removes *one* of the *two* layers of nesting the other run in this
series had (Windows/Hyper-V running WSL2, then WSL2's KVM running a sandbox
VM, then that sandbox's KVM running the inner guest -- three levels deep).
This run has WSL2's KVM running the inner guest directly -- two levels deep,
not zero. **It is not bare-metal, non-virtualized hardware** (not available
here); the comparison below is "one less layer of nesting," not "no nesting
at all."

## The guest actually reached userspace this time

Where the sandboxed run never reached userspace in 60 s, this one did, in
1.12 s of guest time (`Run /init as init process` at 1.124012 s, `=== HV2
READY ===` and a working BusyBox shell immediately after) -- then sat idle
at the interactive shell for the remainder of the 60 s `HV2_SETTLE_SECS`
window. Full console: [`probe.out`](probe.out).

## Per-exit latency, boot phase only

`EXIT_HLT` dominates the raw per-reason table (99% of total time) because
each "exit" duration for a halted, idle CPU is however long it waited for
the next timer tick, not a cost of handling the exit -- not comparable to
anything in the nested run, which never got to idle. Excluding `EXIT_HLT`
(full table: [`exit_latency.txt`](exit_latency.txt)), the boot-relevant
reasons:

| Reason | Count | Mean (µs) | Median (µs) | Max (µs) |
|---|---|---|---|---|
| `EXIT_IOIO` | 36,681 | 12.75 | 6.00 | 11,526 |
| `EXIT_NPF` | 1,106 | 79.96 | 3.00 | 11,282 |
| `EXIT_INTR` | 366 | 45.24 | 16.00 | 5,030 |
| `EXIT_VINTR` | 232 | 5.11 | 4.00 | 63 |

## The comparison: every reason got dramatically slower under the extra layer

| Reason | Host-direct mean (µs) | Nested mean (µs), from the other run | Nested is slower by |
|---|---|---|---|
| `EXIT_IOIO` | 12.75 | 466.6 | **~37x** |
| `EXIT_VINTR` | 5.11 | 139.1 | **~27x** |
| `EXIT_INTR` | 45.24 | 317.6 | **~7x** |
| `EXIT_NPF` | 79.96 | 475.7 | **~6x** |

Read plainly: **the extra layer of nesting adds overhead to every exit
reason, not just the ones that happened to have the most exits or the most
total time in the nested trace.** This is a cleaner answer than either the
count-only or the latency-only breakdown of the nested run alone could give,
because it isolates the *effect of nesting itself* from the *mix of reasons*
a given boot happens to produce. `EXIT_IOIO` and `EXIT_VINTR` -- the two
reasons with the *lowest* per-exit cost host-direct -- are hit hardest in
relative terms (37x and 27x) by the extra nesting layer, even though
`EXIT_NPF` and `EXIT_INTR` still cost more in absolute nested terms because
there are far more of them in a boot that never finishes.

The boot-phase exit *volume* also differs by reason in an interesting way:
host-direct's boot leans heavily on `EXIT_IOIO` (36,681 of it, mostly cheap
per-exit) where the nested boot's `EXIT_NPF` and `EXIT_INTR` counts were
much larger relative to `EXIT_IOIO`. Both guests boot the same kernel and
initramfs, so this difference in exit *mix*, not just *cost*, between one
and two layers of nesting is itself something this comparison surfaces but
does not explain.

## What this does and does not show

Shows: nesting itself, not a specific exit reason, is the dominant source of
the slowdown measured throughout this series -- consistent with (and a
stronger version of) the "about 10 times slower" headline number from
2026-10-27, now broken down by mechanism and reason. Does not show: the
cause of *why* an extra KVM-in-KVM layer multiplies per-exit cost 6-37x (that
would need something below the guest, e.g. tracing the sandbox kernel's own
VM-exit path on real hardware without Hyper-V underneath it, which is not
available here); whether literal bare-metal (zero nesting layers) would show
further improvement over this two-layer baseline, or whether two layers is
already close to the floor.

NOT VERIFIED: the mechanism; a true zero-nesting-layer baseline (needs
hardware not available here); whether this ratio holds on Intel VMX
(this host and WSL2 are both AMD SVM) or on HVF.
