# Matched release private versus standard UDP comparison

Two sequential owned KVM cohorts each pass **51 checks** and **128/128 scored operations**. Each cohort measures 32 alternating private/standard pairs per payload size, after two warmup pairs. Every operation creates fresh TLS, upgrades the chosen node UDP endpoint and verifies one exact framed datagram echo from the same target guest. Both timed paths use the same release daemon, operator mTLS, payloads and target. The source identity belongs to a real running guest; source guest Ethernet/router/connector latency is not timed. Concurrency is one on a shared WSL host with no exclusive CPU reservation.

Ranges below are separate cohort medians, not pooled percentiles. Positive paired differences mean private is slower.

| Payload | Private setup P50 (ms) | Standard setup P50 (ms) | Paired setup overhead P50 (ms) | Paired echo difference P50 (ms) |
|---|---:|---:|---:|---:|
| 64 B | 2.278–2.309 | 1.957–1.992 | 0.311–0.329 | 0.005–0.027 |
| 65,507 B | 2.192–2.193 | 1.830–1.886 | 0.294–0.337 | 0.007–0.025 |

Private setup remains slower: paired median overhead is about 0.29–0.34 ms. Echo differences are much smaller and vary by payload/cohort. This identifies local setup overhead for further profiling. It does not establish a throughput improvement, a before/after optimization, general tail-latency superiority, guest-origin latency, or a competitor win. All per-path P95 values and raw rows remain available; small cohorts on a shared host limit interpretation.

All 256 scored operations preserve exact datagrams, and both complete functional cohorts clean up every guest and tracked daemon/control/Redis/CLI/gateway process. The frozen release daemon SHA256 is `0250dab6fd392c871aa65e041be5b604c690da43a4c296a81e267471b2d9ccc9`. The build driver/log prove locked release compilation from accepted isolated sources. CLI/control/native gateway retain earlier fixture builds and are outside the timed paths. Guest image/client are unchanged accepted inputs. All 139 permitted root/isolate source pairs, accepted isolated core and runtime hashes are revalidated. Protected root core files were not read or built. Production sources are unchanged; this turn adds the owned timing checker and release evidence.

An initial cohort's 136 rows and benchmark summary are retained but excluded. Its later TCP recovery verifier failed because the new benchmark overwrote the expected payload variable. The corrected fixture uses a dedicated UDP payload variable; both final fresh cohorts pass. The excluded cohort emits no success report. Completed rows are flushed to JSONL before connection close and the benchmark summary is saved before scored-failure assertions; this is not fsync/crash-durability proof.

Run `python3 verify-results.py` to independently recompute summary medians/P95 and paired differences, validate alternating pair order and row journals, confirm 256 scored successes and full cleanup, and verify release/input provenance. Frozen build/run drivers require original owned inputs and fresh output paths. Manifest and source context pin archived/runtime/source hashes. IPv6, other lifecycle races, independent-host operation, resource comparisons and external competitor performance remain incomplete or unmeasured.
