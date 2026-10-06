# Deferred native UDP reply-buffer reuse candidate

The isolated candidate retains one reply Vec per peer instead of allocating a fresh Vec for each inbound frame. Frame size validation, exact reads, zero-length payloads and UDP send-length checks remain unchanged. Root production source is unchanged; the candidate is deferred.

Five targeted native UDP tests pass. Eight actual Redis/mTLS/KVM profiles use baseline/candidate/candidate/baseline order for each ingress family. Every profile passes its functional and lifecycle checks, leaves zero guests and reaps all owned processes. Node, control, CLI, kernel, guest image, checker and resource configuration are identical; only the native gateway executable differs. Both gateways are development builds from the same isolated checkout and baseline full build invocation. Rebuilding the candidate with that exact full invocation produces a byte-identical candidate binary (candidate-matched-build.log); the baseline build log is retained. Protected modified root core is excluded.

Each profile internally runs CLI/native/native/CLI blocks. The analyzer uses only the two native blocks: 1,000 timed exact 4 KiB exchanges per peer per block after ten warmups, one outstanding exchange per peer, no retries. Native TCP remains idle; both paths remain live; host CPUs are unpinned. These are internal candidate measurements, not competitor benchmarks. Results average block rates and per-peer percentile values across two runs per variant; they are not pooled percentiles or confidence intervals.

| Metric | IPv4, two peers: baseline | Candidate | Change | IPv6, eight peers: baseline | Candidate | Change |
|---|---:|---:|---:|---:|---:|---:|
| Mean native round trips/s | 1812.27 | 1844.30 | +1.77% | 3919.57 | 3934.41 | +0.38% |
| Mean peer median, ms | 1.07950 | 1.06452 | -1.39% | 1.96185 | 1.95854 | -0.17% |
| Mean peer P99, ms | 1.52079 | 1.47884 | -2.76% | 3.44783 | 3.42144 | -0.77% |

These small development-profile differences do not establish a reliable performance gain. Reuse also retains each peer's largest buffer (up to 65,507 bytes) until its session ends. No retained-memory measurement or matched release comparison was performed. Production is left unchanged and the isolated source is restored after preserving the candidate. Release-profile repeats and memory measurements would be required before promotion.

source-context.json records matching baseline permitted sources, accepted isolated core hashes and the single candidate source hash. Both full native_udp.rs snapshots and candidate.patch are preserved. Runtime reports hash all executable/fixture inputs before/after. No private keys or policies are archived; runtime logs exclude generated credentials. manifest.json covers the reports, logs, snapshots, analyzer and summaries.

Reproduce using the gateway binaries built separately from the baseline snapshot and candidate.patch with the full build invocation shown in the logs. Run the KVM checker with --tls --mtls --owner-context --owner-port-api --owner-port-cli --native-comparison-samples 1000 --native-comparison-bytes 4096 and fresh output directories, alternating baseline/candidate/candidate/baseline. Use the accepted node/control/CLI/kernel/image identities in the reports. Add --local-ipv6 --peer-count 8 for the second family. To regenerate a family summary from this directory:

```sh
python3 analyze-native-gateway-candidate.py --baseline ipv4/0-baseline/report.json --candidate ipv4/1-candidate/report.json --candidate ipv4/2-candidate/report.json --baseline ipv4/3-baseline/report.json --output /fresh/analysis-ipv4.json
```

Use the ipv6 paths for the second summary. The analyzer rejects differing fixture hashes, ingress families, peer counts, payload sizes, sample counts and incomplete cleanup. Absolute report paths in a fresh summary may differ from the archived relative-path normalization; numeric summaries remain reproducible.
