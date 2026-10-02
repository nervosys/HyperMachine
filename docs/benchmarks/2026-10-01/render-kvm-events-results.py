import json
from pathlib import Path
root=Path('/mnt/c/Users/adamm/dev/nervosys/os/HyperMachine/docs/benchmarks/2026-10-01')
r=json.loads((root/'kvm-events-summary.json').read_bytes())
text='''# Preserve KVM vCPU event handoffs in snapshots

New KVM snapshots include the 64-byte x86 vCPU-event payload. Previously, capture kept the LAPIC's pending/in-service registers but omitted exception, interrupt and NMI injection state. A snapshot taken with an interrupt already queued for delivery could therefore restore its controller state while losing the handoff into the vCPU. The event payload is now applied after special registers, LAPIC and clock MSRs, before the guest resumes.

Capture failure is reported instead of silently omitting this state. Restore validates payload length before changing vCPU registers, preserves the getter's capability-dependent flags, and enables restoration of captured NMI-pending/SIPI fields. These flags follow the [KVM get/set event API](https://docs.kernel.org/virt/kvm/api.html#kvm-get-vcpu-events) and [x86 UAPI definitions](https://github.com/torvalds/linux/blob/master/arch/x86/include/uapi/asm/kvm.h). Fields are encoded explicitly without reading Rust padding.

Older snapshots remain readable through an empty default payload and retain their prior restore behavior. They cannot recover event state never stored: recapture or rebuild them with the updated runtime to obtain this preservation. Other backends keep their existing behavior. The capture still does not claim every possible model-specific register or universal snapshot completeness.

## Direct correctness evidence

The explicit real-KVM regression test restores four captured states: an injected interrupt, pending/masked NMI, interrupt shadow, and an injected exception with its error code. Hardware readback matches the captured payload in every case. A tiny real-mode guest strengthens the interrupt case: with the payload restored its interrupt handler outputs 0x22; with the same payload omitted, the main path outputs 0x11. The omission control also reads back no injected interrupt. This proves the specific handoff repair independently of a timing sample or intermittent application timeout.

The [raw direct test](kvm-events-direct-test.txt), [build/test metadata](kvm-events-build-metadata.json), and exact [backend](kvm-events-backend.rs), [FFI](kvm-events-ffi.rs), and [snapshot model](kvm-events-snapshot.rs) identify the implementation tested. Run `cargo test --locked -p hv2-core --lib captured_kvm_events_restore_pending_handoffs -- --ignored --nocapture` on a KVM host. The explicit test fails if KVM is unavailable; regular generic CI leaves it ignored. All 99 snapshot and seven regular KVM regressions passed on Linux, the direct test passed explicitly, strict Clippy passed, and eight snapshot serialization/legacy tests passed on Windows. The [real guest state check](kvm-events-state.json) also passed checkpoint rollback, pause/resume and two state-preserving forks with clean teardown. A second [shipped checkpoint check](kvm-events-state-header.json) confirms the persisted file contains one 64-byte event payload, followed by the same successful rollback/resume/fork checks.

## Matched stateful comparison

The [matrix](kvm-events-results-matrix.json) retains all 2016 attempts across sixteen fresh cohorts. The [coordinator](kvm-events-stateful-coordinator.py) and [driver](kvm-events-matrix.py) pair the saved parent runtime with the event-preserving runtime in alternating adjacent order. Parent production Rust matches commit 5c91bef; intervening commits through 9f48750 changed benchmarks/docs only. Both builds use the same lockfile, compiler and release flags; the parent checkout path can differ in embedded source paths. Original scored binaries are retained. After restoring the canonical scored binary, the daemon entry timestamp was advanced so the next Cargo build relinks rather than treating that retained binary as a fresh current build.

Both runtimes use the same [official-SDK harness](kvm-events-sdk-harness.py), e2b 2.51.0 and previously recorded dependencies, one-vCPU/1024 MiB BusyBox guests, identical kernel/initrd, eight host CPUs and one pinned busy worker. Guests are prepared before each measured resume/fork batch. Successful samples verify the live process, memory marker, boot ID and private filesystem state; forks additionally require independent child writes and unchanged parent state. Start spreads, raw failures, unchanged artifacts, empty-node cleanup and worker liveness/reaping remain checked. The [analysis](analyze-kvm-events.py) excludes incomplete profile pairs from mean timing deltas while retaining every failure.

The parent passed **1007/1008**; the updated runtime passed **1008/1008**. The parent concurrency-1 resume failure retains SDK/coordinator exit 1, `success:false`, its complete diagnostic and 30543.13 ms failed operation interval. The guest-agent readiness deadline remained 15 seconds. No failed sample was converted into a success or excluded from counts. Percentiles below are conditional on successful samples; the incomplete concurrency-1 resume pair has no paired mean delta.

| Operation | Concurrency | Parent passed | Updated passed | P50 ms, parent → updated | P99 ms, parent → updated | Max ms, parent → updated |
|---|---:|---:|---:|---:|---:|---:|
'''
for c in (1,8,50,100):
 for op in ('resume','fork'):
  groups={p:next(g for g in r['profiles'][p] if g['concurrency']==c and g['operation']==op) for p in ('baseline','events')}
  a,b=groups['baseline'],groups['events']
  text+=f"| {op} | {c} | {a['passed']}/{a['attempts']} | {b['passed']}/{b['attempts']} | {a['ready_ms']['p50']:.2f} → {b['ready_ms']['p50']:.2f} | {a['ready_ms']['p99']:.2f} → {b['ready_ms']['p99']:.2f} | {a['ready_ms']['max']:.2f} → {b['ready_ms']['max']:.2f} |\n"
text+='''
At concurrency 100, updated resume mean readiness was 18.71 ms higher and its P99 was higher. Fork mean/P99 were lower in that pair, but its maximum was higher (2126.93 versus 1401.76 ms). One fresh cohort per profile/operation/concurrency, with only two dependent batches at concurrency 50/100, cannot establish a repeatable speed improvement.

The code is retained for the deterministic correctness repair. The pass-count difference does not prove it caused either the earlier fork timeout or this parent resume timeout, and does not establish a reliability SLA. The parent resume diagnostic has a halted vCPU, LAPIC ISR vector 0xec and pending lower-priority work; the actual pre-restore event payload for that failed snapshot was not captured for replay. No causal claim is made from the interrupt dump alone.

These are shared nested WSL/KVM measurements with the SDK client on the same host, warmed templates and observed client scheduling spreads. Bare-metal runs, full synchronized pause bursts, long-duration application state and matched competitor stateful benchmarks remain missing. No competitor or across-the-board performance win is established.
'''
(root/'kvm-events.md').write_text(text)