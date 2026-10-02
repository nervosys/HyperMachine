# Cold guest teardown and allocator retention diagnostics

These two sequential diagnostics each created and deleted three batches of
eight cold guests, using the original scored daemon, identical kernel/initrd,
one vCPU and 1024 MiB per guest. All 48 attempts passed, with empty isolated
nodes, reaped daemons and unchanged artifact hashes. Both used eight-CPU
affinity on shared nested WSL/KVM, without the benchmark's pinned load worker.
Procfs snapshots were taken before creation, while each batch stayed held after
five seconds, and five seconds after its deletion. Reads are sequential, not atomic.

| Post-cleanup observation | Default allocator | Owned daemon with arena limit 2 |
|---|---:|---:|
| Total process PSS, batch 0 (MiB) | 182.02 | 110.02 |
| Total process PSS, batch 1 (MiB) | 217.76 | 97.70 |
| Total process PSS, batch 2 (MiB) | 193.52 | 110.06 |
| Median total process PSS (MiB) | 193.52 | 110.02 |
| KVM VM/vCPU handles after each cleanup | 0 / 0 | 0 / 0 |
| Mappings of at least 1 GiB after each cleanup | 0 | 0 |
| Threads after each cleanup | 17 | 17 |

Held batches had eight VM and eight vCPU handles; their disappearance and the
absence of large mappings narrow the residual-memory investigation. Default
post-cleanup anonymous PSS was 173–209 MiB, in smaller unnamed mappings. The
limited-arena run had about 60 MiB of anonymous PSS, but also retained memory
in its named heap. Total PSS, rather than one mapping category, is therefore
the useful comparison. Categories and mapping sizes do not identify allocation
stacks, prove that all retained bytes are free, or establish a universal lack
of leaks.

The diagnostic option `--allocator-arena-max 2` sets `MALLOC_ARENA_MAX=2` only
in the owned child daemon's startup environment. glibc documents this startup
control for the arena limit in its [allocator parameter manual](https://sourceware.org/glibc/manual/latest/html_node/Malloc-Tunable-Parameters.html).
The daemon links to glibc, whose recorded version is alongside the reports.
The normal daemon environment and production defaults remain unchanged.

This is an allocator-sensitive candidate, not an adopted optimization.
Two sequential runs are not counterbalanced; host scheduling and allocation
history can differ. Latency/tails, snapshot operations, forks and density were
not evaluated with the arena limit. A matched alternating experiment needs to
measure memory and readiness together before deciding whether to adopt it.

```sh
taskset -c 0-7 python3 tools/diagnose-memory-retention.py \
  --daemon /path/to/hv2-sandboxd --kernel /path/to/bzImage \
  --initrd /path/to/guest.cpio.gz --concurrency 8 --batches 3 \
  --idle-seconds 5 --output retention-default.json
# Repeat as a separate owned daemon, adding --allocator-arena-max 2.
```

The [default report](memory-retention-default-c8.json) and
[arena-limited report](memory-retention-arena2-c8.json) retain mapping categories,
the twenty largest PSS mappings, handle/thread counts, raw guest results and
teardown checks. The [summary](memory-retention-summary.json) verifies both exact
executed diagnostic source hashes and all 48 passing attempts. The first source
predates the optional arena flag and is preserved separately; both versions
used an explicit child environment, with no inherited allocator overrides.
