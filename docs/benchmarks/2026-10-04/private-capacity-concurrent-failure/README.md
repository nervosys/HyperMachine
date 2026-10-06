# Failed concurrent private UDP capacity cohort

The owned 32-worker, 100-roundtrip-per-worker workload at private receiving saturation failed: 26 workers completed all round trips; six timed out after two or three completions near the first maximum-size datagram. Raw worker outcomes, checker/driver, daemon log, fixture source and failure stdout are retained. No report.json was published because the checker raised before its final report. A subsequent process inventory showed no tracked hv2 services. Full guest-count cleanup is not independently established by that inventory.

This is failed runtime evidence, not a valid throughput result or competitor comparison. Receiver buffering and transport behavior require diagnosis; no production cause is established. Do not weaken the workload or hide failures. The prior serialized capacity result remains scoped to its frozen checker and cohort.
