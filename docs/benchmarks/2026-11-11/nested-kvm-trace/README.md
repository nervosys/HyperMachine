# A real exit-reason trace of the nested boot, with `trace-cmd` (2026-11-11)

Follow-up to [the memory-light and KVM-counter run](../nested-memlight-and-counters/README.md),
which only had the debugfs counters (aggregate `exits`, `irq_exits`, `io_exits`,
`pf_fixed`, no breakdown by *reason*). This is the "count where time goes" item
from `docs/BOAT_COMPLETION_PLAN.md` section 2 that the counter-only run left
undone: `trace-cmd record -e kvm:*` during a nested boot, giving the actual
per-exit `kvm_exit` reason, not just a total.

## What it took to get here

The sandbox's own guest kernel (`hv2-guest.defconfig`) had no tracing support
at all -- `CONFIG_FTRACE`, `CONFIG_TRACING` and `CONFIG_PERF_EVENTS` were all
off, so `/sys/kernel/tracing` did not exist and `trace-cmd` had nothing to
read. Added a new `--trace` flag to `tools/guest-image/build-kernel.sh` and a
`trace.fragment` (ftrace, tracepoints, perf_events, full kallsyms), rebuilt the
outer sandbox kernel with `--docker --kvm --trace`, and rebuilt the tools image
with `trace-cmd` added to its package list. Both are reusable additions, not
one-off hacks, since nothing in this series had needed guest-side tracing
before. The inner (nested-guest) kernel and initramfs are the same ones reused
across this whole series (`/var/tmp/hm-kernel-build/bzImage`, `initrd.cpio.gz`).

Driver: starts `hv2-sandboxd` with the trace-enabled outer kernel/tools image,
creates a `base` sandbox, uploads the probe and inner fixtures the same way as
the prior runs, mounts `tracefs`, confirms `kvm:*` events exist (debugfs
already had `/dev/kvm`'s counters, but tracepoints are a separate kernel
feature and needed `CONFIG_TRACING` specifically), then runs `trace-cmd record
-e kvm:kvm_exit -e kvm:kvm_entry -e kvm:kvm_inj_virq -e kvm:kvm_apic -e
kvm:kvm_msr -e kvm:kvm_pio -o /tmp/trace.dat -- sleep 62` alongside the usual
60 s `HV2_SETTLE_SECS=60` probe boot (256 MiB, same cmdline as every run in
this series), then `trace-cmd report` inside the sandbox to avoid pulling the
3.6 MB `trace.dat` back over the slow base64-over-exec channel -- only
summarized counts and short head/tail samples were pulled back.

One false start, corrected: the first pass extracted the wrong `awk` field for
the `kvm_exit` reason (the literal word "reason" is itself a field, with the
actual value -- `EXIT_NPF`, `EXIT_CPUID`, etc. -- one field further over) and
so reported an uninformative word-count instead of a reason breakdown. Fixed
and re-run.

## The `kvm_exit` reason breakdown

116,637 `kvm_exit` events in the ~62 s trace window (the probe's own boot
reached 22.2 s of guest time in that window, matching the series' usual
range):

| Reason | Count | Share |
|---|---|---|
| `EXIT_INTR` (exit on an external interrupt) | 45,172 | 38.7% |
| `EXIT_NPF` (nested page fault) | 37,001 | 31.7% |
| `EXIT_VINTR` (virtual-interrupt window: the guest unmasked interrupts KVM was waiting to inject) | 18,089 | 15.5% |
| `EXIT_IOIO` (port I/O) | 15,927 | 13.7% |
| `EXIT_MSR` | 228 | 0.2% |
| `EXIT_CPUID` | 185 | 0.2% |
| everything else (`CR0`/`CR4`/`DR0`/`XSETBV`/`WBINVD`) | 32 | <0.1% |

All 116,637 were on `vcpu-0` -- the inner guest the probe boots is single-vCPU
by its own defaults (`HV2_MEMORY_MIB` was set; no vCPU-count variable was),
not a new finding about the multi-vCPU WHPX work elsewhere in this plan,
which is a different backend and a different guest.

Supporting counts from the same trace: `kvm_inj_virq` 18,303 and `kvm_apic`
18,221 (both close to `EXIT_VINTR`'s 18,089 -- consistent, since a virtual-
interrupt-window exit is how KVM gets a chance to inject the interrupt it
already queued). `kvm_pio` 15,927 matches `EXIT_IOIO` exactly (the same event
counted two ways by `trace-cmd`'s own event list vs. the exit-reason field).
`kvm_msr` 228 matches `EXIT_MSR` exactly.

## What this says about the two standing guesses

**Interrupt handling, not nested paging, is the single largest cost.**
`EXIT_INTR` + `EXIT_VINTR` together are 63,261 exits, 54.2% of all exits --
more than `EXIT_NPF` alone (31.7%). Neither guess from the 2026-10-27 README
("first-touch NPT faults" vs. "slow timer interrupts") is simply right or
simply wrong: nested page faults are real and not negligible (31.7% of
exits), but interrupt delivery and the virtual-interrupt-window mechanism
that AMD SVM uses to re-inject a pending interrupt cost more exits overall.
This is the first evidence in this series that actually weighs the two
guesses against each other rather than ruling one out in isolation.

**`EXIT_NPF` (37,001) is roughly double `pf_fixed` (18,378 at the matching
50 s mark in the counter-only run).** Read plainly: not every nested-page-fault
VM-exit shows up as a "fixed" page fault in the debugfs counter -- some
`EXIT_NPF`s are MMIO traps or other NPF-mechanism uses that KVM handles
without counting them as `pf_fixed`. This means the debugfs counter alone
(used in the prior memory-light run to conclude "not memory-fault-bound")
was undercounting the true volume of NPF-related exits by about half. That
run's conclusion about `pf_fixed` staying flat over time is still true and
still means the guest's memory was touched early, not progressively -- but
the NPF exit *rate*, not just the "newly fixed" rate, is a real and
sizeable contributor that the counter-only picture understated.

**`EXIT_IOIO` (15,927) matches the debugfs `io_exits` counter closely**
(15,069-16,539 across the memory-light run's two variants) -- the two
measurement methods agree here, as a sanity check on both.

## What this does not show

Still not shown: *why* interrupt delivery and NPF handling together cost an
order of magnitude more wall-clock time per guest-second here than on the
un-nested host (2026-10-27: about 1.4 s of guest time in under 10 s of wall
time, with about 37,000 exits -- a higher exit count reached in far less wall
time than this nested run's 116,637 exits in 62 s). The per-exit *cost*
(wall-clock time each `EXIT_INTR`/`EXIT_VINTR`/`EXIT_NPF` actually takes under
one more layer of nesting) was not measured here -- only the exit *counts* and
*reasons*. That would need the trace's own per-event timestamps analyzed for
inter-exit latency, which this run did not do (the raw `trace.dat` was not
pulled back for that level of analysis).

NOT VERIFIED: per-exit latency; whether a non-nested-Hyper-V host shows the
same reason mix at a much lower per-exit cost (needs hardware not available
here); any fix.

Raw samples: [`report_head.txt`](report_head.txt) and
[`report_tail.txt`](report_tail.txt) (80 and 40 lines of the 285,954-line
report). `daemon.log` is not committed (`*.log` is gitignored, same as every
prior directory in this series).

## Per-exit latency: by total time, nested paging dominates, not interrupts

The count-based breakdown above answers "how many of each exit." It does not
answer "which one costs the most wall-clock time" -- a reason with fewer
exits but a higher cost per exit can still dominate the total. This section
does that measurement, the one the first pass of this README explicitly
left undone.

Pulled the full `trace.dat` back this time (not just summarized counts) --
needed two fixes to do it: the sandbox exec endpoint's response is capped at
exactly 1,048,576 bytes (1 MiB; a first attempt at one `base64 -w0` pull came
back silently truncated there), so it was pulled in 700000-byte `dd` blocks
instead; and each block's base64 text has to be decoded on its own and the
*decoded bytes* concatenated, not the base64 *text* concatenated -- a block
whose byte length isn't a multiple of 3 ends in `=` padding, and joining
base64 strings with padding in the middle corrupts anything decoded from the
joined text (a real failure hit and fixed here: the first corrected attempt
still came back with a 2-byte sha256 mismatch from exactly this). The final
pull verified by sha256 against the sandbox's own `sha256sum`.

A small script (`trace-cmd report`, then pairing each `kvm_exit` timestamp
with the next `kvm_entry` timestamp -- the gap between them is time spent
outside the guest handling that exit; grouped by the exit's `reason` field)
gives, for a run with 70,467 exits in this window:

| Reason | Count | Mean (µs) | Median (µs) | Max (µs) | Total (µs) | Share of total time |
|---|---|---|---|---|---|---|
| `EXIT_NPF` | 25,966 | 475.7 | 203.0 | 59,629 | 12,351,574 | **44.8%** |
| `EXIT_IOIO` | 15,969 | 466.6 | 294.0 | 58,209 | 7,450,843 | 27.0% |
| `EXIT_INTR` | 21,145 | 317.6 | 146.0 | 32,856 | 6,715,042 | 24.4% |
| `EXIT_VINTR` | 6,938 | 139.1 | 50.0 | 10,340 | 965,104 | 3.5% |
| everything else | 445 | -- | -- | 4,773 | 71,696 | 0.3% |

Total exit-handling time accounted for: 27.6 s, out of a run whose guest
clock reached only 12.6 s in about 66 s of wall time -- more than a full
guest-second of exit-handling overhead for every guest-second of progress.

**This changes the picture from the count-only breakdown.** By count,
interrupt-related exits (`EXIT_INTR` + `EXIT_VINTR`) outnumbered `EXIT_NPF`.
By total time, `EXIT_NPF` alone (44.8%) outweighs `EXIT_INTR` + `EXIT_VINTR`
combined (27.9%) -- each nested-page-fault exit costs more on average (475.7
µs mean) than each interrupt exit (317.6 µs mean), enough to flip which one
actually dominates wall-clock cost. Read plainly: **nested paging is the
single largest cost by total time, which does support the first-touch-NPT-
fault guess from 2026-10-27 more than the count-based trace alone suggested**
-- though "first-touch" specifically (faults only on newly-touched pages,
cheap after the first touch) is not quite what a 475 µs mean and a 59.6 ms
max look like; those numbers look more like faults that are each individually
expensive under nesting, not merely numerous early on. `pf_fixed` staying
flat in the memory-light run's debugfs counters (true, and still means the
*newly-fixed* count stops growing) is a different measurement from this one
(the *cost per occurrence* of all NPF exits, fixed or not) and the two are
not in tension, just answering different questions.

The heavy tail is itself notable: `EXIT_NPF` and `EXIT_IOIO` each have a
single occurrence taking roughly 58-60 ms, two to three orders of magnitude
above their own medians (203 µs and 294 µs). A handful of exits taking tens
of milliseconds, on top of a few hundred microseconds typical, looks like
occasional host-scheduling delay reaching the exit-handling path -- the same
shape of problem (something that should be fast getting queued behind
unrelated work) as the WHPX I/O-APIC interrupt-delivery fix found and fixed
elsewhere in this plan, though that fix is WHPX-specific and this is an
entirely different (KVM-in-KVM) code path; nothing here establishes that the
same mechanism is at fault.

NOT VERIFIED: the cause of the heavy tail; whether `EXIT_NPF`'s cost is
inherent to this host's nested-NPT implementation or fixable; a non-nested
host's equivalent per-exit-reason latency for comparison (would need the
same trace-cmd method run un-nested, not done here).

