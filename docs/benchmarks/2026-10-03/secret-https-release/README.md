# Matched release KVM HTTPS comparison

Two alternating baseline/candidate pairs compare the original byte-wise linear
replacement module with the current bulk-copy/ordered-lookup module. Full
isolated source catalogs differ in exactly that module; compiler, lockfile,
release profile, other daemon code, kernel and client image are identical.
The separately copied executable hashes differ and remain unchanged through
measurement. Both variants pass all fifteen owned correctness checks per cohort,
including exact bytes/framing of every large request and lifecycle behavior.

| Pair | Baseline median ms | Candidate median ms |
|---|---:|---:|
| 0 | 17.373 | 14.690 |
| 1 | 17.306 | 14.999 |

Median of run medians: 17.340 -> 14.844 ms
(-14.4%). The candidate is lower in both observed pairs.
Each cohort records twelve guest curl time_total samples after three warm-ups,
with new verified TLS connections, 128 bindings and 15,000 placeholders in a
1,035,000-byte raw body rewritten to 180,000 bytes. This includes guest networking,
TLS and the owned Python server. Host API exec timing is excluded. All four
cohorts clean up guests, daemon, listener and private files; 136 successful HTTP
requests are observed across them. Keys, policies and raw responses are not archived.

The owned server uses TCP_NODELAY consistently in both variants. This fixture
version is not comparable to the older debug run that used default server TCP
behavior. These are two unpinned WSL pairs for one synthetic request workload,
not a service P99, fleet throughput, varied application workload or managed
competitor benchmark. Policy loading/sorting is outside the timed calls. The
observed 14.4% reduction does not establish across-the-board superiority.

An initial comparison accidentally copied identical binaries from a shared Cargo
target after artifact reuse across checkouts. Those timings are explicitly
excluded in excluded-identical-summary.json. Rebuilding only the executable still
reused the other checkout's networking library. The final candidate regenerated
both hv2-net and the executable; the baseline had compiled from scratch. Their
source catalogs, separate binary hashes and successful build logs are archived.
The runner now refuses identical binary hashes before creating output or starting
fixtures; identity-guard.json verifies that refusal. Use separate Cargo target
directories when reproducing, or explicitly regenerate both library and executable.
Never infer algorithm identity from a successful cached build alone.

Protected workspace core sources were neither read nor built; both checkouts use
accepted isolated core. The accepted VM benchmark daemon and original initrd
hashes were rechecked unchanged. No deployment, commit or publication occurred.

Reproduce the release builds in isolated accepted-core checkouts differing only
in the archived replacement module, preserving the same lockfile/profile. Run
`python3 tools/bench-secret-https-kvm.py --baseline BASE --candidate CANDIDATE
--kernel KERNEL --initrd HTTPS_CLIENT_IMAGE --output NEW_DIRECTORY`. The directory
must not exist. Linux KVM, OpenSSL and ip are required. Source/build identity still
requires the external catalog/build evidence; the runner's hash gate is not proof
that two different executables implement the intended algorithms.
