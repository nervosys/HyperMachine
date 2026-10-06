# Calendar VM worker: real KVM catch-up verification

One local WSL nested-KVM run passed all 16 functional cases, with zero remaining
sandboxes, no cleanup errors and 22 stopped registered processes.

The calendar case created `30 1 * * *` in `America/Los_Angeles`, starting before
the November 2, 2025 repeated hour. It checked that creation persisted a timezone
database version, then published and executed the 08:30 UTC occurrence through
the automatic worker after pausing the guest. A separate worker invocation
continued to 09:30 UTC. Both represent local 01:30 and have distinct UTC job IDs.
Literal environment values, exit code 7 and durable stdout were recovered from
API-origin receipts. Guest file contents confirmed one execution per occurrence,
and explicit replay was refused. The next published day's occurrence remained
unclaimed after schedule cancellation and another worker invocation.

These historical occurrences were deliberately overdue and executed consecutively
as catch-up. The run did not wait through an actual wall-clock DST transition.
Timezone gap/fold and publication logic have separate unit and CLI/API tests.
No calendar latency, throughput, long-history guarantee or competitor score is
established here. Automatic guest reconciliation, guest-job cancellation,
timezone-rule migration and cross-schedule overlap control remain incomplete.

See the [raw report](scheduled-calendar-run-1.json),
[manifest](scheduled-calendar-manifest.json), and
[frozen coordinator](scheduled-calendar-coordinator.py).
The CLI source is the manifest commit plus the unchanged
[provisional core patch](scheduled-worker-core-source.patch). The run uses the
accepted daemon/control-plane artifacts, not the provisional daemon candidate.

```sh
python3 docs/benchmarks/2026-10-01/scheduled-calendar-coordinator.py \
  --daemon /var/tmp/hm-tcp-api-nodelay/final/hv2-sandboxd-release \
  --control-plane /var/tmp/hm-tcp-api-nodelay/final/hv2-control-plane-release \
  --cli /var/tmp/hm-scheduled-calendar/hm-verified \
  --kernel /var/tmp/hm-competitive/bzImage-known-uart-irq \
  --initrd /var/tmp/hm-tcp-backlog/guest-tcp.cpio.gz \
  --scheduled-worker --scheduled-calendar \
  --output /var/tmp/hm-scheduled-calendar/run-2.json
```

`python tools/verify-scheduled-calendar.py` checks hashes, all 16 cases, the two
fold timestamps and cleanup. The fixture used one vCPU / 1024 MiB per guest.
