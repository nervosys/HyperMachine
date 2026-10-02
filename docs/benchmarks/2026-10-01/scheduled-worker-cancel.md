# Optimized VM worker: cancellation and real KVM verification

The rebuilt CLI includes the redundant schedule-read and initial-page selection
optimizations. One KVM/TLS run passed all 16 functional cases. The worker resumed
a paused guest, persisted exit code 7 and literal stdout, continued to the next
occurrence after restart, and refused explicit replay.

After two executions, the fixture proved that a third committed occurrence
existed. It cancelled the schedule and invoked the worker again. The invocation
exited successfully without output; no claim for that occurrence existed and
the guest execution counter remained at two. Existing receipts were preserved.
This verifies a worker started after cancellation. Cancellation racing an
accepted request, worker loss, guest cancellation and reconciliation are outside
this check.

Cleanup recorded zero remaining sandboxes, no errors and all 22 registered
processes stopped. No performance comparison is reported by this run. Local
selection timing evidence remains in the [backlog diagnostic](dispatch-backlog.md).

See the [raw report](scheduled-worker-cancel-run-1.json),
[manifest](scheduled-worker-cancel-manifest.json) and
[frozen coordinator](scheduled-worker-cancel-coordinator.py).
The CLI source is the manifest commit plus the unchanged
[provisional core patch](scheduled-worker-core-source.patch). The run uses the
accepted frozen daemon and control plane, not the provisional daemon candidate.
The previous worker archive remains unchanged.

Reproduce using the command in [the first worker report](scheduled-worker.md),
substituting this frozen coordinator, CLI
`/var/tmp/hm-scheduled-worker-cancel/hm-verified`, and a fresh output path.
`python tools/verify-scheduled-worker-cancel.py` verifies the archived hashes,
all functional assertions and cleanup. The fixture is one vCPU / 1024 MiB.
