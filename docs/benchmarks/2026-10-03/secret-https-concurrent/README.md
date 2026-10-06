# Eight-client owned KVM HTTPS comparison

Two alternating pairs use exactly the immutable matched release binary, kernel
and client-image hashes from the sequential release archive. No runtime code or
accepted benchmark input changed. The checker starts eight curl processes inside
one guest per batch, waits for every exit status, and verifies every upstream
body/header/Content-Length. Each request sends 1,035,000 bytes containing 15,000
placeholders under a 128-binding policy and receives a small response after the
body is rewritten to 180,000 bytes.

The fixture uses three warm-up batches and twelve measured batches. Each cohort
records 96 curl time_total samples and 139 successful upstream requests including
its normal correctness suite. All fifteen unique checks pass in each of four
cohorts, and every owned guest/process is cleaned up. The owned server observes
HTTP overlap peaks of [5, 5, 7, 6], proving simultaneous HTTP handling beyond
just launching client processes. Server listen backlog is 32 and TCP_NODELAY is
consistently enabled for both engines. The server adds identical overlap tracking
in both cohorts; this fixture version differs from the earlier sequential one.

| Pair | Original median ms | Optimized median ms |
|---|---:|---:|
| 0 | 114.823 | 113.732 |
| 1 | 113.876 | 113.879 |

Median of run medians: 114.349 -> 113.806 ms
(-0.5%). The small aggregate shift and mixed pair results
are essentially unchanged; no reliable concurrent latency improvement is claimed.
The sequential fixture's observed 14.4% reduction does not establish a benefit
for this eight-client workload. A different bottleneck may dominate under load,
but no profiling evidence yet identifies it.

This is one guest, eight client processes and a synthetic request workload on
unpinned nested WSL, not eight VMs, service P99, fleet throughput or competitor
performance. Request medians cannot be converted into burst throughput without
batch makespan measurements. Each request opens verified TLS. Raw payloads,
policies and keys are temporary and not archived. Full original/optimized source
identity evidence is in the preceding release archive and copied build context.

Reproduce: `python3 tools/bench-secret-https-kvm.py --request-concurrency 8
--baseline BASE --candidate OPTIMIZED --kernel KERNEL --initrd HTTPS_CLIENT_IMAGE
--output NEW_DIRECTORY`. Output must not exist. Linux KVM, OpenSSL, ip and the
separate client image are required. Both immutable release binaries remain local.
