# Concurrent TCP transactions and guest listener backlog

The benchmark now supports synchronized streams against each ready guest.
It retains every attempted row and excludes failures from latency summaries
without treating a partial run as successful. Distinct per-client markers
verify binary data independently; thread scheduling after the start barrier,
handshake, transfer and writer completion are included in transaction time.
The six scorer tests pass on Windows and Linux, including simultaneous
distinct streams and retaining all connection refusals.

A second private-adapter TCP_NODELAY experiment combined that setting with
the already-verified API socket fix. Sequential 1 MiB P99 remained 51.19 and
54.44 ms in candidate runs, versus 49.79 and 51.17 ms in bracketed baseline
runs. The candidate was reverted. Its compiled source, release identity and
all sequential/concurrent results remain archived; no adapter performance
change ships here.

The first eight-stream comparisons exposed early Firecracker native handshake
closures. The shared HyperMachine guest agent used a vsock listen backlog of
four, smaller than the simultaneous connection burst. HyperMachine's API
currently serializes guest-forward establishment under its lifecycle lock;
Firecracker's native clients open guest connections concurrently. Therefore
these failures cannot be presented as a general Firecracker reliability result.

The guest-agent backlog is increased from four to a bounded 128 pending
connections. A static new agent was inserted into a copy of the original
fixture image. Independent CPIO decoding verified identical entry sets and
metadata, with only `bin/hv2-guest-agentd` contents changed. The init script,
echo fixture, BusyBox and kernel are unchanged. Each image is used identically
on both engines. The node remains the accepted API-buffering release, not
the rejected adapter candidate.

| Bracketed run | Guest backlog | HyperMachine verified transfers | Firecracker verified transfers | Failed native handshakes |
|---|---:|---:|---:|---:|
| 1, old-image control | 4 | 480/480 | 462/480 | 18 |
| 2, new agent | 128 | 480/480 | 480/480 | 0 |
| 3, new agent | 128 | 480/480 | 480/480 | 0 |
| 4, old-image control | 4 | 480/480 | 461/480 | 19 |

All guest preparations and cleanup passed, all node processes stopped and
zero sandboxes remained, including failed controls. No failed handshake was
retried inside its scored transfer. Both larger-backlog runs passed 960/960;
the 37 failed old-backlog attempts are retained. This supports correcting the
shared guest listener's burst limit, not an engine or platform reliability SLA.

Each run uses 10 pairs of fresh guests, alternating engine order, three
rounds at 4 KiB and 1 MiB, with eight streams per guest per size. Guests have
1 vCPU and 1024 MiB; the client, daemon and VMM inherit eight host CPUs.
Startup and service preparation are excluded. There is no steady arrival
rate, overload schedule, fleet scaling or sustained-throughput claim.

| Run | Payload | HyperMachine P50 / P95 / P99 (ms) | Firecracker P50 / P95 / P99 (ms) | Successful rows HM / FC |
|---|---|---:|---:|---:|
| run-1-backlog4 | 4 KiB | 16.99 / 35.39 / 53.72 | 19.28 / 44.29 / 98.48 | 240/240 / 237/240 |
| run-1-backlog4 | 1 MiB | 75.44 / 153.00 / 187.45 | 103.76 / 247.05 / 335.83 | 240/240 / 225/240 |
| run-2-backlog128 | 4 KiB | 16.08 / 38.27 / 62.51 | 21.29 / 34.09 / 52.52 | 240/240 / 240/240 |
| run-2-backlog128 | 1 MiB | 68.80 / 171.49 / 261.92 | 82.59 / 176.14 / 202.98 | 240/240 / 240/240 |
| run-3-backlog128 | 4 KiB | 14.15 / 29.50 / 39.49 | 20.17 / 40.47 / 119.00 | 240/240 / 240/240 |
| run-3-backlog128 | 1 MiB | 71.26 / 155.74 / 191.60 | 88.50 / 218.60 / 287.04 | 240/240 / 240/240 |
| run-4-backlog4 | 4 KiB | 19.30 / 48.98 / 84.97 | 23.78 / 46.61 / 52.72 | 240/240 / 234/240 |
| run-4-backlog4 | 1 MiB | 107.86 / 185.98 / 218.85 | 119.93 / 300.55 / 308.49 | 240/240 / 227/240 |

Old-image latency summaries are conditional on success and must be read
with the failure counts; they are not successful full-run performance scores.

The larger-backlog runs observed lower HyperMachine medians for both sizes,
but tail ranking varies. These are native end-to-end transactions including
Python client and scheduling costs on a shared WSL nested-KVM host, not an
isolated engine-throughput ranking. Host load varied substantially between
the earlier adapter comparisons and these controls. They must not be merged
into a single workload-independent latency comparison. Repeated transactions
within a guest are correlated; percentiles use nearest rank. Managed products,
TLS performance and across-the-board wins remain unmeasured. A new sequential run with the larger-backlog image passed 400/400 transfers:
4 KiB P50/P99 was 4.26/24.18 ms for HyperMachine versus 2.51/13.26 ms for
Firecracker; 1 MiB was 13.01/54.09 ms versus 12.95/90.23 ms. This does not
establish a sequential median win. The historical
[API comparison](tcp-api-buffering.md) remains separately recorded.

The new agent SHA256 is
`5a684465b96c3ede662317cb99b0badff1b9d591017fe08bae12ae648d98f3c7`.
The new initrd SHA256 is
`7d36dd6384504ec60abf2e69f9d78941cfae8d96110d9e05f5d45ce87fdd1224`;
the preserved old initrd is
`f226c599d385609fc03af2223b6829a5249b28193c96179daeb7d47383005d1b`.
The [manifest](tcp-concurrent-manifest.json) indexes exact compiled source,
image-build output, image verification, runtime logs, successful and failed
transfer rows, and all coordinators. The initial coordinator stopped at a
failed concurrent run; a separate coordinator completed the remaining planned
runs. Both transcripts are retained. The backlog verification coordinator
records successful completion separately from its failed negative controls.

The new guest image passed all 15 real KVM/TLS lifecycle cases with the
accepted node/control-plane releases: authentication, binary transfers,
directional EOF, repeated CLI interruption, idle-stream activity, pause/resume,
fork and deletion. Linux guest tests passed 17/17; Windows protocol tests
passed 15/15. Strict guest-agent Clippy and all six scorer checks passed on
both platforms. Windows protocol test success was observed from the command
output; the archive retains complete Windows lint/scorer and Linux guest-test
transcripts. No host/API source change ships in this milestone.

Reproduce with a KVM host and matched larger-backlog image:

```sh
python3 tools/bench-tcp-local.py \
  --hypermachine /path/to/release/hv2-sandboxd \
  --firecracker /path/to/firecracker-v1.17.0-x86_64 \
  --kernel /path/to/bzImage --initrd /path/to/guest-tcp.cpio.gz \
  --pairs 10 --rounds 3 --concurrency 8 \
  --environment 'describe host and affinity' --output /path/to/new-report.json
python3 tools/test-bench-tcp-local.py
python3 docs/benchmarks/2026-10-01/verify-tcp-concurrent.py --current --staged
```

Adapt the archived static-agent builder and image paths to reconstruct the
image and run the bracketing controls. Higher stream counts, dedicated hosts,
arrival-rate load and multi-node or managed-product comparisons remain required.
