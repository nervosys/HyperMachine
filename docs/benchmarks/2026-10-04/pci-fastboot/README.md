# PCI headless boot options and explicit template identity

The daemon now gives PCI guests the one-UART and conservative keyboard options already used by the MMIO guest: 8250.nr_uarts=1 i8042.noaux i8042.nomux i8042.nopnp i8042.dumbkbd. PCI/APIC discovery remains enabled. The initial isolated test revealed that the resulting MMIO and PCI command lines are identical, making command-line-only cache separation insufficient. PCI now adds an explicit transport discriminator to template identity; historical MMIO identity bytes remain unchanged. The first failing test is retained. The full daemon suite then passes 67 tests, with two ignored.

| PCI cold result, 32 scored attempts per binary | Prior release | Candidate |
| --- | ---: | ---: |
| API create P50 | 654.58 ms | 378.10 ms |
| API create P95 | 765.21 ms | 428.37 ms |
| Create plus exact command P50 | 656.71 ms | 380.24 ms |
| Create plus exact command P95 | 768.38 ms | 431.24 ms |
| Mean sampled process CPU per operation | 417.50 ms | 390.31 ms |
| Immediately held daemon PSS P50 | 199135 KiB | 201469 KiB |

Baseline-candidate-candidate-baseline uses fresh daemons, two excluded warmups and sixteen scored operations each: 72 exact create/command/delete/empty-inventory gates, 64 scored observations. Every cohort logs eighteen Linux image loads and zero restores. The candidate has lower observed cold-create latency and sampled CPU, but slightly higher held PSS. This is one owned WSL/KVM host and kernel with warm host caches; no statistical confidence, high-concurrency, throughput, independent-host or competitor win is established. Defaults remain MMIO.

Both binaries use PCI, the same immutable kernel and buffered guest, one vCPU/1024 MiB, eight-CPU affinity 0..7 and no template. The only permitted production source changed is daemon main.rs. Baseline binary SHA is dc315d313fe2f156ab7aaa593e02c4e0d04ffa024cfb5c69c85d2780a270676e; candidate is 5d136c14b7ac36bd23af194ea82e844573a3ca18bfa7640e8e3d5e669db80f23. source-context retains the prior nine-file catalog and candidate main hash, not full build closure. Accepted isolated protected hashes were checked; protected root sources were not read, built, edited or staged.

The same frozen candidate passes 28 prepared PCI and 28 default-MMIO API checks, including actual /proc/cmdline verification, two sibling forks, parent/sibling isolation, three disk pause/resume cycles, a descendant fork and deletion to empty inventory. Network-enabled PCI passes 56 checks, including seven exact NIC HTTP requests with owned-host receipt and fourteen separate guest-proxy HTTP requests. These are functional checks, not network throughput or negative-egress enforcement. The new PCI cache key creates a new template; adoption/reuse of historical PCI stores remains unverified.

PSS is sampled immediately after the exact command and includes the whole daemon. CPU samples use /proc/stat ticks (10 ms granularity) and include the preceding memory read; latency excludes resource reads, deletion and inventory checks. These values cannot be merged with five-second-idle output-drain Firecracker comparisons.

[Linux kernel parameter documentation](https://docs.kernel.org/6.0/admin-guide/kernel-parameters.html) describes UART count and keyboard probe options. Headless console and guest agent operation are verified; broad kernel/input-device compatibility remains unproven. Run python3 verify.py to check archive hashes, stable process identities, exact coverage, recomputed quantiles/CPU/PSS, Linux-load counts, frozen identities, actual command lines and lifecycle/network cleanup. Drivers retain owned fixture settings; use fresh output directories when reproducing.
