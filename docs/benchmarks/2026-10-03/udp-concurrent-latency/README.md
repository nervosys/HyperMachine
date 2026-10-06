# Two-peer UDP HTTPS/mTLS/KVM characterization

Two fresh runs each measure 1,000 tagged, exact-byte 64-byte request/reply samples per peer after ten warmups per peer. Two local CLI sockets operate concurrently, each with one outstanding request. All peer/sequence tags return to the right socket. Existing full-stack correctness, trust, malformed-frame and lifecycle checks also pass; owned processes are reaped and guest inventory is empty.

| Run | Peer | P50 ms | P95 ms | P99 ms |
|---|---:|---:|---:|---:|
| First | 0 | 1.088 | 1.316 | 1.547 |
| First | 1 | 1.087 | 1.317 | 1.556 |
| Repeat | 0 | 1.060 | 1.297 | 1.549 |
| Repeat | 1 | 1.058 | 1.276 | 1.487 |

Aggregate verified completion rates, including warmups and executor setup, are 1,612 and 1,705 round trips/second. These are closed-loop diagnostic completion rates, not maximum throughput or sustained capacity. P99 is nearest rank over each peer's 1,000 samples. Host CPUs are unpinned, guest uses one vCPU/1 GiB, and binaries are development builds on local WSL. The control API uses HTTPS and the node hop uses mTLS. Runs are short and do not model fleet load or deliberate packet loss.

Reproduce check-udp-cluster-kvm.py with --tls --mtls --concurrent-samples 1000 and recorded inputs, then repeat with a fresh output. Raw samples and unchanged input hashes are archived. The earlier single-peer measurements used an HTTP node hop, so no causal scaling comparison is made. No competitor endpoint or native UDP baseline is measured; no superiority or code speedup is claimed.
