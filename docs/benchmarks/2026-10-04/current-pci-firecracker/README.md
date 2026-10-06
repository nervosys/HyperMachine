# Current PCI release versus Firecracker: matched cold C1 and C8

The improved PCI release and Firecracker 1.17.0 use the same immutable kernel/output-drain guest, one vCPU/1024 MiB per guest, eight-CPU affinity 0..7, disabled templates and no cold admission budget. Both profiles use eight alternating engine-batch pairs and five-second idle holds. All 144 exact-command attempts pass: 8 per engine at C1 and 64 per engine at C8. Inventories empty, daemon exits normally, no cleanup errors occur and frozen inputs remain unchanged.

| Matched cold result | PCI HyperMachine | Firecracker |
| --- | ---: | ---: |
| C1 readiness P50 | 413.50 ms | 381.93 ms |
| C1 readiness P95/P99 | 483.46 ms | 534.24 ms |
| C1 median held PSS after five seconds | 145.35 MiB | 85.87 MiB |
| C1 median incremental PSS | 84.86 MiB | 85.87 MiB |
| C8 readiness P50 | 520.63 ms | 448.77 ms |
| C8 readiness P95 | 620.84 ms | 521.00 ms |
| C8 readiness P99 | 635.73 ms | 535.80 ms |
| C8 median held PSS, eight guests | 860.56 MiB | 670.87 MiB |
| C8 median incremental PSS | 680.13 MiB | 670.87 MiB |

PCI cold-start improvement does not establish competitive superiority: HyperMachine still has higher median readiness and whole-process memory here; its C8 observed tail quantiles are also higher. C1's lower maximum cannot establish a production-tail advantage with eight attempts. C1 P95/P99 and C8 P99 are maxima. Attempts in a batch are dependent; memory has eight batch observations per engine/profile. No confidence interval or general ranking is established.

Incremental PSS subtracts HyperMachine's same-batch empty-daemon baseline; Firecracker starts with zero VMM processes. HyperMachine has a persistent HTTP daemon while Firecracker uses fresh VMM processes and Unix APIs. Held PSS therefore includes different persistent overheads. Medians cannot be subtracted to infer median baseline, and PSS excludes kernel/host allocations. Similar incremental memory does not prove density parity. Affinity does not reserve CPUs from other host work. Sustainable arrivals, prepared restore, throughput, C50/C100, independent hardware and managed boxd/exe.dev/E2B remain unmeasured here.

The accepted prepared PCI guest's actual /proc/cmdline matches Firecracker's configured boot-argument tokens. boot-argument-proof.json retains that separate same-binary/kernel observation (buffered guest); these output-drain benchmark attempts validate exact shell readiness rather than resampling command line. The harness now supports --daemon-guest-transport mmio|pci and records the choice. Default MMIO argv is preserved; invalid transport is rejected before launch. All 17 existing harness tests pass.

Candidate SHA is 5d136c14b7ac36bd23af194ea82e844573a3ca18bfa7640e8e3d5e669db80f23. source-context retains the permitted prior nine-file catalog plus daemon-only candidate hash, not complete build closure. No source builds or production edits occur in this run; protected root sources are excluded. This supersedes no historical archive: older MMIO/C8/C50/C100 and internal buffered-guest transport measurements retain their own binary and measurement scope.

Reproduce driver.py using the exact frozen inputs in a fresh directory; terminal.json retains exact argv and context.json records affinity. Exact harness sources, raw reports/stderr, source identity and independent recomputation are retained. python3 verify.py checks archive hashes, all 144 attempts, actual idle holds, cold/resource/transport configuration, frozen identities, boot-argument proof, quantiles, memory and cleanup. No overall or managed competitor win is claimed.
