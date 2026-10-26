# A `RUN` step that needs the network, during `docker build` (2026-10-25)

`tools/check-docker-sandbox-kvm.py --network` on WSL2's KVM, as root, the
setup in [2026-10-23](../../2026-10-23/docker-sandbox-kvm/README.md). All 15
cases pass in 3 runs in a row (`runs/run1..3.json`; `report.json` is the last).
The new case is the 10th:

- A `busybox httpd` container on the guest's own bridge served a file. A
  Dockerfile's `RUN wget` fetched it by the container's bridge address
  (`--build-arg SRV=...`) while `docker build` ran, and the image, run with
  `--network none`, printed what was fetched.

The first attempt failed in the check and not in Docker: this Docker has no
top-level `NetworkSettings.IPAddress`, so the address is read from
`Networks.bridge.IPAddress`.

NOT VERIFIED: a `RUN` step that reaches the internet (the server was on the
guest's bridge, so this checks the build's network and not outside DNS or
routing; pulling `busybox:1.36` from Docker Hub in other cases did use the
internet); a `RUN` that installs packages; a build behind a proxy.
