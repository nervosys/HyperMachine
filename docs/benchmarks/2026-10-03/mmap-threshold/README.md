# Same-binary GNU allocator threshold comparison

No runtime default is adopted. The candidate sets only `MALLOC_MMAP_THRESHOLD_=131072`
on the accepted HyperMachine daemon. Firecracker controls keep their environment.
Two outer fresh-daemon AB/BA pairs each contain two HM/FC AB/BA batches, with
eight pinned host CPUs and matched 1-vCPU/1-GiB guests. Resource validation runs
after all timed attempts. Preparation, environment checks and named capture are
outside restore readiness timing. Reports validate prepared guest state, clock/RNG
maintenance, artifact identity and complete owned-process cleanup.

| Profile | HM policy | HM passes | Mean ms | P99 ms | Held PSS MiB | Empty PSS MiB | FC control mean ms |
|---|---|---:|---:|---:|---:|---:|---:|
| c8-stable | baseline | 32/32 | 80.38 | 219.96 | 90.14 | 41.96 | 54.64 |
| c8-stable | candidate | 32/32 | 59.76 | 94.65 | 60.32 | 11.88 | 60.24 |
| c100 | baseline | 400/400 | 649.24 | 913.98 | 330.10 | 59.24 | 685.81 |
| c100 | candidate | 400/400 | 611.10 | 768.11 | 314.91 | 31.17 | 647.04 |

The separate C allocation probe demonstrates heap retention after a repeated
13-MiB plus 2-MiB allocation/free cycle under this GNU libc. It is not loaded
into the benchmark daemon and does not prove daemon allocation ownership.
GNU documents that setting the threshold fixes it rather than allowing dynamic
adjustment, and that mmap-backed allocations can return memory on free:
[GNU allocator parameters](https://sourceware.org/glibc/manual/latest/html_node/Malloc-Tunable-Parameters.html).

The initial `c8` collection failed environment observation before scoring in
two cohorts and is excluded in full. The first launch log also retains an
unset-environment-key error before any daemon launch. The corrected driver waits
for the owned executable and exact environment before preparation; no failed
collection is counted as a successful comparison.

Shared WSL nested KVM, cache-warm sources and uncontrolled background load limit
these results. PSS excludes kernel memory and unmapped cache. The per-pair
analysis retains latency regressions and Firecracker control shifts. These are
local engine comparisons, not managed boxd/exe.dev measurements or a fleet win.
The operator lifecycle check separately covers prepared child state, named-source
deletion and pause/resume under both policies. Negative contract tests run under
Python -O on Linux and Windows. Initial issuance, public DNS/CA operation and
the other feature gaps in PLATFORM_PARITY remain open.

At C100, both outer pairs reduced held PSS (14.25 and 15.89 MiB) and empty
PSS (28.01 and 28.13 MiB). Mean and P99 improved in both pairs, while the
aggregate P50 changed from 613.65 to 614.92 ms. Firecracker's mean shifted
+5.10 ms in one pair and -82.64 ms in the other. At C8, the second pair's
candidate P99 worsened by 5.98 ms. These results support further validation
of an opt-in deployment setting; they do not establish a universal latency
improvement or justify changing every deployment's default.

| C100 engine/policy | Restores passed | Mean ms | P50 ms | P99 ms | Median held PSS MiB |
|---|---:|---:|---:|---:|---:|
| HyperMachine baseline | 400/400 | 649.24 | 613.65 | 913.98 | 330.10 |
| Firecracker control beside baseline | 400/400 | 685.81 | 683.18 | 884.63 | 282.94 |
| HyperMachine candidate | 400/400 | 611.10 | 614.92 | 768.11 | 314.91 |
| Firecracker control beside candidate | 400/400 | 647.04 | 653.42 | 753.33 | 278.13 |
