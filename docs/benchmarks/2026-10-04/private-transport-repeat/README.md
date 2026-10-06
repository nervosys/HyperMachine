# Matched receiving TCP repeat

An independent fresh fixture using identical frozen binaries, checker, kernel and guest image passes **30 functional checks**, **128/128 scored operations**, and eight warmups. All guests and processes are cleaned up. This repeats the [first cohort](../private-transport-comparison/README.md) without production changes.

| Path | Payload | Success | Setup P50 / P95 ms | Echo P50 / P95 ms | Payload echo MiB/s P50 | Held target PSS P50 MiB |
|---|---:|---:|---:|---:|---:|---:|
| Private | 64 B | 32/32 | 4.576 / 5.544 | 0.859 / 1.009 | 0.0703 | 66.605 |
| Standard authenticated | 64 B | 32/32 | 3.439 / 3.803 | 0.837 / 1.000 | 0.0729 | 66.602 |
| Private | 1 MiB | 32/32 | 5.747 / 6.321 | 95.783 / 99.396 | 10.438 | 67.465 |
| Standard authenticated | 1 MiB | 32/32 | 4.140 / 4.491 | 95.665 / 98.902 | 10.425 | 67.484 |

The private minus standard setup P50 gap is **1.137 ms** at 64 B and **1.607 ms** at one MiB; both cohorts show higher private setup cost. Echo and throughput are close in both cohorts. The difference does not isolate a causal component or demonstrate a competitor win. Keep per-cohort results separate; no pooled P99 or significance claim is made.

The methodology remains fresh TCP/TLS, same host mTLS client and target KVM guest, concurrency one, alternating paired order, two warmups then 32 scored samples per path/payload. Nearest-rank P50/P95 are recomputed from all 136 retained rows by `python3 verify-results.py`. One-MiB throughput divides payload MiB by complete echo duration, with that payload sent in each direction; it is not one-way line rate. Source guest DNS/router/gateway functional checks are separate and excluded from these timings.

CPU sums across scored requests for private/standard are 290/230 ms at 64 B and 5930/5900 ms at one MiB for the target daemon, including guest-vCPU threads. Redis sums are 10/0 and 20/10 ms. Shared host/background work and 10-ms tick quantization limit interpretation. Held PSS covers the whole target daemon, not incremental connection memory. Independent hosts, higher concurrency, guest-origin timing, Redis crash durability and competitor comparisons remain unmeasured.

All 138 permitted root/isolate source pairs, accepted isolated core hashes and runtime inputs were revalidated. Protected root core files were neither read nor built. `driver.py` preserves the original absolute-path invocation; use equivalent owned inputs and a fresh output directory for reproduction. The source context here corrects inherited historical scope prose while preserving the checked source hashes.
