# Durable calendar publication: real KVM verification

The CLI built from e901a11 passed all 16 functional cases against the accepted daemon and control plane, with zero remaining sandboxes, no cleanup errors and all 22 registered processes stopped.

The calendar case first published the 2025 November Los Angeles fold's 08:30 UTC occurrence, then published a bounded two-occurrence catch-up batch through the next day. The complete committed list matched 08:30 UTC, 09:30 UTC, then the next day's 09:30 UTC. Two separate automatic worker invocations resumed the paused guest and executed the two fold occurrences in order. Guest file contents confirmed one execution each, durable API-origin receipts retained literal output and exit code 7, and explicit replay was refused. Each worker also published one subsequent occurrence. Cancellation preserved history and receipts; a further worker invocation left pending work unclaimed.

These historical timestamps were deliberately overdue. This verifies reuse of the selected immutable calendar batch through publication and VM dispatch; it does not measure calendar throughput or wait through a live DST clock transition. Automatic guest reconciliation and cross-schedule overlap control remain incomplete.

[Raw report](calendar-publication-kvm-run-1.json), [artifact/source manifest](calendar-publication-kvm-manifest.json), [frozen coordinator](calendar-publication-kvm-coordinator.py). The CLI includes the unchanged provisional core source patch listed in the manifest; the daemon uses the previously accepted artifact. No provisional daemon was exercised.

Reproduce on the fixture host:

```sh
python3 docs/benchmarks/2026-10-01/calendar-publication-kvm-coordinator.py \
  --daemon /var/tmp/hm-tcp-api-nodelay/final/hv2-sandboxd-release \
  --control-plane /var/tmp/hm-tcp-api-nodelay/final/hv2-control-plane-release \
  --cli /var/tmp/hm-calendar-publication-kvm/hm-verified \
  --kernel /var/tmp/hm-competitive/bzImage-known-uart-irq \
  --initrd /var/tmp/hm-tcp-backlog/guest-tcp.cpio.gz \
  --scheduled-worker --scheduled-calendar --scheduled-calendar-batch \
  --output /var/tmp/hm-calendar-publication-kvm/run-2.json
```

Verify archived evidence with `python tools/verify-calendar-publication-kvm.py`.
