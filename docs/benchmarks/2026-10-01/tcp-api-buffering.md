# TCP API socket buffering investigation

The native TCP benchmark exposed a repeatable approximately 40–50 ms delay
in HyperMachine's 1 MiB echo transactions. Investigation narrowed it to the
accepted API TCP connection. This change disables Nagle buffering on accepted
API sockets before HTTP or TLS takes ownership. The plaintext node and
control-plane listeners and their shared TLS server use the same setting.
It preserves authentication, routing, bounded copying and directional EOF.
The private adapter and vsock flow-control implementation are unchanged.

The previous adapter-only TCP_NODELAY candidate was rejected and remains
documented in [the original comparison](tcp-performance.md). Before trying
another production change, this investigation tested whether the guest echo
service's buffering caused the delay. Both engines used the same diagnostic
image and unchanged release daemon; only an explicit service option varied.

| Diagnostic run | Guest echo mode | HyperMachine 1 MiB P50 / P99 (ms) | Firecracker 1 MiB P50 / P99 (ms) |
|---|---|---:|---:|
| 1 | Default | 48.83 / 87.93 | 5.40 / 43.80 |
| 2 | TCP_NODELAY | 48.40 / 57.59 | 4.55 / 11.90 |
| 3 | TCP_NODELAY | 48.97 / 57.75 | 5.18 / 12.93 |
| 4 | Default | 48.35 / 56.29 | 4.77 / 44.70 |

All four runs passed 400/400 byte-verified transfers, guest preparation and
cleanup. Disabling service buffering left the HyperMachine median delay
present. The first diagnostic image hash is
`51b32b5940985289dd431964971c1746a7494e4fb8961a369bf93756bd355555`;
the original agent and init script remained identical.

A separate eight-transfer instrumented run enabled device tracing and
recorded client chunk arrivals. Its slow HyperMachine transaction received
65,536 bytes at 11.44 ms and the last 22,528 bytes at 55.56 ms: a 44.12 ms gap.
The device log accounts for 1,048,633 received bytes on that connection
(1,048,576 payload plus the 57-byte Forward acknowledgement) by
16:11:35.732346 UTC, with the window consumed by 16:11:35.732427. The host
close follows at 16:11:35.776948. This trace supported investigating the
API socket rather than changing credit accounting. Instrumented timings
are diagnostic evidence, not performance scores.

The final comparison uses the original fixture image, not either diagnostic
image. It brackets two candidate runs with baseline runs. Each run has
20 alternating engine pairs, five transfers at each size per guest, 1 vCPU,
1024 MiB, the same kernel and eight inherited host CPUs. Only the HyperMachine
daemon artifact differs between baseline and candidate. Startup and service
readiness are excluded; each transfer includes forwarding establishment.

| Run | Payload | HyperMachine P50 / P95 / P99 (ms) | Firecracker P50 / P95 / P99 (ms) | Verified transfers |
|---|---|---:|---:|---:|
| run-1-baseline | 4 KiB | 3.14 / 5.31 / 5.97 | 1.37 / 2.43 / 2.91 | 100/100 per engine |
| run-1-baseline | 1 MiB | 50.76 / 56.44 / 59.36 | 5.46 / 43.91 / 47.01 | 100/100 per engine |
| run-2-candidate | 4 KiB | 2.24 / 3.46 / 3.90 | 1.14 / 2.25 / 2.98 | 100/100 per engine |
| run-2-candidate | 1 MiB | 6.98 / 47.18 / 51.35 | 4.83 / 7.98 / 8.58 | 100/100 per engine |
| run-3-candidate | 4 KiB | 2.57 / 4.31 / 5.11 | 1.35 / 2.50 / 2.84 | 100/100 per engine |
| run-3-candidate | 1 MiB | 7.92 / 14.36 / 48.46 | 5.66 / 11.12 / 43.83 | 100/100 per engine |
| run-4-baseline | 4 KiB | 2.58 / 3.10 / 3.41 | 0.94 / 2.04 / 2.21 | 100/100 per engine |
| run-4-baseline | 1 MiB | 48.46 / 53.19 / 55.21 | 4.25 / 7.91 / 8.27 | 100/100 per engine |

All 1,600 transfers passed byte verification, preparation and cleanup.
The baseline 1 MiB medians were 48.46 and 50.76 ms; candidate medians were
6.98 and 7.92 ms. The observation supports a substantial median improvement
for this workload, while the candidate still trails Firecracker at the median
and retains large tail delays. It does not establish that every stall is fixed.

These are observations on a shared WSL nested-KVM host. CPU affinity is not
CPU isolation. Run coordinators record load averages; unrelated builds and
changing host load limit causal and tail-latency claims. This does not establish
a dedicated-host, concurrent-stream, managed-platform or across-the-board win.
Repeated transfers within a guest are correlated. Percentiles use nearest rank.
The native transport difference remains explicit: authenticated HTTP upgrade
for HyperMachine and a Unix vsock socket plus identical Forward RPC for
Firecracker. Aggregate request-plus-echo MiB/s is not sustained line-rate capacity.

The fixture now supports `--nodelay` and acknowledges it through its ready
marker. The harness's `--fixture-nodelay` option requires that acknowledgement.
Real KVM smoke checks verified both modes, 16/16 transfers each; a matched
negative control using the old image failed all four preparations with an
unsupported-mode error, attempted zero scored transfers and cleaned up all
guests. This prevents silently labeling an unsupported image as unbuffered.
The acknowledgement-enabled diagnostic image is separately identified as
`aafbedda60715278c742fb8b8e8499aeed3e785945d2c732fee9a91a849e5103`.

The final daemon SHA256 is
`03043e33a6efe017bf0f9d2f0d7ee9caf4e0c09705abeaf849f21825128f799d`;
the control-plane SHA256 is
`67059d67fc9dfc3a2d2fc55d958245dc5220bf48fb48bdb7475a8e6ede9a1029`.
The archived release build records exact final source hashes, compiler,
command, Cargo output and restoration of the original previously scored daemon.
An earlier build and smoke run are retained; formatting produced identical
binary hashes, and the full comparison uses the final build.

Final verification passed 89 Windows and 93 Linux regression tests across the
API, cluster, CLI and daemon, plus strict all-target Clippy on both platforms.
The four scorer checks also passed on both platforms. The final release node
and control plane passed all 15 real KVM/TLS lifecycle cases: authentication,
binary transfers, both EOF directions, repeated CLI interruption, idle-stream
activity, pause/resume, fork and deletion. All owned processes stopped and
zero sandboxes remained. No TLS performance score is inferred from these cases.

The [manifest](tcp-flow-manifest.json) indexes exact raw reports, compiled
sources, diagnostic/verification coordinators, image builds and decoded logs.
The original raw TCP and performance evidence remains intact. The original
TCP verifier and manifest are also preserved in this investigation archive;
its current index updates only the revised verifier's hash. Historical verifiers check
historical evidence; `verify-tcp.py --check-original-source` additionally checks
the earlier source revision, which is expected to differ after this fix.
The new verifier can check the final production source and staged evidence:

```sh
python3 tools/test-bench-tcp-local.py
python3 docs/benchmarks/2026-10-01/verify-tcp-flow.py --current --staged
python3 docs/benchmarks/2026-10-01/verify-tcp-performance.py
python3 docs/benchmarks/2026-10-01/verify-tcp.py --staged
```

Use [the original benchmark command](tcp-performance.md) with a new report
path to reproduce native measurements. Use the archived build/verification
coordinators to reconstruct the exact local comparison; paths must be adapted
to your host. TLS lifecycle verification uses the original fixture image and
the final node and control-plane binaries. TLS/control-plane performance,
concurrent transfer capacity and equivalent managed-service comparisons remain
unmeasured. Further work must address the remaining median gap to Firecracker
and the wider feature/performance gaps in [the platform table](../../PLATFORM_PARITY.md).
