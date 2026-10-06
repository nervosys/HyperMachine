# Owned HTTPS guest CPU scaling

One immutable optimized release daemon binary was tested in alternating one/two/two/one-vCPU cohorts. All other fixture inputs were unchanged. Each guest's online CPU count was read from /proc/cpuinfo and checked. Eight concurrent curl processes ran inside one guest; each cohort used 128 bindings, 24 warm-up requests, and 96 measured requests with a 1,035,000-byte raw body rewritten to 180,000 bytes. All four cohorts passed 15 correctness/lifecycle checks and process cleanup; 556 upstream requests succeeded.

| Guest vCPUs | Median of request run medians (ms) | Median measured batch wall (s) |
| --- | ---: | ---: |
| 1 | 115.217 | 1.800 |
| 2 | 45.574 | 0.973 |

The observed request reduction is 60.4%; measured batch wall fell about 46%. This is resource scaling with an extra guest vCPU, not an implementation speedup, equal-resource competitor comparison, or throughput scaling guarantee. Two repetitions per configuration and unfixed CPU placement limit inference. Batch wall includes guest exec API and response validation and differs from curl time_total. Each request opens a new TLS connection. No managed competitor was measured.

For parallel workloads like this synthetic HTTPS case, operators can evaluate --cpu-cores 2 with prepared templates. This fixture confirms two online guest CPUs and working rotation, revocation, TLS rejection, fork, pause/resume, body/header rewriting and framing. Template-specific sizes can override the node default; use a newly prepared template and verify guest CPU count. Do not infer that increasing CPUs improves cold starts, memory usage, single-client latency, or other workloads. Defaults remain unchanged.

The summary records immutable daemon/kernel/initrd identities. Full source/build provenance is in ../secret-https-release/build-context.json. CPU counter scope and limitations are embedded in each report. The archived runner adds stricter finite/sample/body-contract validation after the run; those contracts are also checked directly against all archived reports. Private keys and synthetic HTTP payloads are not archived.
