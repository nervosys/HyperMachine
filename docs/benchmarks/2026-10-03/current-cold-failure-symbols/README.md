# Retained cold-start failure symbols

All 229 retained creation timeouts from the current admission experiment have complete owner snapshots and verified instruction-pointer bounds in an independently booted identical kernel/image. Two owned Firecracker diagnostic guests answer the exact workload and symbol queries, then terminate cleanly. Input hashes remain unchanged and bind each mapping to its original immutable failure report.

| Cohort | Retained failures | Runnable | Halted | Halted symbol |
|---|---:|---:|---:|---|
| Initial | 100 | 100 | 0 | — |
| Repeat | 129 | 102 | 27 | default_idle |

Runnable snapshots span multiple kernel locations, including slab allocation, page clearing, text patching, instruction decoding and initialization. The repeat's 27 halted snapshots all map to default_idle; their retained TSC_DEADLINE values are nonzero and greater than the sampled TSC. This does not demonstrate an expired deadline or missed timer. The mapping guests are separate from the failed guests, and snapshots are not execution traces or simultaneous timing observations.

The evidence separates an unfinished/runnable group from an idle/halted group and rules out a single retained execution location covering every failure. It does not establish starvation, a lost wakeup, the guest's phase history, or a corrective change. Admission's earlier pass-rate benefit remains scoped to its tested cohorts and does not resolve the underlying reliability gap. Raw failure snapshots, exact symbol queries, bounding symbols and counts remain in the reports.

No production source or default changed. This is diagnosis of owned guests, not a benchmark ranking. Reproduce using the archived driver and frozen inputs; the original [admission reports](../current-admission-sixteen/README.md) must retain their hashes.
