# Explicit scheduled VM dispatch: real KVM verification

One local WSL nested-KVM run passed all 16 functional cases, with zero remaining
sandboxes, no cleanup errors and all 22 registered processes stopped.
This establishes no performance comparison or automatic scheduling parity.

The optional `--scheduled-dispatch` case in `tools/e2e-tcp-tunnel.py` created and
published a VM occurrence through the CLI, paused its guest, then dispatched it
through verified API TLS and node mTLS. The guest preserved a literal environment
value containing quotes and command-substitution text, returned exit code 7, and
appended exactly one byte to a guest file. A separate CLI invocation recovered
stdout from the durable receipt. Replaying dispatch was refused and the file
remained unchanged. Cancelling the schedule blocked later publication and
preserved the receipt. The other 15 TCP/lifecycle cases also passed.

See the [raw report](scheduled-dispatch-run-1.json),
[artifact and source manifest](scheduled-dispatch-manifest.json), and
[source patch](scheduled-dispatch-source.patch). The patch contains the harness
addition and the provisional core edits present when the CLI was built. The
daemon and control plane used frozen accepted artifacts; this run neither uses
the borrowed-boot daemon candidate nor measures that candidate.

`python tools/verify-scheduled-dispatch.py` checks archive hashes, functional
assertions and cleanup records. `--cli PATH` additionally checks the tested CLI
artifact hash. Both archive verification and the frozen Linux artifact check passed.

The verified CLI is frozen locally at `/var/tmp/hm-scheduled-dispatch/hm-verified`.
The full run used:

```sh
python3 tools/e2e-tcp-tunnel.py \
  --daemon /var/tmp/hm-tcp-api-nodelay/final/hv2-sandboxd-release \
  --control-plane /var/tmp/hm-tcp-api-nodelay/final/hv2-control-plane-release \
  --cli /var/tmp/hm-scheduled-dispatch/hm-verified \
  --kernel /var/tmp/hm-competitive/bzImage-known-uart-irq \
  --initrd /var/tmp/hm-tcp-backlog/guest-tcp.cpio.gz \
  --scheduled-dispatch --output /var/tmp/hm-scheduled-dispatch/run-2.json
```

Only one run is recorded. Worker loss during guest execution, guest reconciliation,
automatic dispatch, recurring cron execution, execution overlap control and
guest-job cancellation remain incomplete. The fixture used a one-vCPU, 1024-MiB
guest and an operator profile pointing to the owned control plane.
