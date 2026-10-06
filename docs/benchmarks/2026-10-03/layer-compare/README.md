# Layered restore compare-before-write experiment

This isolated candidate reads the current base bytes before applying a present layered page, skipping its write only when every required byte matches. It changes only vm.rs; named capture, snapshot format and dependency lifetime are unchanged. All 550 accepted source files were validated before generating the patch. Windows and Linux patches are byte-identical. No production change is adopted.

The release build and strict core-library Clippy passed. Existing core tests report 2,298 passed, zero failed and two ignored. Guest correctness is additionally verified by the real KVM fixtures, rather than inferred solely from those tests.

The real named-source deletion/pause/resume fixture passes for baseline and candidate, preserving prepared files, child writes and a live process. Inputs are unchanged, guest inventory is zero, and owned nodes exit zero.

The scored concurrency-eight cohort in c8-clean counterbalances two fresh-daemon outer pairs with internal HM/Firecracker order reversal, one CPU and 1 GiB per guest, eight host CPUs, and resource checks after timed attempts. Preparation and capture are outside readiness timing. An earlier c8 cohort overlapped core-test compilation and is retained as diagnostic evidence only, excluded from the table below. Results are local shared WSL/KVM measurements, with no managed competitor win established.

| Measurement | Accepted baseline | Candidate |
|---|---:|---:|
| Passed restores | 32.00  | 32.00  |
| Mean readiness | 65.39 ms | 76.16 ms |
| P99 readiness | 102.44 ms | 104.15 ms |
| Median held daemon PSS | 90.97 MiB | 92.62 MiB |
| Median named capture | 10.76 ms | 9.03 ms |

The candidate is deferred: mean readiness is worse in both outer pairs and held PSS is higher in both. Tail differences are mixed, and Firecracker controls also shift, so these measurements do not isolate a general causal slowdown. The data provides no latency or memory win supporting adoption.

See c8-clean/analysis.json for paired differences, Firecracker controls and empty-node memory. All 128 scored restores passed and 13 damaged evidence contracts are rejected. This cohort cannot prove high-concurrency scaling or stable tail improvements. Adoption requires corroborating measurements and correctness coverage for persisted layered state.
