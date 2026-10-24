# Docker's engine inside a sandbox, on KVM (2026-10-23)

`tools/check-docker-sandbox-kvm.py --network` on WSL2's KVM, running as root: an
owned `hv2-sandboxd` whose `base` template is the initramfs from
`build-tools-image.sh --docker docker-29.9.2.tgz` (Ubuntu 24.04, systemd, Docker's
static binaries, 122 MiB) on a Linux 6.6.52 kernel from `build-kernel.sh --docker`
(`resolved.config` is its full configuration), at 2048 MiB. 8 of 8 cases pass in
each of 3 runs in a row (`runs/run1..3.json`; `report.json` is run 1). Docker is
the engine inside the guest; nothing on the host used it.

1. `dockerd` is up under systemd: containerd snapshotter `overlayfs`, cgroup v2,
   Docker 29.9.2, seccomp and cgroupns.
2. A container runs from an image imported from a tarball, with no network.
3. `--memory 64m` reaches the container's cgroup (`memory.max` 67108864).
4. Two containers on a user-defined bridge: one serves a line with `nc`, the other
   reads it by container name.
5. `docker build` of a Dockerfile (`FROM scratch`, `COPY`, a `RUN` step, `CMD`): the
   RUN step executes and the built image prints what it wrote.
6. `-p 8080:9000`: the container's port answers HTTP from the guest (`127.0.0.1:8080`)
   and from outside the sandbox through the node's port proxy
   (`Host: 8080-<sandbox>.localhost`); a DNAT rule for 8080 exists.
7. A checkpoint and a fork carry a running container: with a published port
   serving, a checkpoint is taken, the container removed and a file changed; the
   restore brings back the container (still serving), the file as it was, and the
   fork has the container serving and can run another.
8. `docker run hello-world` pulls from Docker Hub and prints "Hello from Docker!".

What it took, each found by a failing run: the default kernel has no memory cgroup,
overlayfs, veth, bridge, user namespaces or iptables `raw` table (`docker.fragment`
adds them); `iptables` must be the legacy flavour (the kernel has no nf_tables);
and runc cannot `pivot_root` out of an initramfs, so `runc-nopivot` adds
`--no-pivot` and is Docker's default runtime.

## NOT VERIFIED

`docker compose`, a `docker build` from a pulled base (only `FROM scratch` was
built), `RUN` steps that need the network, volumes beyond a read-only bind mount of
one file, a published port from outside the node, a checkpoint or fork of a sandbox
with more than one container, with a volume holding data, or with a build in
progress (one container with a published port was checkpointed and forked), user namespaces with `--userns-remap`, the default kernel
(the Docker kernel is a separate build, opted into with `HV2_KERNEL`), and Docker
together with `/dev/kvm`.
