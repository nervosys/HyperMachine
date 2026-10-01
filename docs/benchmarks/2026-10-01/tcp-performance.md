# Matched native TCP forwarding transactions

HyperMachine trails Firecracker at the median for both payload sizes in
these local runs. A proposed loopback-buffering change did not establish
an improvement and was reverted. No production performance change ships
with this benchmark.

Each run uses 20 pairs of fresh guests, alternating engine order, with five
transactions at each payload size per guest. Both engines use the same
kernel, fixture image, 1 vCPU and 1024 MiB. Each successful run verifies
400/400 transactions, exact binary contents, all guest preparations and
cleanup, zero remaining sandboxes and unchanged artifacts. Repeated rows
within a guest are correlated. Percentiles use nearest rank.

| Run | Payload | HyperMachine transaction P50 / P99 (ms) | Firecracker transaction P50 / P99 (ms) | Verified transfers |
|---|---|---:|---:|---:|
| Initial streaming baseline | 4 KiB | 4.10 / 12.45 | 2.36 / 13.87 | 100/100 per engine |
| Initial streaming baseline | 1 MiB | 51.60 / 91.71 | 8.43 / 48.39 | 100/100 per engine |
| Baseline with client TCP_NODELAY | 4 KiB | 4.71 / 8.38 | 2.83 / 66.55 | 100/100 per engine |
| Baseline with client TCP_NODELAY | 1 MiB | 53.99 / 89.05 | 9.77 / 303.87 | 100/100 per engine |
| Rejected adapter TCP_NODELAY candidate, client TCP_NODELAY | 4 KiB | 3.12 / 12.44 | 1.50 / 19.74 | 100/100 per engine |
| Rejected adapter TCP_NODELAY candidate, client TCP_NODELAY | 1 MiB | 48.55 / 95.60 | 6.19 / 70.52 | 100/100 per engine |

These are separate runs on a shared WSL nested-KVM host with eight inherited
host CPUs, not CPU isolation. Unrelated builds were observed. Changing host
load and Firecracker's varying tails prevent attributing differences between
runs to the candidate. The adapter change left the large-transfer median
delay present, so it was not accepted as a performance fix.

Timing begins before establishing each new forwarding connection and ends
after receiving and checking the expected byte count and joining the writer.
Guest creation, readiness commands and starting the fixture are excluded.
HyperMachine uses an authenticated loopback HTTP/1.1 upgrade; Firecracker
uses its native Unix vsock socket followed by the identical guest Forward
RPC. The difference in host transports is explicit: this measures the
available native forwarding paths, not equivalent managed-platform APIs.
Both use the same streaming service on guest port 18082. Payload markers
identify the pair, round and size and match between engines.

Raw reports include handshake, transfer and total transaction distributions.
Aggregate MiB/s counts request plus echo bytes and includes the writer-thread
overhead. It is not sustained line-rate capacity. TLS, the control plane,
the CLI, concurrent transfer load and fleet capacity are excluded.

The first smoke attempt used the EOF-buffered service on port 18080.
HyperMachine passed all eight transfers; Firecracker returned zero response
bytes in all eight after the host write half-close. All guest preparations
and cleanup passed. That failed attempt is retained in
[tcp-perf-smoke-1.json](tcp-perf-smoke-1.json); it is not a scored performance
run or a claim about every Firecracker TCP configuration. Its original
harness hash is recorded, but that initial harness source was not archived.
The subsequent streaming smoke passed 16/16 and its exact harness is archived.

The [evidence manifest](tcp-perf-manifest.json) covers raw reports, exact
streaming harness versions, imported helper sources, baseline/candidate
release-build metadata and their compiled source bytes. Both builds preserved
the original previously scored daemon. The candidate's exact uncommitted
source is retained even though it was reverted. The daemon release hashes are:

| Artifact | SHA256 |
|---|---|
| Baseline daemon | `1eacfe17e8a43e4273b9b7dea201d48444e931d7da1a7009a8a126d66ce39a86` |
| Rejected candidate daemon | `fe2262c194135bff1dc3b2377de98dd08cf807fcc2c8717966e69686900e95f9` |
| Matched kernel | `afaa2129c3eacc519fd1ca35fe8bfc47e6c44251b5840d504f1e140705daebdd` |
| Matched fixture initrd | `f226c599d385609fc03af2223b6829a5249b28193c96179daeb7d47383005d1b` |

Reproduce on Linux with KVM and the fixture image documented in
[the TCP functional verification](tcp-tunnel.md):

```sh
python3 tools/bench-tcp-local.py \
  --hypermachine /path/to/release/hv2-sandboxd \
  --firecracker /path/to/firecracker-v1.17.0-x86_64 \
  --kernel /path/to/bzImage --initrd /path/to/guest-tcp.cpio.gz \
  --pairs 20 --rounds 5 --environment 'describe host and affinity' \
  --output /path/to/new-unmodified-report.json
python3 tools/test-bench-tcp-local.py
python3 docs/benchmarks/2026-10-01/verify-tcp-performance.py
```

The four scorer tests pass on Windows and Linux and are added to the sandbox
tool CI gate. They verify binary transfers larger than socket buffers and
reject corrupted responses, premature EOF and handshake failure. Existing
TCP functional/lifecycle verification remains applicable to the unchanged
production implementation. Dedicated-host repetition and profiling the
1 MiB delay remain required before claiming a performance improvement.
