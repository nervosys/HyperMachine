# Grouped durable audit writes

The grouped writer is adopted for the opt-in protected-API audit feature. One
dedicated thread drains up to 64 already-queued records into an ordered write
and sync, then acknowledges every caller in the group. Its queue backpressures
at 1024 queued records. It adds no batching timer. Admission still finishes
durably before dispatch, and completion before returning the response. Any
write/sync failure stops all later appends, including queued callers. The writer
closes and drains its queue before releasing its file lock on owner drop.

All **204,516 attempted API requests passed**, including explicitly retained
warmups. The six cohorts contain 185,040 measured requests and 19,476 warmups.
All **187,416 audit records** verify independently, with zero unmatched admissions.
No failed run was removed. Every owned control-plane process stopped, and all
input identities were unchanged. No competitor or guest startup win is claimed.

## Main matched audited comparison

Both audited variants use the same 1/8/50/100-worker protected inventory workload,
fresh processes, storage root and CPU affinity. Each worker issues two warmups
and twenty measured requests using persistent HTTP connections. All six orders
of parent-audited, candidate-audit-disabled and candidate-audited are used once
per profile. Percentiles are nearest rank across measured successful requests;
throughput is the median of six measured batch rates.

| Workers | Per-record P50 / P99 ms | Grouped P50 / P99 ms | Per-record requests/s | Grouped requests/s | Grouped faster mean pairs |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 4.72 / 5.80 | 4.78 / 5.41 | 209 | 205 | 2 / 6 |
| 8 | 36.23 / 39.69 | 9.49 / 10.96 | 219 | 834 | 6 / 6 |
| 50 | 229.32 / 243.61 | 10.81 / 17.10 | 217 | 4239 | 6 / 6 |
| 100 | 452.62 / 798.18 | 17.69 / 39.47 | 219 | 4640 | 6 / 6 |

This improves concurrent audited API service substantially. The lone-worker
path is slightly slower and is retained as a tradeoff. The candidate changes
both writer scheduling and sync grouping; this comparison does not isolate
their individual contributions or measure actual sync-call counts.

## Controls and repeats

`initial/` compares the prior control plane, the new per-record implementation
with auditing disabled, and that same binary with auditing enabled. All 62,964
requests passed. Audited throughput stayed near 211–217 requests/s as workers
increased; audit-disabled throughput was similar to the prior binary.
This motivated the writer candidate.

`audited/` is the main 62,964-request comparison above. `default/` contains
62,964 requests comparing both binaries with auditing disabled and the grouped
binary with auditing enabled. Audit-disabled median throughput at 8/50/100 was
5114/5054/5007 requests/s for the parent versus 5075/5085/5034 for the candidate.
The short one-worker control was lower for the candidate, so longer one-worker
controls were retained too rather than claiming universal non-regression.

`single/` uses ten warmups and one hundred measured requests per worker, with
the parent audited. All 1980 requests passed. Parent/grouped P50 was 9.973/9.993 ms
and throughput 99.87/99.27 requests/s; grouped mean latency improved in two of
six pairs. `single-default/` repeats that length with the parent audit-disabled,
also passing all 1980 requests. Audit-disabled P50 was 0.1418/0.1420 ms and
throughput 6532/6614 requests/s; candidate mean latency improved in four of six
pairs. These controls do not establish an improvement in every profile.

Storage timing changed between cohorts: the longer single-worker audited
comparison was slower for both implementations. `repeat/` therefore makes a
new matched comparison at workers 8/100, with one warmup and five measured
requests per worker. All 11,664 requests passed. Parent/grouped throughput was
110/430 and 109/3823 requests/s, with grouped mean latency lower in all six
pairs of both profiles. Absolute rates vary with the shared host and storage;
the paired comparisons preserve that variation and verify repeat gains.

## Correctness and scope

Windows and Linux each passed 14 core audit tests, 41 cluster library tests,
23 real-HTTP tests and strict cluster Clippy. A gated sink test holds the write
unfinished and proves no queued caller is acknowledged early, then verifies a
ten-record group as part of one valid chain. File tests cover mixed single/grouped
records and verified restart. Existing tests retain admission/completion failure
boundaries and permanent failure latching for twenty concurrent appends.

The shipped-process fixture passed 13 checks. An owned process's file-size limit
first rejects all writes, then separately permits exactly 64 bytes of a new
record. Both cases stop later appends. The partial case refuses restart and
preserves the corrupt tail; its raw file and hash are retained. Only the fixture
restores its known synthetic prefix afterward. The product never truncates or
repairs operator history automatically. The restored prefix has 20 verified
records. The fixture's audit key is public synthetic data (`42` repeated 32 times).

All 22 KVM/TLS lifecycle, named SSH and binary tunnel checks passed with the
grouped control plane; 202 API audit records verified, with zero unfinished
admissions or credentials present. Zero guests remained and all 22 owned
processes stopped. Daemon, CLI, kernel and SSH guest image were unchanged.
The isolated candidate overlays only two audited-writer source files on the
verified access-audit source; the three provisional boot edits are excluded.

These are authenticated `GET /sandboxes` requests against an empty in-memory
inventory over local HTTP on shared WSL hardware. There is no TLS, guest work,
external fleet, added CPU worker or managed competitor in the performance
cohorts. Closed-loop batches, coarse process CPU/write counters, a Python driver
and variable storage timing do not establish sustainable capacity or a hardware
durability SLA. Raw process counters are retained without attributing the gain
to CPU savings or physical storage write amplification.

The archive retains all matrix samples, every audited raw chain, daemon logs,
frozen harnesses/analyzer/verifier, source overlays, build provenance and functional
evidence. Per-run reports duplicate matrix samples and are omitted; fixture keys
are omitted because the verification key is the documented synthetic constant.
`manifest.json` hashes every file except itself. Run `python verify.py` to verify
all cohorts, recomputed summaries, audit chains, source identities and cleanup.

Main candidate invocation:

```sh
python3 /var/tmp/hm-audit-batch/bench-access-audit.py \
  --previous /var/tmp/hm-access-audit/control-plane \
  --current /var/tmp/hm-audit-batch/control-plane --previous-audit \
  --output /var/tmp/hm-audit-batch/audited-cohort
```
