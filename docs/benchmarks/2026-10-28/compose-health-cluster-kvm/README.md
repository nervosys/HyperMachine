# Compose `healthcheck:` and `service_healthy` (KVM, one cluster node)

`tools/check-compose-health-cluster-kvm.py`: owned Redis, one `hv2-sandboxd` node with
`--network --guest-kit`, `hv2-control-plane` over TLS and node mTLS, `hv2-native-gateway`.
No Docker. Six cases, **3 of 3 runs passed** (`report-run1..3.json`, `run1..3.log`); no
failed runs.

- `db` becomes ready about 20 s after it starts (`test -f /tmp/ready`); `app` has
  `depends_on: {db: {condition: service_healthy}}`. `up` returned after 26.3, 26.5 and
  26.3 s, and the file existed in `db` when it returned.
- Control: the same files with `depends_on: [db]` returned in 0.3, 0.3 and 0.1 s.
- A `db` whose test is `false` (2 s, 2 retries) makes `up` fail with "unhealthy" and leaves
  no sandbox.
- `service_healthy` on a service with no `healthcheck` is refused before anything is created.
- `down` removes both sandboxes; no key or token is in anything `hm` printed.

The timings are bounds that show the wait happens, not measurements of anything.

## Not verified

- `restart:` (refused as an unknown key; the CLI exits after `up` and cannot supervise, and
  whether the node's supervisor covers v2 sandboxes was not checked).
- Health after `up` returns: nothing watches it. No `start_period` or `timeout`.
- `condition: service_completed_successfully` (refused).
- A build-based service with a health check, and a test that needs a shell the image lacks.
- Two nodes, and a standalone (non-cluster) node.
