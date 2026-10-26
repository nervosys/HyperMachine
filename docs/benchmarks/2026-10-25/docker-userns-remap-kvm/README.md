# `--userns-remap` in a sandbox (2026-10-25)

`tools/check-docker-sandbox-kvm.py --network` on WSL2's KVM, as root, the setup
in [2026-10-23](../../2026-10-23/docker-sandbox-kvm/README.md). All 16 cases
pass in 3 runs in a row (`runs/run1..3.json`; `report.json` is the last). The
new case is the last Docker one:

- In the guest, `dockremap` was created and `/etc/subuid` and `/etc/subgid` got
  `dockremap:100000:65536`. `daemon.json` got `"userns-remap": "dockremap"`
  and dockerd restarted. A container's `/proc/self/uid_map` read
  `0 100000 65536`, and a file its root created in a bind-mounted directory was
  owned by uid **100000** on the guest. The original `daemon.json` was put back
  and dockerd restarted afterwards.

The remapped daemon keeps its own image store, so the case imports the image
again.

NOT VERIFIED: remapping together with `docker compose` or `docker build`; the
remapped daemon across a checkpoint and fork; remapping from boot (the case
restarts dockerd in a running sandbox); a Docker kernel without
`CONFIG_USER_NS` (it is in `docker.fragment`).
