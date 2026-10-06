# Automatic scheduled VM worker: real KVM verification

One local WSL nested-KVM run passed all 16 functional cases. Cleanup recorded
zero remaining sandboxes, no errors and 22 stopped registered processes.
This is functional evidence, with no performance comparison.

The worker resumed a paused guest through verified API TLS and node mTLS,
recorded literal environment values and exit code 7, then continued to the next
occurrence on a separate invocation. Guest file contents confirmed one execution
per occurrence. Explicit replay was refused, and cancellation preserved receipts.
The other 15 TCP and lifecycle cases passed.

The [raw report](scheduled-worker-run-1.json), [manifest](scheduled-worker-manifest.json),
[frozen coordinator](scheduled-worker-coordinator.py) and
[provisional core patch](scheduled-worker-core-source.patch) preserve the evidence.
The CLI was built from the manifest commit plus the core patch. The accepted
daemon and control plane were frozen separately; this does not test the
provisional borrowed-boot daemon candidate.

Reproduce with the frozen coordinator and these local artifacts:

```sh
python3 docs/benchmarks/2026-10-01/scheduled-worker-coordinator.py \
  --daemon /var/tmp/hm-tcp-api-nodelay/final/hv2-sandboxd-release \
  --control-plane /var/tmp/hm-tcp-api-nodelay/final/hv2-control-plane-release \
  --cli /var/tmp/hm-scheduled-worker/hm-verified \
  --kernel /var/tmp/hm-competitive/bzImage-known-uart-irq \
  --initrd /var/tmp/hm-tcp-backlog/guest-tcp.cpio.gz \
  --scheduled-worker --output /var/tmp/hm-scheduled-worker/run-2.json
```

Worker loss during guest execution, uncertain-result reconciliation, guest-job
cancellation, cron, cross-schedule overlap control and long-history performance
remain unverified or incomplete. The fixture used one vCPU and 1024 MiB per guest.
