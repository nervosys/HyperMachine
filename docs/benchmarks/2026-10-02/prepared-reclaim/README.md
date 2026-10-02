# Prepared-source heap reclaimability

Two fresh-daemon sham/trim pairs alternate AB/BA on the shared eight-CPU
WSL/KVM fixture. Both variants use the same accepted HyperMachine daemon and
the same preload helper. After both named HyperMachine and full Firecracker
sources are prepared and their parents stopped, a dedicated helper thread
performs exactly one operation: no-op for sham, `malloc_trim(0)` for trim.
No allocator operation runs in a signal handler. Five-second waits bracket
the operation; raw owned-daemon smaps are retained before and after.

Each variant then runs two counterbalanced engine pairs at concurrency eight,
using the [prepared restore workload](../prepared-engines/README.md): recover
a file and live process environment, then verify child-write isolation. All
128 restores pass (64 per engine, including 32 HyperMachine restores after
trim). All guest records are removed, owned daemons exit zero, Firecracker
processes stop, prepared sources remain unchanged and all input hashes match.
The helper changes the process, so these are diagnostic runs excluded from
latency rankings. The daemon executable itself remains unchanged.

| Pair | Sham-adjusted empty PSS reduction | Sham-adjusted heap PSS reduction | Held HyperMachine PSS, sham | Held HyperMachine PSS, trim | Held reduction |
|---|---|---|---|---|---|
| 0 | 28.703 MiB | 28.391 MiB | 89.149 MiB | 62.380 MiB | 26.770 MiB |
| 1 | 28.747 MiB | 28.395 MiB | 89.700 MiB | 61.737 MiB | 27.962 MiB |

Empty reduction is before-minus-after PSS, adjusted by the same difference
in the paired sham. Held PSS is the median across each variant's two complete
HyperMachine batches. Independent source preparation and host activity vary
between variants. Trim operations took 2.452 and 2.077 ms; these are helper
operation durations, not startup or restore latency measurements.

The earlier [mapping diagnostic](../prepared-memory/README.md) located much
of the gap outside guest-sized mappings. This intervention now demonstrates
that about 28.7 MiB of the prepared empty daemon's observed PSS is reclaimable
in this fixture, predominantly from `[heap]`, and that lower held PSS persists
through these subsequent restore batches. It does not identify the allocations'
owner, prove latency neutrality, or eliminate the remaining memory gap.

The intervention happens after named-source preparation on a listening daemon.
It does not establish that trimming once after initial template construction
would reclaim the same amount. A production candidate needs its own clean
build, baseline/candidate latency and held-memory comparison, lifecycle checks,
and cross-platform behavior checks before adoption. Previous cold-path periodic
or per-operation reclamation experiments remain separate evidence; this data
does not overturn their rejected latency tradeoffs. No runtime change is adopted.

Raw smaps reads are sequential and not atomic. PSS excludes kernel memory and
unmapped page cache. The shared host has uncontrolled background load. There
is no managed-service win or fleet-density result. No executable or snapshot
memory file is committed.

`helper-build.json` records compiler, strict build flags and source/binary
hashes, including an identical independent rebuild. The accepted daemon build
context and compiled main are frozen; the three provisional working-tree core
files were excluded. Every coordinator dependency is frozen and hash-bound.

From the repository root:

```sh
python3 -O tools/verify-prepared-reclaim.py docs/benchmarks/2026-10-02/prepared-reclaim
python3 -O tools/check-prepared-reclaim.py docs/benchmarks/2026-10-02/prepared-reclaim/report.json
```

Both Linux and Windows verify with assertions disabled. Ten malformed cases
are rejected, including missing runs, wrong counterbalance or operation,
repeated trim, different process identity, fabricated PSS, changed source,
failed guest validation, different binaries and a runtime adoption claim.

To reproduce on a Linux glibc/KVM host with matching inputs from `manifest.json`:

```sh
gcc -Wall -Wextra -Werror -O2 -fPIC -shared -pthread tools/heap-reclaim-probe.c -o /owned/path/helper.so
python3 tools/diagnose-prepared-reclaim.py --hypermachine /path/hv2-sandboxd --firecracker /path/firecracker --kernel /path/bzImage --initrd /path/initrd.cpio.gz --helper /owned/path/helper.so --pairs 2 --concurrency 8 --output /new/path/report.json
python3 -O tools/analyze-prepared-reclaim.py /new/path/report.json --output /new/path/analysis.json
```

The coordinator refuses an existing output, saves every variant as it finishes,
and retains setup errors. Its prepared driver owns temporary guests and stops
processes on failure; the wrapper also stops its captured owned daemon.
