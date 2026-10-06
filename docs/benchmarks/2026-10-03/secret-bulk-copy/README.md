# Bounded bulk-copy secret replacement

The raw replacement loop now copies ordinary byte runs up to the next possible
'h' prefix using memchr, while retaining exact placeholder matching, host scope,
one-pass insertion and output bounds. With no eligible host binding it returns
a bounded input copy. The direct memchr dependency uses the existing locked
2.8.3 version. No token-index or protocol change is introduced.

Four alternating baseline/candidate pairs use the frozen original release binary
and a checked candidate on identical synthetic cases. Each run records nine
batch averages of ten replacements. All input/output size and checksum contracts
match; checksums are non-cryptographic comparison aids, not correctness proofs.
The headline run began after owned build and KVM processes terminated. An earlier
run concurrent with the daemon build is retained as excluded-build-contended.json
and excluded from the table. External load and CPU placement remain uncontrolled.

| Case | Baseline us | Candidate us | Observed change |
|---|---:|---:|---:|
| small-header | 0.12 | 0.05 | -58.2% |
| plain-64k | 37.38 | 1.22 | -96.7% |
| plain-1m | 647.03 | 19.78 | -96.9% |
| unscoped-1m | 634.31 | 13.31 | -97.9% |
| sparse-1m | 637.36 | 25.04 | -96.1% |
| dense-one | 43.88 | 42.94 | -2.1% |
| dense-last-of-128 | 3888.83 | 3882.72 | -0.2% |
| dense-unmatched | 4583.83 | 3896.73 | -15.0% |

Values are medians of four per-run medians. Plain/sparse body reductions are large
in this component fixture. Dense matched tokens are essentially unchanged; their
small percentage shifts do not establish a reliable improvement. The remaining
128-binding linear lookup is still a performance target. An unscoped library
call's speedup need not translate to a gateway request, which can bypass rewriting.

All 189 networking tests pass, including 516 partial-prefix/binary boundary
combinations. The separate candidate daemon passes fourteen real KVM checks,
nineteen successful HTTPS requests and a pre-HTTP upstream hostname refusal.
Body formats, bounds, binary preservation, rotation and lifecycle behavior retain
coverage. The full isolated source catalog identifies both builds. Protected
workspace core sources were neither read nor built; accepted VM performance
binaries and images were not replaced.

This measures raw in-process replacement, not JSON parsing, complete HTTP/TLS,
KVM request latency, concurrency, service P99 or managed competitor performance.
No across-the-board product win is claimed. Sources/tests are retained for review;
no commit, deployment or external publication occurred.

Reproduce each release example from its archived source/lock in the isolated
accepted-core checkout; copy the baseline/candidate binaries separately. Run
`python3 tools/bench-secret-rewriting.py --baseline BASE --candidate CANDIDATE
--output NEW.json`. Run cargo test -p hv2-net and the archived KVM checker against
a separately built daemon for correctness. Output paths must not exist.
