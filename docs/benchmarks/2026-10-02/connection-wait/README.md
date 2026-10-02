# Connection-wait experiment

The isolated candidate replaces the guest-agent connection's 5-ms periodic
wait with a wait until a device notification or the original absolute deadline.
Host-side connection close, removal and device reset notify observers in the
candidate. Guest packet processing already notifies them. Refused-connection
retry delays and other request/stream waits are unchanged.

The candidate was **not adopted**. All 800 cold-create-to-command attempts passed,
but it improved batch mean readiness in only two of four counterbalanced pairs.
Successful P50 was slightly slower. The lower P99 is insufficient evidence for
a repeatable improvement on this shared host. No wakeup counts or CPU savings
were measured, and the earlier 100-guest failure was not reproduced or fixed.

| Variant | Passed / attempted | P50 ms | P99 ms |
| --- | ---: | ---: | ---: |
| Current baseline | 400 / 400 | 6460.28 | 8935.46 |
| Event-driven connection candidate | 400 / 400 | 6469.43 | 7124.00 |

Nearest-rank percentiles cover successful, cleaned-up attempts. Candidate mean
reductions by pair were +661.99, -40.20, +1780.61 and -43.33 ms. The median
paired mean reduction was 310.89 ms; the two large gains coincided with slower
baseline batches. Shared-host variation prevents attributing those gains to
the wait policy.

Both variants started a fresh owned daemon per batch, with 100 barrier-released
guests, identical kernel/initrd, one vCPU and 1024 MiB per guest, 15-second agent
readiness limits, eight host CPUs in affinity and no added CPU load. Order was
baseline/candidate, candidate/baseline, repeated twice. Every batch retained its
guests until validation and process-memory capture, then deleted them. Empty
inventory, process termination and unchanged input hashes were verified.
These are local native-engine measurements; no competitor endpoints were used.

The candidate used the previously verified clean source, excluding the three
pending boot edits. Core/agent tests passed: 2295 and 531 respectively, with two
core tests ignored. The first candidate test compile failed because of a trait
import; its log is retained. The corrected run covers host teardown wakeups,
notifications arriving before waiting and deadlines under unrelated progress.
The reused burst harness also passed its existing 17 checks.

`c100.json` retains all attempts and daemon log tails. `analysis.json` is
recomputed by the frozen analyzer. Source snapshots, patches, build logs and
`build-context.json` record the candidate. `manifest.json` hashes every archived
file except itself. Run `python verify.py` to verify the archive.

Exact run:

```sh
python3 /var/tmp/hm-event-connect/bench-connection-wait.py \
  --baseline /var/tmp/hm-registration-mutations/daemon \
  --candidate /var/tmp/hm-event-connect/hv2-sandboxd \
  --kernel /var/tmp/hm-competitive/bzImage-known-uart-irq \
  --initrd /var/tmp/hm-competitive/guest-output-drain.cpio.gz \
  --output /var/tmp/hm-event-connect/c100.json --pairs 4 --concurrency 100
```
