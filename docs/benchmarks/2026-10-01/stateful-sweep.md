# Synchronized SDK resume and fork sweep

`tools/bench-e2b-sdk.py --synchronized-operation-batches` prepares an entire batch before timing resume or fork. Resume peers are verified paused; fork parents have already passed the live-state probe. Preparation failure aborts the barrier and releases pending peers to cleanup. Default rolling samples remain available, and synchronized create is rejected. Each measured operation records its client start offset, and reports include the actual start spread. The adapter now retains bounded, API-key-redacted fork exception details and failed-interval elapsed time with its timing origin. Nineteen adapter regressions pass on Windows and Linux; CI already runs this suite.

The [executed SDK harness](stateful-sdk-harness.py) is archived exactly; its bytes predate the subsequent fork-exception and failed-interval diagnostic improvements. The [coordinator](stateful-sweep-coordinator.py), [matrix](stateful-sweep-matrix.py), [dependency lock](stateful-sdk-requirements.txt), [raw matrix with hashes and exits](stateful-results-matrix.json), and [analysis](analyze-stateful-sweep.py) identify the actual run. The daemon is the current-parent `5c91bef` build from the [matched build metadata](boot-sizing-build-metadata.json), with unchanged production Rust code. No rejected allocation candidate was used.

Each cohort starts a fresh snapshot-backed node with the fixed UART IRQ kernel and output-draining BusyBox initrd, one vCPU and 1024 MiB per guest. Both images and the daemon are fingerprinted. The Linux client uses official e2b 2.51.0 with the recorded dependency versions. Eight host CPUs and one pinned busy worker match the cold-burst host profile. This remains shared nested WSL/KVM, with the client and node on the same host.

A live `sleep` process carries a unique environment memory marker; a private file records the marker, PID and boot ID. These are verified before the operation and again by the first readiness command. Fork success additionally requires a distinct child ID, an independent child write, and unchanged parent live-process/file state. The timed interval stops at the first state-verified command; the subsequent parent/child isolation checks must still pass for the sample to count. Resume's pause preparation latency is recorded separately and is not a synchronized pause-burst measurement.

## Results

The expanded sweep retains all **1008 attempts: 1007 passed, one failed**. Resume passed 504/504; fork passed 503/504. The failed concurrency-1 fork retains `success:false`, SDK and coordinator exit 1, its raw sample and node diagnostic log. All eight cohorts passed artifact identity, empty-node cleanup, owned worker liveness and reaping checks. SDK retries were disabled. Successful timing percentiles exclude the failed attempt and cannot hide its failure rate.

| Operation | Concurrency | Batches | Passed/attempted | P50 readiness ms | P95 ms | P99 ms | Maximum client start spread ms |
|---|---:|---:|---:|---:|---:|---:|---:|
| [resume](stateful-final-resume-c1.json) | 1 | 100 | 100/100 | 113.22 | 117.39 | 141.52 | 0.00 |
| [fork](stateful-final-fork-c1.json) | 1 | 100 | 99/100 | 116.12 | 125.43 | 129.33 | 0.00 |
| [fork](stateful-final-fork-c8.json) | 8 | 13 | 104/104 | 136.60 | 154.18 | 162.78 | 2.89 |
| [resume](stateful-final-resume-c8.json) | 8 | 13 | 104/104 | 133.24 | 148.67 | 150.39 | 2.86 |
| [resume](stateful-final-resume-c50.json) | 50 | 2 | 100/100 | 396.96 | 635.97 | 641.25 | 21.64 |
| [fork](stateful-final-fork-c50.json) | 50 | 2 | 100/100 | 446.51 | 499.78 | 511.59 | 16.50 |
| [fork](stateful-final-fork-c100.json) | 100 | 2 | 200/200 | 902.48 | 976.16 | 1064.27 | 39.43 |
| [resume](stateful-final-resume-c100.json) | 100 | 2 | 200/200 | 692.16 | 1043.33 | 1089.49 | 37.26 |

At concurrency 100, resume P99 was 1089.49 ms and fork P99 1064.27 ms, conditional on successful samples. The initial smaller sweep retained 636/636 passes, plus two passing smoke samples; those exact raw reports and executed sources remain under the `stateful-initial-` prefix, including [its matrix](stateful-initial-matrix.json). The expanded run does not replace or discard those observations.

## Failure and remaining work

In [fork concurrency 1, sample 8](stateful-final-fork-c1.json), the child missed the unchanged 15-second guest-agent readiness deadline. The node log records a halted vCPU at `default_idle`, CR8=1, LAPIC ISR/IRR containing vector 0x22 and a TSC-deadline timer. These are observations, not a root-cause determination. The original harness returned a generic failed-fork message; the node diagnostic preserves the underlying readiness failure. Future runs now retain the SDK's fork exception details too.

Source review found that KVM vCPU snapshots preserve LAPIC, clock MSRs and run state, but omit vCPU event state and zero the restored special-register interrupt bitmap. This is a concrete state-preservation gap to investigate; it has not been shown to cause this particular timeout. No interrupt state was edited to manufacture a passing replay.

The higher-concurrency profiles contain only two batches, and guests in a batch share load. Client scheduling spreads, SDK pooling, same-host networking and warmed templates contribute to the measurements. These short BusyBox probes do not establish application recovery, large-heap behavior, oversubscription density, sustained throughput, bare-metal results or a reliability SLA. Full synchronized pause bursts and matched competitor stateful runs remain missing. No competitor performance win or across-the-board feature parity is established.
