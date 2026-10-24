# Docker's engine inside a sandbox, on KVM (2026-10-23)

`tools/check-docker-sandbox-kvm.py --network` on WSL2's KVM, running as root: an
owned `hv2-sandboxd` whose `base` template is the initramfs from
`build-tools-image.sh --docker docker-29.9.2.tgz` (Ubuntu 24.04, systemd, Docker's
static binaries, 122 MiB) on a Linux 6.6.52 kernel from `build-kernel.sh --docker`
(`resolved.config` is its full configuration), at 2048 MiB. 5 of 5 cases pass in
each of 3 runs in a row (`runs/run1..3.json`; `report.json` is run 1). Docker is
the engine inside the guest; nothing on the host used it.

1. `dockerd` is up under systemd: containerd snapshotter `overlayfs`, cgroup v2,
   Docker 29.9.2, seccomp and cgroupns.
2. A container runs from an image imported from a tarball, with no network.
3. `--memory 64m` reaches the container's cgroup (`memory.max` 67108864).
4. Two containers on a user-defined bridge: one serves a line with `nc`, the other
   reads it by container name.
5. `docker run hello-world` pulls from Docker Hub and prints "Hello from Docker!".

What it took, each found by a failing run: the default kernel has no memory cgroup,
overlayfs, veth, bridge, user namespaces or iptables `raw` table (`docker.fragment`
adds them); `iptables` must be the legacy flavour (the kernel has no nf_tables);
and runc cannot `pivot_root` out of an initramfs, so `runc-nopivot` adds
`--no-pivot` and is Docker's default runtime.

## NOT VERIFIED

Building images (`docker build`), `docker compose`, published ports from a
container to outside the sandbox, volumes, a checkpoint or fork of a sandbox with
containers running, user namespaces with `--userns-remap`, the default kernel
(the Docker kernel is a separate build, opted into with `HV2_KERNEL`), and
`/dev/kvm` in the guest.
