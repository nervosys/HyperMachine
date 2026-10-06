# UDP combined frame writes

CLI UDP now queues the length prefix and payload in one bounded allocation, then emits the complete frame with one write_all call instead of separate prefix and payload calls. The queue count and datagram bounds are unchanged; each queued frame adds only the two-byte prefix. Empty/binary/maximum-size payloads, peer isolation, trust/key refusal, malformed guest-frame refusal and lifecycle cleanup pass in every cohort. Twelve existing VM CLI tests also pass.

Distinct preserved baseline/candidate binaries are compared in ABBA order across four fresh HTTPS/mTLS/Redis/KVM stacks. Each cohort uses two concurrent peers, ten warmups each, then synchronized 1,000 measured tagged 64-byte round trips per peer. Rates exclude warmups and use the slower peer's measured interval. All exact bytes pass, all services are reaped, guest inventory is empty and input hashes stay unchanged.

| CLI | Cohort measured round trips/sec |
|---|---:|
| Baseline | 1,529.8 and 1,501.1 |
| Combined write | 1,823.1 and 1,827.4 |

The average cohort rate changes from 1515.4 to 1825.3 round trips/second (+20.4%). Median latency remains around 1.06–1.08 ms with mixed pair directions; no consistent median improvement is claimed. Per-peer sampled P99 is lower for the candidate in both paired comparisons, but four short cohorts do not establish fleet tail performance. Earlier exploratory runs also showed fewer large candidate stalls; the synchronized comparison is the archived measurement basis here.

Reproduce check-udp-cluster-kvm.py --tls --mtls --concurrent-samples 1000 with the recorded daemon/control/kernel/image inputs and distinct CLI paths in baseline/candidate/candidate/baseline order, using fresh outputs. Baseline/candidate source files and binary hashes are archived. The initial exploratory copy lost executable permission and was corrected before this run; binary contents did not change.

This is a local development-build closed-loop workload on unpinned WSL CPUs, one guest vCPU/1 GiB, with no concurrent build. It does not prove sustained capacity, a universal latency gain, IPv6/native UDP performance or competitor superiority. Larger payload/load/resource tradeoffs remain unmeasured. Current implementation is retained for this demonstrated local rate improvement and passing correctness gates.
