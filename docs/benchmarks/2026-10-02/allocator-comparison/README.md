# Same-daemon jemalloc comparison

The accepted daemon was tested with GNU libc and preloaded jemalloc 5.3.0-3,
without rebuilding or changing runtime defaults. Both use the same binary,
kernel/initrd, sixteen admission slots, eight-CPU affinity, one vCPU and 1024 MiB
per guest, and unchanged 15-second guest readiness deadline. Total readiness
includes admission queueing. Fresh daemons run in AB/BA order.

A separate loader preflight proves malloc/calloc/realloc/free bind to jemalloc.
The timed daemon's `/proc/PID/maps` proves the allocator is loaded only in the
candidate. Explicit child environments differ only by that preload, with no
arena/decay tuning. The driver and other processes retain their allocators.

| Cohort | Allocator | Passed / attempted | Successful P50 ms | Successful P99 ms | Median held PSS MiB |
|---|---|---:|---:|---:|---:|
| C8 smoke, two pairs | GNU libc | 16/16 | 996 | 1639 | 803.81 |
| C8 smoke, two pairs | jemalloc | 16/16 | 843 | 1239 | 711.40 |
| Complete C100, four pairs | GNU libc | 400/400 | 7952 | 21890 | 8527.68 |
| Complete C100, four pairs | jemalloc | 398/400 | 11124 | 29944 | 8426.14 |

The complete C100 cohort retains two candidate creation failures. All three
fully passing pairs have worse candidate mean, P50 and P99 readiness, while
held PSS is lower by 88.70, 122.42 and 47.78 MiB respectively. The failed pair
has no paired latency or memory conclusion. Pooled latency is conditional on
success; memory medians include only successful batches (four baseline, three
candidate). Aggregate medians are not paired estimates. Final inventories are
empty and all eight daemons exit zero. The tested jemalloc defaults are rejected
for adoption; production allocator behavior is unchanged.

An interrupted C100 cohort remains unchanged: four of eight planned batches,
400 attempted guests, 38 candidate creation timeouts and a failed cleanup check
with one retained guest before daemon shutdown. Its handle was missing and no
coordinator/daemon remained live at inspection. A WSL shutdown was observed;
the interruption cause is not established. Final artifact identity and a full
cohort summary are unavailable. Its 400 unattempted planned guests are not
passes or runtime failures. The fresh complete comparison is a separate cohort.

The first preflight failed because `--help` requires guest environment variables
before option parsing; its loader log and exact coordinator are retained. The
corrected preflight supplies those variables. Loader diagnostics are excluded
from timed bursts. An analyzer field-name error (`pss_kib` versus recorded
`Pss_kib`) was corrected before producing an accepted analysis.

PSS is measured after a five-second hold with guests alive; empty-daemon and
incremental readings are recorded. Kernel allocations and fleet density are
excluded. Shared nested KVM and uncontrolled host load limit causal and timing
conclusions. This is no Firecracker/managed-platform comparison or universal
allocator claim. The [daemon build evidence](../object-backup/README.md) excludes
the three provisional workspace boot edits.

Reports, loader logs, frozen tools and package context are archived, without
executables, guest memory or private credentials. Verify file hashes, recomputed
analyses and rejection of twelve malformed reports under Python `-O`:

```sh
python -O tools/verify-daemon-allocator.py docs/benchmarks/2026-10-02/allocator-comparison
```

Reproduce on Linux with owned binaries/images and a new private output:

```sh
python -O tools/bench-daemon-allocator.py --daemon /owned/hv2-sandboxd \
  --allocator /usr/lib/x86_64-linux-gnu/libjemalloc.so.2 \
  --kernel /owned/bzImage --initrd /owned/guest-output-drain.cpio.gz \
  --pairs 4 --concurrency 100 --cold-start-concurrency 16 --output /private/new-run
python -O tools/analyze-daemon-allocator.py /private/new-run/report.json \
  --output /private/new-run/analysis.json
```
