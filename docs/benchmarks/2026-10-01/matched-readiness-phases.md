# Corresponding native readiness phases (2026-10-01)

One diagnostic pair ran 16 cold attempts per engine at five arrivals/s, eight
client workers and eight pinned host CPUs, with the same images, guest resources
and retained event-preserving HyperMachine build as the preceding comparison.
All 32 attempts passed with unchanged artifacts and no sandbox records left.
HyperMachine readiness tracing was enabled, so this is diagnostic evidence,
not a scored performance comparison. The single order was HM then Firecracker.

| Engine / measured phase | Mean (ms) |
|---|---|
| HyperMachine HTTP create through ready response | 368.05 |
| HyperMachine HTTP command through output | 6.21 |
| Firecracker process/API setup through InstanceStart | 16.46 |
| Firecracker agent connection and readiness ping | 319.96 |
| Firecracker command on that connection through output | 1.15 |

The HyperMachine internal logs separately measured mean blocking-worker queue
0.07 ms, connection wait 346.43 ms and ping 1.16 ms. Connection waits include
guest boot, not just handshake overhead. These phase boundaries do not cover
equivalent work in isolation: HTTP create includes VM setup and host service
registration, while Firecracker's setup and connection are separate. Both
end-to-end attempts still require matching guest command output and resources.

Command transport also differs: Firecracker reuses its readiness connection,
while HyperMachine opens a new guest-agent connection behind HTTP. The roughly
5 ms command difference is consistent with several possible costs, including
HTTP and reconnect behavior; this run does not identify which caused it. The
host agent currently backs off 5 ms after a refused connection. A matched
current-source baseline is being prepared for a bounded refusal-backoff
candidate; no candidate is adopted or improvement claimed here.

The shared harness now records create/exec phases for HyperMachine and API
setup/connect/exec phases for Firecracker, plus phase and elapsed time on an
attempt failure. Phase sums are checked against end-to-end durations in the
analyzer. Seventeen harness tests pass on Windows and Linux. Exact executed
source, raw report, execution manifest and phase summary are retained beside
this report. Longer loads, repeated orders and transport-specific instrumentation
are needed before attributing a performance difference to a runtime mechanism.
