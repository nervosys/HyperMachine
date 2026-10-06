# Combined authorization read: release ABBA verification

Four sequential **baseline–candidate–candidate–baseline** KVM cohorts each pass **51 checks** and **128/128 scored operations**. Total scored success is **512/512**, with all 544 raw rows retained. Each cohort measures 32 alternating private/standard pairs at 64 and 65,507 bytes, after two warmup pairs. Both paths use fresh TLS and one exact framed UDP echo to the same target guest. The host, kernel, image, checker, payloads and other runtime binaries are unchanged. Both daemon variants are locked release builds. Source catalogs differ only in `store.rs` and `forwards.rs`; exact before/after snapshots are retained.

Positive values below mean private setup is slower than the standard UDP tunnel. Ranges are separate cohort medians, not pooled percentiles.

| Payload | Baseline paired setup overhead P50 (ms) | Candidate paired setup overhead P50 (ms) |
|---|---:|---:|
| 64 B | 0.267–0.286 | 0.282–0.323 |
| 65,507 B | 0.296–0.358 | 0.250–0.370 |

**The ABBA result does not establish a consistent latency improvement.** At 64 bytes the candidate range is slightly higher and overlaps the baseline; maximum-size results vary between candidate cohorts. Private setup remains slower. Fewer Redis commands are verified, but command count alone does not demonstrate lower latency or resource use. The atomic combined route/live-node view is retained as implemented behavior; it is not labeled a proven performance optimization. Further profiling is needed to address the measured setup gap. No throughput, resource, guest-origin latency or competitor win follows from these small shared-host cohorts.

The changed candidate passes actual KVM ownership/authentication, membership/generation/stale-binding, maximum guest UDP, source/target pause/deletion, route-store lookup refusal/recovery, TCP/DNS and cleanup gates. Zero guests remain and all tracked processes are reaped in every cohort. Both setup barriers remain fresh. Source pause/delete observations retain exact target relay identities and remain shorter than natural probe expiration. Baseline SHA256: `0250dab6fd392c871aa65e041be5b604c690da43a4c296a81e267471b2d9ccc9`; candidate: `8afb215a2ff476147b8ebf9ce96f25d161ae361876d16b32f9f72b97808c9fa3`.

All 139 permitted current root/isolate source pairs, accepted isolated core and all runtime hashes are revalidated. Protected root core sources were not read or built. Candidate build log/driver and full ABBA driver retain locked isolated build and owned execution inputs. CLI/control/native gateway remain their earlier fixture builds and are outside timed transport paths. Production source is unchanged from the preceding implemented combined-snapshot candidate.

Run `python3 verify-results.py` to independently recompute medians/P95 and paired differences, check alternating order, all scored rows, release/source provenance, exact two-file source delta, lifecycle scope and complete cleanup. Frozen drivers require original owned inputs and fresh output paths. Manifest and source catalogs pin evidence. This supersedes candidate-KVM-unverified notes, while leaving performance improvement unproven. IPv6, other races/stress, independent hosts and external competitor measurements remain incomplete or unmeasured.
