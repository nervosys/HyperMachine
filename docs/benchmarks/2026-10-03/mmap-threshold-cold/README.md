# Static GNU allocator threshold: cold creation

This extends the [prepared restore comparison](../mmap-threshold/README.md)
to the native cold create-to-command lifecycle. No runtime default is adopted.
Only candidate HyperMachine daemons receive `MALLOC_MMAP_THRESHOLD_=131072`;
all four engine/kernel/initrd input binaries are identical across variants.
The accepted source catalog and original daemon build context are retained.

Two outer fresh-daemon baseline/candidate AB/BA pairs each contain two inner
HM/FC AB/BA pairs. Both HM policies use the same 16 cold-start slots, eight
pinned host CPUs, 1-vCPU/1-GiB guests and 15-second guest-readiness deadline.
The barrier-released create-to-command timer includes admission queueing.
Owned executable and exact environment verification occur before all timed
attempts. Held PSS is read after every guest has validated and a five-second
hold; empty-daemon PSS is measured separately in each batch. No allocator
probe, preload, diagnostic logging or explicit trimming runs in scored daemons.

| Profile | HM policy | Engine | Passes/planned | Mean ms | P50 ms | P99 ms | Held PSS MiB |
|---|---|---|---:|---:|---:|---:|---:|
| c8 | baseline | hypermachine | 32/32 | 507.90 | 500.06 | 600.93 | 879.55 |
| c8 | baseline | firecracker | 32/32 | 430.95 | 425.40 | 472.37 | 670.78 |
| c8 | candidate | hypermachine | 32/32 | 517.62 | 514.04 | 552.61 | 678.24 |
| c8 | candidate | firecracker | 32/32 | 416.73 | 416.04 | 493.51 | 671.01 |
| c100 | baseline | hypermachine | 400/400 | 3877.37 | 3752.10 | 8138.99 | 8597.77 |
| c100 | baseline | firecracker | 400/400 | 8481.07 | 7951.29 | 10440.96 | 8361.44 |
| c100 | candidate | hypermachine | 400/400 | 4253.31 | 4085.48 | 9640.11 | 8364.36 |
| c100 | candidate | firecracker | 400/400 | 8935.53 | 7684.90 | 12565.15 | 8362.43 |

At C8, the policy reduced aggregate median held PSS from 879.55 to 678.24 MiB,
but mean readiness increased from 507.90 to 517.62 ms. One pair improved mean
by 4.75 ms; the other worsened by 24.19 ms. Firecracker mean shifts were -35.13
and +6.69 ms. HyperMachine remained slower than Firecracker in that profile.
These retained regressions prevent an across-the-board improvement claim.

GNU describes the static threshold and mmap-backed allocation reclamation in
its [allocator parameter documentation](https://sourceware.org/glibc/manual/latest/html_node/Malloc-Tunable-Parameters.html).
The prepared archive's separate C probe is mechanism evidence, not proof of
daemon allocation ownership or a reason to treat all PSS differences as heap.

The analyzer checks exact environment isolation, running executable hashes,
counterbalance, all planned sample identities, input hashes, admission budget,
queue timing, unchanged deadlines, complete held-guest measurements and owned
process cleanup. Nineteen altered-contract checks are replayed with Python -O
on Linux and Windows. Failures remain in the denominator; latency and memory
are conditional on passing samples and complete batches respectively.

Shared WSL nested KVM and host background load remain limitations. These
cache-warm host runs do not measure dropped-cache storage, sustainable arrivals,
kernel memory, fleet density, managed boxd/exe.dev latency or public availability.
No automatic runtime setting or managed competitor win is established.

At C100, all 1,600 cold attempts passed with verified cleanup. Candidate median
held PSS fell from 8597.77 to 8364.36 MiB, but HM mean increased from 3877.37
to 4253.31 ms and P99 from 8138.99 to 9640.11 ms. Mean worsened in both outer
pairs (55.43 and 696.46 ms); pair P99 improved by 307.49 ms and worsened by
1772.57 ms respectively. Firecracker control mean shifted +1687.25 and -778.33 ms.
The policy is deferred as a general-purpose deployment default. Separate
prepared-restore gains do not prove an improvement across cold workloads.
This experiment cannot distinguish allocator CPU cost from shared-host noise.
