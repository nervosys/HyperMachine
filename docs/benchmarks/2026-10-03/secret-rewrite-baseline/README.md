# Raw secret replacement performance baseline

The release-mode synthetic component benchmark records eight cases, nine samples
per case and ten replacements per sample, after ten correctness/warm-up calls.
Input sizes, binding counts, output sizes and a non-cryptographic output checksum
are recorded with every case. The complete source catalog and release binary
hash are in build-context.json; the tested module/example match workspace bytes.
Only isolated accepted core sources were compiled; protected workspace core
files were neither read nor built. Accepted daemon/guest benchmark inputs are
unchanged. The separate local baseline binary is retained for comparison.

| Case | Median per replacement (us) |
|---|---:|
| small-header | 0.13 |
| plain-64k | 37.62 |
| plain-1m | 634.10 |
| unscoped-1m | 643.15 |
| sparse-1m | 639.72 |
| dense-one | 44.01 |
| dense-last-of-128 | 3865.96 |
| dense-unmatched | 4507.04 |

Plain and sparse 1 MiB bodies currently scan byte by byte. Dense matching at the
last of 128 bindings and unmatched prefixes perform many token comparisons. These
measurements select an optimization target; they establish no improvement yet.
The benchmark's warm-up checks repeat the implementation against its initial
output; this is not independent proof of algorithm correctness. Output checksums
are comparison aids, not cryptographic integrity or correctness proofs.

This is one unpinned process on nested WSL, fixed case order and synthetic raw
replacement only. No HTTP framing, JSON parsing, TLS, guest, concurrency or
managed competitor timing is measured. Nine batch averages are not service P99.
No product throughput, fleet latency or competitor superiority is claimed.

Reproduce in the isolated accepted-source checkout: `cargo build --release
-p hv2-net --example secret_substitution_bench`, then run the resulting example.
Debug builds are refused. Record source/binary hashes and compare all output
contracts before interpreting a candidate's timings.
