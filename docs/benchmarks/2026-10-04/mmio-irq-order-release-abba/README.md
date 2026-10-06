# Combined candidate release ABBA comparison

The fixed baseline/candidate/candidate/baseline comparison passes all four original-deadline cohorts: 220 KVM checks, 512 scored fresh TLS/upgrade setup-plus-echo operations and 51,200 scored concurrent datagrams. Both private and standard routes stay held throughout each four-block comparison. Every block completes exact payloads with zero UDP error/drop deltas; every cohort records zero guests remaining and full owned-process cleanup.

| Measured concurrent metric | Baseline | Combined candidate |
|---|---:|---:|
| private throughput range, MiB/s | 21.450–21.982 | 22.494–22.799 |
| private target CPU, ms/MiB | 316.70 | 35.28 |
| standard throughput range, MiB/s | 21.780–22.286 | 22.445–23.228 |
| standard target CPU, ms/MiB | 316.70 | 34.94 |

Across matched blocks, aggregate throughput rises 3.62% and target CPU per payload MiB falls 88.91%. CPU includes guest vCPU and host relay work, with whole-process ticks bracketed around traffic (including PSS sampling). Resources for target/source daemons, Redis and checker are preserved; PSS observations do not establish a per-route memory advantage.

| Fresh setup + echo, total latency | Baseline P50 range, ms | Candidate P50 range, ms | Baseline P95 range, ms | Candidate P95 range, ms |
|---|---:|---:|---:|---:|
| private 64 bytes | 2.668–2.965 | 2.334–2.352 | 3.209–3.756 | 2.519–2.714 |
| private 65507 bytes | 3.026–3.081 | 2.598–2.626 | 3.276–3.831 | 3.009–3.084 |
| standard 64 bytes | 2.306–2.665 | 2.036–2.097 | 2.551–3.005 | 2.269–2.478 |
| standard 65507 bytes | 2.592–2.666 | 2.228–2.232 | 2.911–3.113 | 2.443–2.485 |

Ranges are the two independent cohort percentiles for each kind, not pooled percentiles. Setup and echo component metrics are in metrics.json and independently recomputed from raw rows. All candidate measured P50/P95 component and total ranges are below corresponding baseline ranges in these cohorts.

Baseline is accepted setup-overlap release aef81259…; candidate is frozen aab60303… containing per-connection vsock progress, acknowledged pause and serialized MMIO IRQ transitions. The candidate also passes three independent 20-resume cohorts. This comparison supports the combined change; it does not attribute CPU reduction or latency changes to one component. Earlier failed experiments are retained, and no permanent-elimination claim is made.

The candidate catalog pins 145 permitted root/isolated source inputs. Baseline provenance covers 143: two shared entries differ, while vm.rs and virtio_mmio.rs are additional candidate snapshots. Accepted isolated backend/boot inputs match. This coverage does not prove an exact whole-source four-file delta. Protected root sources are excluded. Locked release build and regression evidence (16 MMIO, 47 selected core, 26 vsock, 531 agent, 64 daemon) are preserved.

Owned same-host WSL2 KVM, buffered guest fixture, operator mTLS routes, 32 workers × 100 mixed 64/1280/65507-byte datagrams per block. Source-guest gateway latency is not scored. This is HyperMachine versus its own baseline, not managed competitor evidence, public Internet performance, bare-metal scaling or an across-the-board product win. Remaining private-network and broader feature gaps remain open.

Run verify.py and verify-metrics.py from this archive to independently check raw payloads, counters, resource identities, ticks/PSS, latency summaries and derived metrics. The first attempt to invoke the archive-relative verifier from /var/tmp failed to locate analysis.json; the verifier then passed from its intended archive directory. verification.txt records the successful terminal result.
