# Memory-light variant and KVM counters during a nested boot (2026-11-11)

Follow-up to [the 2026-11-11 re-run](../nested-boot-rerun/README.md), itself a
re-run of [the 2026-10-27 diagnostic](../../2026-10-27/nested-real-kernel-diag/README.md).
Covers two of the still-open items in `docs/BOAT_COMPLETION_PLAN.md` section 2:
the memory-light test (is the slowness proportional to guest memory, pointing
at first-touch NPT faults) and counting where time goes (the sandbox kernel's
own per-VM KVM debugfs counters, sampled at 20 s and 50 s, same method as
`nested_diag5.py` on 2026-10-27).

Setup: same rebuilt fixtures as the prior re-run (`/var/tmp/hm-docker/bzImage`
+ `tools.cpio.gz` for the outer sandbox, `/var/tmp/hm-kernel-build/bzImage` +
`initrd.cpio.gz` as the inner nested guest, release `hv2-sandboxd` and
`linux_boot_probe`), one sandbox, two variants run back to back in it: the
256 MiB baseline repeated, then a 96 MiB memory-light variant. One run each,
not controlled for host load, same caveat as every run in this series.

## Guest progress: the memory-light variant did not boot further

| | 256 MiB (baseline) | 96 MiB (memory-light) |
|---|---|---|
| Wall time | 60.2 s | 60.2 s |
| Guest time reached | **25.8 s** | **16.6 s** |
| Userspace exits (probe's own count) | 15,641 | 15,903 |
| Reached userspace | no | no |

The smaller guest reached *less* guest time in the same wall time, not more --
the opposite of what "less memory, fewer first-touch NPT faults, faster boot"
would predict. This one run does not refute that guess either: the
2026-10-27 three-variant table showed the same kind of reversal in the other
direction (96 MiB reached 7.0 s against a 256 MiB baseline's 4.9 s and 14.2 s
in different runs), and that README's own conclusion stands here too --
**the run-to-run spread is as large as any difference between variants, so no
conclusion about the memory-light guess follows from this either.** Two runs
across two sessions now, in both directions; a controlled A/B (alternating
variants, several runs each, host load held constant) would be needed to say
anything stronger, and is not done here.

## KVM debugfs counters, sampled at 20 s and 50 s into each boot

| Counter | 256 MiB @20s | 256 MiB @50s | 96 MiB @20s | 96 MiB @50s | 2026-10-27 @20s | 2026-10-27 @50s |
|---|---|---|---|---|---|---|
| `exits` | 41,369 | 103,198 | 23,604 | 63,837 | 31,152 | 86,444 |
| `halt_exits` | 0 | 0 | 0 | 0 | 0 | 0 |
| `irq_exits` | 6,821 | 35,684 | 3,105 | 19,024 | 5,320 | 30,394 |
| `io_exits` | 13,411 | 15,069 | 6,445 | 16,353 | 7,695 | 16,539 |
| `irq_injections` | 1,531 | 17,095 | 0 | 6,607 | 0 | 10,763 |
| `pf_fixed` | 18,376 | 18,378 | 13,981 | 14,798 | 18,096 | 18,442 |
| `tlb_flush` | 8 | 9 | n/a | n/a | n/a | n/a |

What this confirms, consistent with 2026-10-27: **the guest is still not
halted** (`halt_exits` = 0 throughout, both variants, same as before -- the
"waiting on slow timer ticks" guess stays ruled out). `pf_fixed` is still
roughly flat between the 20 s and 50 s samples for the 256 MiB variant (18,376
-> 18,378, +2, matching 2026-10-27's own 18,096 -> 18,442), so the guest's
memory is still touched early and cheaply, not progressively -- **still not
bound by first-touch memory faults**, for the baseline. The 96 MiB variant's
`pf_fixed` moved a bit more in the same window (13,981 -> 14,798, +817) and
started from a lower base (less memory to map, as expected), which is a
difference from the baseline's near-zero movement but a small one, and not
itself the smoking gun the memory-light guess hoped for -- `pf_fixed` is not
what's keeping the 96 MiB variant behind on guest-time progress (it is
*ahead* of the baseline's count at 20s on a per-MiB basis, not behind).

The 256 MiB exit and injection counts here are a bit higher than the 2026-10-27
run at the same timestamps (e.g. 103,198 vs 86,444 total exits at 50 s), in the
same direction as this run's higher guest-time progress (25.8 s vs 17.7-19.3 s)
-- consistent with "more exits serviced, more guest progress," which is the
same relationship the 2026-10-27 README already noted, not a new finding.

## What this does and does not show

Confirms, again: the guest is not halted, and (for the 256 MiB case) not
memory-fault-bound. Does not show: why either variant is an order of magnitude
slower than the un-nested host (still roughly 10 to 25 s of guest time in 60 s
of wall time, against about 1.4 s on the host directly, per 2026-10-27). The
memory-light guess is not supported by either session's data and is not
refuted by either alone; a proper test needs more runs than this series has
done, with host load controlled.

NOT VERIFIED: the cause, still. `perf kvm` / `trace-cmd` inside the sandbox
(the one item left from the 2026-10-27 "where to look next" list that no run
in this series has done); a non-nested-Hyper-V host for comparison.
