# PCI C8 jemalloc experiment: repeated memory reduction, unstable latency

Four fresh daemon cohorts run glibc, jemalloc, jemalloc, glibc using the same frozen current PCI daemon and matched Firecracker references. Each cohort has four alternating engine pairs of eight simultaneous cold starts and five-second idle holds. All 256 exact-command attempts pass (64 per engine per setting); inventory empties, daemon exits normally, frozen identities remain unchanged and no cleanup errors occur. Product defaults are unchanged.

| HyperMachine PCI C8, eight guests held | glibc | jemalloc combination |
| --- | ---: | ---: |
| Passed attempts | 64/64 | 64/64 |
| Readiness P50 | 721.82 ms | 1233.41 ms |
| Readiness P95 | 2045.63 ms | 1699.11 ms |
| Readiness P99 | 2310.58 ms | 1924.26 ms |
| Median held PSS | 813.29 MiB | 688.80 MiB |
| Median incremental PSS | 669.18 MiB | 668.46 MiB |
| Median empty-daemon baseline PSS | 124.06 MiB | 20.47 MiB |

Held HyperMachine PSS medians by cohort are 826.95, 688.43, 688.99 and 786.18 MiB. Both candidate cohorts have lower held memory than either glibc cohort. This supports a repeated memory candidate in the exact fixture. Incremental guest memory is similar; the main observed difference is persistent process overhead. Medians cannot be subtracted to infer another median. PSS excludes kernel allocations and has eight batch observations per engine per setting, not 64 independent memory samples.

Latency is not stable enough to establish a speed improvement: cohort readiness P50 is 1489.21, 1291.16, 1035.27 and 656.02 ms. The final glibc cohort is much faster than either candidate, and pooled candidate P50 is worse. Firecracker reference medians pool to 891.64/1038.81 ms with P95 1658.21/2409.74 ms, exposing substantial time-window variation. Reference held PSS stays about 670.4 MiB. P99 of 64 attempts is the maximum, and attempts within a burst are dependent. This result justifies no allocator default or latency superiority; CPU/throughput, prepared lifecycle, longer retention, default MMIO and independent-host gates remain open.

The installed libjemalloc2:amd64 5.3.0-3 is frozen to an owned output path; SHA256 is 51952ffe97354b56197c9b765582023d435cfbe1930b0c03b778bdb5faa1fff5. Only HyperMachine receives LD_PRELOAD and MALLOC_CONF=abort_conf:true,background_thread:true,dirty_decay_ms:1000,muzzy_decay_ms:1000. This is a combined allocator-and-reclamation experiment, not an isolated allocator effect. [jemalloc's primary documentation](https://jemalloc.net/jemalloc.3.html) describes background threads and decay controls. The library itself is not committed; reproduction requires the exact installed artifact/hash retained by driver.py. No package was installed or downloaded.

The benchmark adds opt-in --daemon-jemalloc-library PATH. Unset preserves prior child environment. The report records requested configuration, library identity and mapped-library presence after daemon startup; artifact checks include the library. Missing/ambiguous library paths and mixed glibc allocator options are refused before launch. Default/configured child environment checks and all 17 existing harness tests pass. Firecracker receives no experiment configuration.

Inputs are the same frozen PCI release 5d136c14b7ac36bd23af194ea82e844573a3ca18bfa7640e8e3d5e669db80f23, kernel and output-drain guest throughout, one vCPU/1024 MiB per VM, eight-CPU affinity 0..7, disabled templates, warn logging and no admission budget. Affinity does not reserve CPUs. The permitted prior nine-file source catalog plus candidate daemon hash is not complete build closure; protected root sources were excluded. No production source was changed or built.

Reproduce driver.py in a fresh directory with exact frozen inputs. The archive retains raw reports, stderr, context, exact harnesses and independent analysis. python3 verify.py checks all archive hashes and recomputes exact coverage, library identity/mapping, requested settings, quantiles, per-pair memory, actual idle holds and cleanup. No managed competitor, sustainable-capacity or across-the-board win is established.
