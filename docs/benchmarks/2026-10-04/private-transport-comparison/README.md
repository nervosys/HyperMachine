# Matched private and standard receiving TCP comparison

The owned KVM fixture passes **30 checks**, including its existing guest DNS, binary traffic and stream revocation checks. All 128 scored transport operations and eight warmups succeed. Every guest and fixture process is cleaned up. This compares two HyperMachine receiving paths, not competing products.

| Receiving path | Payload | Success | Setup P50 / P95 ms | Echo P50 / P95 ms | Payload echo MiB/s P50 | Held target PSS P50 MiB |
|---|---:|---:|---:|---:|---:|---:|
| Private | 64 B | 32/32 | 5.163 / 5.937 | 0.954 / 1.246 | 0.0625 | 66.408 |
| Standard authenticated | 64 B | 32/32 | 3.626 / 4.123 | 0.892 / 1.022 | 0.0678 | 66.404 |
| Private | 1 MiB | 32/32 | 6.097 / 6.945 | 96.583 / 100.932 | 10.352 | 67.241 |
| Standard authenticated | 1 MiB | 32/32 | 4.274 / 4.636 | 96.428 / 100.078 | 10.347 | 67.241 |

Private setup has an observed median cost of 1.536 ms (42.4%) for 64 B and 1.822 ms (42.6%) for one MiB. One-MiB echo duration and throughput are effectively comparable in this cohort; the small difference does not establish superiority. The experiment identifies receiving-path setup overhead but does not attribute it to an individual operation.

The same host mTLS client connects to the same target daemon and KVM guest, with fresh TCP/TLS connections, the same credentials, port and payload, and paired alternating first path. Each path/payload receives two warmups followed by 32 scored operations at concurrency one. Setup runs from connection initiation through the authenticated HTTP 101 upgrade. Echo uses sequential 16-KiB send/receive chunks; each direction carries the stated payload. Payload MiB divided by complete echo duration is not isolated one-way bandwidth or aggregate line rate. P50/P95 use nearest rank; no P99 claim is made. Source guest Ethernet, DNS and Rust source router/connector are exercised by separate functional checks but are not timed here.

Whole target-daemon PSS is sampled while the connection remains held after echo; it is not incremental connection memory. Target daemon CPU (including guest-vCPU threads) summed across scored operations is 330/230 ms for private/standard at 64 B and 6070/6000 ms at one MiB. Owned Redis CPU sums are 10/0 and 20/10 ms respectively. These counters have 10-ms tick granularity and include background activity on a shared host, so small differences and zero deltas do not imply exclusive resource costs.

`report.json` retains all raw samples, summaries, environment, cleanup and runtime input hashes. Run `python3 verify-results.py` to independently recompute ranks, success counts, CPU sums, paired order and throughput. `driver.py` preserves the original invocation and absolute owned-fixture input paths; to reproduce elsewhere, supply equivalent inputs to `checker.py` and a fresh output path. The production binary is unchanged from the previously verified private guest gateway release. All 138 permitted root/isolate source pairs and accepted isolated core hashes were rechecked; protected root core files were neither read nor built. Redis crash durability, independent hosts, higher concurrency, guest-origin performance and competitor comparisons remain unmeasured here.
