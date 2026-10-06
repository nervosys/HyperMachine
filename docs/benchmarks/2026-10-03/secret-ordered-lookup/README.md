# Ordered placeholder lookup

Validated bindings are now sorted once by their unique fixed-width placeholder.
Filtering by authenticated host preserves this order. Requests with up to eight
eligible bindings retain linear lookup; larger sets use binary search over the
eligible references. Host scope, output bounds and one-pass replacement remain.
No extra secret copies or per-token host lookups are introduced.

Four alternating release binary pairs compare with the previously verified bulk
copy candidate. Compilation finished before measurement. Input/output contracts
match across all eight cases. Each run has nine batch averages of ten replacements.
The table reports the median of four run medians, not service P99.

| Case | Bulk-copy baseline us | Ordered candidate us | Observed change |
|---|---:|---:|---:|
| small-header | 0.05 | 0.05 | +0.0% |
| plain-64k | 1.22 | 1.25 | +2.4% |
| plain-1m | 20.34 | 20.17 | -0.8% |
| unscoped-1m | 13.31 | 13.34 | +0.2% |
| sparse-1m | 25.54 | 21.80 | -14.6% |
| dense-one | 43.27 | 43.55 | +0.6% |
| dense-last-of-128 | 3842.64 | 649.18 | -83.1% |
| dense-unmatched | 3861.55 | 743.87 | -80.7% |

Dense 128-binding matches and misses show large component reductions; sparse
inputs also improve in this sample. Non-lookup-heavy cases are broadly unchanged
and include small regressions. Their small percentage shifts are not reliable
proof of improvement or regression on an unpinned WSL process. Policy validation
and one-time sorting costs are outside the timed region; startup/reload latency
and complete HTTP/TLS latency are not measured. No across-the-board win is claimed.

All 190 networking tests pass. A new test loads all 128 placeholders in reverse
order with two distinct host scopes and checks every expected binary value/token.
The previous 516 binary/partial-prefix boundary combinations still pass. A
separate candidate daemon passes fourteen real KVM checks with full 128-binding
policies, nineteen successful owned HTTPS requests and pre-HTTP hostname refusal.
The exercised real token sorts after all 127 dummy placeholders. Rotation,
revocation, body formats, fork exclusion and resume remain covered.

The tested module/example match workspace and isolated checkout bytes. The full
isolated source catalog identifies both builds; protected workspace core was
neither read nor built. Accepted VM performance binaries/images are unchanged.
Output checksums are non-cryptographic comparison aids; tests supply separate
correctness evidence. No competitor, fleet or production performance is measured.

Reproduce with the prior bulk-copy release binary and this release example:
`python3 tools/bench-secret-rewriting.py --baseline BULK --candidate ORDERED
--output NEW.json`. Run cargo test -p hv2-net, build a separate daemon, and run
`tools/check-secret-substitution-kvm.py --bindings 128` with its required input
arguments. Full example/build inputs are identified by the source catalog.
