# Equal-resource two-vCPU secret HTTPS comparison

The distinct matched original and optimized release daemon binaries documented in ../secret-https-release/build-context.json were tested with two guest vCPUs each, alternating AB then BA. The guest online CPU count was checked. Eight concurrent curl processes ran inside each guest, with 128 bindings, 24 warm-up requests, and 96 measured requests per cohort. The raw input body was 1,035,000 bytes with 15,000 placeholders and the verified output was 180,000 bytes. All four cohorts passed 15 correctness/lifecycle checks, 556 upstream requests succeeded, HTTP overlap was observed, and processes were reaped.

| Pair | Original median request (ms) | Optimized median request (ms) | Original daemon CPU (s) | Optimized daemon CPU (s) |
| --- | ---: | ---: | ---: | ---: |
| AB | 48.892 | 48.132 | 2.65 | 2.49 |
| BA | 47.248 | 49.172 | 2.63 | 2.45 |

Median of run medians: 48.070 to 48.652 ms (+1.2%). Pair directions disagree; no concurrent latency improvement is established. User-plus-system daemon CPU fell in both cohorts, 2.65 to 2.49 and 2.63 to 2.45 seconds. These are short quantized accounting windows, not a fleet efficiency claim. User already includes guest time. Batch wall was 1.018/0.990 seconds for original and 1.010/0.992 for optimized and includes exec API and validation.

The earlier one-to-two-vCPU resource scaling gain is distinct from this equal-resource code comparison. Component lookup gains and the single-client HTTPS improvement must not be presented as concurrent request latency gains. CPU placement is unfixed; two local WSL pairs do not establish service P99, throughput scaling, or managed competitor superiority. Every request opens a new TLS connection. Full reports contain all timings, input identities, and accounting scope. No raw secret values, private keys, or synthetic HTTP payloads are archived.
