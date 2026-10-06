# Current release allocator experiment: C1 cold creation

Opt-in MALLOC_ARENA_MAX=2 applies only to the owned HyperMachine daemon. The same frozen release, kernel, output-drain guest, eight-CPU affinity, 1 vCPU/1024 MiB, disabled templates and five-second idle measurement are used as the preceding matched C1 run. Both engines pass 8/8 exact-command attempts; inventory is empty, daemon exits normally and artifacts remain unchanged.

| C1 result | HyperMachine arena limit 2 | Firecracker 1.17.0 |
| --- | ---: | ---: |
| Cold readiness P50 | 441.27 ms | 470.32 ms |
| Cold readiness P95/P99 | 3346.86 ms | 1740.96 ms |
| Median held PSS after five seconds | 123.81 MiB | 85.85 MiB |
| Median incremental PSS | 83.05 MiB | 85.85 MiB |

The earlier default-allocator C1 run held 151.98 MiB for HyperMachine and 85.88 MiB for Firecracker. This experiment observes 28.16 MiB less whole-daemon PSS for HyperMachine, but it is a separate sequential run, not an interleaved allocator ABBA comparison. The prior baseline and experiment do not isolate allocator causality or establish a repeatable improvement. Unrelated compilation was observed on the shared WSL host during this run; affinity does not reserve CPUs. HyperMachine's maximum latency rises to 3.35 seconds. With eight attempts P95/P99 are maxima. No default change or performance superiority is justified.

Incremental PSS subtracts same-batch empty daemon memory for HyperMachine; Firecracker starts with zero VMM processes. Medians cannot be subtracted to recover the median baseline. PSS excludes kernel allocations. C1 cold creation does not establish high-concurrency density, prepared creation or network throughput. Managed competitors remain unmeasured. The permitted nine-file source catalog is not a complete build closure; protected root sources were excluded.

context.json retains the exact argv and fixed affinity. Reproduce in a fresh directory with those immutable inputs, using --daemon-allocator-arena-max 2 only for HyperMachine. No sources were built or changed. Raw report, terminal status, stderr, exact harnesses and independent analysis are retained. Run python3 verify.py to check archive hashes, attempt coverage, input/harness identities, quantiles, PSS, actual idle holds and cleanup. The prior default-allocator comparison is preserved separately in ../current-release-firecracker-c1/.
