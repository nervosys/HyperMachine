# Cold-start admission

`hv2-sandboxd --cold-start-concurrency N` limits simultaneous cold VM boots.
The option accepts 1–1024 and is disabled by default. It counts VMs, not vCPUs.
Choose a budget for the node's CPU capacity and workload; eight is a tested
setting on an eight-CPU shared nested-KVM fixture, not a recommended universal value.

A cold creation waits before constructing and launching its VM. The slot is
released after the guest agent answers, before networking, volumes and environment
setup. Errors release the slot. Snapshot restoration and forks bypass this budget;
startup and dynamic template construction are outside its scope.

Queued requests already occupy sandbox capacity and continue consuming their
request timeout. Queue time is included in the readiness benchmarks. The guest
agent's existing 15-second readiness deadline starts after launch and is unchanged.
Operators should account for upstream timeouts when enabling admission.

The live-VM fixture verifies a two-slot bound, release after an unsuccessful boot,
and snapshot resume while a one-slot cold budget is occupied. This does not establish
complete cancellation cleanup or resolve the cause of previously observed boot
timeouts. See [measurements and reproducibility](benchmarks/2026-10-02/cold-start-admission/README.md).

[Direct eight/sixteen-slot comparisons](benchmarks/2026-10-02/cold-budget-comparison/README.md)
retain two repeats: sixteen lowered pooled P99 in both, but paired tail improvement
was inconsistent in the repeat and paired means split evenly overall. Both settings
improved local median readiness over Firecracker in their respective cohorts, with
worse P99 and held process memory. These measurements establish no universal budget.

[Current optimized-binary uncapped/sixteen-slot repeats](benchmarks/2026-10-03/current-admission-sixteen/README.md) at 100 simultaneous one-vCPU starts passed 800/800 with sixteen slots versus 571/800 uncapped on eight allowed CPUs. Queueing is included. Sixteen improved paired mean/P50 in four of five fully successful pairs, but P99 in only two; pooled successful P99 was higher in both cohorts. This is a tested option for that workload, with a reliability/tail tradeoff, not a universal recommendation. The disabled default remains unchanged. Failed starts remain in the evidence, and successful-only latency summaries cannot establish a reliability SLA.

Idle eviction (`--evict-idle-after`) excludes guests whose local registration is still pending. [Controlled KVM evidence](benchmarks/2026-10-03/pending-registration-idle-eviction/README.md) verifies that a newer eligible idle guest can release a slot while an older uncertain guest stays preserved for reconciliation. The cold-start boot budget and idle-eviction policy remain separate controls.
