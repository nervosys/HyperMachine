# `hm sandbox vm run --network` (KVM, owned cluster)

`tools/check-compose-network-cluster-kvm.py` gained a case; an owned Redis, one `hv2-sandboxd` node
with `--network`, control plane and gateway over mTLS, no Docker. The check now has 7 cases.
**3 of 3 runs passed all 7** (`report-run1..3.json`, `run1..3.log`); no failed runs.

What the new case checks, after `compose up -p web` (services `server`, which echoes on TCP 8080,
and `client`):

- `run --network web alpine:3.20 sh -c 'echo hello-from-run | nc -w 3 server 8080'` prints
  `hello-from-run`: the run joined the project's private network and the bare name `server`
  resolved through the search domain.
- The run's sandbox is gone afterwards; only the two compose sandboxes remain.
- `--network "Not A Name"` is refused with "network name" before anything is sent.

## Not verified

- That other sandboxes can resolve or reach the `run` sandbox (it has no service name).
- A run on a node with no private networks, or a key with no principal: it fails (the 404/403
  from the node is returned as an error); no case.
- `--network` with `-p` (the join happens before the detached start; no case), and a project
  name of a different principal (the compose check already shows isolation for `up`, not for `run`).
