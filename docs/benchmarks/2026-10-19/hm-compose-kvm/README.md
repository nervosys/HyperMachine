# `hm sandbox vm compose up|down` on KVM (2026-10-19)

**What this checks:** that the real `hm` binary, against an owned
`hv2-sandboxd` with a guest kit, starts the services of a compose file, each in
a VM of its own, detached and in `depends_on` order, and removes them. No
Docker anywhere. See [`RUN_IMAGES.md`](../../../RUN_IMAGES.md#compose).

## Method

`tools/check-hm-compose-kvm.py` (7 cases) drives `hm sandbox vm compose` against
`alpine:3.20`, pulled by the node. The node runs on real KVM (WSL2, nested); the
guest has no network.

1. Two services, `db` and `app` (`depends_on: [db]`): `up` starts `db` first, and
   each service has a sandbox ID.
2. The commands are still running after `up` returned (a file each wrote is
   there and a `sleep 600` process exists), and `app` has its `environment`.
3. `down` deletes both, `list` shows neither, and a second `down` deletes nothing.
4. A cycle, an unknown dependency, a port range and `volumes:` each fail and create
   no sandbox.
5. A second `up` without `down` is refused and leaves the first alone.
6. A `build:` service builds, runs its CMD, and goes away with `down`.
7. `compose logs` shows each service's own output, only one service on request,
   and refuses an unknown name.

## Result

7 of 7 cases passed on 3 runs in a row (`runs/run1..3.json`; `report.json` is
run 1). The inputs' SHA-256 sums are in each report.

## Not verified

- Services reaching one another: v1 has no network between them (private
  networks between sandboxes are unfinished), so `depends_on` is start order
  only, with no health check.
- `volumes:` is refused, not implemented. `ports:` (a public-port reservation per
  container port) is NOT exercised here: it needs the control plane's
  `--native-port-range` and the gateway cluster, so only its parsing is tested.
- A service whose command exits or crashes is not restarted; its output goes
  to `/tmp/hm-compose.log` in the guest, which `compose logs` reads.
- A compose file taken from an untrusted source: the YAML parser
  (`serde_yaml_ng`, on `unsafe-libyaml`, which is unmaintained) reads it.
- Other platforms: run only on Linux/KVM here.
